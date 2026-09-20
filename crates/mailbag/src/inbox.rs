// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail each account received in this run, and the one load that fills it.

#[cfg(test)]
mod tests;

use goa_adapter::{AccountId, ImapAccessError};
use mailbag_content::{ContentExplanation, DisplayFields};
use mailbag_imap::{ImapError, ImapFailure, ServerReply};
use std::{collections::BTreeMap, fmt, rc::Rc};

/// One account's Inbox as a single load received it.
pub struct ReceivedBatch {
    pub account_id: AccountId,
    /// The Inbox version these UIDs belong to.
    pub uid_validity: Option<u32>,
    /// Newest first, at most 100.
    pub messages: Vec<ReceivedMessage>,
}

/// One message of a batch. Raw MIME is released once it is decoded.
pub struct ReceivedMessage {
    pub uid: u32,
    pub fields: DisplayFields,
    /// INTERNALDATE as seconds since the Unix epoch.
    pub internal_date: Option<i64>,
    pub seen: bool,
    pub content: ReceivedContent,
}

/// The text of a message, or why the reader shows none.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceivedContent {
    Text(String),
    Explained(ContentExplanation),
}

/// Why a refresh delivered no mail, at the step where it stopped.
#[derive(Clone, PartialEq, Eq)]
pub enum LoadFailure {
    /// Online Accounts did not give the settings or the password.
    OnlineAccounts(ImapAccessError),
    /// The connection, the sign-in or the transfer failed.
    Server(ServerFailure),
    /// The mail worker stopped without a result.
    WorkerStopped,
}

/// A failed server step, with what the server said about it.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerFailure {
    pub failure: ImapFailure,
    /// The server's own reason, for the failure explanation only.
    pub server_reply: Option<ServerReply>,
    pub alerts: Vec<String>,
}

impl From<ImapFailure> for ServerFailure {
    /// A failure the load itself found, which the server did not explain.
    fn from(failure: ImapFailure) -> Self {
        Self {
            failure,
            server_reply: None,
            alerts: Vec::new(),
        }
    }
}

impl From<ImapError> for ServerFailure {
    fn from(error: ImapError) -> Self {
        Self {
            failure: error.failure,
            server_reply: error.server_reply,
            alerts: error.alerts,
        }
    }
}

/// How one load ended.
#[derive(Debug)]
pub enum LoadResult {
    Received(ReceivedBatch),
    Failed(LoadFailure),
    /// Cancelled by a confirmed exclusion or by quitting; the connection is
    /// closed, so the next refresh may start.
    Cancelled,
}

/// What one account shows for its mail. No entry means that nothing has been
/// loaded for it in this run.
#[derive(Debug)]
pub enum AccountInbox {
    Loading,
    Received(Rc<ReceivedBatch>),
    Failed(LoadFailure),
}

/// The step a running load is on, such as the Online Accounts request or the
/// transfer on the mail worker. Dropping it cancels the load.
pub trait CancelsLoadOnDrop {}

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
        let finished = self
            .running_load
            .as_ref()
            .is_some_and(|running| running.account_id == *account_id);
        if !finished {
            return;
        }
        self.running_load = None;
        match result {
            LoadResult::Received(batch) => {
                self.show_result(account_id, AccountInbox::Received(Rc::new(batch)));
            }
            LoadResult::Failed(failure) => {
                self.show_result(account_id, AccountInbox::Failed(failure));
            }
            LoadResult::Cancelled => {}
        }
    }

    /// Discards the mail of accounts Online Accounts no longer shows and
    /// cancels a load running for one of them.
    pub fn discard_excluded(&mut self, is_visible: impl Fn(&AccountId) -> bool) {
        self.inboxes.retain(|account_id, _| is_visible(account_id));
        if let Some(running) = self
            .running_load
            .as_mut()
            .filter(|running| !is_visible(&running.account_id))
        {
            // Dropping the step closes the connection; the load then reports
            // that it was cancelled.
            running.cancellation = None;
        }
    }

    /// Quit: cancels a running load. The worker closes its connection on its
    /// own thread, which GTK never waits for.
    pub fn cancel_load(&mut self) {
        self.running_load = None;
    }

    /// A result reaches the account only while its mail is still loading, so
    /// mail discarded by a confirmed exclusion stays discarded.
    fn show_result(&mut self, account_id: &AccountId, inbox: AccountInbox) {
        if let Some(loading) = self
            .inboxes
            .get_mut(account_id)
            .filter(|inbox| matches!(inbox, AccountInbox::Loading))
        {
            *loading = inbox;
        }
    }
}

// Received mail is shown to the user, never written to diagnostics.
impl fmt::Debug for ReceivedBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceivedBatch")
            .field("account_id", &self.account_id)
            .field("uid_validity", &self.uid_validity)
            .field("messages", &self.messages)
            .finish()
    }
}

impl fmt::Debug for ReceivedMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceivedMessage")
            .field("uid", &self.uid)
            .field("seen", &self.seen)
            .field("content", &self.content)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ReceivedContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => write!(formatter, "Text({} characters)", text.chars().count()),
            Self::Explained(explanation) => write!(formatter, "Explained({explanation:?})"),
        }
    }
}

impl fmt::Debug for LoadFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnlineAccounts(error) => write!(formatter, "OnlineAccounts({error:?})"),
            Self::Server(failure) => write!(formatter, "Server({failure:?})"),
            Self::WorkerStopped => write!(formatter, "WorkerStopped"),
        }
    }
}

impl fmt::Debug for ServerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerFailure")
            .field("failure", &self.failure)
            .field(
                "server_code",
                &self.server_reply.as_ref().map(|reply| &reply.code),
            )
            .field("alert_count", &self.alerts.len())
            .finish()
    }
}
