// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::test_record::CapturedRecord;
use crate::worker::{LoadKind, MailWorker, report_outcome};
use goa_adapter::{AccountId, GraphAccess, ImapAccess, ImapCredential, ImapEncryption};
use mailbag_content::{ContentExplanation, DisplayFields};
use mailbag_graph::{GraphFailure, test_server as graph_service};
use mailbag_imap::{
    GmailRow,
    test_server::{
        FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture, PRIVATE_MARKERS,
        TEST_ACCESS_TOKEN, TEST_LOGIN, TEST_PASSWORD, test_certificates_trusted,
    },
};
use std::time::{Duration, Instant};

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

/// Runs a Generic IMAP load to its end, as the window would.
fn load_inbox(fixture: &ImapFixture) -> LoadOutcome {
    load_with_kind(LoadKind::GenericImap(account_access(fixture)))
}

/// Runs one load sequence to its end, as the window would.
fn load_with_kind(kind: LoadKind) -> LoadOutcome {
    run_on_context(async {
        let worker = MailWorker::new();
        let (sender, outcomes) = async_channel::bounded(1);
        let _handle = worker.load_inbox(kind, move |outcome| {
            sender.try_send(outcome).ok();
        });
        outcomes.recv().await.expect("the load reports its outcome")
    })
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

fn published_batch(outcome: LoadOutcome) -> ReceivedBatch {
    match outcome {
        LoadOutcome::Loaded(batch) => batch,
        other => panic!("the load published no batch: {other:?}"),
    }
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
        let batch = published_batch(load_inbox(&fixture));
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
    let batch = published_batch(load_inbox(&fixture));
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
            &ReceivedContent::Explained(ContentExplanation::UnreadableStructure)
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
    let batch = published_batch(load_inbox(&fixture));
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
    match load_inbox(&fixture) {
        LoadOutcome::Failed(LoadFailure::Server(failure)) => {
            assert_eq!(
                failure.failure,
                mailbag_imap::ImapFailure::Failed(mailbag_imap::ImapStep::FetchText)
            );
        }
        other => panic!("an interrupted transfer must not publish: {other:?}"),
    }
}

#[test]
fn a_cancelled_load_closes_its_connection_before_it_ends() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        fault: Some((FaultyCommand::Text, FaultKind::Stall)),
        ..FixtureSetup::default()
    });
    run_on_context(async {
        let worker = MailWorker::new();
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
        assert!(matches!(outcome, LoadOutcome::Cancelled), "{outcome:?}");
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
    match load_inbox(&fixture) {
        LoadOutcome::Failed(LoadFailure::Server(failure)) => {
            assert_eq!(failure.failure, mailbag_imap::ImapFailure::InboxChanged);
        }
        other => panic!("an emptied window must not publish a batch: {other:?}"),
    }
}

#[test]
fn text_the_server_does_not_return_keeps_its_row_with_an_explanation() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // The server answers for message 20 without the section it asked for.
        missing_body_uid: Some(20),
        ..FixtureSetup::default()
    });
    let batch = published_batch(load_inbox(&fixture));
    let contents: Vec<(&MessageIdentity, &ReceivedContent)> = batch
        .messages
        .iter()
        .map(|message| (&message.identity, &message.content))
        .collect();
    assert_eq!(
        contents[0],
        (
            &MessageIdentity::ImapUid(20),
            &ReceivedContent::Explained(ContentExplanation::TextNotReturned)
        )
    );
    assert_eq!(text_of(contents[1].1), "Text 1");
}

#[test]
fn a_stopped_worker_ends_the_load_with_a_visible_failure() {
    run_on_context(async {
        // A worker thread that stopped leaves its outcome channel closed.
        let (sender, outcome) = async_channel::bounded::<LoadOutcome>(1);
        drop(sender);
        for reported in [Some(outcome), None] {
            let mut outcome = None;
            report_outcome(reported, |result| outcome = Some(result)).await;
            assert!(
                matches!(outcome, Some(LoadOutcome::WorkerStopped)),
                "{outcome:?}"
            );
        }
    });
}

#[test]
fn the_next_load_starts_a_new_worker_after_one_stopped() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let outcome = run_on_context(async {
        let worker = MailWorker::new();
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
    assert_eq!(published_batch(outcome).messages.len(), 1);
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
    let batch = published_batch(load_inbox(&fixture));
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
    let batch = published_batch(load_inbox(&fixture));
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
    let batch = published_batch(load_inbox(&fixture));
    assert_eq!(
        batch
            .messages
            .iter()
            .map(|m| &m.identity)
            .collect::<Vec<_>>(),
        [&MessageIdentity::ImapUid(20)]
    );
    let Some(IncompleteList::ServerRefused(refusal)) = batch.incomplete else {
        panic!("the list is not marked as refused: {:?}", batch.incomplete);
    };
    assert_eq!(refusal.text, "Some messages could not be FETCHed");
}

/// Manual acceptance of the whole chain against a running `serve_fixture`:
/// the real Online Accounts service provides the settings and credential, and
/// the host's trust store decides the connection, see quickstart.md:
/// `MAILBAG_TEST_ACCOUNT_ID=account_… cargo test --locked -p mailbag online_accounts -- --ignored --nocapture`
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
        (Ok("success") | Err(_), Ok(LoadOutcome::Loaded(batch))) => {
            println!("loaded {} messages", batch.messages.len());
        }
        (Ok("rejected"), Ok(LoadOutcome::Failed(LoadFailure::Server(failure)))) => {
            assert_eq!(
                failure.failure,
                mailbag_imap::ImapFailure::Failed(mailbag_imap::ImapStep::SecureConnection)
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
) -> Result<LoadOutcome, goa_adapter::AccessError> {
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
    let worker = MailWorker::new();
    let _load = worker.load_inbox(LoadKind::GenericImap(access), move |outcome| {
        finished.try_send(outcome).ok();
    });
    Ok(outcomes.recv().await.expect("the load reports its outcome"))
}

/// Runs a load as the window starts it and returns the record of the test
/// thread and the worker, which inherits the dispatcher started here.
fn load_inbox_with_account(
    access: ImapAccess,
    level: tracing::Level,
) -> (LoadOutcome, CapturedRecord) {
    let record = CapturedRecord::start(level);
    let outcome = load_with_kind(LoadKind::GenericImap(access));
    (outcome, record)
}

#[test]
fn a_refused_sign_in_leaves_the_error_line_to_the_load() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    let mut access = account_access(&fixture);
    access.credential = ImapCredential::Password("wrong password".to_owned());
    let (outcome, record) = load_inbox_with_account(access, tracing::Level::DEBUG);
    let text = record.text();
    assert!(matches!(outcome, LoadOutcome::Failed(_)), "{outcome:?}");
    assert!(record.lines_at("ERROR").is_empty(), "{text}");
    assert!(!text.contains("wrong password"), "{text}");
}

#[test]
fn no_private_value_reaches_the_record_at_any_level() {
    for level in [tracing::Level::INFO, tracing::Level::DEBUG] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: vec![FixtureMessage::with_private_markers(10)],
            ..FixtureSetup::default()
        });
        let (outcome, record) = load_inbox_with_account(account_access(&fixture), level);
        let text = record.text();
        // The markers were read, so the record had the chance to leak them.
        let batch = published_batch(outcome);
        assert_eq!(text_of(&batch.messages[0].content), "marker-body-text");
        assert_eq!(
            batch.messages[0].fields.subject.as_deref(),
            Some("marker-subject")
        );
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
    let batch = published_batch(load_with_kind(LoadKind::Gmail(gmail_access(&fixture))));
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
    published_batch(load_with_kind(LoadKind::Gmail(gmail_access(&fixture))));
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
    published_batch(load_with_kind(LoadKind::Gmail(gmail_access(&fixture))));
    let text = record.text();
    assert!(text.contains("gmail_message_id=10000"), "{text}");
    assert!(text.contains("Important"), "{text}");
    assert!(!text.contains(TEST_ACCESS_TOKEN), "{text}");
}

/// The Generic IMAP load asks Gmail's server for none of it.
#[test]
fn a_generic_imap_load_sends_no_gmail_command_and_carries_no_gmail_fields() {
    let fixture = gmail_fixture(plain_messages(1));
    let batch = published_batch(load_with_kind(LoadKind::GenericImap(account_access(
        &fixture,
    ))));
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
    let by_gmail = published_batch(load_with_kind(LoadKind::Gmail(gmail_access(&gmail))));
    let by_imap = published_batch(load_with_kind(LoadKind::GenericImap(account_access(
        &generic,
    ))));
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

/// Runs a Microsoft 365 load against the scripted service, as the window
/// would, with the scripted service in place of Microsoft Graph.
fn load_microsoft365(service: &graph_service::ScriptedService) -> LoadOutcome {
    load_with_kind(LoadKind::Microsoft365 {
        access: GraphAccess {
            account_id: AccountId::try_from("synthetic-microsoft365").unwrap(),
            access_token: graph_service::TEST_ACCESS_TOKEN.to_owned(),
        },
        service_url: service.url().to_owned(),
    })
}

#[test]
fn a_microsoft_365_load_publishes_the_services_messages_and_text() {
    let service = graph_service::ScriptedService::start(graph_service::ScriptedAnswer::inbox(3));
    let batch = published_batch(load_microsoft365(&service));
    assert_eq!(
        batch.account_id,
        AccountId::try_from("synthetic-microsoft365").unwrap()
    );
    assert_eq!(batch.uid_validity, None);
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
                &ReceivedContent::Explained(ContentExplanation::TextNotReturned),
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
    let batch = published_batch(load_microsoft365(&full));
    assert_eq!(batch.messages.len(), 100);
    assert_eq!(batch.incomplete, None);

    let cut_short =
        graph_service::ScriptedService::start(graph_service::ScriptedAnswer::page_with_more(1));
    let batch = published_batch(load_microsoft365(&cut_short));
    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.incomplete, Some(IncompleteList::MoreAvailable));
}

#[test]
fn a_refused_microsoft_365_request_fails_the_load_after_one_request() {
    let service =
        graph_service::ScriptedService::start(graph_service::ScriptedAnswer::sign_in_refused());
    match load_microsoft365(&service) {
        LoadOutcome::Failed(LoadFailure::MicrosoftGraph(error)) => assert_eq!(
            error.failure,
            GraphFailure::Refused {
                status: 401,
                code: Some("InvalidAuthenticationToken".to_owned()),
            }
        ),
        other => panic!("a refused request must fail the load: {other:?}"),
    }
    assert_eq!(service.received_requests().len(), 1);
}

#[test]
fn a_microsoft_365_load_names_each_message_and_never_the_token() {
    let service = graph_service::ScriptedService::start(graph_service::ScriptedAnswer::inbox(3));
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    published_batch(load_microsoft365(&service));
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
