// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a load reads, what it tells its caller, and how it ended. Received
//! mail stays in this crate: the mail worker stores it, and only the events
//! cross to whatever shows the mail, which reads the store
//! (specs/007-mail-storage FR-001); a failure crosses as the domain's
//! `Failure`.

use goa_adapter::AccessError;
use mailbag_domain::{Failure, FolderRef, IncompleteList};
use mailbag_graph::GraphError;
use mailbag_imap::ImapError;

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
/// of stored batches, then exactly one end (specs/009-synchronization
/// research §7).
#[derive(Debug)]
pub enum LoadEvent {
    /// The cycle stored a batch of its folder; the window reads the store
    /// again.
    BatchStored,
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
