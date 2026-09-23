// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail each account received in this run, and the one load that fills it.
//! The batch itself and the load belong to `mailbag-providers`; this module
//! keeps what the window shows and writes the record of a finished load.

#[cfg(test)]
mod tests;

use goa_adapter::AccountId;
use mailbag_content::ContentExplanation;
use mailbag_providers::{
    CancelsLoadOnDrop, LoadFailure, LoadResult, ReceivedBatch, ReceivedContent, ServerFailure,
};
use std::{collections::BTreeMap, fmt, rc::Rc};

/// What one account shows for its mail. No entry means that nothing has been
/// loaded for it in this run.
#[derive(Debug)]
pub enum AccountInbox {
    Loading,
    Received(Rc<ReceivedBatch>),
    Failed(LoadFailure),
}

/// Each account's received mail and the single load that may be running.
#[derive(Default)]
pub struct InboxController {
    inboxes: BTreeMap<AccountId, AccountInbox>,
    running_load: Option<RunningLoad>,
}

struct RunningLoad {
    account_id: AccountId,
    /// None once the load has been cancelled and is closing its connection.
    cancellation: Option<Box<dyn CancelsLoadOnDrop>>,
}

impl InboxController {
    pub fn inbox_of(&self, account_id: &AccountId) -> Option<&AccountInbox> {
        self.inboxes.get(account_id)
    }

    /// While a load runs, Refresh Inbox stays unavailable and the sidebar
    /// shows its spinner.
    pub fn is_loading(&self) -> bool {
        self.running_load.is_some()
    }

    /// Refresh Inbox: clears the account's mail and enters Loading. Returns
    /// false while another load runs, because Refresh is not queued.
    pub fn begin_load(&mut self, account_id: &AccountId) -> bool {
        if self.running_load.is_some() {
            return false;
        }
        self.inboxes
            .insert(account_id.clone(), AccountInbox::Loading);
        tracing::info!(account = account_id.as_str(), "Inbox load started");
        self.running_load = Some(RunningLoad {
            account_id: account_id.clone(),
            cancellation: None,
        });
        true
    }

    /// Keeps what cancels the load `begin_load` started. A load that already
    /// reported its result, such as a settings failure Online Accounts
    /// answered at once, cancels the handle here instead.
    pub fn hold_cancellation(
        &mut self,
        account_id: &AccountId,
        cancellation: Box<dyn CancelsLoadOnDrop>,
    ) {
        match self.running_load.as_mut() {
            Some(running) if running.account_id == *account_id => {
                running.cancellation = Some(cancellation);
            }
            _ => drop(cancellation),
        }
    }

    /// Stores how the load ended under the account it was started for, and
    /// leaves Loading so Refresh Inbox becomes available again.
    pub fn finish_load(&mut self, account_id: &AccountId, result: LoadResult) {
        if self
            .running_load
            .take_if(|running| running.account_id == *account_id)
            .is_none()
        {
            return;
        }
        let account = account_id.as_str();
        let inbox = match result {
            // The cancellation was recorded where it was requested.
            LoadResult::Cancelled => return,
            _ if !self.awaits_result(account_id) => {
                tracing::info!(
                    account,
                    "Inbox load result discarded: the account is no longer shown"
                );
                return;
            }
            LoadResult::Received(batch) => {
                log_received_batch(account, &batch);
                AccountInbox::Received(Rc::new(batch))
            }
            LoadResult::Failed(failure) => {
                log_load_failure(account, &failure);
                AccountInbox::Failed(failure)
            }
        };
        self.inboxes.insert(account_id.clone(), inbox);
    }

    /// Discards the mail of accounts Online Accounts no longer shows and
    /// cancels a load running for one of them.
    pub fn discard_excluded(&mut self, is_visible: impl Fn(&AccountId) -> bool) {
        self.inboxes.retain(|account_id, inbox| {
            let visible = is_visible(account_id);
            // Only a received batch holds mail; a failed or running load holds none.
            if !visible
                && let AccountInbox::Received(batch) = inbox
                && !batch.messages.is_empty()
            {
                tracing::info!(
                    account = account_id.as_str(),
                    messages = batch.messages.len(),
                    "mail of an account no longer shown was discarded"
                );
            }
            visible
        });
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

    /// A result reaches the account only while its mail is still loading, so
    /// mail discarded by a confirmed exclusion stays discarded.
    fn awaits_result(&self, account_id: &AccountId) -> bool {
        matches!(self.inboxes.get(account_id), Some(AccountInbox::Loading))
    }
}

/// How an accepted load ended, with warnings for what the reader cannot show.
fn log_received_batch(account: &str, batch: &ReceivedBatch) {
    let explanations = || {
        batch
            .messages
            .iter()
            .filter_map(|message| match &message.content {
                ReceivedContent::Explained(explanation) => Some(explanation),
                ReceivedContent::Text(_) => None,
            })
    };
    tracing::info!(
        account,
        messages = batch.messages.len(),
        unsupported = explanations()
            .filter(|explanation| is_unsupported(explanation))
            .count(),
        "Inbox load finished"
    );
    let unreadable = explanations()
        .filter(|explanation| !is_unsupported(explanation))
        .count();
    if unreadable > 0 {
        tracing::warn!(
            account,
            messages = unreadable,
            "some messages have content that could not be read"
        );
    }
    if let Some(refusal) = &batch.list_refusal {
        tracing::warn!(
            account,
            code = refusal.code.as_deref(),
            "the server refused to finish the message list"
        );
    }
}

/// Content this version does not show by design, as opposed to content that
/// could not be read.
fn is_unsupported(explanation: &ContentExplanation) -> bool {
    matches!(
        explanation,
        ContentExplanation::NoPlainText { .. }
            | ContentExplanation::Encrypted
            | ContentExplanation::SecuredWithSMime
    )
}

/// The load's single error line: the failure value the UI explains, the
/// server's response code and the number of alerts, never the server's text.
/// The failure values hold no server text, so the record can name them as they
/// are (`ImapFailure`, `ImapAccessError`).
fn log_load_failure(account: &str, failure: &LoadFailure) {
    let (cause, server): (&dyn fmt::Debug, Option<&ServerFailure>) = match failure {
        LoadFailure::OnlineAccounts(error) => (error, None),
        LoadFailure::Server(server) => (&server.failure, Some(server)),
        LoadFailure::WorkerStopped => (&"WorkerStopped", None),
    };
    tracing::error!(
        account,
        cause = ?cause,
        code = server.and_then(|server| server.server_reply.as_ref()?.code.as_deref()),
        alerts = server
            .map(|server| server.alerts.len())
            .filter(|alerts| *alerts > 0),
        "Inbox load failed"
    );
}
