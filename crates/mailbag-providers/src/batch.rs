// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one load delivered, and how a load that failed is described. These
//! types cross from the mail worker to whatever shows the mail.

use goa_adapter::{AccountId, ImapAccessError};
use mailbag_content::{ContentExplanation, DisplayFields};
use mailbag_imap::{ImapError, ImapFailure, ServerReply};
use std::fmt;

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

/// The step a running load is on, such as the Online Accounts request or the
/// transfer on the mail worker. Dropping it cancels the load.
pub trait CancelsLoadOnDrop {}

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
