// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::renewal::AccessRenewal;
use crate::store_load::{PortionWriter, store_folder_list};
use crate::test_record::CapturedRecord;
use crate::worker::{LoadKind, MailWorker, report_events};
use goa_adapter::{GraphAccess, ImapAccess, ImapCredential, ImapEncryption};
use mailbag_domain::{
    AccountId, ContentExplanation, DisplayFields, Failure, FailureKind, Folder, FolderPortion,
    FolderRef, FolderRole, FolderState, IncompleteList, Message, ReceivedContent,
};
use mailbag_graph::test_server as graph_service;
use mailbag_imap::test_server::{
    FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture, PRIVATE_MARKERS,
    TEST_ACCESS_TOKEN, TEST_LOGIN, TEST_PASSWORD, test_certificates_trusted,
};
use mailbag_store::StoreWrite;
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

/// The folder `identity` of the test account.
fn folder_of(account: &str, identity: &str) -> FolderRef {
    FolderRef {
        account: AccountId::try_from(account).unwrap(),
        identity: identity.to_owned(),
    }
}

/// Runs one cycle of the kind's Inbox on a new worker into a new store, and
/// returns how it ended with what the Inbox then holds.
fn synchronize_inbox(kind: LoadKind) -> (LoadResult, Vec<Message>) {
    let store = Arc::new(Store::in_memory());
    let inbox = inbox_of(&kind);
    let outcome = load_with_kind(kind, &store);
    let messages = read_stored_messages(&store, &inbox)
        .expect("the store reads")
        .unwrap_or_default();
    (outcome, messages)
}

/// A cycle of the Generic IMAP Inbox of `fixture`.
fn synchronize_imap_inbox(fixture: &ImapFixture) -> (LoadResult, Vec<Message>) {
    synchronize_inbox(LoadKind::GenericImap(account_access(fixture)))
}

/// A Gmail load of `fixture`, whose renewal nobody answers.
fn gmail_kind(fixture: &ImapFixture) -> LoadKind {
    LoadKind::Gmail {
        access: gmail_access(fixture),
        renewal: AccessRenewal::answered_by_test().0,
    }
}

/// A Generic IMAP message's identity in the fixture's Inbox, whose
/// UIDVALIDITY is 1 unless the setup changes it.
fn imap_identity(uid: u32) -> String {
    format!("imap:INBOX/1/{uid}")
}

fn identities(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.identity.as_str())
        .collect()
}

/// The failure a load ended with.
fn failure_of(outcome: LoadResult) -> Failure {
    match outcome {
        LoadResult::Failed(failure) => failure,
        other => panic!("the load did not fail: {other:?}"),
    }
}

/// The Inbox of the kind's account, by the name its provider opens it by.
fn inbox_of(kind: &LoadKind) -> FolderRef {
    let (account, identity) = match kind {
        LoadKind::GenericImap(access) | LoadKind::Gmail { access, .. } => {
            (&access.account_id, "INBOX")
        }
        LoadKind::Microsoft365 { access, .. } => (&access.account_id, "inbox"),
        LoadKind::PanicsForTest(account) => (account, "INBOX"),
    };
    FolderRef {
        account: account.clone(),
        identity: identity.to_owned(),
    }
}

/// Runs one load of the kind's Inbox on a new worker to its end and its
/// write into `store`, which first gets the Inbox as the account's folder.
fn load_with_kind(kind: LoadKind, store: &Arc<Store>) -> LoadResult {
    let inbox = inbox_of(&kind);
    store_inbox(store, &inbox);
    load_target(kind, LoadTarget::Mailbox(inbox), store)
}

/// Runs one load of `target` on a new worker to its end and its writes into
/// `store`.
fn load_target(kind: LoadKind, target: LoadTarget, store: &Arc<Store>) -> LoadResult {
    let worker = MailWorker::new(store.clone());
    run_on_context(finish_load(&worker, kind, target)).0
}

/// The messages a load stored in `folder`.
fn stored_messages(store: &Store, folder: &FolderRef) -> Vec<Message> {
    read_stored_messages(store, folder)
        .expect("the store reads")
        .expect("the folder was loaded")
}

/// The folder's stored messages whole, as the list reads their rows and the
/// reader their contents; `None` for a folder never loaded.
fn read_stored_messages(
    store: &Store,
    folder: &FolderRef,
) -> Result<Option<Vec<Message>>, Failure> {
    let Some(rows) = store.read_folder_rows(folder)? else {
        return Ok(None);
    };
    let messages = rows
        .into_iter()
        .map(|row| {
            let content = store
                .read_message_content(&folder.account, &row.identity)?
                .expect("a listed message is stored");
            Ok(Message {
                identity: row.identity,
                fields: row.fields,
                received_unix: row.received_unix,
                seen: row.seen,
                content,
            })
        })
        .collect::<Result<_, Failure>>()?;
    Ok(Some(messages))
}

/// Runs one load of `target` on `worker` and waits for its end; returns it
/// with how many portions the load reported stored before.
async fn finish_load(
    worker: &MailWorker,
    kind: LoadKind,
    target: LoadTarget,
) -> (LoadResult, usize) {
    let (sender, events) = async_channel::unbounded();
    let _handle = worker.start_load(kind, target, move |event| {
        sender.try_send(event).ok();
    });
    load_end(&events).await
}

/// The end of a load whose events arrive on `events`, and how many portions
/// it reported stored before.
async fn load_end(events: &async_channel::Receiver<LoadEvent>) -> (LoadResult, usize) {
    let mut portions = 0;
    loop {
        match events.recv().await.expect("the load reports its end") {
            LoadEvent::PortionStored => portions += 1,
            LoadEvent::Finished(outcome) => return (outcome, portions),
        }
    }
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

fn text_of(content: &ReceivedContent) -> &str {
    match content {
        ReceivedContent::Text(text) => text,
        other => panic!("the message has no text: {other:?}"),
    }
}

#[test]
fn a_first_fill_stores_every_message_newest_first_with_its_text() {
    for count in [0, 1, 100, 101] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(count),
            ..FixtureSetup::default()
        });
        let (outcome, stored) = synchronize_imap_inbox(&fixture);
        assert!(
            matches!(outcome, LoadResult::Stored { incomplete: None }),
            "{outcome:?}"
        );
        let expected: Vec<String> = (1..=count)
            .rev()
            .map(|number| imap_identity(number * 10))
            .collect();
        assert_eq!(identities(&stored), expected, "{count} messages");
        for (message, number) in stored.iter().zip((1..=count).rev()) {
            assert_eq!(text_of(&message.content).trim(), format!("Text {number}"));
            assert_eq!(
                message.fields.subject.as_deref(),
                Some(format!("Message {}", number * 10).as_str())
            );
            assert!(message.received_unix.is_some());
        }
        // Rows are fetched highest UID first, a hundred at a time.
        let row_sets: Vec<String> = fixture
            .log()
            .fetches
            .into_iter()
            .filter(|fetch| fetch.items.contains(&"INTERNALDATE".to_owned()))
            .map(|fetch| fetch.message_set)
            .collect();
        let expected_sets = match count {
            0 => Vec::new(),
            1 => vec!["10".to_owned()],
            100 => vec![
                (1..=100)
                    .rev()
                    .map(|n| (n * 10).to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ],
            _ => vec![
                (2..=101)
                    .rev()
                    .map(|n| (n * 10).to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                "10".to_owned(),
            ],
        };
        assert_eq!(row_sets, expected_sets, "{count} messages");
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
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(identities(&stored), [imap_identity(20), imap_identity(10)]);
    assert_eq!(stored[0].content, ReceivedContent::StructureUnreadable);
    assert_eq!(text_of(&stored[1].content), "readable");
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
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(text_of(&stored[0].content), "the readable part");
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

/// A portion whose text transfer broke is not stored; the listing's proof
/// is, and the folder stays "no mail loaded" (spec FR-008, FR-010).
#[test]
fn an_interrupted_transfer_stores_no_part_of_its_portion() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        fault: Some((FaultyCommand::Text, FaultKind::Close)),
        ..FixtureSetup::default()
    });
    let (outcome, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(
        failure_of(outcome).kind,
        FailureKind::ServerStepFailed(mailbag_domain::ServerStep::FetchText)
    );
    assert!(stored.is_empty());
}

#[test]
fn a_cancelled_load_closes_its_connection_before_it_ends() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        fault: Some((FaultyCommand::Text, FaultKind::Stall)),
        ..FixtureSetup::default()
    });
    run_on_context(async {
        let inbox = folder_of("synthetic-account", "INBOX");
        let worker = MailWorker::new(Arc::new(store_with_inbox(&inbox)));
        let (sender, events) = async_channel::unbounded();
        let handle = worker.start_load(
            LoadKind::GenericImap(account_access(&fixture)),
            LoadTarget::Mailbox(inbox),
            move |event| {
                sender.try_send(event).ok();
            },
        );
        // Cancel while the server is stalling on the text command, after
        // the listing, the rows and the structures.
        wait_until(|| fixture.log().fetches.len() == 4).await;
        let cancelled_at = Instant::now();
        drop(handle);
        let (outcome, _) = load_end(&events).await;
        assert!(matches!(outcome, LoadResult::Cancelled), "{outcome:?}");
        // Reported within a second while the server stays silent (spec FR-010).
        assert!(cancelled_at.elapsed() < Duration::from_secs(1));
        // The worker closes the socket before it reports the outcome; the
        // server sees the closed connection as soon as it runs again.
        wait_until(|| fixture.log().closed_connections == 1).await;
    });
}

/// Messages that disappear between the listing and their text are left out;
/// nothing else fails, and the next listing tells whether they are gone.
#[test]
fn messages_that_vanish_before_their_text_are_left_out() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // Another client moves both messages away before their text is read.
        vanishing_text_uids: vec![10, 20],
        ..FixtureSetup::default()
    });
    let (outcome, stored) = synchronize_imap_inbox(&fixture);
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert!(stored.is_empty());
}

#[test]
fn text_the_server_does_not_return_keeps_its_row_with_an_explanation() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        // The server answers for message 20 without the section it asked for.
        missing_body_uid: Some(20),
        ..FixtureSetup::default()
    });
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(identities(&stored), [imap_identity(20), imap_identity(10)]);
    assert_eq!(stored[0].content, ReceivedContent::TextNotReturned);
    assert_eq!(text_of(&stored[1].content), "Text 1");
}

#[test]
fn a_stopped_worker_ends_the_load_with_a_visible_failure() {
    run_on_context(async {
        // A worker thread that stopped leaves its event channel closed.
        let (sender, events) = async_channel::unbounded::<LoadEvent>();
        drop(sender);
        let account_id = AccountId::try_from("synthetic-account").unwrap();
        for reported in [Some(events), None] {
            let mut outcome = None;
            report_events(account_id.clone(), "mailbox", reported, |event| {
                if let LoadEvent::Finished(result) = event {
                    outcome = Some(result);
                }
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
        match finish_load(&worker, panicking, LoadTarget::FolderList)
            .await
            .0
        {
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
        let next_kind = LoadKind::GenericImap(account_access(&fixture));
        let inbox = inbox_of(&next_kind);
        store_inbox(&store, &inbox);
        let (next, _) = finish_load(&worker, next_kind, LoadTarget::Mailbox(inbox)).await;
        assert!(matches!(next, LoadResult::Stored { .. }), "{next:?}");
    });
    let inbox = folder_of("synthetic-account", "INBOX");
    assert_eq!(stored_messages(&store, &inbox).len(), 1);
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

        let inbox = folder_of("synthetic-account", "INBOX");
        store_inbox(&store, &inbox);
        finish_load(
            &worker,
            LoadKind::GenericImap(account_access(&fixture)),
            LoadTarget::Mailbox(inbox),
        )
        .await
        .0
    });
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    let inbox = folder_of("synthetic-account", "INBOX");
    assert_eq!(stored_messages(&store, &inbox).len(), 1);
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
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(text_of(&stored[0].content).trim(), "Text inside related");
}

/// A message listed and then gone before its row, even with a flag change
/// reported for it meanwhile, keeps no row (spec Edge Cases).
#[test]
fn a_message_gone_between_the_listing_and_its_row_keeps_no_row() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        vanishing_uid: Some(10),
        flag_change_uids: vec![10],
        ..FixtureSetup::default()
    });
    let (outcome, stored) = synchronize_imap_inbox(&fixture);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(identities(&stored), [imap_identity(20)]);
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
/// its Inbox on the mail worker, as Refresh Mailbox does. An account Online
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
    let kind = LoadKind::GenericImap(access);
    let inbox = inbox_of(&kind);
    let worker = MailWorker::new(Arc::new(store_with_inbox(&inbox)));
    Ok(finish_load(&worker, kind, LoadTarget::Mailbox(inbox))
        .await
        .0)
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
    let kind = LoadKind::GenericImap(access);
    let inbox = inbox_of(&kind);
    let outcome = load_target(kind, LoadTarget::Mailbox(inbox), store);
    (outcome, record)
}

#[test]
fn a_refused_sign_in_is_one_error_line_of_the_load() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    let mut access = account_access(&fixture);
    access.credential = ImapCredential::Password("wrong password".to_owned());
    let store = Arc::new(Store::in_memory());
    // A failed load leaves the Inbox an earlier load stored as it was
    // (specs/007-mail-storage FR-004).
    let inbox = folder_of(access.account_id.as_str(), "INBOX");
    store_inbox(&store, &inbox);
    store_completed_cycle(&store, &inbox, &[stored_earlier_message()], || false).unwrap();
    let (outcome, record) = load_inbox_with_account(access, &store, tracing::Level::DEBUG);
    let text = record.text();
    assert!(matches!(outcome, LoadResult::Failed(_)), "{outcome:?}");
    let errors = record.lines_at("ERROR");
    assert_eq!(errors.len(), 1, "{text}");
    assert!(errors[0].contains("cause=ServerRejectedSignIn"), "{text}");
    assert!(!text.contains("wrong password"), "{text}");
    assert_eq!(
        read_stored_messages(&store, &inbox).unwrap(),
        Some(vec![stored_earlier_message()])
    );
}

/// A message an earlier load stored.
fn stored_earlier_message() -> Message {
    Message {
        identity: "uid:1".to_owned(),
        fields: DisplayFields::default(),
        received_unix: None,
        seen: true,
        content: ReceivedContent::Text("Stored earlier".to_owned()),
    }
}

#[test]
fn no_private_value_reaches_the_record_at_any_level() {
    for level in [tracing::Level::INFO, tracing::Level::DEBUG] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: vec![FixtureMessage::with_private_markers(10)],
            ..FixtureSetup::default()
        });
        let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
        let (outcome, record) = load_inbox_with_account(account_access(&fixture), &store, level);
        let text = record.text();
        // The markers were read and stored, so the record had the chance to
        // leak them.
        assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
        let stored = stored_messages(&store, &folder_of("synthetic-account", "INBOX"));
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
    ImapFixture::start(gmail_fixture_setup(messages))
}

fn gmail_fixture_setup(messages: Vec<FixtureMessage>) -> FixtureSetup {
    let messages = messages
        .into_iter()
        .map(|message| {
            let uid = message.uid;
            message.with_gmail_attributes(u64::from(uid) * 1_000, &["\\Important", "Счета"])
        })
        .collect();
    FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        capabilities_after_sign_in: vec!["UTF8=ACCEPT"],
        messages,
        ..FixtureSetup::default()
    }
}

fn gmail_access(fixture: &ImapFixture) -> ImapAccess {
    ImapAccess {
        credential: ImapCredential::AccessToken(TEST_ACCESS_TOKEN.to_owned()),
        ..account_access(fixture)
    }
}

#[test]
fn a_gmail_message_is_stored_under_its_gmail_identifier() {
    let fixture = gmail_fixture(plain_messages(2));
    let (outcome, stored) = synchronize_inbox(gmail_kind(&fixture));
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert_eq!(identities(&stored), ["gmail:20000", "gmail:10000"]);
}

/// A message Gmail listed without its identifier is not stored under a
/// guessed identity (research §4).
#[test]
fn a_gmail_message_listed_without_its_identifier_is_left_out() {
    let mut messages = plain_messages(2)
        .into_iter()
        .map(|message| {
            let uid = message.uid;
            message.with_gmail_attributes(u64::from(uid) * 1_000, &[])
        })
        .collect::<Vec<_>>();
    messages[0].gmail_message_id = None;
    let fixture = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        messages,
        ..FixtureSetup::default()
    });
    let record = CapturedRecord::start(tracing::Level::INFO);
    let (_, stored) = synchronize_inbox(gmail_kind(&fixture));
    assert_eq!(identities(&stored), ["gmail:20000"]);
    let warnings = record.lines_at("WARN");
    assert!(
        warnings.iter().any(|line| line.contains("uid=10")),
        "{}",
        record.text()
    );
}

#[test]
fn the_gmail_load_offers_utf8_names_and_names_itself_before_the_listing() {
    let fixture = gmail_fixture(plain_messages(1));
    synchronize_inbox(gmail_kind(&fixture));
    let commands = fixture.log().commands;
    let position = |name: &str| commands.iter().position(|command| command == name);
    assert!(
        position("ENABLE") > position("AUTHENTICATE"),
        "{commands:?}"
    );
    assert!(position("ID") > position("AUTHENTICATE"), "{commands:?}");
    assert!(position("ENABLE") < position("UID FETCH"), "{commands:?}");
    assert!(position("ID") < position("UID FETCH"), "{commands:?}");
    assert!(!commands.contains(&"LOGIN".to_owned()), "{commands:?}");
    assert_eq!(fixture.log().sign_in_mechanisms, ["XOAUTH2"]);
}

#[test]
fn the_record_names_gmails_fields_and_never_the_token() {
    let fixture = gmail_fixture(plain_messages(1));
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    synchronize_inbox(gmail_kind(&fixture));
    let text = record.text();
    assert!(text.contains("gmail_message_id=10000"), "{text}");
    assert!(text.contains("Important"), "{text}");
    assert!(!text.contains(TEST_ACCESS_TOKEN), "{text}");
}

/// The Generic IMAP load asks Gmail's server for none of Gmail's own
/// extensions; UTF-8 names follow the server's capabilities on every load.
#[test]
fn a_generic_imap_load_sends_no_gmail_command_and_asks_for_no_gmail_fields() {
    let fixture = gmail_fixture(plain_messages(1));
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(identities(&stored), [imap_identity(10)]);
    let log = fixture.log();
    assert!(
        !log.commands.contains(&"ID".to_owned()),
        "{:?}",
        log.commands
    );
    assert!(
        log.fetches
            .iter()
            .all(|fetch| !fetch.items.iter().any(|item| item.starts_with("X-GM"))),
        "{:?}",
        log.fetches
    );
}

/// Text acquisition is the shared step, so Gmail reads the same parts and
/// gives the same explanations as a Generic IMAP load.
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
    let (_, by_gmail) = synchronize_inbox(gmail_kind(&gmail));
    let (_, by_imap) = synchronize_imap_inbox(&generic);
    let contents = |messages: &[Message]| {
        messages
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
            .filter(|item| item.starts_with("BODY.PEEK[") && !item.contains("HEADER.FIELDS"))
            .collect::<Vec<_>>()
    };
    assert_eq!(sections(&gmail), sections(&generic));
}

/// A Microsoft 365 load of the scripted service's Inbox with `token`, whose
/// renewal Online Accounts answers once with `renewed`, as a thread of its
/// own stands in for GTK's context.
fn microsoft365_kind_with(
    service: &graph_service::ScriptedService,
    token: &str,
    renewed: Option<&str>,
) -> LoadKind {
    let (renewal, requests) = AccessRenewal::answered_by_test();
    let account_id = AccountId::try_from("synthetic-microsoft365").unwrap();
    let mut renewed = renewed.map(|token| GraphAccess {
        account_id: account_id.clone(),
        access_token: token.to_owned(),
    });
    std::thread::spawn(move || {
        while let Ok(reply) = requests.recv_blocking() {
            reply.send_blocking(renewed.take()).ok();
        }
    });
    LoadKind::Microsoft365 {
        access: GraphAccess {
            account_id,
            access_token: token.to_owned(),
        },
        service_url: service.url().to_owned(),
        renewal,
    }
}

/// The Microsoft 365 load of the scripted service with the test token, whose
/// renewal nobody answers.
fn microsoft365_kind(service: &graph_service::ScriptedService) -> LoadKind {
    microsoft365_kind_with(service, graph_service::TEST_ACCESS_TOKEN, None)
}

fn delta_page(
    entries: Vec<serde_json::Value>,
    next: graph_service::ScriptedNext,
) -> graph_service::ScriptedPage {
    graph_service::ScriptedPage::Entries { entries, next }
}

/// A scripted mailbox: its delta pages by token and its messages in the
/// Inbox, as `GET /me/messages/{id}` answers them.
fn graph_mailbox(
    pages: Vec<(&str, graph_service::ScriptedPage)>,
    messages: Vec<serde_json::Value>,
) -> graph_service::ScriptedChanges {
    graph_service::ScriptedChanges {
        pages: pages
            .into_iter()
            .map(|(token, page)| (token.to_owned(), page))
            .collect(),
        messages,
        token_accepted_requests: None,
    }
}

fn inbox_messages(numbers: &[u32]) -> Vec<serde_json::Value> {
    numbers
        .iter()
        .map(|number| graph_service::stored_message(*number, "inbox"))
        .collect()
}

fn graph_identity(number: u32) -> String {
    format!("graph:{}", graph_service::fixture_immutable_id(number))
}

/// The paths the service was asked for, in order.
fn graph_paths(service: &graph_service::ScriptedService) -> Vec<String> {
    service
        .received_requests()
        .into_iter()
        .map(|request| request.path)
        .collect()
}

/// SC-004: a first fill page by page, texts by each page's date range and
/// only within 30 days, the place saved with each page.
#[test]
fn a_microsoft_365_first_fill_stores_page_by_page_with_recent_texts() {
    use graph_service::{ScriptedNext::*, delta_entry};
    // Message 800 arrived 33 days before the others.
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![
            (
                "first",
                delta_page(vec![delta_entry(1), delta_entry(2)], More("page-2")),
            ),
            (
                "page-2",
                delta_page(vec![delta_entry(3), delta_entry(800)], Done("round-1")),
            ),
        ],
        inbox_messages(&[1, 2, 3, 800]),
    ));
    let store = Arc::new(Store::in_memory());
    let kind = microsoft365_kind(&service);
    let inbox = inbox_of(&kind);
    store_inbox(&store, &inbox);
    let (outcome, stored, portions) = synchronize_kind_again(kind, &store);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(portions, 2);
    assert_eq!(identities(&stored), [1, 2, 3, 800].map(graph_identity));
    assert_eq!(text_of(&stored[0].content), "Text 1");
    // Message 3 has no body in the service.
    assert_eq!(stored[2].content, ReceivedContent::TextNotReturned);
    assert_eq!(stored[3].content, ReceivedContent::NotDownloaded);
    let state = store.read_folder_sync(&inbox).unwrap().state;
    assert!(state.synchronized);
    assert!(
        state
            .server_position
            .is_some_and(|link| link.ends_with("$deltatoken=round-1"))
    );
    // One delta page and one text range per page; the old message's date
    // is outside the second range.
    let requests = service.received_requests();
    let text_ranges: Vec<&String> = requests
        .iter()
        .filter(|request| request.query.contains("$filter="))
        .map(|request| &request.query)
        .collect();
    assert_eq!(text_ranges.len(), 2, "{requests:?}");
}

/// SC-005 and research §5: a first fill stopped after a page continues from
/// its saved place without reading that page again, then reads one more
/// round, which reports the changes made during the pause.
#[test]
fn a_continued_microsoft_365_first_fill_reads_one_more_round() {
    use graph_service::{ScriptedNext::*, delta_entry};
    let mut mailbox = graph_mailbox(
        vec![
            ("first", delta_page(vec![delta_entry(1)], More("page-2"))),
            ("page-2", delta_page(vec![delta_entry(2)], Done("round-1"))),
            (
                "round-1",
                delta_page(
                    vec![
                        serde_json::json!({"id": graph_service::fixture_immutable_id(2), "isRead": true}),
                    ],
                    Done("round-2"),
                ),
            ),
        ],
        inbox_messages(&[1, 2]),
    );
    // The token runs out after the first page and its texts.
    mailbox.token_accepted_requests = Some(2);
    let service = graph_service::ScriptedService::start_with_changes(mailbox);
    let store = Arc::new(Store::in_memory());
    let inbox = folder_of("synthetic-microsoft365", "inbox");
    store_inbox(&store, &inbox);
    let (outcome, stored, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
    assert_eq!(failure_of(outcome).kind, FailureKind::ServiceRejectedSignIn);
    assert_eq!(identities(&stored), [graph_identity(1)]);
    let place = store.read_folder_sync(&inbox).unwrap().state;
    assert!(!place.synchronized);
    assert!(
        place
            .server_position
            .is_some_and(|link| link.ends_with("$skiptoken=page-2"))
    );
    let asked_before = service.received_requests().len();
    let kind = microsoft365_kind_with(&service, "another-token", None);
    let (outcome, stored, _) = synchronize_kind_again(kind, &store);
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert_eq!(identities(&stored), [graph_identity(1), graph_identity(2)]);
    // Message 2 was unread when listed and read in the round after.
    assert!(stored[1].seen);
    let later: Vec<String> = service.received_requests()[asked_before..]
        .iter()
        .map(|request| request.query.clone())
        .collect();
    assert!(later[0].contains("$skiptoken=page-2"), "{later:?}");
    assert!(
        later
            .iter()
            .any(|query| query.contains("$deltatoken=round-1")),
        "{later:?}"
    );
    let state = store.read_folder_sync(&inbox).unwrap().state;
    assert!(state.synchronized);
    assert!(
        state
            .server_position
            .is_some_and(|link| link.ends_with("$deltatoken=round-2"))
    );
}

/// A first fill that completed, then a round of changes into `store`.
fn graph_round(
    round: Vec<serde_json::Value>,
    messages: Vec<serde_json::Value>,
    store: &Arc<Store>,
) -> (graph_service::ScriptedService, LoadResult, Vec<Message>) {
    use graph_service::{ScriptedNext::*, delta_entry};
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![
            (
                "first",
                delta_page((1..=3).map(delta_entry).collect(), Done("round-1")),
            ),
            ("round-1", delta_page(round, Done("round-2"))),
        ],
        messages,
    ));
    store_inbox(store, &folder_of("synthetic-microsoft365", "inbox"));
    synchronize_kind_again(microsoft365_kind(&service), store);
    let (outcome, stored, _) = synchronize_kind_again(microsoft365_kind(&service), store);
    (service, outcome, stored)
}

/// SC-004: removals, repeated and reordered entries, a partial entry that
/// changes a stored message's fields, and a read state of a message the
/// store lacks.
#[test]
fn a_microsoft_365_round_applies_removals_partial_entries_and_arrivals() {
    let id = graph_service::fixture_immutable_id;
    let mut renamed = graph_service::stored_message(3, "inbox");
    renamed["subject"] = "Renamed".into();
    let mut messages = inbox_messages(&[1, 2, 4]);
    messages.push(renamed);
    let store = Arc::new(Store::in_memory());
    let (_, outcome, stored) = graph_round(
        vec![
            serde_json::json!({"id": id(2), "isRead": false}),
            serde_json::json!({"id": id(1), "@removed": {"reason": "deleted"}}),
            serde_json::json!({"id": id(3), "subject": "Renamed"}),
            // Repeated, the later entry wins.
            serde_json::json!({"id": id(2), "isRead": true}),
            // A read state of a message the store lacks: it is read in full.
            serde_json::json!({"id": id(4), "isRead": false}),
        ],
        messages,
        &store,
    );
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert_eq!(
        identities(&stored),
        [graph_identity(2), graph_identity(3), graph_identity(4)]
    );
    assert!(stored[0].seen);
    assert_eq!(stored[1].fields.subject.as_deref(), Some("Renamed"));
    // The stored content stays with the renamed message: the service had
    // no text for message 3.
    assert_eq!(stored[1].content, ReceivedContent::TextNotReturned);
    assert_eq!(text_of(&stored[2].content), "Text 4");
}

/// Research §5: a message moved from the Inbox to Archive and marked unread
/// there; an older read-state entry in the Inbox's round must not mark it
/// read. The message is read again and, being in Archive now, left alone.
#[test]
fn an_entry_for_a_message_another_folder_holds_is_read_again_first() {
    use graph_service::{ScriptedNext::*, delta_entry};
    let id = graph_service::fixture_immutable_id;
    let mut messages = inbox_messages(&[1, 3]);
    messages.push(graph_service::stored_message(2, "archive"));
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![
            (
                "first",
                delta_page((1..=3).map(delta_entry).collect(), Done("round-1")),
            ),
            (
                "round-1",
                delta_page(
                    vec![serde_json::json!({"id": id(2), "isRead": true})],
                    Done("round-2"),
                ),
            ),
        ],
        messages,
    ));
    let store = Arc::new(Store::in_memory());
    let account = AccountId::try_from("synthetic-microsoft365").unwrap();
    let folders = ["inbox", "archive"].map(|identity| Folder {
        identity: identity.to_owned(),
        name: identity.to_owned(),
        parent: None,
        role: None,
        selectable: true,
    });
    store.replace_folders(&account, &folders, || false).unwrap();
    synchronize_kind_again(microsoft365_kind(&service), &store);
    // Archive's own cycle related the message there, unread.
    let archive = folder_of("synthetic-microsoft365", "archive");
    let archived = FolderPortion {
        known_arrived: vec![(graph_identity(2), false)],
        ..FolderPortion::default()
    };
    store.store_portion(&archive, &archived, || false).unwrap();
    synchronize_kind_again(microsoft365_kind(&service), &store);
    assert!(!stored_messages(&store, &archive)[0].seen);
    assert!(
        graph_paths(&service).contains(&format!("/me/messages/{}", id(2))),
        "{:?}",
        graph_paths(&service)
    );
}

/// Research §5: the arrivals of a round of changes are scattered in time, so
/// each text is read by its identifier, never by a range between them.
#[test]
fn a_rounds_arrivals_get_their_texts_one_by_one() {
    use graph_service::delta_entry;
    let store = Arc::new(Store::in_memory());
    // Message 600 arrived 25 days before message 4.
    let (service, _, stored) = graph_round(
        vec![delta_entry(4), delta_entry(600)],
        inbox_messages(&[1, 2, 3, 4, 600]),
        &store,
    );
    assert_eq!(stored.len(), 5);
    assert!(stored.iter().all(|message| matches!(
        message.content,
        ReceivedContent::Text(_) | ReceivedContent::TextNotReturned
    )));
    let requests = service.received_requests();
    let ranges = requests
        .iter()
        .filter(|request| request.query.contains("$filter="))
        .count();
    // Only the first fill's page used a range.
    assert_eq!(ranges, 1, "{requests:?}");
    for number in [4, 600] {
        let path = format!(
            "/me/messages/{}",
            graph_service::fixture_immutable_id(number)
        );
        assert!(graph_paths(&service).contains(&path), "{path}");
    }
}

/// FR-007: a saved position the service rejects starts a full reading that
/// keeps the listed messages and removes the others at its end.
#[test]
fn a_rejected_position_rereads_the_folder_and_removes_what_it_did_not_list() {
    use graph_service::{ScriptedNext::*, delta_entry};
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![(
            "first",
            delta_page(vec![delta_entry(1), delta_entry(2)], Done("round-1")),
        )],
        inbox_messages(&[1, 2]),
    ));
    let store = Arc::new(Store::in_memory());
    let inbox = folder_of("synthetic-microsoft365", "inbox");
    store_inbox(&store, &inbox);
    // A message deleted meanwhile, and a position the service no longer knows.
    let earlier = FolderPortion {
        arrived: vec![Message {
            identity: graph_identity(9),
            ..stored_earlier_message()
        }],
        state: Some(FolderState {
            server_position: Some(format!(
                "{}/me/mailFolders/inbox/messages/delta?$deltatoken=expired",
                service.url()
            )),
            synchronized: true,
        }),
        ..FolderPortion::default()
    };
    store.store_portion(&inbox, &earlier, || false).unwrap();
    let (outcome, stored, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    assert_eq!(identities(&stored), [graph_identity(1), graph_identity(2)]);
    let state = store.read_folder_sync(&inbox).unwrap().state;
    assert!(
        state
            .server_position
            .is_some_and(|link| link.ends_with("$deltatoken=round-1"))
    );
}

/// SC-010: a token refused mid-fill is asked for once more and a different
/// one completes the fill; the same token, or a second refusal, is the
/// refused sign-in.
#[test]
fn a_token_refused_mid_fill_is_renewed_once() {
    use graph_service::{ScriptedNext::*, delta_entry};
    let mailbox = || {
        let mut mailbox = graph_mailbox(
            vec![
                ("first", delta_page(vec![delta_entry(1)], More("page-2"))),
                ("page-2", delta_page(vec![delta_entry(2)], Done("round-1"))),
            ],
            inbox_messages(&[1, 2]),
        );
        mailbox.token_accepted_requests = Some(2);
        mailbox
    };
    for (renewed, completes) in [
        (Some("renewed-token"), true),
        (Some(graph_service::TEST_ACCESS_TOKEN), false),
        (None, false),
    ] {
        let service = graph_service::ScriptedService::start_with_changes(mailbox());
        let store = Arc::new(Store::in_memory());
        store_inbox(&store, &folder_of("synthetic-microsoft365", "inbox"));
        let kind = microsoft365_kind_with(&service, graph_service::TEST_ACCESS_TOKEN, renewed);
        let (outcome, stored, _) = synchronize_kind_again(kind, &store);
        match completes {
            true => {
                assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
                assert_eq!(stored.len(), 2);
            }
            false => assert_eq!(
                failure_of(outcome).kind,
                FailureKind::ServiceRejectedSignIn,
                "{renewed:?}"
            ),
        }
    }
}

#[test]
fn a_refused_microsoft_365_request_fails_the_load_after_one_request() {
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![(
            "first",
            graph_service::ScriptedPage::Refused(graph_service::ScriptedAnswer::sign_in_refused()),
        )],
        Vec::new(),
    ));
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
    use graph_service::{ScriptedNext::*, delta_entry};
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![(
            "first",
            delta_page((1..=3).map(delta_entry).collect(), Done("round-1")),
        )],
        inbox_messages(&[1, 2, 3]),
    ));
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    synchronize_inbox(microsoft365_kind(&service));
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
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![(
            "first",
            delta_page(
                (1..=3).map(graph_service::delta_entry).collect(),
                graph_service::ScriptedNext::Done("round-1"),
            ),
        )],
        inbox_messages(&[1, 2, 3]),
    ));
    let text = |text: &str| ReceivedContent::Text(text.to_owned());
    let graph = |number| format!("graph:{}", graph_service::fixture_immutable_id(number));
    let loads = [
        (
            LoadKind::GenericImap(account_access(&imap)),
            "synthetic-account",
            vec![
                (imap_identity(20), text("Text 2")),
                (imap_identity(10), text("Text 1")),
            ],
        ),
        (
            gmail_kind(&gmail),
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
        let inbox = inbox_of(&kind);
        let outcome = load_with_kind(kind, &store);
        assert!(
            matches!(outcome, LoadResult::Stored { incomplete: None }),
            "{account}: {outcome:?}"
        );
        let stored = stored_messages(&store, &inbox);
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
    let inbox = folder_of("synthetic-account", "INBOX");
    assert_eq!(stored_messages(&store, &inbox).len(), 1);
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

/// A store that holds the account's Inbox as its one folder, not loaded.
fn store_with_inbox(inbox: &FolderRef) -> Store {
    let store = Store::in_memory();
    store_inbox(&store, inbox);
    store
}

/// Stores `inbox` as its account's one folder, not loaded, as a completed
/// folder list leaves it.
fn store_inbox(store: &Store, inbox: &FolderRef) {
    let folder = Folder {
        identity: inbox.identity.clone(),
        name: inbox.identity.clone(),
        parent: None,
        role: None,
        selectable: true,
    };
    store
        .replace_folders(&inbox.account, &[folder], || false)
        .unwrap();
}

/// A message with `content`, as a cycle stores it.
fn message_with(number: u32, content: ReceivedContent) -> Message {
    Message {
        identity: format!("message-{number}"),
        fields: DisplayFields::default(),
        received_unix: None,
        seen: false,
        content,
    }
}

#[test]
fn a_portion_of_a_cancelled_load_is_not_stored() {
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = store_with_inbox(&inbox);
    let (cancel, cancelled) = async_channel::bounded::<()>(1);
    drop(cancel);
    let (events, reported) = async_channel::unbounded();
    let mut portions = PortionWriter::new(&store, inbox.clone(), &cancelled, &events);
    let portion = FolderPortion {
        arrived: vec![message_with(10, ReceivedContent::Text("Text".to_owned()))],
        ..FolderPortion::default()
    };
    assert!(matches!(
        portions.store(&portion),
        Err(LoadResult::Cancelled)
    ));
    assert_eq!(read_stored_messages(&store, &inbox), Ok(None));
    assert!(reported.is_empty());
}

#[test]
fn unreadable_content_and_a_refused_list_each_warn_without_server_text() {
    let inbox = folder_of("account_1726920000_0", "INBOX");
    let store = store_with_inbox(&inbox);
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    let (_cancel, cancelled) = async_channel::bounded::<()>(1);
    let (events, _reported) = async_channel::unbounded();
    let mut portions = PortionWriter::new(&store, inbox, &cancelled, &events);
    let portion = FolderPortion {
        arrived: vec![
            message_with(30, ReceivedContent::Text("Text".to_owned())),
            // Not supported by design, so counted at info and not warned about.
            message_with(
                20,
                ReceivedContent::Explained(ContentExplanation::NoPlainText { has_html: true }),
            ),
            message_with(
                10,
                ReceivedContent::Explained(ContentExplanation::UnknownCharset("x".to_owned())),
            ),
        ],
        ..FolderPortion::default()
    };
    portions.store(&portion).expect("the portion is stored");
    portions.finish(
        3,
        Some(IncompleteList::ServerRefused {
            reply: "private refusal text".to_owned(),
            code: Some("LIMIT".to_owned()),
        }),
    );
    let text = record.text();
    let warnings = record.lines_at("WARN");
    assert_eq!(warnings.len(), 2, "{text}");
    assert!(warnings[0].contains("messages=1"), "{}", warnings[0]);
    assert!(warnings[1].contains(r#"code="LIMIT""#), "{}", warnings[1]);
    assert!(!text.contains("private refusal text"), "{text}");
}

/// Each stored folder's identity, shown name, parent and role, by identity.
fn stored_folders(
    store: &Store,
    account: &str,
) -> Vec<(String, String, Option<String>, Option<FolderRole>)> {
    let account = AccountId::try_from(account).unwrap();
    let mut folders: Vec<_> = store
        .read_folders(&account)
        .expect("the store reads")
        .into_iter()
        .map(|folder| (folder.identity, folder.name, folder.parent, folder.role))
        .collect();
    folders.sort_by(|left, right| left.0.cmp(&right.0));
    folders
}

fn stored(
    identity: &str,
    name: &str,
    parent: Option<&str>,
    role: Option<FolderRole>,
) -> (String, String, Option<String>, Option<FolderRole>) {
    (
        identity.to_owned(),
        name.to_owned(),
        parent.map(str::to_owned),
        role,
    )
}

#[test]
fn each_providers_folder_list_load_stores_its_folders_with_their_roles() {
    let imap = ImapFixture::start(FixtureSetup {
        mailboxes: vec![
            ("\\HasNoChildren", "/", "INBOX"),
            ("\\HasNoChildren \\Sent", "/", "Sent Messages"),
            ("\\HasChildren \\Noselect", "/", "Projects"),
            ("\\HasNoChildren", "/", "Projects/Reports"),
        ],
        ..FixtureSetup::default()
    });
    let gmail = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        mailboxes: vec![
            ("\\HasNoChildren", "/", "INBOX"),
            ("\\HasChildren \\Noselect", "/", "[Gmail]"),
            ("\\HasNoChildren \\Sent", "/", "[Gmail]/Sent Mail"),
            ("\\HasNoChildren", "/", "Work"),
        ],
        ..FixtureSetup::default()
    });
    let service = graph_service::ScriptedService::start_with_folders(
        graph_service::ScriptedAnswer::inbox(1),
        graph_service::ScriptedFolders {
            pages: vec![Ok(vec![
                graph_service::folder_entry("inbox-id", "Incoming", "root-id"),
                graph_service::folder_entry("projects-id", "Projects", "root-id"),
            ])],
            well_known: vec![(
                "inbox",
                graph_service::ScriptedAnswer::folder_id("inbox-id"),
            )],
        },
    );
    let loads = [
        (
            LoadKind::GenericImap(account_access(&imap)),
            "synthetic-account",
            vec![
                stored("INBOX", "INBOX", None, Some(FolderRole::Inbox)),
                stored("Projects", "Projects", None, None),
                stored("Projects/Reports", "Reports", Some("Projects"), None),
                stored(
                    "Sent Messages",
                    "Sent Messages",
                    None,
                    Some(FolderRole::Sent),
                ),
            ],
        ),
        (
            gmail_kind(&gmail),
            "synthetic-account",
            vec![
                stored("INBOX", "INBOX", None, Some(FolderRole::Inbox)),
                stored("Work", "Work", None, None),
                stored(
                    "[Gmail]/Sent Mail",
                    "Sent Mail",
                    None,
                    Some(FolderRole::Sent),
                ),
            ],
        ),
        (
            microsoft365_kind(&service),
            "synthetic-microsoft365",
            vec![
                stored("inbox-id", "Incoming", None, Some(FolderRole::Inbox)),
                stored("projects-id", "Projects", None, None),
            ],
        ),
    ];
    for (kind, account, expected) in loads {
        let store = Arc::new(Store::in_memory());
        let outcome = load_target(kind, LoadTarget::FolderList, &store);
        assert!(
            matches!(outcome, LoadResult::Stored { incomplete: None }),
            "{account}: {outcome:?}"
        );
        assert_eq!(stored_folders(&store, account), expected, "{account}");
    }
}

#[test]
fn a_folder_list_that_is_cut_short_or_fails_a_page_changes_nothing_stored() {
    let cut = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", "INBOX")],
        list_completion: None,
        ..FixtureSetup::default()
    });
    let failing_page = graph_service::ScriptedService::start_with_folders(
        graph_service::ScriptedAnswer::inbox(1),
        graph_service::ScriptedFolders {
            pages: vec![
                Ok(vec![graph_service::folder_entry(
                    "inbox-id", "Incoming", "root-id",
                )]),
                Err(graph_service::ScriptedAnswer::throttled()),
            ],
            well_known: Vec::new(),
        },
    );
    let loads = [
        (
            LoadKind::GenericImap(account_access(&cut)),
            folder_of("synthetic-account", "INBOX"),
        ),
        (
            microsoft365_kind(&failing_page),
            folder_of("synthetic-microsoft365", "inbox"),
        ),
    ];
    for (kind, earlier) in loads {
        let store = Arc::new(store_with_inbox(&earlier));
        let before = stored_folders(&store, earlier.account.as_str());
        let outcome = load_target(kind, LoadTarget::FolderList, &store);
        assert!(matches!(outcome, LoadResult::Failed(_)), "{outcome:?}");
        assert_eq!(stored_folders(&store, earlier.account.as_str()), before);
    }
}

#[test]
fn a_completed_list_without_any_folder_leaves_the_stored_list_as_it_was() {
    // The default server lists no mailbox.
    let fixture = ImapFixture::start(FixtureSetup::default());
    let earlier = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&earlier));
    let before = stored_folders(&store, "synthetic-account");
    let outcome = load_target(
        LoadKind::GenericImap(account_access(&fixture)),
        LoadTarget::FolderList,
        &store,
    );
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(stored_folders(&store, "synthetic-account"), before);
}

#[test]
fn a_folder_list_load_cancelled_before_its_write_stores_nothing() {
    let store = Store::in_memory();
    let account = AccountId::try_from("synthetic-account").unwrap();
    let folder = Folder {
        identity: "INBOX".to_owned(),
        name: "INBOX".to_owned(),
        parent: None,
        role: Some(FolderRole::Inbox),
        selectable: true,
    };
    let outcome = store_folder_list(&store, &account, vec![folder], || true);
    assert!(matches!(outcome, LoadResult::Cancelled), "{outcome:?}");
    assert_eq!(stored_folders(&store, "synthetic-account"), []);
}

#[test]
fn a_mailbox_load_stores_the_messages_of_the_folder_it_names() {
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", "INBOX"), ("", "/", "Work")],
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    let kind = || LoadKind::GenericImap(account_access(&fixture));
    load_target(kind(), LoadTarget::FolderList, &store);
    let work = folder_of("synthetic-account", "Work");
    let outcome = load_target(kind(), LoadTarget::Mailbox(work.clone()), &store);
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
    let stored = read_stored_messages(&store, &work)
        .unwrap()
        .expect("a loaded folder");
    let identities: Vec<&str> = stored
        .iter()
        .map(|message| message.identity.as_str())
        .collect();
    assert_eq!(identities, ["imap:Work/1/20", "imap:Work/1/10"]);
    assert_eq!(
        read_stored_messages(&store, &folder_of("synthetic-account", "INBOX")),
        Ok(None)
    );
    assert_eq!(fixture.log().examined_mailboxes, ["Work"]);
}

#[test]
fn a_gmail_message_under_two_loaded_labels_is_one_message_in_both() {
    let fixture = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        mailboxes: vec![("", "/", "Work"), ("", "/", "Travel")],
        messages: vec![
            FixtureMessage::plain_text(10, "Text").with_gmail_attributes(1_000, &["Travel"]),
        ],
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    let kind = || gmail_kind(&fixture);
    load_target(kind(), LoadTarget::FolderList, &store);
    let (work, travel) = (
        folder_of("synthetic-account", "Work"),
        folder_of("synthetic-account", "Travel"),
    );
    for label in [&work, &travel] {
        load_target(kind(), LoadTarget::Mailbox(label.clone()), &store);
    }
    let in_work = read_stored_messages(&store, &work)
        .unwrap()
        .expect("a loaded label");
    assert_eq!(in_work[0].identity, "gmail:1000");
    assert_eq!(read_stored_messages(&store, &travel), Ok(Some(in_work)));
}

/// The UIDs of the rows each row fetch asked for, in order.
fn row_fetches(fixture: &ImapFixture) -> Vec<String> {
    fixture
        .log()
        .fetches
        .into_iter()
        .filter(|fetch| fetch.items.contains(&"INTERNALDATE".to_owned()))
        .map(|fetch| fetch.message_set)
        .collect()
}

/// A cycle of the Generic IMAP Inbox of `fixture` into `store`, which already
/// lists the Inbox; returns how it ended, what the Inbox then holds and how
/// many portions were reported stored.
fn synchronize_again(
    fixture: &ImapFixture,
    store: &Arc<Store>,
) -> (LoadResult, Vec<Message>, usize) {
    synchronize_kind_again(LoadKind::GenericImap(account_access(fixture)), store)
}

fn synchronize_kind_again(kind: LoadKind, store: &Arc<Store>) -> (LoadResult, Vec<Message>, usize) {
    let inbox = inbox_of(&kind);
    let worker = MailWorker::new(store.clone());
    let (outcome, portions) = run_on_context(finish_load(
        &worker,
        kind,
        LoadTarget::Mailbox(inbox.clone()),
    ));
    let messages = read_stored_messages(store, &inbox)
        .expect("the store reads")
        .unwrap_or_default();
    (outcome, messages, portions)
}

fn imap_server(messages: Vec<FixtureMessage>) -> ImapFixture {
    ImapFixture::start(FixtureSetup {
        messages,
        ..FixtureSetup::default()
    })
}

/// The seconds since the Unix epoch `days` before now.
fn days_ago(days: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    now - days * 86_400
}

#[test]
fn only_messages_of_the_last_30_days_get_their_text() {
    let fixture = imap_server(vec![
        FixtureMessage::plain_text(10, "Old").received_at(days_ago(60)),
        FixtureMessage::plain_text(20, "Recent").received_at(days_ago(29)),
    ]);
    let (_, stored) = synchronize_imap_inbox(&fixture);
    assert_eq!(identities(&stored), [imap_identity(20), imap_identity(10)]);
    assert_eq!(text_of(&stored[0].content).trim(), "Recent");
    assert_eq!(stored[1].content, ReceivedContent::NotDownloaded);
    // Neither the structure nor the text of the old message was asked for.
    let asked: Vec<String> = fixture
        .log()
        .fetches
        .into_iter()
        .filter(|fetch| !fetch.items.contains(&"INTERNALDATE".to_owned()))
        .map(|fetch| fetch.message_set)
        .collect();
    assert_eq!(asked, ["1:*", "20", "20"]);
}

/// SC-002: a folder that did not change costs one listing and no write.
#[test]
fn a_second_cycle_without_changes_fetches_no_message_and_stores_nothing() {
    let fixture = imap_server(plain_messages(3));
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    let (_, first, _) = synchronize_again(&fixture, &store);
    let fetches_before = fixture.log().fetches.len();
    let (outcome, second, portions) = synchronize_again(&fixture, &store);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(second, first);
    assert_eq!(portions, 0);
    let later = &fixture.log().fetches[fetches_before..];
    assert_eq!(later.len(), 1, "{later:?}");
    assert_eq!(later[0].message_set, "1:*");
}

/// SC-003: arrivals are fetched, read states change in place and messages
/// the complete listing no longer reports leave.
#[test]
fn a_later_cycle_brings_arrivals_read_states_and_removals() {
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    synchronize_again(&imap_server(plain_messages(3)), &store);
    let mut changed = plain_messages(4);
    changed.remove(0); // 10 is gone
    changed[0].seen = true; // 20 was read elsewhere
    let fixture = imap_server(changed);
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(
        identities(&stored),
        [imap_identity(40), imap_identity(30), imap_identity(20)]
    );
    assert!(stored[2].seen);
    // Only the arrival was fetched.
    assert_eq!(row_fetches(&fixture), ["40"]);
}

/// RFC 3501 §7.4.1: a message expunged during the listing is left out of a
/// complete listing, which proves it gone.
#[test]
fn a_message_expunged_during_the_listing_leaves_the_folder() {
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    synchronize_again(&imap_server(plain_messages(3)), &store);
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        expunged_during_listing: vec![20],
        ..FixtureSetup::default()
    });
    let (_, stored, _) = synchronize_again(&fixture, &store);
    assert_eq!(identities(&stored), [imap_identity(30), imap_identity(10)]);
}

/// FR-004: without a complete listing nothing is removed.
#[test]
fn a_refused_or_lost_listing_removes_nothing() {
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    let (_, before, _) = synchronize_again(&imap_server(plain_messages(4)), &store);
    let refused = ImapFixture::start(FixtureSetup {
        messages: plain_messages(4),
        listing_refused: true,
        ..FixtureSetup::default()
    });
    match synchronize_again(&refused, &store).0 {
        LoadResult::Stored {
            incomplete: Some(IncompleteList::ServerRefused { reply, .. }),
        } => assert_eq!(reply, "Listing not available now"),
        other => panic!("the refused listing is not reported: {other:?}"),
    }
    let lost = ImapFixture::start(FixtureSetup {
        fault: Some((FaultyCommand::Listing, FaultKind::Close)),
        ..plain_listing_setup(4)
    });
    let (outcome, after, _) = synchronize_again(&lost, &store);
    assert_eq!(
        failure_of(outcome).kind,
        FailureKind::ServerStepFailed(mailbag_domain::ServerStep::FetchMessages)
    );
    assert_eq!(after, before);
}

/// A setup whose mailbox holds `count` plain messages.
fn plain_listing_setup(count: u32) -> FixtureSetup {
    FixtureSetup {
        messages: plain_messages(count),
        ..FixtureSetup::default()
    }
}

/// A complete listing proves removals even when the server then refuses a
/// portion's rows; the folder does not complete (research §3).
#[test]
fn a_refused_row_fetch_keeps_the_proven_removals_and_does_not_complete() {
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    synchronize_again(&imap_server(plain_messages(2)), &store);
    let mut replaced = plain_messages(4);
    replaced.drain(..2); // 10 and 20 are gone; 30 and 40 arrive
    let fixture = ImapFixture::start(FixtureSetup {
        messages: replaced,
        unfetchable_uids: vec![30],
        ..FixtureSetup::default()
    });
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert!(
        matches!(
            outcome,
            LoadResult::Stored {
                incomplete: Some(IncompleteList::ServerRefused { .. })
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(identities(&stored), [imap_identity(40)]);
    assert!(!store.read_folder_sync(&inbox).unwrap().state.synchronized);
}

/// FR-005: after the server renumbered the folder, no old row or text is
/// attached to a new message: the old ones leave, the new ones arrive.
#[test]
fn a_renumbered_generic_imap_folder_replaces_its_rows() {
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    synchronize_again(&imap_server(plain_messages(2)), &store);
    let renumbered = ImapFixture::start(FixtureSetup {
        messages: vec![
            FixtureMessage::plain_text(10, "Another first"),
            FixtureMessage::plain_text(20, "Another second"),
        ],
        uid_validity: 2,
        ..FixtureSetup::default()
    });
    let (_, stored, _) = synchronize_again(&renumbered, &store);
    assert_eq!(identities(&stored), ["imap:INBOX/2/20", "imap:INBOX/2/10"]);
    assert_eq!(text_of(&stored[0].content).trim(), "Another second");
    assert_eq!(row_fetches(&renumbered), ["20,10"]);
}

/// A row stored before identities carried the numbering version leaves with
/// the first complete listing.
#[test]
fn a_row_without_a_numbering_version_leaves_with_the_first_complete_listing() {
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    let unversioned = Message {
        identity: "imap:INBOX/10".to_owned(),
        ..stored_earlier_message()
    };
    store_completed_cycle(&store, &inbox, &[unversioned], || false).unwrap();
    let (_, stored, _) = synchronize_again(&imap_server(plain_messages(1)), &store);
    assert_eq!(identities(&stored), [imap_identity(10)]);
}

/// Gmail's identity survives a renumbering, so nothing is fetched again.
#[test]
fn a_renumbered_gmail_folder_is_matched_by_identity() {
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    synchronize_kind_again(gmail_kind(&gmail_fixture(plain_messages(2))), &store);
    let messages = plain_messages(2)
        .into_iter()
        .zip([10_000, 20_000])
        .map(|(message, gmail_id)| {
            let renumbered = message.uid + 5;
            FixtureMessage {
                uid: renumbered,
                ..message
            }
            .with_gmail_attributes(gmail_id, &[])
        })
        .collect();
    let renumbered = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        messages,
        uid_validity: 7,
        ..FixtureSetup::default()
    });
    let (_, stored, _) = synchronize_kind_again(gmail_kind(&renumbered), &store);
    assert_eq!(identities(&stored), ["gmail:20000", "gmail:10000"]);
    assert!(row_fetches(&renumbered).is_empty());
}

/// Research §4: a message another label already stored is related to this
/// label with its listed read state, without fetching it.
#[test]
fn a_gmail_message_stored_through_another_label_is_related_without_fetching() {
    let fixture = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        mailboxes: vec![("", "/", "Work"), ("", "/", "Travel")],
        messages: vec![FixtureMessage::plain_text(10, "Text").with_gmail_attributes(1_000, &[])],
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    load_target(gmail_kind(&fixture), LoadTarget::FolderList, &store);
    let work = folder_of("synthetic-account", "Work");
    let travel = folder_of("synthetic-account", "Travel");
    load_target(
        gmail_kind(&fixture),
        LoadTarget::Mailbox(work.clone()),
        &store,
    );
    let fetched_before = row_fetches(&fixture).len();
    load_target(
        gmail_kind(&fixture),
        LoadTarget::Mailbox(travel.clone()),
        &store,
    );
    assert_eq!(row_fetches(&fixture).len(), fetched_before);
    assert_eq!(
        stored_messages(&store, &travel),
        stored_messages(&store, &work)
    );
}

/// SC-005: a first fill stopped after some portions continues without
/// fetching the stored messages again; the stopped fill is not an empty
/// folder.
#[test]
fn a_stopped_first_fill_continues_without_fetching_stored_messages_again() {
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    let fixture = imap_server(plain_messages(250));
    run_on_context(async {
        let worker = MailWorker::new(store.clone());
        let (sender, events) = async_channel::unbounded();
        let handle = worker.start_load(
            LoadKind::GenericImap(account_access(&fixture)),
            LoadTarget::Mailbox(inbox.clone()),
            move |event| {
                sender.try_send(event).ok();
            },
        );
        // The listing's portion, then the first hundred.
        for _ in 0..2 {
            let event = events.recv().await.expect("a portion");
            assert!(matches!(event, LoadEvent::PortionStored), "{event:?}");
        }
        drop(handle);
        let (outcome, _) = load_end(&events).await;
        assert!(matches!(outcome, LoadResult::Cancelled), "{outcome:?}");
    });
    let stored_after_stop = store
        .read_folder_rows(&inbox)
        .unwrap()
        .expect("rows are shown");
    assert!(
        stored_after_stop.len() >= 100,
        "{}",
        stored_after_stop.len()
    );
    assert!(!store.read_folder_sync(&inbox).unwrap().state.synchronized);
    let continued = imap_server(plain_messages(250));
    let (outcome, stored, _) = synchronize_again(&continued, &store);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(stored.len(), 250);
    let refetched: Vec<String> = row_fetches(&continued)
        .iter()
        .flat_map(|set| set.split(',').map(str::to_owned).collect::<Vec<_>>())
        .filter(|uid| {
            stored_after_stop
                .iter()
                .any(|row| row.identity == imap_identity(uid.parse().unwrap()))
        })
        .collect();
    assert!(refetched.is_empty(), "{refetched:?}");
}

/// A stopped refill of a folder never completed shows "no mail loaded".
#[test]
fn a_first_fill_stopped_before_any_row_is_no_mail_loaded() {
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    let fixture = ImapFixture::start(FixtureSetup {
        fault: Some((FaultyCommand::Rows, FaultKind::Close)),
        ..plain_listing_setup(2)
    });
    synchronize_again(&fixture, &store);
    assert_eq!(store.read_folder_rows(&inbox), Ok(None));
}

/// SC-001, SC-002 on a scripted folder of 10 000 messages. The times are
/// printed for the reader; the machine decides them.
#[test]
fn a_large_folder_fills_in_portions_and_a_second_cycle_fetches_nothing() {
    let messages = (1..=10_000)
        .map(|uid| FixtureMessage::plain_text(uid, "Text").received_at(days_ago(60)))
        .collect();
    let fixture = imap_server(messages);
    let inbox = folder_of("synthetic-account", "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    let started = Instant::now();
    let (first_portion, outcome, portions) = run_on_context(async {
        let worker = MailWorker::new(store.clone());
        let (sender, events) = async_channel::unbounded();
        let _handle = worker.start_load(
            LoadKind::GenericImap(account_access(&fixture)),
            LoadTarget::Mailbox(inbox.clone()),
            move |event| {
                sender.try_send(event).ok();
            },
        );
        // The listing's portion comes first, then the first hundred.
        events.recv().await.expect("the listing's portion");
        events.recv().await.expect("the first hundred");
        let first_portion = started.elapsed();
        let (outcome, portions) = load_end(&events).await;
        (first_portion, outcome, portions + 2)
    });
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    println!(
        "10 000 messages: first hundred stored after {first_portion:?}, all after {:?}",
        started.elapsed()
    );
    assert_eq!(portions, 1 + 100 + 1);
    let fetches_before = fixture.log().fetches.len();
    let started = Instant::now();
    let (_, stored, portions) = synchronize_again(&fixture, &store);
    println!(
        "a second cycle without changes took {:?}",
        started.elapsed()
    );
    assert_eq!(stored.len(), 10_000);
    assert_eq!(portions, 0);
    assert_eq!(fixture.log().fetches.len(), fetches_before + 1);
}

/// A Gmail load whose renewal Online Accounts answers once with `token`, as
/// a thread of its own stands in for GTK's context.
fn gmail_kind_renewed_with(fixture: &ImapFixture, token: &str) -> LoadKind {
    let (renewal, requests) = AccessRenewal::answered_by_test();
    let mut renewed = Some(ImapAccess {
        credential: ImapCredential::AccessToken(token.to_owned()),
        ..account_access(fixture)
    });
    std::thread::spawn(move || {
        while let Ok(reply) = requests.recv_blocking() {
            reply.send_blocking(renewed.take()).ok();
        }
    });
    LoadKind::Gmail {
        access: gmail_access(fixture),
        renewal,
    }
}

/// SC-010: Gmail ends the session mid-fill, the cycle signs in again once
/// with a renewed token and completes.
#[test]
fn a_gmail_session_ended_mid_fill_is_renewed_once_and_the_fill_completes() {
    let fixture = ImapFixture::start(FixtureSetup {
        renewed_access_token: Some("renewed-token".to_owned()),
        fault: Some((FaultyCommand::Rows, FaultKind::Bye)),
        ..gmail_fixture_setup(plain_messages(2))
    });
    let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
    let kind = gmail_kind_renewed_with(&fixture, "renewed-token");
    let (outcome, stored, _) = synchronize_kind_again(kind, &store);
    assert!(
        matches!(outcome, LoadResult::Stored { incomplete: None }),
        "{outcome:?}"
    );
    assert_eq!(identities(&stored), ["gmail:20000", "gmail:10000"]);
    assert_eq!(fixture.log().connections, 2);
}

/// Research §13: the same token, or a second end, stands with Gmail's words.
#[test]
fn a_gmail_session_end_stands_with_the_same_token_or_a_second_end() {
    for (token, fault_times) in [(TEST_ACCESS_TOKEN, 1), ("renewed-token", 2)] {
        let fixture = ImapFixture::start(FixtureSetup {
            renewed_access_token: Some("renewed-token".to_owned()),
            fault: Some((FaultyCommand::Listing, FaultKind::Bye)),
            fault_times,
            ..gmail_fixture_setup(plain_messages(1))
        });
        let store = Arc::new(store_with_inbox(&folder_of("synthetic-account", "INBOX")));
        let kind = gmail_kind_renewed_with(&fixture, token);
        let (outcome, stored, _) = synchronize_kind_again(kind, &store);
        let failure = failure_of(outcome);
        assert_eq!(
            failure.kind,
            FailureKind::ServerStepFailed(mailbag_domain::ServerStep::FetchMessages),
            "{token}"
        );
        assert!(
            failure
                .remote_texts
                .iter()
                .any(|text| text.text == "Server is restarting"),
            "{failure:?}"
        );
        assert!(stored.is_empty());
    }
}

/// Stores `messages` as the folder's whole content, as a completed cycle
/// leaves it: stored messages not among them leave.
fn store_completed_cycle(
    store: &Store,
    folder: &FolderRef,
    messages: &[Message],
    load_cancelled: impl FnOnce() -> bool,
) -> Result<StoreWrite, Failure> {
    let removed = store
        .read_folder_rows(folder)?
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.identity)
        .filter(|identity| !messages.iter().any(|message| message.identity == *identity))
        .collect();
    let portion = FolderPortion {
        removed,
        arrived: messages.to_vec(),
        state: Some(FolderState {
            server_position: None,
            synchronized: true,
        }),
        ..FolderPortion::default()
    };
    store.store_portion(folder, &portion, load_cancelled)
}

/// Only a saved link is answered with a full reading: a rejected first
/// reading ends the cycle instead of asking again forever.
#[test]
fn a_rejected_first_reading_fails_the_cycle() {
    // The scripted service knows no page, so it rejects the first reading.
    let service =
        graph_service::ScriptedService::start_with_changes(graph_mailbox(Vec::new(), Vec::new()));
    let (outcome, _) = synchronize_inbox(microsoft365_kind(&service));
    assert_eq!(failure_of(outcome).kind, FailureKind::RequestRefused);
    assert_eq!(service.received_requests().len(), 1);
}
