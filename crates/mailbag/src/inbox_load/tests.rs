// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::inbox::ReceivedBatch;
use goa_adapter::AccountId;
use mailbag_imap::test_server::{
    FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture, TEST_LOGIN, TEST_PASSWORD,
    test_certificates_trusted,
};
use std::time::{Duration, Instant};

fn account_access(fixture: &ImapFixture) -> ImapAccess {
    ImapAccess {
        account_id: AccountId::try_from("synthetic-account").unwrap(),
        host: format!("localhost:{}", fixture.port()),
        login: TEST_LOGIN.to_owned(),
        password: TEST_PASSWORD.to_owned(),
        encryption: ImapEncryption::ImplicitTls,
    }
}

fn plain_messages(count: u32) -> Vec<FixtureMessage> {
    (1..=count)
        .map(|number| FixtureMessage::plain_text(number * 10, &format!("Text {number}")))
        .collect()
}

/// Runs a load to its end on a fresh GLib context, as the window would.
fn load_inbox(fixture: &ImapFixture) -> LoadOutcome {
    run_on_context(async {
        let worker = MailWorker::new();
        let (sender, outcomes) = async_channel::bounded(1);
        let _handle = worker.load_inbox(account_access(fixture), move |outcome| {
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
        let expected: Vec<u32> = (1..=count)
            .rev()
            .take(100)
            .map(|number| number * 10)
            .collect();
        let uids: Vec<u32> = batch.messages.iter().map(|message| message.uid).collect();
        assert_eq!(uids, expected, "{count} messages");
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
    let contents: Vec<(u32, &ReceivedContent)> = batch
        .messages
        .iter()
        .map(|message| (message.uid, &message.content))
        .collect();
    assert_eq!(contents.len(), 2);
    assert_eq!(
        contents[0],
        (
            20,
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
        LoadOutcome::Failed(failure) => {
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
        let handle = worker.load_inbox(account_access(&fixture), move |outcome| {
            sender.try_send(outcome).ok();
        });
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
        LoadOutcome::Failed(failure) => {
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
    let contents: Vec<(u32, &ReceivedContent)> = batch
        .messages
        .iter()
        .map(|message| (message.uid, &message.content))
        .collect();
    assert_eq!(
        contents[0],
        (
            20,
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
        let _handle = worker.load_inbox(account_access(&fixture), move |outcome| {
            sender.try_send(outcome).ok();
        });
        outcomes.recv().await.expect("the load reports its outcome")
    });
    assert_eq!(published_batch(outcome).messages.len(), 1);
}

/// Manual acceptance of the whole chain against a running `serve_fixture`:
/// the real Online Accounts service provides the settings and password, and
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
        (Ok("rejected"), Ok(LoadOutcome::Failed(failure))) => {
            assert_eq!(
                failure.failure,
                mailbag_imap::ImapFailure::Failed(mailbag_imap::ImapStep::SecureConnection)
            );
            println!("refused at the secure-connection step, so no password was sent");
        }
        (Ok("no-encryption"), Err(error)) => {
            assert_eq!(error, goa_adapter::ImapAccessError::NoEncryption);
            println!("refused for its encryption setting, without requesting the password");
        }
        (expectation, loaded) => {
            panic!("expected {expectation:?}, got {loaded:?}");
        }
    }
}

/// Reads the account's settings and password from Online Accounts, then loads
/// its Inbox on the mail worker, as Refresh Inbox does. An account Online
/// Accounts cannot give settings for never reaches the worker.
async fn load_with_online_accounts(
    account_id: AccountId,
) -> Result<LoadOutcome, goa_adapter::ImapAccessError> {
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
    let _load = worker.load_inbox(access, move |outcome| {
        finished.try_send(outcome).ok();
    });
    Ok(outcomes.recv().await.expect("the load reports its outcome"))
}
