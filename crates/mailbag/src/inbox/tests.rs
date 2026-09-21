// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use mailbag_imap::ImapStep;
use std::cell::Cell;

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn batch_of(account_id: &AccountId, uids: &[u32]) -> ReceivedBatch {
    ReceivedBatch {
        account_id: account_id.clone(),
        uid_validity: Some(1),
        list_refusal: None,
        messages: uids
            .iter()
            .map(|uid| ReceivedMessage {
                uid: *uid,
                fields: DisplayFields::default(),
                internal_date: None,
                seen: false,
                content: ReceivedContent::Text("Text".to_owned()),
            })
            .collect(),
    }
}

fn sign_in_failure() -> LoadFailure {
    LoadFailure::Server(ImapFailure::Failed(ImapStep::SignIn).into())
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
        Some(AccountInbox::Received(batch)) => batch.messages.iter().map(|m| m.uid).collect(),
        other => panic!("the account shows no batch: {other:?}"),
    }
}

/// Starts a load that has reached the mail worker.
fn start_load(controller: &mut InboxController, account_id: &AccountId) -> Rc<Cell<usize>> {
    let cancellations = Rc::new(Cell::new(0));
    assert!(controller.begin_load(account_id));
    controller.hold_cancellation(account_id, counted_step(&cancellations));
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
    assert!(!controller.begin_load(&loading));
    assert!(!controller.begin_load(&other));
    assert!(controller.inbox_of(&other).is_none());
    assert_eq!(cancellations.get(), 0);

    controller.finish_load(&loading, LoadResult::Received(batch_of(&loading, &[10])));
    assert!(controller.begin_load(&other));
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
        Some(AccountInbox::Failed(LoadFailure::Server(_)))
    ));
    assert!(!controller.is_loading());
}

#[test]
fn a_confirmed_exclusion_discards_the_mail_and_cancels_its_load() {
    let mut controller = InboxController::default();
    let excluded = account("excluded-account");
    let kept = account("kept-account");
    controller.begin_load(&kept);
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
