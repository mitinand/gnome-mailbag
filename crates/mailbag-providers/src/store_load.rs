// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A completed load's result into the store: a folder list replaced in one
//! step, or a folder's messages in the form the application keeps, and the
//! record of how the load ended (specs/007-mail-storage FR-001, FR-004;
//! specs/008-folders FR-001, FR-004).

use crate::{
    LoadResult,
    batch::{MessageIdentity, ReceivedBatch, ReceivedMessage},
    failure::log_load_failure,
};
use mailbag_domain::{
    AccountId, Failure, Folder, FolderRef, IncompleteList, Message, ReceivedContent,
};
use mailbag_store::{Store, StoreWrite};

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
    let messages = kept_messages(batch.messages, &batch.folder);
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

/// The received messages as the application keeps them, in the load's order.
/// A message's identity is Gmail's own identifier where the load read it,
/// Microsoft Graph's immutable identifier, or, for Generic IMAP, whose
/// message has no identity beyond its place, the folder and the UID
/// (specs/008-folders FR-004).
fn kept_messages(received: Vec<ReceivedMessage>, folder: &FolderRef) -> Vec<Message> {
    received
        .into_iter()
        .map(|received| Message {
            identity: match (&received.gmail, &received.identity) {
                (Some(gmail), MessageIdentity::ImapUid(_)) => format!("gmail:{}", gmail.message_id),
                (None, MessageIdentity::ImapUid(uid)) => format!("imap:{}/{uid}", folder.identity),
                (_, MessageIdentity::GraphImmutableId(id)) => format!("graph:{id}"),
            },
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
    // Content the reader does not show by design, and content that could not
    // be read, counted once over the batch.
    let (unsupported, unreadable) = messages.iter().fold(
        (0, 0),
        |(unsupported, unreadable), message| match &message.content {
            ReceivedContent::Text(_) | ReceivedContent::NotDownloaded => (unsupported, unreadable),
            ReceivedContent::Explained(explanation) if explanation.is_by_design() => {
                (unsupported + 1, unreadable)
            }
            ReceivedContent::Explained(_)
            | ReceivedContent::StructureUnreadable
            | ReceivedContent::TextNotReturned => (unsupported, unreadable + 1),
        },
    );
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
