// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one load delivered, and how it ended. The batch stays in this crate:
//! the mail worker stores it, and only the result crosses to whatever shows
//! the mail, which reads the store (specs/007-mail-storage FR-001); a failure
//! crosses as the domain's `Failure`.

use goa_adapter::AccessError;
use mailbag_domain::{DisplayFields, Failure, FolderRef, IncompleteList, ReceivedContent};
use mailbag_graph::GraphError;
use mailbag_imap::ImapError;
use std::fmt;

/// How many of the newest messages of a folder one load delivers, for every
/// provider (specs/002-imap-integration FR-002).
pub(crate) const BATCH_SIZE: u32 = 100;

/// What a load reads (specs/008-folders FR-001, FR-010).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadTarget {
    /// The account's folder list, which Refresh Account loads.
    FolderList,
    /// One cycle of a folder, which Refresh Mailbox runs
    /// (specs/009-synchronization FR-001).
    Mailbox(FolderRef),
}

/// What a load tells whoever started it, on that caller's context: any number
/// of stored portions, then exactly one end (specs/009-synchronization
/// research §7).
#[derive(Debug)]
pub enum LoadEvent {
    /// The cycle stored a portion of its folder; the window reads the store
    /// again.
    PortionStored,
    Finished(LoadResult),
}

impl LoadTarget {
    /// What the record calls the load; the folder's name stays out of it.
    pub fn record_name(&self) -> &'static str {
        match self {
            Self::FolderList => "folder list",
            Self::Mailbox(_) => "mailbox",
        }
    }
}

/// One Microsoft 365 folder's newest messages as a single load received
/// them, until that folder synchronizes too.
#[derive(Debug)]
pub(crate) struct ReceivedBatch {
    pub(crate) folder: FolderRef,
    /// Newest first, at most `BATCH_SIZE`.
    pub(crate) messages: Vec<ReceivedMessage>,
    /// Why messages are missing from this batch. `None` when it is complete.
    pub(crate) incomplete: Option<IncompleteList>,
}

/// One message of a batch.
pub(crate) struct ReceivedMessage {
    /// Microsoft Graph's immutable identifier, which survives moves between
    /// folders.
    pub(crate) graph_id: String,
    pub(crate) fields: DisplayFields,
    /// The received date as seconds since the Unix epoch.
    pub(crate) internal_date: Option<i64>,
    pub(crate) seen: bool,
    pub(crate) content: ReceivedContent,
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
    /// The folder list or the folder's messages are stored now, or the folder
    /// list completed without any folder and nothing was written
    /// (specs/008-folders FR-001); `incomplete` says why messages the folder
    /// offered are missing, and is `None` for a folder list.
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
            .field("graph_id", &self.graph_id)
            .field("seen", &self.seen)
            .field("content", &self.content)
            .finish_non_exhaustive()
    }
}
