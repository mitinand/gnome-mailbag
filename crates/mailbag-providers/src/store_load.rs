// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A load's result into the store: a folder list replaced in one step, or a
//! cycle's batches each stored whole, and the record of how the load ended
//! (specs/007-mail-storage FR-001; specs/008-folders FR-001;
//! specs/009-synchronization FR-008).

use crate::{LoadEvent, LoadResult, failure::log_load_failure};
use mailbag_domain::{
    AccountId, Failure, Folder, FolderBatch, FolderRef, IncompleteList, MessageFlag, PendingChange,
    ReceivedContent,
};
use mailbag_store::{FolderSync, Store, StoreWrite};
use std::collections::HashSet;

/// A cycle's access to its folder in the store: it reads what the cycle
/// starts from and the user's pending changes, stores each batch and the
/// server's answers to those changes and tells the window, and counts what
/// it wrote for the cycle's record line. A read or write that fails
/// or finds the load cancelled ends the cycle with its result.
pub(crate) struct BatchWriter<'a> {
    store: &'a Store,
    pub(crate) folder: FolderRef,
    cancelled: &'a async_channel::Receiver<()>,
    events: &'a async_channel::Sender<LoadEvent>,
    counts: BatchCounts,
}

/// What a cycle's stored batches held, and how many pending changes ended
/// with the server's agreement.
#[derive(Default)]
struct BatchCounts {
    settled: usize,
    removed: usize,
    flag_states: usize,
    related: usize,
    arrived: usize,
    texts: usize,
    /// Content the reader does not show by design.
    unsupported: usize,
    /// Content that could not be read.
    unreadable: usize,
}

impl<'a> BatchWriter<'a> {
    pub(crate) fn new(
        store: &'a Store,
        folder: FolderRef,
        cancelled: &'a async_channel::Receiver<()>,
        events: &'a async_channel::Sender<LoadEvent>,
    ) -> Self {
        Self {
            store,
            folder,
            cancelled,
            events,
            counts: BatchCounts::default(),
        }
    }

    /// What the cycle starts from: the folder's state and stored messages.
    pub(crate) fn read_folder_sync(&self) -> Result<FolderSync, LoadResult> {
        self.store
            .read_folder_sync(&self.folder)
            .map_err(|failure| self.store_failed(failure))
    }

    /// Which of `identities` another folder of the account holds.
    pub(crate) fn identities_in_other_folders(
        &self,
        identities: &[String],
    ) -> Result<HashSet<String>, LoadResult> {
        self.store
            .identities_in_other_folders(&self.folder, identities)
            .map_err(|failure| self.store_failed(failure))
    }

    /// Which of `identities` the account already holds.
    pub(crate) fn stored_identities(
        &self,
        identities: &[String],
    ) -> Result<HashSet<String>, LoadResult> {
        self.store
            .stored_identities(&self.folder.account, identities)
            .map_err(|failure| self.store_failed(failure))
    }

    /// The folder's pending changes, each with the server's value as stored.
    pub(crate) fn pending_changes(&self) -> Result<Vec<PendingChange>, LoadResult> {
        self.store
            .read_pending_changes(&self.folder)
            .map_err(|failure| self.store_failed(failure))
    }

    /// The server has `value` of `flag` for the messages: it becomes their
    /// server value and ends a pending value equal to it. The window shows
    /// the pending value already, so it is not told.
    pub(crate) fn settle(
        &mut self,
        identities: &[String],
        flag: MessageFlag,
        value: bool,
    ) -> Result<(), LoadResult> {
        self.store
            .settle_flags(&self.folder.account, identities, flag, value)
            .map_err(|failure| self.store_failed(failure))?;
        self.counts.settled += identities.len();
        Ok(())
    }

    /// The server refused `refused` of `flag` for the messages: a pending
    /// value equal to it ends, and the window is told, since its rows show
    /// the change undone (specs/011-read-and-star FR-010).
    pub(crate) fn drop_pending(
        &self,
        identities: &[String],
        flag: MessageFlag,
        refused: bool,
    ) -> Result<(), LoadResult> {
        self.store
            .drop_pending_flags(&self.folder.account, identities, flag, refused)
            .map_err(|failure| self.store_failed(failure))?;
        self.events.try_send(LoadEvent::StoreChanged).ok();
        Ok(())
    }

    /// Stores one batch whole and tells the window; a batch that changes
    /// nothing is not written. A load cancelled before the store took the
    /// batch writes nothing (specs/007-mail-storage/research.md §6).
    pub(crate) fn store(&mut self, batch: &FolderBatch) -> Result<(), LoadResult> {
        if *batch == FolderBatch::default() {
            return Ok(());
        }
        let written = self
            .store
            .store_batch(&self.folder, batch, || self.cancelled.is_closed());
        match written {
            Ok(StoreWrite::Stored) => {
                self.counts.add(batch);
                self.events.try_send(LoadEvent::StoreChanged).ok();
                Ok(())
            }
            // The cancellation was recorded where it was requested.
            Ok(StoreWrite::LoadCancelled) => Err(LoadResult::Cancelled),
            Err(failure) => Err(self.store_failed(failure)),
        }
    }

    /// A store read or write that failed ends the cycle as a failure of the
    /// mailbox load, with its error line.
    fn store_failed(&self, failure: Failure) -> LoadResult {
        failed_write(&self.folder.account, "mailbox", failure)
    }

    /// Ends the cycle as stored, with its record line: how many messages the
    /// server listed, or on Microsoft 365 reported, how many pending changes
    /// the server agreed with, and what the batches changed. The folder's
    /// name stays at debug (specs/003-logging FR-010).
    pub(crate) fn finish(&self, listed: usize, incomplete: Option<IncompleteList>) -> LoadResult {
        let account = self.folder.account.as_str();
        let counts = &self.counts;
        tracing::debug!(
            account,
            folder = self.folder.identity,
            "the cycle read this mailbox"
        );
        tracing::info!(
            account,
            listed,
            settled = counts.settled,
            removed = counts.removed,
            flag_states = counts.flag_states,
            related = counts.related,
            arrived = counts.arrived,
            texts = counts.texts,
            unsupported = counts.unsupported,
            "mailbox cycle finished"
        );
        warn_about_content(account, counts.unreadable, incomplete.as_ref());
        LoadResult::Stored { incomplete }
    }
}

impl BatchCounts {
    fn add(&mut self, batch: &FolderBatch) {
        self.removed += batch.removed.len();
        self.flag_states += batch.flag_states.len();
        self.related += batch.known_arrived.len();
        self.arrived += batch.arrived.len();
        for message in &batch.arrived {
            match &message.content {
                ReceivedContent::Text(_) => self.texts += 1,
                ReceivedContent::NotDownloaded => {}
                // Not shown by design, such as an HTML-only message.
                ReceivedContent::Explained(explanation) if explanation.is_by_design() => {
                    self.unsupported += 1
                }
                ReceivedContent::Explained(_)
                | ReceivedContent::StructureUnreadable
                | ReceivedContent::TextNotReturned => self.unreadable += 1,
            }
        }
    }
}

/// Warns about content that could not be read and a list the server did not
/// finish, without the server's words.
fn warn_about_content(account: &str, unreadable: usize, incomplete: Option<&IncompleteList>) {
    if unreadable > 0 {
        tracing::warn!(
            account,
            messages = unreadable,
            "some messages have content that could not be read"
        );
    }
    match incomplete {
        Some(IncompleteList::ServerRefused { code, .. }) => tracing::warn!(
            account,
            code = code.as_deref(),
            "the server refused to finish the message list"
        ),
        None => {}
    }
}

/// Stores a completed folder list as the account's, and says how the load
/// ended. A list without any folder stores nothing (specs/008-folders
/// FR-001); a load cancelled before the store took the list writes nothing;
/// a write that fails is the load's failure.
pub(crate) fn store_folder_list(
    store: &Store,
    account: &AccountId,
    folders: Vec<Folder>,
    load_cancelled: impl FnOnce() -> bool,
) -> LoadResult {
    if folders.is_empty() {
        tracing::info!(
            account = account.as_str(),
            folders = 0,
            "folder list load finished"
        );
        return LoadResult::Stored { incomplete: None };
    }
    match store.replace_folders(account, &folders, load_cancelled) {
        Ok(StoreWrite::Stored) => {
            tracing::info!(
                account = account.as_str(),
                folders = folders.len(),
                "folder list load finished"
            );
            LoadResult::Stored { incomplete: None }
        }
        // The cancellation was recorded where it was requested.
        Ok(StoreWrite::LoadCancelled) => LoadResult::Cancelled,
        Err(failure) => failed_write(account, "folder list", failure),
    }
}

fn failed_write(account: &AccountId, record_name: &'static str, failure: Failure) -> LoadResult {
    log_load_failure(account, record_name, failure.kind, None, None, 0);
    LoadResult::Failed(failure)
}
