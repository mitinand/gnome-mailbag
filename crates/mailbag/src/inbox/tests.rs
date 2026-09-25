// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::logging::{LogLevel, capture::start_record};
use goa_adapter::AccessError;
use mailbag_content::ContentExplanation;
use mailbag_content::DisplayFields;
use mailbag_graph::{GraphError, GraphFailure};
use mailbag_imap::{ImapError, ImapFailure, ImapStep, ServerReply};
use mailbag_providers::{MessageIdentity, ReceivedMessage};
use std::cell::Cell;

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn batch_of(account_id: &AccountId, uids: &[u32]) -> ReceivedBatch {
    ReceivedBatch {
        account_id: account_id.clone(),
        uid_validity: Some(1),
        incomplete: None,
        messages: uids
            .iter()
            .map(|uid| ReceivedMessage {
                identity: MessageIdentity::ImapUid(*uid),
                fields: DisplayFields::default(),
                internal_date: None,
                seen: false,
                content: ReceivedContent::Text("Text".to_owned()),
                gmail: None,
            })
            .collect(),
    }
}

fn sign_in_failure() -> LoadFailure {
    LoadFailure::Imap(ImapFailure::Failed(ImapStep::SignIn).into())
}

/// A running step that records its cancellation, as dropping the Online
/// Accounts request or the worker's handle does.
struct CountedStep(Rc<Cell<usize>>);

impl CancelsLoadOnDrop for CountedStep {}

impl Drop for CountedStep {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn counted_step(cancellations: &Rc<Cell<usize>>) -> Box<dyn CancelsLoadOnDrop> {
    Box::new(CountedStep(cancellations.clone()))
}

fn received_uids(inbox: Option<&AccountInbox>) -> Vec<u32> {
    match inbox {
        Some(AccountInbox::Received(batch)) => batch
            .messages
            .iter()
            .map(|message| match &message.identity {
                MessageIdentity::ImapUid(uid) => *uid,
                other => panic!("not an IMAP message: {other:?}"),
            })
            .collect(),
        other => panic!("the account shows no batch: {other:?}"),
    }
}

/// Starts a load that has reached the mail worker.
fn start_load(controller: &mut InboxController, account_id: &AccountId) -> Rc<Cell<usize>> {
    let cancellations = Rc::new(Cell::new(0));
    assert!(!controller.is_loading());
    controller.begin_load(account_id, counted_step(&cancellations));
    cancellations
}

#[test]
fn an_account_without_a_refresh_has_no_mail_and_no_load() {
    let controller = InboxController::default();
    assert!(controller.inbox_of(&account("never-refreshed")).is_none());
    assert!(!controller.is_loading());
}

#[test]
fn refreshing_clears_the_account_and_runs_one_load() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    start_load(&mut controller, &id);
    controller.finish_load(&id, LoadResult::Received(batch_of(&id, &[20, 10])));
    assert_eq!(received_uids(controller.inbox_of(&id)), [20, 10]);
    assert!(!controller.is_loading());

    start_load(&mut controller, &id);
    assert!(matches!(
        controller.inbox_of(&id),
        Some(AccountInbox::Loading)
    ));
    assert!(controller.is_loading());
}

#[test]
fn refresh_inbox_is_unavailable_while_a_load_runs() {
    let mut controller = InboxController::default();
    let loading = account("loading-account");
    let other = account("other-account");
    let cancellations = start_load(&mut controller, &loading);
    assert!(controller.is_loading());
    assert!(controller.inbox_of(&other).is_none());
    assert_eq!(cancellations.get(), 0);

    controller.finish_load(&loading, LoadResult::Received(batch_of(&loading, &[10])));
    assert!(!controller.is_loading());
    start_load(&mut controller, &other);
    assert!(controller.is_loading());
}

#[test]
fn a_result_reaches_only_the_account_its_load_started_for() {
    let mut controller = InboxController::default();
    let loading = account("loading-account");
    let selected = account("selected-account");
    start_load(&mut controller, &loading);
    controller.finish_load(&selected, LoadResult::Received(batch_of(&selected, &[30])));
    assert!(controller.inbox_of(&selected).is_none());
    assert!(controller.is_loading());

    controller.finish_load(&loading, LoadResult::Received(batch_of(&loading, &[10])));
    assert_eq!(received_uids(controller.inbox_of(&loading)), [10]);
    assert!(!controller.is_loading());
}

#[test]
fn a_failed_load_leaves_the_account_without_mail() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    start_load(&mut controller, &id);
    controller.finish_load(&id, LoadResult::Received(batch_of(&id, &[10])));
    start_load(&mut controller, &id);
    controller.finish_load(&id, LoadResult::Failed(sign_in_failure()));
    assert!(matches!(
        controller.inbox_of(&id),
        Some(AccountInbox::Failed(LoadFailure::Imap(_)))
    ));
    assert!(!controller.is_loading());
}

#[test]
fn a_confirmed_exclusion_discards_the_mail_and_cancels_its_load() {
    let mut controller = InboxController::default();
    let excluded = account("excluded-account");
    let kept = account("kept-account");
    start_load(&mut controller, &kept);
    controller.finish_load(&kept, LoadResult::Received(batch_of(&kept, &[10])));
    let cancellations = start_load(&mut controller, &excluded);

    controller.discard_excluded(|account_id| *account_id == kept);
    assert_eq!(cancellations.get(), 1);
    assert!(controller.inbox_of(&excluded).is_none());
    assert_eq!(received_uids(controller.inbox_of(&kept)), [10]);
    // The load ends only once its connection is closed.
    assert!(controller.is_loading());

    // A result that arrives after the exclusion restores nothing.
    controller.finish_load(&excluded, LoadResult::Received(batch_of(&excluded, &[40])));
    assert!(controller.inbox_of(&excluded).is_none());
    assert!(!controller.is_loading());
}

#[test]
fn a_cancelled_load_ends_without_showing_a_failure() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    let cancellations = start_load(&mut controller, &id);
    controller.discard_excluded(|_| false);
    controller.finish_load(&id, LoadResult::Cancelled);
    assert_eq!(cancellations.get(), 1);
    assert!(controller.inbox_of(&id).is_none());
    assert!(!controller.is_loading());
}

#[test]
fn quitting_cancels_the_running_load() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    let cancellations = start_load(&mut controller, &id);
    controller.cancel_load();
    assert_eq!(cancellations.get(), 1);
    assert!(!controller.is_loading());
}

#[test]
fn discarding_received_mail_of_an_account_no_longer_shown_is_recorded() {
    let record = start_record(LogLevel::Info);
    let shown = account("record_discard_shown");
    let excluded = account("record_discard_excluded");
    let failed = account("record_discard_failed");
    let mut controller = InboxController::default();
    for (account_id, result) in [
        (&shown, LoadResult::Received(batch_of(&shown, &[10]))),
        (
            &excluded,
            LoadResult::Received(batch_of(&excluded, &[10, 20])),
        ),
        (&failed, LoadResult::Failed(sign_in_failure())),
    ] {
        start_load(&mut controller, account_id);
        controller.finish_load(account_id, result);
    }
    let before_discarding = record.text().lines().count();
    controller.discard_excluded(|account_id| *account_id == shown);
    let text = record.text();
    let lines: Vec<&str> = text.lines().skip(before_discarding).collect();
    assert_eq!(lines.len(), 1, "only a received batch holds mail: {text}");
    let account = r#"account="record_discard_excluded""#;
    assert!(
        lines[0].contains(" INFO ") && lines[0].contains(account),
        "{text}"
    );
    assert!(lines[0].contains("messages=2"), "{text}");
}

fn message_with(uid: u32, content: ReceivedContent) -> ReceivedMessage {
    ReceivedMessage {
        identity: MessageIdentity::ImapUid(uid),
        fields: DisplayFields::default(),
        internal_date: None,
        seen: false,
        content,
        gmail: None,
    }
}

#[test]
fn unreadable_content_and_a_refused_list_each_warn_without_server_text() {
    let record = start_record(LogLevel::Debug);
    let loaded = account("account_1726920000_0");
    let mut controller = InboxController::default();
    start_load(&mut controller, &loaded);
    let batch = ReceivedBatch {
        account_id: loaded.clone(),
        uid_validity: Some(1),
        incomplete: Some(IncompleteList::ServerRefused(ServerReply {
            code: Some("LIMIT".to_owned()),
            text: "private refusal text".to_owned(),
        })),
        messages: vec![
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
    };
    controller.finish_load(&loaded, LoadResult::Received(batch));
    let text = record.text();
    let warnings: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(" WARN "))
        .collect();
    assert_eq!(warnings.len(), 2, "{text}");
    assert!(warnings[0].contains("messages=1"), "{}", warnings[0]);
    assert!(warnings[1].contains(r#"code="LIMIT""#), "{}", warnings[1]);
    assert!(!text.contains("private refusal text"), "{text}");
}

#[test]
fn each_failed_load_is_one_error_line_naming_its_cause() {
    let refused_sign_in = ImapError {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: Some("AUTHENTICATIONFAILED".to_owned()),
            text: "private server text".to_owned(),
        }),
        alerts: vec!["private alert".to_owned()],
    };
    let failures = [
        (
            LoadFailure::Imap(refused_sign_in),
            r#"cause=Failed(SignIn) code="AUTHENTICATIONFAILED" alerts=1"#,
        ),
        (
            LoadFailure::Imap(ImapFailure::TimedOut(ImapStep::FetchText).into()),
            "cause=TimedOut(FetchText)",
        ),
        (
            LoadFailure::Imap(ImapFailure::InboxChanged.into()),
            "cause=InboxChanged",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::Timeout),
            "cause=Timeout",
        ),
        (
            LoadFailure::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused {
                    status: 401,
                    code: Some("InvalidAuthenticationToken".to_owned()),
                },
                reason: Some("private server text".to_owned()),
            }),
            r#"cause=Refused status=401 code="InvalidAuthenticationToken""#,
        ),
        (LoadFailure::WorkerStopped(None), "cause=WorkerStopped"),
    ];
    for (failure, fields) in failures {
        let record = start_record(LogLevel::Debug);
        let failed = account("account_1726920000_1");
        let mut controller = InboxController::default();
        start_load(&mut controller, &failed);
        controller.finish_load(&failed, LoadResult::Failed(failure));
        let text = record.text();
        let errors: Vec<&str> = text
            .lines()
            .filter(|line| line.contains(" ERROR "))
            .collect();
        assert_eq!(errors.len(), 1, "{text}");
        assert!(
            errors[0].contains("Inbox load failed") && errors[0].contains(fields),
            "{fields}: {}",
            errors[0]
        );
        assert!(
            !text.contains("private") && !text.contains(" WARN "),
            "{text}"
        );
    }
}

#[test]
fn a_cancelled_load_is_one_info_line_whatever_follows() {
    let record = start_record(LogLevel::Debug);
    let excluded = account("account_1726920000_2");
    let mut controller = InboxController::default();
    start_load(&mut controller, &excluded);
    controller.discard_excluded(|_| false);
    controller.discard_excluded(|_| false);
    controller.cancel_load();
    controller.finish_load(&excluded, LoadResult::Cancelled);
    let closed = account("account_1726920000_3");
    start_load(&mut controller, &closed);
    controller.cancel_load();
    controller.finish_load(&closed, LoadResult::Cancelled);
    let text = record.text();
    let cancelled: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("Inbox load cancelled"))
        .collect();
    assert_eq!(cancelled.len(), 2, "{text}");
    assert!(
        cancelled[0].contains(r#"account="account_1726920000_2""#)
            && cancelled[0].contains(r#"reason="account excluded""#)
    );
    assert!(
        cancelled[1].contains(r#"account="account_1726920000_3""#)
            && cancelled[1].contains(r#"reason="quitting""#)
    );
    assert!(
        !text.contains(" WARN ") && !text.contains(" ERROR "),
        "{text}"
    );
    assert!(
        !text.contains("finished") && !text.contains("discarded"),
        "{text}"
    );
}

#[test]
fn a_late_result_for_an_excluded_account_writes_no_outcome() {
    for late_result in [
        LoadResult::Received(batch_of(&account("account_1726920000_4"), &[10])),
        LoadResult::Failed(sign_in_failure()),
    ] {
        let record = start_record(LogLevel::Debug);
        let excluded = account("account_1726920000_4");
        let mut controller = InboxController::default();
        start_load(&mut controller, &excluded);
        controller.discard_excluded(|_| false);
        controller.finish_load(&excluded, late_result);
        let text = record.text();
        for outcome in ["finished", " WARN ", " ERROR "] {
            assert!(
                !text.contains(outcome),
                "{outcome} for a discarded result:\n{text}"
            );
        }
    }
}
