// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::logging::{LogLevel, capture::start_record};
use mailbag_domain::FailureKind;
use std::{cell::Cell, rc::Rc};

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn stored() -> LoadResult {
    LoadResult::Stored { incomplete: None }
}

fn sign_in_failure() -> Failure {
    Failure {
        kind: FailureKind::ServerRejectedSignIn,
        remote_texts: Vec::new(),
        details: "Failure: ServerRejectedSignIn".to_owned(),
    }
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

fn is_stored(outcome: Option<&RefreshOutcome>) -> bool {
    matches!(outcome, Some(RefreshOutcome::Stored(None)))
}

/// Starts a load that has reached the mail worker.
fn start_load(controller: &mut InboxController, account_id: &AccountId) -> Rc<Cell<usize>> {
    let cancellations = Rc::new(Cell::new(0));
    assert!(!controller.is_loading());
    controller.begin_load(account_id, counted_step(&cancellations));
    cancellations
}

#[test]
fn an_account_without_a_refresh_has_no_outcome_and_no_load() {
    let controller = InboxController::default();
    assert!(controller.outcome_of(&account("never-refreshed")).is_none());
    assert!(!controller.is_loading());
}

#[test]
fn a_refresh_keeps_the_previous_outcome_until_its_load_ends() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    start_load(&mut controller, &id);
    controller.finish_load(&id, LoadResult::Failed(sign_in_failure()));
    start_load(&mut controller, &id);
    assert!(controller.is_loading_account(&id));
    assert!(matches!(
        controller.outcome_of(&id),
        Some(RefreshOutcome::Failed(_))
    ));
    controller.finish_load(&id, stored());
    assert!(is_stored(controller.outcome_of(&id)));
    assert!(!controller.is_loading());
}

#[test]
fn refresh_inbox_is_unavailable_while_a_load_runs() {
    let mut controller = InboxController::default();
    let loading = account("loading-account");
    let other = account("other-account");
    let cancellations = start_load(&mut controller, &loading);
    assert!(controller.is_loading());
    assert!(!controller.is_loading_account(&other));
    assert_eq!(cancellations.get(), 0);

    controller.finish_load(&loading, stored());
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
    controller.finish_load(&selected, stored());
    assert!(controller.outcome_of(&selected).is_none());
    assert!(controller.is_loading());

    controller.finish_load(&loading, stored());
    assert!(is_stored(controller.outcome_of(&loading)));
    assert!(!controller.is_loading());
}

#[test]
fn a_confirmed_exclusion_forgets_the_outcome_and_cancels_its_load() {
    let mut controller = InboxController::default();
    let excluded = account("excluded-account");
    let kept = account("kept-account");
    start_load(&mut controller, &kept);
    controller.finish_load(&kept, stored());
    start_load(&mut controller, &excluded);
    controller.finish_load(&excluded, LoadResult::Failed(sign_in_failure()));
    let cancellations = start_load(&mut controller, &excluded);

    controller.discard_excluded(|account_id| *account_id == kept);
    assert_eq!(cancellations.get(), 1);
    assert!(controller.outcome_of(&excluded).is_none());
    assert!(is_stored(controller.outcome_of(&kept)));
    // The load ends only once its connection is closed.
    assert!(controller.is_loading());

    // A result that arrives after the exclusion records nothing.
    controller.finish_load(&excluded, stored());
    assert!(controller.outcome_of(&excluded).is_none());
    assert!(!controller.is_loading());
}

#[test]
fn a_cancelled_load_ends_without_an_outcome() {
    let mut controller = InboxController::default();
    let id = account("generic-imap");
    let cancellations = start_load(&mut controller, &id);
    controller.discard_excluded(|_| false);
    controller.finish_load(&id, LoadResult::Cancelled);
    assert_eq!(cancellations.get(), 1);
    assert!(controller.outcome_of(&id).is_none());
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
    for late_result in [stored(), LoadResult::Failed(sign_in_failure())] {
        let record = start_record(LogLevel::Debug);
        let excluded = account("account_1726920000_4");
        let mut controller = InboxController::default();
        start_load(&mut controller, &excluded);
        controller.discard_excluded(|_| false);
        controller.finish_load(&excluded, late_result);
        assert!(controller.outcome_of(&excluded).is_none());
        let text = record.text();
        for outcome in ["finished", " WARN ", " ERROR "] {
            assert!(
                !text.contains(outcome),
                "{outcome} for a discarded result:\n{text}"
            );
        }
    }
}
