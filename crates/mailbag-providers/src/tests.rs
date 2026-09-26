// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::batch::{MessageIdentity, ReceivedBatch, ReceivedMessage};
use crate::gmail::load_gmail_inbox;
use crate::imap::load_imap_inbox;
use crate::microsoft365::load_microsoft365_inbox;
use crate::store_load::store_batch;
use crate::test_record::CapturedRecord;
use crate::worker::{LoadKind, MailWorker, report_outcome};
use goa_adapter::{GraphAccess, ImapAccess, ImapCredential, ImapEncryption};
use mailbag_domain::{
    AccountId, ContentExplanation, DisplayFields, Failure, FailureKind, IncompleteList, Message,
    ReceivedContent,
};
use mailbag_graph::{GraphError, test_server as graph_service};
use mailbag_imap::{
    GmailRow, ImapError, ImapFailure, ImapStep,
    test_server::{
        FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture, PRIVATE_MARKERS,
        TEST_ACCESS_TOKEN, TEST_LOGIN, TEST_PASSWORD, test_certificates_trusted,
    },
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn account_access(fixture: &ImapFixture) -> ImapAccess {
    ImapAccess {
        account_id: AccountId::try_from("synthetic-account").unwrap(),
        host: format!("localhost:{}", fixture.port()),
        login: TEST_LOGIN.to_owned(),
        credential: ImapCredential::Password(TEST_PASSWORD.to_owned()),
        encryption: ImapEncryption::ImplicitTls,
    }
}

fn plain_messages(count: u32) -> Vec<FixtureMessage> {
    (1..=count)
        .map(|number| FixtureMessage::plain_text(number * 10, &format!("Text {number}")))
        .collect()
}

/// Runs the Generic IMAP load sequence to its end, without the worker and the
/// store.
fn load_inbox(fixture: &ImapFixture) -> Result<ReceivedBatch, ImapError> {
    run_on_context(load_imap_inbox(account_access(fixture)))
}

/// Runs the Gmail load sequence to its end, without the worker and the store.
fn load_gmail(fixture: &ImapFixture) -> Result<ReceivedBatch, ImapError> {
    run_on_context(load_gmail_inbox(gmail_access(fixture)))
}

/// Runs one load on a new worker to its end and its write into `store`, as the
/// window would.
fn load_with_kind(kind: LoadKind, store: &Arc<Store>) -> LoadResult {
    run_on_context(finish_load(&MailWorker::new(store.clone()), kind))
}

/// The messages a load stored for the account.
fn stored_messages(store: &Store, account: &str) -> Vec<Message> {
    let account = AccountId::try_from(account).unwrap();
    store
        .read_inbox(&account)
        .expect("the store reads")
        .expect("the account has a stored Inbox")
}

/// Runs one load on `worker` and waits for its outcome.
async fn finish_load(worker: &MailWorker, kind: LoadKind) -> LoadResult {
    let (sender, outcomes) = async_channel::bounded(1);
    let _handle = worker.load_inbox(kind, move |outcome| {
        sender.try_send(outcome).ok();
    });
    outcomes.recv().await.expect("the load reports its outcome")
}
fn run_on_context<T>(future: impl Future<Output = T>) -> T {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .unwrap()
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "test server deadline");
        glib::timeout_future(Duration::from_millis(10)).await;
    }
}

fn received_batch<E: std::fmt::Debug>(loaded: Result<ReceivedBatch, E>) -> ReceivedBatch {
    loaded.expect("the load received a batch")
}

fn text_of(content: &ReceivedContent) -> &str {
    match content {
        ReceivedContent::Text(text) => text,
        other => panic!("the message has no text: {other:?}"),
    }
}

#[test]
fn batches_hold_the_newest_hundred_messages_with_their_text() {
    for count in [0, 1, 100, 101] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(count),
            ..FixtureSetup::default()
        });
        let batch = received_batch(load_inbox(&fixture));
        let expected: Vec<MessageIdentity> = (1..=count)
            .rev()
            .take(100)
            .map(|number| MessageIdentity::ImapUid(number * 10))
            .collect();
        let identities: Vec<MessageIdentity> = batch
            .messages
            .iter()
            .map(|message| message.identity.clone())
            .collect();
        assert_eq!(identities, expected, "{count} messages");
        assert_eq!(
            batch.account_id,
            AccountId::try_from("synthetic-account").unwrap()
        );
        for (message, number) in batch.messages.iter().zip((1..=count).rev()) {
            assert_eq!(text_of(&message.content).trim(), format!("Text {number}"));
            assert_eq!(
                message.fields.subject.as_deref(),
                Some(format!("Message {}", number * 10).as_str())
            );
            assert!(message.internal_date.is_some());
        }
    }
}

#[test]
fn a_message_the_server_cannot_describe_keeps_its_row() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![
            FixtureMessage::plain_text(10, "readable"),
            // Nested deeper than the IMAP parser accepts.
            FixtureMessage::deeply_nested(20, 40),
        ],
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    let contents: Vec<(&MessageIdentity, &ReceivedContent)> = batch
        .messages
        .iter()
        .map(|message| (&message.identity, &message.content))
        .collect();
    assert_eq!(contents.len(), 2);
    assert_eq!(
        contents[0],
        (
            &MessageIdentity::ImapUid(20),
            &ReceivedContent::StructureUnreadable
        )
    );
    assert_eq!(text_of(contents[1].1), "readable");
}

#[test]
fn only_the_selected_text_parts_are_requested() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::multipart(
            10,
            &[("plain", "the readable part"), ("html", "<p>skip me</p>")],
        )],
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    assert_eq!(text_of(&batch.messages[0].content), "the readable part");
    let requested: Vec<String> = fixture
        .log()
        .fetches
        .into_iter()
        .flat_map(|fetch| fetch.items)
        .collect();
    assert!(
        requested.contains(&"BODY.PEEK[1]".to_owned()),
        "{requested:?}"
    );
    // The HTML alternative is described but never downloaded.
    assert!(
        !requested.iter().any(|item| item.contains("[2]")),
        "{requested:?}"
    );
}

#[test]
fn an_interrupted_transfer_publishes_no_batch() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        fault: Some((FaultyCommand::Text, FaultKind::Close)),
        ..FixtureSetup::default()
    });
    let error = load_inbox(&fixture).unwrap_err();
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchText));
}

#[test]
fn a_cancelled_load_closes_its_connection_before_it_ends() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        fault: Some((FaultyCommand::Text, FaultKind::Stall)),
        ..FixtureSetup::default()
    });
    run_on_context(async {
        let worker = MailWorker::new(Arc::new(Store::in_memory()));
        let (sender, outcomes) = async_channel::bounded(1);
        let handle = worker.load_inbox(
            LoadKind::GenericImap(account_access(&fixture)),
            move |outcome| {
                sender.try_send(outcome).ok();
            },
        );
        // Cancel while the server is stalling on the text command.
        wait_until(|| fixture.log().fetches.len() == 3).await;
        drop(handle);
        let outcome = outcomes.recv().await.expect("the load reports its outcome");
        assert!(matches!(outcome, LoadResult::Cancelled), "{outcome:?}");
        // The worker closes the socket before it reports the outcome; the
        // server sees the closed connection as soon as it runs again.
        wait_until(|| fixture.log().closed_connections == 1).await;
    });
}

#[test]
fn a_window_that_empties_during_the_load_is_not_an_empty_inbox() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // Another client moves both messages away before their text is read.
        vanishing_text_uids: vec![10, 20],
        ..FixtureSetup::default()
    });
    let error = load_inbox(&fixture).unwrap_err();
    assert_eq!(error.failure, ImapFailure::InboxChanged);
}

#[test]
fn text_the_server_does_not_return_keeps_its_row_with_an_explanation() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // The server answers for message 20 without the section it asked for.
        missing_body_uid: Some(20),
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    let contents: Vec<(&MessageIdentity, &ReceivedContent)> = batch
        .messages
        .iter()
        .map(|message| (&message.identity, &message.content))
        .collect();
    assert_eq!(
        contents[0],
        (
            &MessageIdentity::ImapUid(20),
            &ReceivedContent::TextNotReturned
        )
    );
    assert_eq!(text_of(contents[1].1), "Text 1");
}

#[test]
fn a_stopped_worker_ends_the_load_with_a_visible_failure() {
    run_on_context(async {
        // A worker thread that stopped leaves its outcome channel closed.
        let (sender, outcome) = async_channel::bounded::<LoadResult>(1);
        drop(sender);
        let account_id = AccountId::try_from("synthetic-account").unwrap();
        for reported in [Some(outcome), None] {
            let mut outcome = None;
            report_outcome(account_id.clone(), reported, |result| {
                outcome = Some(result)
            })
            .await;
            assert!(
                matches!(
                    outcome,
                    Some(LoadResult::Failed(Failure {
                        kind: FailureKind::Stopped,
                        ..
                    }))
                ),
                "{outcome:?}"
            );
        }
    });
}

#[test]
fn a_panic_ends_its_load_with_the_place_and_the_worker_serves_the_next() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    run_on_context(async {
        let worker = MailWorker::new(store.clone());
        let panicking = LoadKind::PanicsForTest(AccountId::try_from("synthetic-account").unwrap());
        match finish_load(&worker, panicking).await {
            LoadResult::Failed(failure) => {
                assert_eq!(failure.kind, FailureKind::Stopped);
                let details = failure.details;
                assert!(details.contains("a load panicked on purpose"), "{details}");
                assert!(details.contains("worker.rs:"), "{details}");
            }
            other => panic!("unexpected {other:?}"),
        }
        // The same thread keeps its queue and loads the next Inbox.
        let loads = worker.loads.borrow().clone().expect("the worker started");
        assert!(!loads.is_closed());
        let next = finish_load(&worker, LoadKind::GenericImap(account_access(&fixture))).await;
        assert!(matches!(next, LoadResult::Stored { .. }), "{next:?}");
    });
    assert_eq!(stored_messages(&store, "synthetic-account").len(), 1);
}

#[test]
fn the_next_load_starts_a_new_worker_after_one_stopped() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    let outcome = run_on_context(async {
        let worker = MailWorker::new(store.clone());
        // Leave behind the closed queue of a worker that has stopped.
        let (loads, requests) = async_channel::unbounded();
        drop(requests);
        *worker.loads.borrow_mut() = Some(loads);

        let (sender, outcomes) = async_channel::bounded(1);
        let _handle = worker.load_inbox(
            LoadKind::GenericImap(account_access(&fixture)),
            move |outcome| {
                sender.try_send(outcome).ok();
            },
        );
        outcomes.recv().await.expect("the load reports its outcome")
    });
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert_eq!(stored_messages(&store, "synthetic-account").len(), 1);
}

/// The whole path: the server reports the Content-ID, the selection follows
/// the start parameter, and the text of that part is what the reader gets.
#[test]
fn a_related_message_reads_the_part_its_start_names() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::related_with_start(
            10,
            "Text inside related",
        )],
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    assert_eq!(
        text_of(&batch.messages[0].content).trim(),
        "Text inside related"
    );
}

/// A message that disappeared while its flag change was reported keeps no row,
/// unlike a message whose structure the server could not read.
#[test]
fn a_message_that_vanished_after_a_flag_change_keeps_no_row() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        vanishing_uid: Some(10),
        flag_change_uids: vec![10],
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    assert_eq!(
        batch
            .messages
            .iter()
            .map(|m| &m.identity)
            .collect::<Vec<_>>(),
        [&MessageIdentity::ImapUid(20)]
    );
}

#[test]
fn a_batch_short_of_a_refused_message_says_why() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // A damaged message the server cannot return.
        unfetchable_uids: vec![10],
        ..FixtureSetup::default()
    });
    let batch = received_batch(load_inbox(&fixture));
    assert_eq!(
        batch
            .messages
            .iter()
            .map(|m| &m.identity)
            .collect::<Vec<_>>(),
        [&MessageIdentity::ImapUid(20)]
    );
    let Some(IncompleteList::ServerRefused { reply, .. }) = batch.incomplete else {
        panic!("the list is not marked as refused: {:?}", batch.incomplete);
    };
    assert_eq!(reply, "Some messages could not be FETCHed");
}

/// Manual acceptance of the whole chain against a running `serve_fixture`:
/// the real Online Accounts service provides the settings and credential, and
/// the host's trust store decides the connection, see quickstart.md:
/// `MAILBAG_TEST_ACCOUNT_ID=account_… cargo test --locked -p mailbag-providers online_accounts -- --ignored --nocapture`
///
/// `MAILBAG_IMAP_EXPECT` is `success`, `rejected` or `no-encryption`.
#[test]
#[ignore = "needs a disposable Online Accounts account and a running serve_fixture"]
fn online_accounts_settings_load_the_inbox() {
    assert!(
        !test_certificates_trusted(),
        "run this test by its own filter: another test replaced the trust database"
    );
    let account_id = AccountId::try_from(
        std::env::var("MAILBAG_TEST_ACCOUNT_ID")
            .expect("set MAILBAG_TEST_ACCOUNT_ID to the disposable account")
            .as_str(),
    )
    .expect("account id");
    let loaded = run_on_context(load_with_online_accounts(account_id));
    match (std::env::var("MAILBAG_IMAP_EXPECT").as_deref(), loaded) {
        (Ok("success") | Err(_), Ok(LoadResult::Stored { .. })) => {
            println!("loaded and stored the Inbox");
        }
        (Ok("rejected"), Ok(LoadResult::Failed(failure))) => {
            assert_eq!(
                failure.kind,
                FailureKind::ServerStepFailed(mailbag_domain::ServerStep::SecureConnection)
            );
            println!("refused at the secure-connection step, so no password was sent");
        }
        (Ok("no-encryption"), Err(error)) => {
            assert_eq!(error, goa_adapter::AccessError::NoEncryption);
            println!("refused for its encryption setting, without requesting the password");
        }
        (expectation, loaded) => {
            panic!("expected {expectation:?}, got {loaded:?}");
        }
    }
}

/// Reads the account's settings and credential from Online Accounts, then loads
/// its Inbox on the mail worker, as Refresh Inbox does. An account Online
/// Accounts cannot give settings for never reaches the worker.
async fn load_with_online_accounts(
    account_id: AccountId,
) -> Result<LoadResult, goa_adapter::AccessError> {
    let observed_account = account_id.clone();
    let (observed, observations) = async_channel::unbounded();
    let accounts = goa_adapter::GoaAdapter::start(move |update| {
        observed
            .try_send(update.accounts.contains_key(&observed_account))
            .ok();
    });
    while !observations.recv().await.expect("an account update") {}
    let (reported, access_results) = async_channel::bounded(1);
    let _request = accounts.request_imap_access(&account_id, move |access| {
        reported.try_send(access).ok();
    });
    let access = access_results
        .recv()
        .await
        .expect("the access request reports its result");
    accounts.stop();
    let access = access?;
    let (finished, outcomes) = async_channel::bounded(1);
    let worker = MailWorker::new(Arc::new(Store::in_memory()));
    let _load = worker.load_inbox(LoadKind::GenericImap(access), move |outcome| {
        finished.try_send(outcome).ok();
    });
    Ok(outcomes.recv().await.expect("the load reports its outcome"))
}

/// Runs a load as the window starts it, with its write into `store`, and
/// returns the record of the test thread and the worker, which inherits the
/// dispatcher started here.
fn load_inbox_with_account(
    access: ImapAccess,
    store: &Arc<Store>,
    level: tracing::Level,
) -> (LoadResult, CapturedRecord) {
    let record = CapturedRecord::start(level);
    let outcome = load_with_kind(LoadKind::GenericImap(access), store);
    (outcome, record)
}

#[test]
fn a_refused_sign_in_is_one_error_line_of_the_load() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    let mut access = account_access(&fixture);
    access.credential = ImapCredential::Password("wrong password".to_owned());
    let store = Arc::new(Store::in_memory());
    let (outcome, record) = load_inbox_with_account(access, &store, tracing::Level::DEBUG);
    let text = record.text();
    assert!(matches!(outcome, LoadResult::Failed(_)), "{outcome:?}");
    let errors = record.lines_at("ERROR");
    assert_eq!(errors.len(), 1, "{text}");
    assert!(errors[0].contains("cause=ServerRejectedSignIn"), "{text}");
    assert!(!text.contains("wrong password"), "{text}");
}

#[test]
fn no_private_value_reaches_the_record_at_any_level() {
    for level in [tracing::Level::INFO, tracing::Level::DEBUG] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: vec![FixtureMessage::with_private_markers(10)],
            ..FixtureSetup::default()
        });
        let store = Arc::new(Store::in_memory());
        let (outcome, record) = load_inbox_with_account(account_access(&fixture), &store, level);
        let text = record.text();
        // The markers were read and stored, so the record had the chance to
        // leak them.
        assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
        let stored = stored_messages(&store, "synthetic-account");
        assert_eq!(text_of(&stored[0].content), "marker-body-text");
        assert_eq!(stored[0].fields.subject.as_deref(), Some("marker-subject"));
        for marker in PRIVATE_MARKERS {
            assert!(
                !text.contains(marker),
                "{level:?}: {marker} reached the record:\n{text}"
            );
        }
        if level == tracing::Level::INFO {
            // A folder name, a host and a message identifier never reach info.
            for detail in ["INBOX", "localhost", "uid="] {
                assert!(!text.contains(detail), "{detail} at info:\n{text}");
            }
        } else {
            // Debug names the server and the message, so the check above ran
            // on a record that could have carried them.
            for detail in ["localhost", "uid="] {
                assert!(text.contains(detail), "{detail} missing at debug:\n{text}");
            }
            assert!(text.contains("text part left out as a file"), "{text}");
        }
    }
}

/// A server that offers the token mechanism, with Gmail's attributes on every
/// message, and an account that signs in with a token.
fn gmail_fixture(messages: Vec<FixtureMessage>) -> ImapFixture {
    let messages = messages
        .into_iter()
        .map(|message| {
            let uid = message.uid;
            message.with_gmail_attributes(u64::from(uid) * 1_000, &["\\Important", "Счета"])
        })
        .collect();
    ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        messages,
        ..FixtureSetup::default()
    })
}

fn gmail_access(fixture: &ImapFixture) -> ImapAccess {
    ImapAccess {
        credential: ImapCredential::AccessToken(TEST_ACCESS_TOKEN.to_owned()),
        ..account_access(fixture)
    }
}

#[test]
fn a_gmail_batch_carries_the_message_identifier_and_labels_of_every_row() {
    let fixture = gmail_fixture(plain_messages(2));
    let batch = received_batch(load_gmail(&fixture));
    let carried: Vec<Option<GmailRow>> = batch
        .messages
        .iter()
        .map(|message| message.gmail.clone())
        .collect();
    assert_eq!(
        carried,
        [20_u32, 10].map(|uid| Some(GmailRow {
            message_id: u64::from(uid) * 1_000,
            labels: vec!["\\Important".to_owned(), "Счета".to_owned()],
        }))
    );
}

#[test]
fn the_gmail_load_offers_utf8_names_and_names_itself_before_the_row_fetch() {
    let fixture = gmail_fixture(plain_messages(1));
    received_batch(load_gmail(&fixture));
    let commands = fixture.log().commands;
    let position = |name: &str| commands.iter().position(|command| command == name);
    assert!(
        position("ENABLE") > position("AUTHENTICATE"),
        "{commands:?}"
    );
    assert!(position("ID") > position("AUTHENTICATE"), "{commands:?}");
    assert!(position("ENABLE") < position("FETCH"), "{commands:?}");
    assert!(position("ID") < position("FETCH"), "{commands:?}");
    assert!(!commands.contains(&"LOGIN".to_owned()), "{commands:?}");
    assert_eq!(fixture.log().sign_in_mechanisms, ["XOAUTH2"]);
}

#[test]
fn the_record_names_gmails_fields_and_never_the_token() {
    let fixture = gmail_fixture(plain_messages(1));
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    received_batch(load_gmail(&fixture));
    let text = record.text();
    assert!(text.contains("gmail_message_id=10000"), "{text}");
    assert!(text.contains("Important"), "{text}");
    assert!(!text.contains(TEST_ACCESS_TOKEN), "{text}");
}

/// The Generic IMAP load asks Gmail's server for none of it.
#[test]
fn a_generic_imap_load_sends_no_gmail_command_and_carries_no_gmail_fields() {
    let fixture = gmail_fixture(plain_messages(1));
    let batch = received_batch(load_inbox(&fixture));
    assert_eq!(batch.messages[0].gmail, None);
    let commands = fixture.log().commands;
    assert!(!commands.contains(&"ENABLE".to_owned()), "{commands:?}");
    assert!(!commands.contains(&"ID".to_owned()), "{commands:?}");
}

/// Text acquisition is the shared step, so Gmail reads the same parts and
/// gives the same explanations as a Generic IMAP load (T025).
#[test]
fn gmail_reads_the_same_text_parts_and_gives_the_same_explanations() {
    let messages = vec![
        FixtureMessage::plain_text(10, "Plain text"),
        FixtureMessage::multipart(20, &[("html", "<p>Only HTML</p>")]),
        FixtureMessage::with_private_markers(30),
    ];
    let gmail = gmail_fixture(messages.clone());
    let generic = ImapFixture::start(FixtureSetup {
        messages,
        ..FixtureSetup::default()
    });
    let by_gmail = received_batch(load_gmail(&gmail));
    let by_imap = received_batch(load_inbox(&generic));
    let contents = |batch: &ReceivedBatch| {
        batch
            .messages
            .iter()
            .map(|message| message.content.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(contents(&by_gmail), contents(&by_imap));
    let sections = |fixture: &ImapFixture| {
        fixture
            .log()
            .fetches
            .iter()
            .flat_map(|fetch| fetch.items.clone())
            .filter(|item| item.starts_with("BODY.PEEK["))
            .collect::<Vec<_>>()
    };
    assert_eq!(sections(&gmail), sections(&generic));
}

/// The Microsoft 365 load, with the scripted service in place of Microsoft
/// Graph.
fn microsoft365_kind(service: &graph_service::ScriptedService) -> LoadKind {
    LoadKind::Microsoft365 {
        access: GraphAccess {
            account_id: AccountId::try_from("synthetic-microsoft365").unwrap(),
            access_token: graph_service::TEST_ACCESS_TOKEN.to_owned(),
        },
        service_url: service.url().to_owned(),
    }
}

/// Runs the Microsoft 365 load sequence to its end, without the worker and the
/// store.
fn load_microsoft365(
    service: &graph_service::ScriptedService,
) -> Result<ReceivedBatch, GraphError> {
    let LoadKind::Microsoft365 {
        access,
        service_url,
    } = microsoft365_kind(service)
    else {
        unreachable!("a Microsoft 365 load")
    };
    run_on_context(load_microsoft365_inbox(access, &service_url))
}

#[test]
fn a_microsoft_365_load_publishes_the_services_messages_and_text() {
    let service = graph_service::ScriptedService::start(graph_service::ScriptedAnswer::inbox(3));
    let batch = received_batch(load_microsoft365(&service));
    assert_eq!(
        batch.account_id,
        AccountId::try_from("synthetic-microsoft365").unwrap()
    );
    assert_eq!(batch.incomplete, None);
    let summary: Vec<_> = batch
        .messages
        .iter()
        .map(|message| {
            (
                &message.identity,
                &message.fields,
                message.internal_date,
                message.seen,
                &message.content,
            )
        })
        .collect();
    let fields = |number, to: Option<&str>| DisplayFields {
        subject: Some(format!("Subject {number}")),
        from: Some(format!("Sender {number}")),
        to: to.map(str::to_owned),
    };
    let identity =
        |number| MessageIdentity::GraphImmutableId(graph_service::fixture_immutable_id(number));
    let received = |number| Some(graph_service::fixture_received_unix(number));
    assert_eq!(
        summary,
        [
            (
                &identity(1),
                &fields(1, Some("Recipient")),
                received(1),
                true,
                &ReceivedContent::Text("Text 1".to_owned()),
            ),
            (
                &identity(2),
                &fields(2, None),
                received(2),
                false,
                &ReceivedContent::Text("Text 2".to_owned()),
            ),
            (
                &identity(3),
                &fields(3, Some("Recipient")),
                received(3),
                true,
                &ReceivedContent::TextNotReturned,
            ),
        ]
    );
}

#[test]
fn only_a_microsoft_365_page_cut_short_is_published_as_incomplete() {
    // The Inbox holds more than one batch: the service offers a further page
    // after a full one, which is complete.
    let full =
        graph_service::ScriptedService::start(graph_service::ScriptedAnswer::page_with_more(100));
    let batch = received_batch(load_microsoft365(&full));
    assert_eq!(batch.messages.len(), 100);
    assert_eq!(batch.incomplete, None);

    let cut_short =
        graph_service::ScriptedService::start(graph_service::ScriptedAnswer::page_with_more(1));
    let batch = received_batch(load_microsoft365(&cut_short));
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.incomplete, Some(IncompleteList::MoreAvailable));
}

#[test]
fn a_refused_microsoft_365_request_fails_the_load_after_one_request() {
    let service =
        graph_service::ScriptedService::start(graph_service::ScriptedAnswer::sign_in_refused());
    match load_with_kind(microsoft365_kind(&service), &Arc::new(Store::in_memory())) {
        LoadResult::Failed(failure) => assert_eq!(
            failure.details,
            "Failure: ServiceRejectedSignIn\nStatus: 401\nService code: InvalidAuthenticationToken"
        ),
        other => panic!("a refused request must fail the load: {other:?}"),
    }
    assert_eq!(service.received_requests().len(), 1);
}

#[test]
fn a_microsoft_365_load_names_each_message_and_never_the_token() {
    let service = graph_service::ScriptedService::start(graph_service::ScriptedAnswer::inbox(3));
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    received_batch(load_microsoft365(&service));
    let text = record.text();
    for number in 1..=3 {
        let named = format!(
            r#"immutable_id="{}""#,
            graph_service::fixture_immutable_id(number)
        );
        assert!(
            record
                .lines_at("DEBUG")
                .iter()
                .any(|line| line.contains(&named) && line.contains("received_unix")),
            "{named} is missing: {text}"
        );
    }
    assert!(!text.contains(graph_service::TEST_ACCESS_TOKEN), "{text}");
}

fn received_message(uid: u32, content: ReceivedContent) -> ReceivedMessage {
    ReceivedMessage {
        identity: MessageIdentity::ImapUid(uid),
        fields: DisplayFields::default(),
        internal_date: None,
        seen: false,
        content,
        gmail: None,
    }
}

/// The identity and the content of each stored message, the text without the
/// line end the IMAP fixture adds.
fn stored_summary(messages: &[Message]) -> Vec<(String, ReceivedContent)> {
    messages
        .iter()
        .map(|message| {
            let content = match &message.content {
                ReceivedContent::Text(text) => ReceivedContent::Text(text.trim().to_owned()),
                other => other.clone(),
            };
            (message.identity.clone(), content)
        })
        .collect()
}

#[test]
fn each_providers_load_stores_its_messages_and_reports_them_stored() {
    let imap = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let gmail = gmail_fixture(plain_messages(2));
    let service = graph_service::ScriptedService::start(graph_service::ScriptedAnswer::inbox(3));
    let text = |text: &str| ReceivedContent::Text(text.to_owned());
    let graph = |number| format!("graph:{}", graph_service::fixture_immutable_id(number));
    let loads = [
        (
            LoadKind::GenericImap(account_access(&imap)),
            "synthetic-account",
            vec![
                ("uid:20".to_owned(), text("Text 2")),
                ("uid:10".to_owned(), text("Text 1")),
            ],
        ),
        (
            LoadKind::Gmail(gmail_access(&gmail)),
            "synthetic-account",
            vec![
                ("gmail:20000".to_owned(), text("Text 2")),
                ("gmail:10000".to_owned(), text("Text 1")),
            ],
        ),
        (
            microsoft365_kind(&service),
            "synthetic-microsoft365",
            vec![
                (graph(1), text("Text 1")),
                (graph(2), text("Text 2")),
                (graph(3), ReceivedContent::TextNotReturned),
            ],
        ),
    ];
    for (kind, account, expected) in loads {
        let store = Arc::new(Store::in_memory());
        let outcome = load_with_kind(kind, &store);
        assert!(
            matches!(outcome, LoadResult::Stored { incomplete: None }),
            "{account}: {outcome:?}"
        );
        let stored = stored_messages(&store, account);
        assert_eq!(stored_summary(&stored), expected, "{account}");
    }
}

#[test]
fn a_list_the_server_refused_to_finish_is_stored_with_the_refusal() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        unfetchable_uids: vec![10],
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    match load_with_kind(LoadKind::GenericImap(account_access(&fixture)), &store) {
        LoadResult::Stored {
            incomplete: Some(IncompleteList::ServerRefused { reply, .. }),
        } => assert_eq!(reply, "Some messages could not be FETCHed"),
        other => panic!("the refused list is not reported: {other:?}"),
    }
    assert_eq!(stored_messages(&store, "synthetic-account").len(), 1);
}

#[test]
fn a_store_that_cannot_be_opened_fails_the_load_with_one_error_line() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    // A device stands where the store's directory would be created.
    let store = Arc::new(Store::at(PathBuf::from("/dev/null/mailbag/mail.sqlite")));
    let (outcome, record) =
        load_inbox_with_account(account_access(&fixture), &store, tracing::Level::DEBUG);
    match outcome {
        LoadResult::Failed(failure) => assert_eq!(failure.kind, FailureKind::MailNotSaved),
        other => panic!("an unwritten load must fail: {other:?}"),
    }
    let errors = record.lines_at("ERROR");
    assert_eq!(errors.len(), 1, "{}", record.text());
    assert!(errors[0].contains("cause=MailNotSaved"), "{}", errors[0]);
}

#[test]
fn a_load_cancelled_before_its_write_stores_nothing() {
    let store = Store::in_memory();
    let account = AccountId::try_from("synthetic-account").unwrap();
    let batch = ReceivedBatch {
        account_id: account.clone(),
        messages: vec![received_message(
            10,
            ReceivedContent::Text("Text".to_owned()),
        )],
        incomplete: None,
    };
    let outcome = store_batch(&store, batch, || true);
    assert!(matches!(outcome, LoadResult::Cancelled), "{outcome:?}");
    assert_eq!(store.read_inbox(&account), Ok(None));
}

#[test]
fn unreadable_content_and_a_refused_list_each_warn_without_server_text() {
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    let batch = ReceivedBatch {
        account_id: AccountId::try_from("account_1726920000_0").unwrap(),
        incomplete: Some(IncompleteList::ServerRefused {
            reply: "private refusal text".to_owned(),
            code: Some("LIMIT".to_owned()),
        }),
        messages: vec![
            received_message(30, ReceivedContent::Text("Text".to_owned())),
            // Not supported by design, so counted at info and not warned about.
            received_message(
                20,
                ReceivedContent::Explained(ContentExplanation::NoPlainText { has_html: true }),
            ),
            received_message(
                10,
                ReceivedContent::Explained(ContentExplanation::UnknownCharset("x".to_owned())),
            ),
        ],
    };
    store_batch(&Store::in_memory(), batch, || false);
    let text = record.text();
    let warnings = record.lines_at("WARN");
    assert_eq!(warnings.len(), 2, "{text}");
    assert!(warnings[0].contains("messages=1"), "{}", warnings[0]);
    assert!(warnings[1].contains(r#"code="LIMIT""#), "{}", warnings[1]);
    assert!(!text.contains("private refusal text"), "{text}");
}
