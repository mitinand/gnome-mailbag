// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one load delivered, and how it ended. The batch stays in this crate:
//! the mail worker stores it, and only the result crosses to whatever shows
//! the mail, which reads the store (specs/007-mail-storage FR-001); a failure
//! crosses as the domain's `Failure`.

use goa_adapter::AccessError;
use mailbag_domain::{AccountId, DisplayFields, Failure, IncompleteList, ReceivedContent};
use mailbag_graph::GraphError;
use mailbag_imap::{GmailRow, ImapError};
use std::fmt;

/// How many of the newest Inbox messages one load delivers, for every
/// provider (specs/002-imap-integration FR-002).
pub(crate) const BATCH_SIZE: u32 = 100;

/// One account's Inbox as a single load received it.
#[derive(Debug)]
pub(crate) struct ReceivedBatch {
    pub(crate) account_id: AccountId,
    /// Newest first, at most `BATCH_SIZE`.
    pub(crate) messages: Vec<ReceivedMessage>,
    /// Why messages are missing from this batch. `None` when it is complete.
    pub(crate) incomplete: Option<IncompleteList>,
}

/// One message of a batch. Raw MIME is released once it is decoded.
pub(crate) struct ReceivedMessage {
    pub(crate) identity: MessageIdentity,
    pub(crate) fields: DisplayFields,
    /// INTERNALDATE as seconds since the Unix epoch.
    pub(crate) internal_date: Option<i64>,
    pub(crate) seen: bool,
    pub(crate) content: ReceivedContent,
    /// Gmail's own identifier and labels; `None` for every other provider.
    pub(crate) gmail: Option<GmailRow>,
}

/// How the message's provider identifies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MessageIdentity {
    /// The IMAP UID, valid with the batch's `uid_validity`.
    ImapUid(u32),
    /// Microsoft Graph's identifier, which survives moves between folders.
    GraphImmutableId(String),
}

/// Why a refresh delivered no mail, at the step where it stopped, in the
/// protocol's terms. It stays in this crate; the application receives the
/// `Failure` it turns into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadFailure {
    /// Online Accounts did not give the settings or the credential.
    OnlineAccounts(AccessError),
    /// The IMAP connection, the sign-in or the transfer failed.
    Imap(ImapError),
    /// The Microsoft Graph request failed or was refused.
    MicrosoftGraph(GraphError),
    /// The mail worker stopped the load without a result: a panic, with its
    /// message and place as `message at file:line`, or `None` when the worker
    /// vanished without one.
    WorkerStopped(Option<String>),
}

/// How one load ended.
#[derive(Debug)]
pub enum LoadResult {
    /// The load's messages are the account's stored Inbox now; `incomplete`
    /// says why messages the Inbox offered are missing.
    Stored {
        incomplete: Option<IncompleteList>,
    },
    Failed(Failure),
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
