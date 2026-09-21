// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail each account received in this run, and the one load that fills it.

#[cfg(test)]
mod tests;

use goa_adapter::{AccountId, ImapAccessError};
use mailbag_content::{ContentExplanation, DisplayFields};
use mailbag_imap::{ImapError, ImapFailure, ImapStep, ServerReply};
use std::{collections::BTreeMap, fmt, rc::Rc};

/// One account's Inbox as a single load received it.
pub struct ReceivedBatch {
    pub account_id: AccountId,
    /// The Inbox version these UIDs belong to.
    pub uid_validity: Option<u32>,
    /// Newest first, at most 100.
    pub messages: Vec<ReceivedMessage>,
    /// What the server said when it refused to finish the message list, which
    /// means messages are missing from this batch. `None` when it is complete.
    pub list_refusal: Option<ServerReply>,
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
    /// Names the account on every line of the load, in any crate and thread.
    span: tracing::Span,
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
        let span = tracing::error_span!("load", account = account_id.as_str());
        span.in_scope(|| tracing::info!("Inbox load started"));
        self.running_load = Some(RunningLoad {
            account_id: account_id.clone(),
            cancellation: None,
            span,
        });
        true
    }

    /// The running load's span, which the loader and its callbacks enter.
    pub fn load_span(&self) -> tracing::Span {
        self.running_load
            .as_ref()
            .map_or_else(tracing::Span::none, |running| running.span.clone())
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
        let Some(running) = self
            .running_load
            .take_if(|running| running.account_id == *account_id)
        else {
            return;
        };
        let _load = running.span.enter();
        let inbox = match result {
            // The cancellation was recorded where it was requested.
            LoadResult::Cancelled => return,
            _ if !self.awaits_result(account_id) => {
                tracing::info!("Inbox load result discarded: the account is no longer shown");
                return;
            }
            LoadResult::Received(batch) => {
                log_received_batch(&batch);
                AccountInbox::Received(Rc::new(batch))
            }
            LoadResult::Failed(failure) => {
                log_load_failure(&failure);
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
                running.span.in_scope(|| {
                    tracing::info!(reason = "account excluded", "Inbox load cancelled");
                });
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
            running.span.in_scope(|| {
                tracing::info!(reason = "quitting", "Inbox load cancelled");
            });
        }
    }

    /// A result reaches the account only while its mail is still loading, so
    /// mail discarded by a confirmed exclusion stays discarded.
    fn awaits_result(&self, account_id: &AccountId) -> bool {
        matches!(self.inboxes.get(account_id), Some(AccountInbox::Loading))
    }
}

/// How an accepted load ended, with warnings for what the reader cannot show
/// (log-events.md "Inbox load").
fn log_received_batch(batch: &ReceivedBatch) {
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
            messages = unreadable,
            "some messages have content that could not be read"
        );
    }
    if let Some(refusal) = &batch.list_refusal {
        tracing::warn!(
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

/// The load's single error line: the failure values the UI explains, the
/// server's response code and the number of alerts, never the server's text.
fn log_load_failure(failure: &LoadFailure) {
    let (step, cause, server) = match failure {
        LoadFailure::OnlineAccounts(error) => {
            (Some("OnlineAccounts"), access_failure_name(*error), None)
        }
        LoadFailure::Server(server) => {
            let (step, cause) = match server.failure {
                ImapFailure::Failed(step) => (Some(step_name(step)), "Failed"),
                ImapFailure::TimedOut(step) => (Some(step_name(step)), "TimedOut"),
                ImapFailure::NoSignInMethod => {
                    (Some(step_name(ImapStep::SignIn)), "NoSignInMethod")
                }
                ImapFailure::InboxChanged => (None, "InboxChanged"),
            };
            (step, cause, Some(server))
        }
        LoadFailure::WorkerStopped => (None, "WorkerStopped", None),
    };
    tracing::error!(
        step,
        cause,
        code = server.and_then(|server| server.server_reply.as_ref()?.code.as_deref()),
        alerts = server
            .map(|server| server.alerts.len())
            .filter(|alerts| *alerts > 0),
        "Inbox load failed"
    );
}

fn step_name(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "Connect",
        ImapStep::SecureConnection => "SecureConnection",
        ImapStep::SignIn => "SignIn",
        ImapStep::OpenInbox => "OpenInbox",
        ImapStep::FetchMessages => "FetchMessages",
        ImapStep::FetchText => "FetchText",
    }
}

fn access_failure_name(error: ImapAccessError) -> &'static str {
    match error {
        ImapAccessError::Settings => "Settings",
        ImapAccessError::NoEncryption => "NoEncryption",
        ImapAccessError::Password => "Password",
        ImapAccessError::Timeout => "Timeout",
        ImapAccessError::Cancelled => "Cancelled",
    }
}

// Received mail is shown to the user, never written to diagnostics.
impl fmt::Debug for ReceivedBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceivedBatch")
            .field("account_id", &self.account_id)
            .field("uid_validity", &self.uid_validity)
            .field(
                "list_refusal",
                &self.list_refusal.as_ref().map(|reply| &reply.code),
            )
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
