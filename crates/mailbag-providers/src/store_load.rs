// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A load's result into the store: a folder list replaced in one step, a
//! cycle's portions each stored whole, or a Microsoft 365 folder's newest
//! messages, and the record of how the load ended (specs/007-mail-storage
//! FR-001; specs/008-folders FR-001; specs/009-synchronization FR-008).

use crate::{
    LoadEvent, LoadResult,
    batch::{ReceivedBatch, ReceivedMessage},
    failure::log_load_failure,
};
use mailbag_domain::{
    AccountId, Failure, Folder, FolderPortion, FolderRef, IncompleteList, Message, ReceivedContent,
};
use mailbag_store::{FolderSync, Store, StoreWrite};
use std::collections::HashSet;

/// A cycle's access to its folder in the store: it reads what the cycle
/// starts from, stores each portion and tells the window, and counts what
/// the portions held for the cycle's record line. A read or write that fails
/// or finds the load cancelled ends the cycle with its result.
pub(crate) struct PortionWriter<'a> {
    store: &'a Store,
    pub(crate) folder: FolderRef,
    cancelled: &'a async_channel::Receiver<()>,
    events: &'a async_channel::Sender<LoadEvent>,
    counts: PortionCounts,
}

/// What a cycle's stored portions held.
#[derive(Default)]
struct PortionCounts {
    removed: usize,
    read_states: usize,
    related: usize,
    arrived: usize,
    texts: usize,
    /// Content the reader does not show by design.
    unsupported: usize,
    /// Content that could not be read.
    unreadable: usize,
}

impl<'a> PortionWriter<'a> {
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
            counts: PortionCounts::default(),
        }
    }

    /// Stores a Microsoft 365 folder's newest messages in place of its
    /// stored ones, until its cycle exists.
    pub(crate) fn store_newest_messages(&self, batch: ReceivedBatch) -> LoadResult {
        store_mailbox(self.store, batch, || self.cancelled.is_closed())
    }

    /// What the cycle starts from: the folder's state and stored messages.
    pub(crate) fn read_folder_sync(&self) -> Result<FolderSync, LoadResult> {
        self.store
            .read_folder_sync(&self.folder)
            .map_err(|failure| failed_write(&self.folder.account, "mailbox", failure))
    }

    /// Which of `identities` the account already holds.
    pub(crate) fn stored_identities(
        &self,
        identities: &[String],
    ) -> Result<HashSet<String>, LoadResult> {
        self.store
            .stored_identities(&self.folder.account, identities)
            .map_err(|failure| failed_write(&self.folder.account, "mailbox", failure))
    }

    /// Stores one portion whole and tells the window; a portion that changes
    /// nothing is not written. A load cancelled before the store took the
    /// portion writes nothing (specs/007-mail-storage/research.md §6).
    pub(crate) fn store(&mut self, portion: &FolderPortion) -> Result<(), LoadResult> {
        if *portion == FolderPortion::default() {
            return Ok(());
        }
        let written = self
            .store
            .store_portion(&self.folder, portion, || self.cancelled.is_closed());
        match written {
            Ok(StoreWrite::Stored) => {
                self.counts.add(portion);
                self.events.try_send(LoadEvent::PortionStored).ok();
                Ok(())
            }
            // The cancellation was recorded where it was requested.
            Ok(StoreWrite::LoadCancelled) => Err(LoadResult::Cancelled),
            Err(failure) => Err(failed_write(&self.folder.account, "mailbox", failure)),
        }
    }

    /// Ends the cycle as stored, with its record line: how many messages the
    /// server listed and what the portions changed. The folder's name stays
    /// at debug (specs/003-logging FR-010).
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
            removed = counts.removed,
            read_states = counts.read_states,
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

impl PortionCounts {
    fn add(&mut self, portion: &FolderPortion) {
        self.removed += portion.removed.len();
        self.read_states += portion.read_states.len();
        self.related += portion.known_arrived.len();
        self.arrived += portion.arrived.len();
        for message in &portion.arrived {
            match content_class(&message.content) {
                ContentClass::Text => self.texts += 1,
                ContentClass::Unsupported => self.unsupported += 1,
                ContentClass::Unreadable => self.unreadable += 1,
                ContentClass::NotDownloaded => {}
            }
        }
    }
}

/// How the record counts a message's content.
enum ContentClass {
    Text,
    /// Not shown by design, such as an HTML-only message.
    Unsupported,
    /// Could not be read, which is warned about.
    Unreadable,
    NotDownloaded,
}

fn content_class(content: &ReceivedContent) -> ContentClass {
    match content {
        ReceivedContent::Text(_) => ContentClass::Text,
        ReceivedContent::NotDownloaded => ContentClass::NotDownloaded,
        ReceivedContent::Explained(explanation) if explanation.is_by_design() => {
            ContentClass::Unsupported
        }
        ReceivedContent::Explained(_)
        | ReceivedContent::StructureUnreadable
        | ReceivedContent::TextNotReturned => ContentClass::Unreadable,
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
        Some(IncompleteList::MoreAvailable) => tracing::warn!(
            account,
            "the mail service offered more messages than one request holds"
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

/// Stores the batch as its folder's messages and says how the load ended,
/// as `store_folder_list` does.
pub(crate) fn store_mailbox(
    store: &Store,
    batch: ReceivedBatch,
    load_cancelled: impl FnOnce() -> bool,
) -> LoadResult {
    let messages = kept_messages(batch.messages);
    let written = store.replace_mailbox(&batch.folder, &messages, load_cancelled);
    mailbox_load_result(written, &batch.folder, &messages, batch.incomplete)
}

fn mailbox_load_result(
    written: Result<StoreWrite, Failure>,
    folder: &FolderRef,
    messages: &[Message],
    incomplete: Option<IncompleteList>,
) -> LoadResult {
    match written {
        Ok(StoreWrite::Stored) => {
            log_received_batch(folder, messages, incomplete.as_ref());
            LoadResult::Stored { incomplete }
        }
        // The cancellation was recorded where it was requested.
        Ok(StoreWrite::LoadCancelled) => LoadResult::Cancelled,
        Err(failure) => failed_write(&folder.account, "mailbox", failure),
    }
}

fn failed_write(account: &AccountId, record_name: &'static str, failure: Failure) -> LoadResult {
    log_load_failure(account, record_name, failure.kind, None, None, 0);
    LoadResult::Failed(failure)
}

/// The received messages as the application keeps them, each under Microsoft
/// Graph's immutable identifier (specs/008-folders FR-004).
fn kept_messages(received: Vec<ReceivedMessage>) -> Vec<Message> {
    received
        .into_iter()
        .map(|received| Message {
            identity: format!("graph:{}", received.graph_id),
            fields: received.fields,
            received_unix: received.internal_date,
            seen: received.seen,
            content: received.content,
        })
        .collect()
}

/// How a stored load ended, with warnings for what the reader cannot show.
/// The folder's name stays at debug (specs/003-logging FR-010).
fn log_received_batch(
    folder: &FolderRef,
    messages: &[Message],
    incomplete: Option<&IncompleteList>,
) {
    let account = folder.account.as_str();
    let classes = messages
        .iter()
        .map(|message| content_class(&message.content));
    let unsupported = classes
        .clone()
        .filter(|class| matches!(class, ContentClass::Unsupported))
        .count();
    let unreadable = classes
        .filter(|class| matches!(class, ContentClass::Unreadable))
        .count();
    tracing::debug!(
        account,
        folder = folder.identity,
        "the load read this mailbox"
    );
    tracing::info!(
        account,
        messages = messages.len(),
        unsupported,
        "mailbox load finished"
    );
    warn_about_content(account, unreadable, incomplete);
}
