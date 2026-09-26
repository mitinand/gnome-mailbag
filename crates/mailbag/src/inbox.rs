// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! How each account's latest refresh in this run ended, and the one load that
//! may be running. The mail itself is in the store, which a load writes and
//! the window reads (specs/007-mail-storage FR-001); a refresh's outcome stays
//! in memory (specs/006-error-handling FR-007).

#[cfg(test)]
mod tests;

use mailbag_domain::{AccountId, Failure, IncompleteList};
use mailbag_providers::{CancelsLoadOnDrop, LoadResult};
use std::collections::BTreeMap;

/// How an account's latest refresh in this run ended. No entry means that no
/// refresh of it ended in this run.
#[derive(Debug)]
pub enum RefreshOutcome {
    /// The load's messages are stored; `Some` says why some are missing.
    Stored(Option<IncompleteList>),
    Failed(Failure),
}

/// Each account's latest refresh outcome and the single load that may be
/// running.
#[derive(Default)]
pub struct InboxController {
    outcomes: BTreeMap<AccountId, RefreshOutcome>,
    running_load: Option<RunningLoad>,
}

struct RunningLoad {
    account_id: AccountId,
    /// None once the load has been cancelled and is closing its connection.
    cancellation: Option<Box<dyn CancelsLoadOnDrop>>,
}

impl InboxController {
    pub fn outcome_of(&self, account_id: &AccountId) -> Option<&RefreshOutcome> {
        self.outcomes.get(account_id)
    }

    /// While a load runs, Refresh Inbox stays unavailable and the sidebar
    /// shows its spinner.
    pub fn is_loading(&self) -> bool {
        self.running_load.is_some()
    }

    /// Whether the running load is this account's.
    pub fn is_loading_account(&self, account_id: &AccountId) -> bool {
        self.running_load
            .as_ref()
            .is_some_and(|running| running.account_id == *account_id)
    }

    /// Refresh Inbox: keeps what cancels the load just started. The latest
    /// outcome stays until the load ends, so a banner stays over the stored
    /// rows meanwhile. The caller checks `is_loading` first, because Refresh
    /// is not queued.
    pub fn begin_load(&mut self, account_id: &AccountId, cancellation: Box<dyn CancelsLoadOnDrop>) {
        debug_assert!(self.running_load.is_none(), "one load at a time");
        tracing::info!(account = account_id.as_str(), "Inbox load started");
        self.running_load = Some(RunningLoad {
            account_id: account_id.clone(),
            cancellation: Some(cancellation),
        });
    }

    /// Records how the load ended under the account it was started for, and
    /// leaves loading so Refresh Inbox becomes available again. The result of
    /// a load cancelled by an exclusion is not recorded.
    pub fn finish_load(&mut self, account_id: &AccountId, result: LoadResult) {
        let Some(running) = self
            .running_load
            .take_if(|running| running.account_id == *account_id)
        else {
            return;
        };
        let outcome = match result {
            // The cancellation was recorded where it was requested.
            LoadResult::Cancelled => return,
            _ if running.cancellation.is_none() => {
                tracing::info!(
                    account = account_id.as_str(),
                    "Inbox load result discarded: the account is no longer shown"
                );
                return;
            }
            LoadResult::Stored { incomplete } => RefreshOutcome::Stored(incomplete),
            LoadResult::Failed(failure) => RefreshOutcome::Failed(failure),
        };
        self.outcomes.insert(account_id.clone(), outcome);
    }

    /// Forgets the outcomes of accounts Online Accounts no longer shows and
    /// cancels a load running for one of them.
    pub fn discard_excluded(&mut self, is_visible: impl Fn(&AccountId) -> bool) {
        self.outcomes.retain(|account_id, _| is_visible(account_id));
        if let Some(running) = self
            .running_load
            .as_mut()
            .filter(|running| !is_visible(&running.account_id))
        {
            // Dropping the step closes the connection; the load then reports
            // that it was cancelled.
            if let Some(cancellation) = running.cancellation.take() {
                tracing::info!(
                    account = running.account_id.as_str(),
                    reason = "account excluded",
                    "Inbox load cancelled"
                );
                drop(cancellation);
            }
        }
    }

    /// Quit: cancels a running load. The worker closes its connection on its
    /// own thread, which GTK never waits for.
    pub fn cancel_load(&mut self) {
        if let Some(running) = self.running_load.take()
            && running.cancellation.is_some()
        {
            tracing::info!(
                account = running.account_id.as_str(),
                reason = "quitting",
                "Inbox load cancelled"
            );
        }
    }
}
