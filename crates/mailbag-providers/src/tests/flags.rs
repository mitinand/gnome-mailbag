// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A cycle sends the user's pending changes of read state and star
//! (specs/011-read-and-star FR-006 to FR-010, SC-001 to SC-006) to the
//! scripted servers.

use super::*;
use mailbag_domain::{MessageFlag, PendingChange, RemoteSource, ServerStep};
use mailbag_imap::test_server::StoreFault;

const IMAP_ACCOUNT: &str = "synthetic-account";
const MICROSOFT365_ACCOUNT: &str = "synthetic-microsoft365";

/// Stores the user's wish for the message, as the window does.
fn want(store: &Store, account: &str, identity: &str, flag: MessageFlag, wanted: bool) {
    let account = AccountId::try_from(account).unwrap();
    store
        .write_pending_flag(&account, identity, flag, wanted)
        .unwrap();
}

fn pending_in(store: &Store, folder: &FolderRef) -> Vec<PendingChange> {
    store.read_pending_changes(folder).unwrap()
}

/// The `UID STORE` commands the server received, in order.
fn store_commands(fixture: &ImapFixture) -> Vec<String> {
    (fixture.log().commands.into_iter())
        .filter(|command| command.starts_with("UID STORE"))
        .collect()
}

/// The Inbox of the IMAP test account with `messages`, after a first cycle.
fn synchronized_imap_inbox(fixture: &ImapFixture) -> (Arc<Store>, FolderRef) {
    let inbox = folder_of(IMAP_ACCOUNT, "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    synchronize_again(fixture, &store);
    (store, inbox)
}

fn assert_stored(outcome: &LoadResult) {
    assert!(matches!(outcome, LoadResult::Stored { .. }), "{outcome:?}");
}

/// SC-001's server side: one command per flag and value, sent once.
#[test]
fn a_refresh_sends_each_pending_change_once_and_settles_it() {
    use MessageFlag::{Flagged, Seen};
    let fixture = imap_server(plain_messages(4));
    let (store, inbox) = synchronized_imap_inbox(&fixture);
    want(&store, IMAP_ACCOUNT, &imap_identity(10), Seen, true);
    want(&store, IMAP_ACCOUNT, &imap_identity(30), Flagged, true);
    want(&store, IMAP_ACCOUNT, &imap_identity(20), Flagged, true);
    // The server's value already: it ends without a command.
    want(&store, IMAP_ACCOUNT, &imap_identity(40), Seen, false);
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(
        store_commands(&fixture),
        [
            r"UID STORE 10 +FLAGS.SILENT (\Seen)",
            r"UID STORE 20,30 +FLAGS.SILENT (\Flagged)",
        ]
    );
    assert_eq!(pending_in(&store, &inbox), []);
    let server_flags = store.read_folder_sync(&inbox).unwrap().stored;
    assert!(server_flags[&imap_identity(10)].seen);
    assert!(server_flags[&imap_identity(20)].flagged && server_flags[&imap_identity(30)].flagged);
    // Newest first: 40, 30, 20, 10.
    let flags: Vec<(bool, bool)> = (stored.iter())
        .map(|message| (message.seen, message.flagged))
        .collect();
    assert_eq!(
        flags,
        [(false, false), (false, true), (false, true), (true, false)]
    );
    synchronize_again(&fixture, &store);
    assert_eq!(store_commands(&fixture).len(), 2);
}

#[test]
fn hundreds_of_changes_go_a_hundred_messages_per_command() {
    let fixture = imap_server(plain_messages(250));
    let (store, inbox) = synchronized_imap_inbox(&fixture);
    for uid in (1..=250).map(|number| number * 10) {
        want(
            &store,
            IMAP_ACCOUNT,
            &imap_identity(uid),
            MessageFlag::Flagged,
            true,
        );
    }
    synchronize_again(&fixture, &store);
    let uids_per_command: Vec<usize> = (store_commands(&fixture).iter())
        .map(|command| command.split(' ').nth(2).unwrap().split(',').count())
        .collect();
    assert_eq!(uids_per_command, [100, 100, 50]);
    assert_eq!(pending_in(&store, &inbox), []);
}

/// Research §14 and SC-003: the user unstars while the star is on its way;
/// the accepted star leaves the newer wish, which goes before the next batch.
#[test]
fn a_change_made_while_a_command_is_out_is_sent_before_the_next_batch() {
    let inbox = folder_of(IMAP_ACCOUNT, "INBOX");
    let store = Arc::new(store_with_inbox(&inbox));
    synchronize_again(&imap_server(plain_messages(1)), &store);
    let (release, held) = async_channel::bounded(1);
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(300),
        store_fault: Some(StoreFault::HoldCompletion(held)),
        ..FixtureSetup::default()
    });
    let starred = imap_identity(10);
    want(&store, IMAP_ACCOUNT, &starred, MessageFlag::Flagged, true);
    let worker = MailWorker::new(store.clone());
    let kind = LoadKind::GenericImap(account_access(&fixture));
    let (outcome, _) = run_on_context(async {
        let load = glib::spawn_future_local(async move {
            finish_load(
                &worker,
                kind,
                LoadTarget::Mailbox(folder_of(IMAP_ACCOUNT, "INBOX")),
            )
            .await
        });
        wait_until(|| store_commands(&fixture).len() == 1).await;
        want(&store, IMAP_ACCOUNT, &starred, MessageFlag::Flagged, false);
        release.send(()).await.unwrap();
        load.await.unwrap()
    });
    assert_stored(&outcome);
    assert_eq!(
        store_commands(&fixture),
        [
            r"UID STORE 10 +FLAGS.SILENT (\Flagged)",
            r"UID STORE 10 -FLAGS.SILENT (\Flagged)",
        ]
    );
    // The unstar went after the first batch of rows and before the second.
    let log = fixture.log();
    let unstar = (log.commands.iter())
        .position(|command| command.contains("-FLAGS"))
        .unwrap();
    let fetches_before = (log.commands[..unstar].iter())
        .filter(|command| *command == "UID FETCH")
        .count();
    let row_batches_before = (log.fetches[..fetches_before].iter())
        .filter(|fetch| fetch.items.contains(&"INTERNALDATE".to_owned()))
        .count();
    assert_eq!(row_batches_before, 1);
    assert_eq!(row_fetches(&fixture).len(), 3);
    assert_eq!(pending_in(&store, &inbox), []);
    assert!(!store.read_folder_sync(&inbox).unwrap().stored[&starred].flagged);
}

/// SC-004: whether a change reached the server before the connection broke
/// is settled by the next listing, never by sending it blindly.
#[test]
fn a_change_whose_answer_was_lost_is_settled_by_the_next_listing() {
    for (fault, commands_in_all) in [
        (StoreFault::CloseAfterApplying, 1),
        (StoreFault::CloseBeforeApplying, 2),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(2),
            store_fault: Some(fault.clone()),
            ..FixtureSetup::default()
        });
        let (store, inbox) = synchronized_imap_inbox(&fixture);
        want(
            &store,
            IMAP_ACCOUNT,
            &imap_identity(10),
            MessageFlag::Flagged,
            true,
        );
        let (outcome, _, _) = synchronize_again(&fixture, &store);
        assert!(
            matches!(outcome, LoadResult::Failed(_)),
            "{fault:?}: {outcome:?}"
        );
        assert_eq!(pending_in(&store, &inbox).len(), 1, "{fault:?}");
        let (outcome, stored, _) = synchronize_again(&fixture, &store);
        assert_stored(&outcome);
        assert_eq!(pending_in(&store, &inbox), [], "{fault:?}");
        assert!(stored[1].flagged, "{fault:?}");
        assert_eq!(store_commands(&fixture).len(), commands_in_all, "{fault:?}");
    }
}

/// SC-005: a refused change is dropped, the window reads the store again,
/// and the cycle fails with the server's words.
#[test]
fn a_refused_change_is_dropped_and_fails_the_cycle_with_the_reply() {
    for status in ["NO [CANNOT]", "BAD [CLIENTBUG]"] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(1),
            store_completion: Some(format!("{{tag}} {status} Flags are locked\r\n")),
            ..FixtureSetup::default()
        });
        let (store, inbox) = synchronized_imap_inbox(&fixture);
        want(
            &store,
            IMAP_ACCOUNT,
            &imap_identity(10),
            MessageFlag::Flagged,
            true,
        );
        let (outcome, stored, store_changes) = synchronize_again(&fixture, &store);
        // The drop is the cycle's one change of the store.
        assert_eq!(store_changes, 1, "{status}");
        let failure = failure_of(outcome);
        assert_eq!(
            failure.kind,
            FailureKind::ServerStepFailed(ServerStep::ChangeFlags)
        );
        assert!(
            (failure.remote_texts.iter()).any(|remote| remote.source == RemoteSource::ServerReply
                && remote.text == "Flags are locked"),
            "{status}: {failure:?}"
        );
        assert!(!stored[0].flagged);
        assert_eq!(pending_in(&store, &inbox), []);
        synchronize_again(&fixture, &store);
        assert_eq!(store_commands(&fixture).len(), 1, "{status}");
    }
}

/// SC-006: Gmail keeps flags on the message, so the label that sends a
/// change leaves nothing for another label to send.
#[test]
fn a_gmail_change_is_sent_by_one_label_only() {
    let fixture = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        mailboxes: vec![("", "/", "Work"), ("", "/", "Travel")],
        messages: vec![
            FixtureMessage::plain_text(10, "Text").with_gmail_attributes(1_000, &["Travel"]),
        ],
        ..FixtureSetup::default()
    });
    let store = Arc::new(Store::in_memory());
    let labels = [
        folder_of(IMAP_ACCOUNT, "Work"),
        folder_of(IMAP_ACCOUNT, "Travel"),
    ];
    load_target(gmail_kind(&fixture), LoadTarget::FolderList, &store);
    for label in &labels {
        load_target(
            gmail_kind(&fixture),
            LoadTarget::Mailbox(label.clone()),
            &store,
        );
    }
    want(&store, IMAP_ACCOUNT, "gmail:1000", MessageFlag::Seen, true);
    for label in &labels {
        let outcome = load_target(
            gmail_kind(&fixture),
            LoadTarget::Mailbox(label.clone()),
            &store,
        );
        assert_stored(&outcome);
    }
    assert_eq!(
        store_commands(&fixture),
        [r"UID STORE 10 +FLAGS.SILENT (\Seen)"]
    );
    assert!(stored_messages(&store, &labels[1])[0].seen);
}

/// A change waits while the server does not show its message: a listing it
/// refused to finish, or a sign-in it refused (SC-002's server half).
#[test]
fn a_change_waits_until_a_listing_shows_its_message() {
    let (store, inbox) = synchronized_imap_inbox(&imap_server(plain_messages(4)));
    want(
        &store,
        IMAP_ACCOUNT,
        &imap_identity(10),
        MessageFlag::Flagged,
        true,
    );
    want(
        &store,
        IMAP_ACCOUNT,
        &imap_identity(40),
        MessageFlag::Flagged,
        true,
    );
    // The listing reports 10 and 20 only.
    let half_listed = ImapFixture::start(FixtureSetup {
        listing_refused: true,
        ..plain_listing_setup(4)
    });
    synchronize_again(&half_listed, &store);
    assert_eq!(
        store_commands(&half_listed),
        [r"UID STORE 10 +FLAGS.SILENT (\Flagged)"]
    );
    let refusing = ImapFixture::start(FixtureSetup {
        credentials: Some((TEST_LOGIN.to_owned(), "another password".to_owned())),
        ..plain_listing_setup(4)
    });
    let (outcome, _, _) = synchronize_again(&refusing, &store);
    assert!(matches!(outcome, LoadResult::Failed(_)), "{outcome:?}");
    assert_eq!(pending_in(&store, &inbox).len(), 1);
    let listing_all = imap_server(plain_messages(4));
    synchronize_again(&listing_all, &store);
    assert_eq!(
        store_commands(&listing_all),
        [r"UID STORE 40 +FLAGS.SILENT (\Flagged)"]
    );
    assert_eq!(pending_in(&store, &inbox), []);
}

/// A scripted mailbox of messages 1 to 3 whose first reading is followed by
/// `rounds`, at most three, each leading to the next; then a round without
/// changes that leads to itself, as a mailbox with no more changes.
fn graph_mailbox_with_rounds(
    rounds: Vec<Vec<serde_json::Value>>,
) -> graph_service::ScriptedChanges {
    use graph_service::{ScriptedNext::Done, delta_entry};
    const TOKENS: [&str; 4] = ["round-1", "round-2", "round-3", "round-4"];
    let last = TOKENS[rounds.len()];
    let mut pages = vec![(
        "first",
        delta_page((1..=3).map(delta_entry).collect(), Done(TOKENS[0])),
    )];
    for (index, entries) in rounds.into_iter().enumerate() {
        pages.push((TOKENS[index], delta_page(entries, Done(TOKENS[index + 1]))));
    }
    pages.push((last, delta_page(Vec::new(), Done(last))));
    graph_mailbox(pages, inbox_messages(&[1, 2, 3]))
}

/// The PATCH requests the service received: each message id and body.
fn patches(service: &graph_service::ScriptedService) -> Vec<(String, String)> {
    (service.received_requests().into_iter())
        .filter(|request| request.method == "PATCH")
        .map(|request| (request.path, request.body))
        .collect()
}

fn microsoft365_inbox() -> (Arc<Store>, FolderRef) {
    let inbox = folder_of(MICROSOFT365_ACCOUNT, "inbox");
    (Arc::new(store_with_inbox(&inbox)), inbox)
}

/// SC-001 on Microsoft 365: each change is one request after the round;
/// the next round's reports of the same values keep what the user sees.
#[test]
fn microsoft_365_changes_go_after_the_round_and_its_reports_keep_them() {
    let id = graph_service::fixture_immutable_id;
    let mut starred = graph_service::delta_entry(2);
    starred["flag"] = serde_json::json!({ "flagStatus": "flagged" });
    let service =
        graph_service::ScriptedService::start_with_changes(graph_mailbox_with_rounds(vec![
            Vec::new(),
            vec![starred, serde_json::json!({ "id": id(1), "isRead": false })],
            Vec::new(),
        ]));
    let (store, inbox) = microsoft365_inbox();
    synchronize_kind_again(microsoft365_kind(&service), &store);
    want(
        &store,
        MICROSOFT365_ACCOUNT,
        &graph_identity(2),
        MessageFlag::Flagged,
        true,
    );
    want(
        &store,
        MICROSOFT365_ACCOUNT,
        &graph_identity(1),
        MessageFlag::Seen,
        false,
    );
    let (outcome, _, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
    assert_stored(&outcome);
    let expected = [
        (
            format!("/me/messages/{}", id(1)),
            r#"{"isRead":false}"#.to_owned(),
        ),
        (
            format!("/me/messages/{}", id(2)),
            r#"{"flag":{"flagStatus":"flagged"}}"#.to_owned(),
        ),
    ];
    let mut sent = patches(&service);
    sent.sort();
    assert_eq!(sent, expected);
    assert_eq!(pending_in(&store, &inbox), []);
    let (_, stored, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
    let flags: Vec<(bool, bool)> = (stored.iter())
        .map(|message| (message.seen, message.flagged))
        .collect();
    assert_eq!(flags, [(false, false), (false, true), (true, false)]);
    assert_eq!(patches(&service).len(), 2);
}

/// SC-005 on Microsoft 365 and spec FR-009: a 4xx refuses the change, which
/// is dropped; a 5xx leaves its outcome unknown, so it is sent again.
#[test]
fn a_microsoft_365_refusal_drops_the_change_and_a_504_keeps_it() {
    let refusals = [
        (
            graph_service::ScriptedAnswer::error(
                400,
                "ErrorInvalidIdMalformed",
                "Id is malformed.",
            ),
            false,
        ),
        (
            graph_service::ScriptedAnswer::error(504, "GatewayTimeout", "The gateway timed out."),
            true,
        ),
    ];
    for (answer, kept) in refusals {
        let status = answer.status;
        let mut mailbox = graph_mailbox_with_rounds(Vec::new());
        mailbox.patch_answer = Some(answer);
        let service = graph_service::ScriptedService::start_with_changes(mailbox);
        let (store, inbox) = microsoft365_inbox();
        synchronize_kind_again(microsoft365_kind(&service), &store);
        want(
            &store,
            MICROSOFT365_ACCOUNT,
            &graph_identity(2),
            MessageFlag::Flagged,
            true,
        );
        let (outcome, stored, store_changes) =
            synchronize_kind_again(microsoft365_kind(&service), &store);
        assert!(
            matches!(outcome, LoadResult::Failed(_)),
            "{status}: {outcome:?}"
        );
        assert_eq!(
            pending_in(&store, &inbox).len(),
            usize::from(kept),
            "{status}"
        );
        // The round's new position, and the drop of a refused change.
        assert_eq!(store_changes, 1 + usize::from(!kept), "{status}");
        assert_eq!(stored[1].flagged, kept, "{status}");
        let (outcome, stored, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
        assert_stored(&outcome);
        assert_eq!(patches(&service).len(), 1 + usize::from(kept), "{status}");
        assert_eq!(stored[1].flagged, kept, "{status}");
    }
}

/// The token's refusal of a change is renewed once, as for any request.
#[test]
fn a_change_refused_for_its_token_is_sent_again_with_a_renewed_one() {
    let mut mailbox = graph_mailbox_with_rounds(Vec::new());
    // The first fill's page and texts, then the round, then the change.
    mailbox.token_accepted_requests = Some(3);
    let service = graph_service::ScriptedService::start_with_changes(mailbox);
    let (store, inbox) = microsoft365_inbox();
    synchronize_kind_again(microsoft365_kind(&service), &store);
    assert_eq!(service.received_requests().len(), 2);
    want(
        &store,
        MICROSOFT365_ACCOUNT,
        &graph_identity(2),
        MessageFlag::Flagged,
        true,
    );
    let kind = microsoft365_kind_with(
        &service,
        graph_service::TEST_ACCESS_TOKEN,
        Some("renewed-token"),
    );
    let (outcome, _, _) = synchronize_kind_again(kind, &store);
    assert_stored(&outcome);
    let authorizations: Vec<Option<String>> = (service.received_requests().into_iter())
        .filter(|request| request.method == "PATCH")
        .map(|request| request.authorization)
        .collect();
    assert_eq!(
        authorizations,
        [
            Some(format!("Bearer {}", graph_service::TEST_ACCESS_TOKEN)),
            Some("Bearer renewed-token".to_owned()),
        ]
    );
    assert_eq!(pending_in(&store, &inbox), []);
}

/// Research §14: the service may report a message twice in one round, its
/// read state on one page and its star alone on the next; both last.
#[test]
fn a_message_reported_on_two_pages_of_a_round_keeps_both_changes() {
    use graph_service::{ScriptedNext::*, delta_entry};
    let id = graph_service::fixture_immutable_id;
    let service = graph_service::ScriptedService::start_with_changes(graph_mailbox(
        vec![
            (
                "first",
                delta_page((1..=3).map(delta_entry).collect(), Done("round-1")),
            ),
            (
                "round-1",
                delta_page(
                    vec![serde_json::json!({ "id": id(2), "isRead": true })],
                    More("round-1b"),
                ),
            ),
            (
                "round-1b",
                delta_page(
                    vec![serde_json::json!({ "id": id(2), "flag": { "flagStatus": "flagged" } })],
                    Done("round-2"),
                ),
            ),
        ],
        inbox_messages(&[1, 2, 3]),
    ));
    let (store, _) = microsoft365_inbox();
    synchronize_kind_again(microsoft365_kind(&service), &store);
    let (outcome, stored, _) = synchronize_kind_again(microsoft365_kind(&service), &store);
    assert_stored(&outcome);
    assert_eq!(stored[1].identity, graph_identity(2));
    assert!(stored[1].seen && stored[1].flagged, "{:?}", stored[1]);
}
