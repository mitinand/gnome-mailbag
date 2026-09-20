// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail one account received in this run, and why a load failed.

use goa_adapter::AccountId;
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

/// Why a load failed, with what the server said about it.
#[derive(Clone, PartialEq, Eq)]
pub struct LoadFailure {
    pub failure: ImapFailure,
    /// The server's own reason, for the failure explanation only.
    pub server_reply: Option<ServerReply>,
    pub alerts: Vec<String>,
}

impl From<ImapFailure> for LoadFailure {
    /// A failure the load itself found, which the server did not explain.
    fn from(failure: ImapFailure) -> Self {
        Self {
            failure,
            server_reply: None,
            alerts: Vec::new(),
        }
    }
}

impl From<ImapError> for LoadFailure {
    fn from(error: ImapError) -> Self {
        Self {
            failure: error.failure,
            server_reply: error.server_reply,
            alerts: error.alerts,
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
        formatter
            .debug_struct("LoadFailure")
            .field("failure", &self.failure)
            .field(
                "server_code",
                &self.server_reply.as_ref().map(|reply| &reply.code),
            )
            .field("alert_count", &self.alerts.len())
            .finish()
    }
}
