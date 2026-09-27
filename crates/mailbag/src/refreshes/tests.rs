// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::logging::{LogLevel, capture::start_record};
use mailbag_domain::{FailureKind, FolderRef};
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

fn is_stored(outcome: Option<(&LoadTarget, &RefreshOutcome)>) -> bool {
    matches!(outcome, Some((_, RefreshOutcome::Stored(None))))
}

/// Starts a load of the account's folder list that has reached the mail
/// worker.
fn start_load(refreshes: &mut Refreshes, account_id: &AccountId) -> Rc<Cell<usize>> {
    start_load_of(refreshes, account_id, LoadTarget::FolderList)
}

fn start_load_of(
    refreshes: &mut Refreshes,
    account_id: &AccountId,
    target: LoadTarget,
) -> Rc<Cell<usize>> {
    let cancellations = Rc::new(Cell::new(0));
    assert!(!refreshes.is_loading());
    refreshes.begin_load(account_id, target, counted_step(&cancellations));
    cancellations
}

#[test]
fn an_account_without_a_refresh_has_no_outcome_and_no_load() {
    let refreshes = Refreshes::default();
    assert!(refreshes.outcome_of(&account("never-refreshed")).is_none());
    assert!(!refreshes.is_loading());
}

#[test]
fn a_refresh_keeps_the_previous_outcome_until_its_load_ends() {
    let mut refreshes = Refreshes::default();
    let id = account("generic-imap");
    start_load(&mut refreshes, &id);
    refreshes.finish_load(&id, LoadResult::Failed(sign_in_failure()));
    start_load(&mut refreshes, &id);
    assert_eq!(refreshes.loading_target(&id), Some(&LoadTarget::FolderList));
    assert!(matches!(
        refreshes.outcome_of(&id),
        Some((_, RefreshOutcome::Failed(_)))
    ));
    refreshes.finish_load(&id, stored());
    assert!(is_stored(refreshes.outcome_of(&id)));
    assert!(!refreshes.is_loading());
}

#[test]
fn a_refresh_is_unavailable_while_a_load_runs() {
    let mut refreshes = Refreshes::default();
    let loading = account("loading-account");
    let other = account("other-account");
    let cancellations = start_load(&mut refreshes, &loading);
    assert!(refreshes.is_loading());
    assert_eq!(refreshes.loading_target(&other), None);
    assert_eq!(cancellations.get(), 0);

    refreshes.finish_load(&loading, stored());
    assert!(!refreshes.is_loading());
    start_load(&mut refreshes, &other);
    assert!(refreshes.is_loading());
}

/// An account keeps one outcome, of its latest load whatever it loaded,
/// with what that load loaded (specs/008-folders FR-010).
#[test]
fn an_account_keeps_the_outcome_of_its_latest_load_with_its_target() {
    let mut refreshes = Refreshes::default();
    let id = account("generic-imap");
    let inbox = LoadTarget::Mailbox(FolderRef {
        account: id.clone(),
        identity: "INBOX".to_owned(),
    });
    start_load_of(&mut refreshes, &id, inbox.clone());
    assert_eq!(refreshes.loading_target(&id), Some(&inbox));
    refreshes.finish_load(&id, LoadResult::Failed(sign_in_failure()));
    assert!(matches!(
        refreshes.outcome_of(&id),
        Some((target, RefreshOutcome::Failed(_))) if *target == inbox
    ));

    // A folder list without any folder is a completed load, which replaces
    // the mailbox's failure.
    start_load(&mut refreshes, &id);
    refreshes.finish_load(&id, LoadResult::EmptyFolderList);
    assert!(matches!(
        refreshes.outcome_of(&id),
        Some((LoadTarget::FolderList, RefreshOutcome::Stored(None)))
    ));
}

#[test]
fn a_result_reaches_only_the_account_its_load_started_for() {
    let mut refreshes = Refreshes::default();
    let loading = account("loading-account");
    let selected = account("selected-account");
    start_load(&mut refreshes, &loading);
    refreshes.finish_load(&selected, stored());
    assert!(refreshes.outcome_of(&selected).is_none());
    assert!(refreshes.is_loading());

    refreshes.finish_load(&loading, stored());
    assert!(is_stored(refreshes.outcome_of(&loading)));
    assert!(!refreshes.is_loading());
}

#[test]
fn a_confirmed_exclusion_forgets_the_outcome_and_cancels_its_load() {
    let mut refreshes = Refreshes::default();
    let excluded = account("excluded-account");
    let kept = account("kept-account");
    start_load(&mut refreshes, &kept);
    refreshes.finish_load(&kept, stored());
    start_load(&mut refreshes, &excluded);
    refreshes.finish_load(&excluded, LoadResult::Failed(sign_in_failure()));
    let cancellations = start_load(&mut refreshes, &excluded);

    refreshes.discard_excluded(|account_id| *account_id == kept);
    assert_eq!(cancellations.get(), 1);
    assert!(refreshes.outcome_of(&excluded).is_none());
    assert!(is_stored(refreshes.outcome_of(&kept)));
    // The load ends only once its connection is closed.
    assert!(refreshes.is_loading());

    // A result that arrives after the exclusion records nothing.
    refreshes.finish_load(&excluded, stored());
    assert!(refreshes.outcome_of(&excluded).is_none());
    assert!(!refreshes.is_loading());
}

#[test]
fn a_cancelled_load_ends_without_an_outcome() {
    let mut refreshes = Refreshes::default();
    let id = account("generic-imap");
    let cancellations = start_load(&mut refreshes, &id);
    refreshes.discard_excluded(|_| false);
    refreshes.finish_load(&id, LoadResult::Cancelled);
    assert_eq!(cancellations.get(), 1);
    assert!(refreshes.outcome_of(&id).is_none());
    assert!(!refreshes.is_loading());
}

#[test]
fn quitting_cancels_the_running_load() {
    let mut refreshes = Refreshes::default();
    let id = account("generic-imap");
    let cancellations = start_load(&mut refreshes, &id);
    refreshes.cancel_load();
    assert_eq!(cancellations.get(), 1);
    assert!(!refreshes.is_loading());
}

#[test]
fn a_cancelled_load_is_one_info_line_whatever_follows() {
    let record = start_record(LogLevel::Debug);
    let excluded = account("account_1726920000_2");
    let mut refreshes = Refreshes::default();
    start_load(&mut refreshes, &excluded);
    refreshes.discard_excluded(|_| false);
    refreshes.discard_excluded(|_| false);
    refreshes.cancel_load();
    refreshes.finish_load(&excluded, LoadResult::Cancelled);
    let closed = account("account_1726920000_3");
    start_load(&mut refreshes, &closed);
    refreshes.cancel_load();
    refreshes.finish_load(&closed, LoadResult::Cancelled);
    let text = record.text();
    let cancelled: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("load cancelled"))
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
    assert!(!text.contains("ignored"), "{text}");
}

#[test]
fn a_late_failure_for_an_excluded_account_records_no_outcome() {
    let excluded = account("account_1726920000_4");
    let mut refreshes = Refreshes::default();
    start_load(&mut refreshes, &excluded);
    refreshes.discard_excluded(|_| false);
    refreshes.finish_load(&excluded, LoadResult::Failed(sign_in_failure()));
    assert!(refreshes.outcome_of(&excluded).is_none());
}
