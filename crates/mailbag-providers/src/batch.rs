// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one load delivered, and how a load that failed is described. These
//! types cross from the mail worker to whatever shows the mail.

use goa_adapter::{AccessError, AccountId};
use mailbag_content::{ContentExplanation, DisplayFields};
use mailbag_graph::GraphError;
use mailbag_imap::{GmailRow, ImapError, ServerReply};
use std::fmt;

/// How many of the newest Inbox messages one load delivers, for every
/// provider (specs/002-imap-integration FR-002).
pub(crate) const BATCH_SIZE: u32 = 100;

/// One account's Inbox as a single load received it.
#[derive(Debug)]
pub struct ReceivedBatch {
    pub account_id: AccountId,
    /// The Inbox version these UIDs belong to.
    pub uid_validity: Option<u32>,
    /// Newest first, at most `BATCH_SIZE`.
    pub messages: Vec<ReceivedMessage>,
    /// Why messages are missing from this batch. `None` when it is complete.
    pub incomplete: Option<IncompleteList>,
}

/// Why a batch holds fewer messages than the Inbox offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncompleteList {
    /// The server refused to finish the message list; this is what it said.
    ServerRefused(ServerReply),
    /// The mail service offered further messages beyond the one request.
    MoreAvailable,
}

/// One message of a batch. Raw MIME is released once it is decoded.
pub struct ReceivedMessage {
    pub identity: MessageIdentity,
    pub fields: DisplayFields,
    /// INTERNALDATE as seconds since the Unix epoch.
    pub internal_date: Option<i64>,
    pub seen: bool,
    pub content: ReceivedContent,
    /// Gmail's own identifier and labels; `None` for every other provider.
    pub gmail: Option<GmailRow>,
}

/// How the message's provider identifies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageIdentity {
    /// The IMAP UID, valid with the batch's `uid_validity`.
    ImapUid(u32),
    /// Microsoft Graph's identifier, which survives moves between folders.
    GraphImmutableId(String),
}

/// The text of a message, or why the reader shows none.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceivedContent {
    Text(String),
    /// Why the content rules found no text to show.
    Explained(ContentExplanation),
    /// The server could not describe the message, so nothing was read.
    StructureUnreadable,
    /// The server or the service did not return the message's text.
    TextNotReturned,
}

/// Why a refresh delivered no mail, at the step where it stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadFailure {
    /// Online Accounts did not give the settings or the credential.
    OnlineAccounts(AccessError),
    /// The IMAP connection, the sign-in or the transfer failed.
    Imap(ImapError),
    /// The Microsoft Graph request failed or was refused.
    MicrosoftGraph(GraphError),
    /// The mail worker stopped without a result.
    WorkerStopped,
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
impl fmt::Debug for ReceivedMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceivedMessage")
            .field("identity", &self.identity)
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
            Self::StructureUnreadable => write!(formatter, "StructureUnreadable"),
            Self::TextNotReturned => write!(formatter, "TextNotReturned"),
        }
    }
}
