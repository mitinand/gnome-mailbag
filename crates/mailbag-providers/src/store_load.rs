// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A completed load's batch into the store: every message in the form the
//! application keeps, the account's Inbox replaced in one step, and the
//! record of how the load ended (specs/007-mail-storage FR-001, FR-004).

use crate::{
    LoadResult,
    batch::{MessageIdentity, ReceivedBatch, ReceivedMessage},
    failure::log_load_failure,
};
use mailbag_domain::{IncompleteList, Message, ReceivedContent};
use mailbag_store::{InboxWrite, Store};

/// Stores the batch as its account's Inbox and says how the load ended. A
/// load cancelled before the store took its messages writes nothing; a write
/// that fails is the load's failure.
pub(crate) fn store_batch(
    store: &Store,
    batch: ReceivedBatch,
    load_cancelled: impl FnOnce() -> bool,
) -> LoadResult {
    let messages: Vec<Message> = batch.messages.into_iter().map(message).collect();
    match store.replace_inbox(&batch.account_id, &messages, load_cancelled) {
        Ok(InboxWrite::Stored) => {
            log_received_batch(
                batch.account_id.as_str(),
                &messages,
                batch.incomplete.as_ref(),
            );
            LoadResult::Stored {
                incomplete: batch.incomplete,
            }
        }
        // The cancellation was recorded where it was requested.
        Ok(InboxWrite::LoadCancelled) => LoadResult::Cancelled,
        Err(failure) => {
            log_load_failure(&batch.account_id, failure.kind, None, None, 0);
            LoadResult::Failed(failure)
        }
    }
}

/// A received message as the application keeps it. Its identity is text for
/// the record: Gmail's own identifier where the load read it, else the IMAP
/// UID or Microsoft Graph's immutable identifier.
fn message(received: ReceivedMessage) -> Message {
    let identity = match (&received.gmail, &received.identity) {
        (Some(gmail), _) => format!("gmail:{}", gmail.message_id),
        (None, MessageIdentity::ImapUid(uid)) => format!("uid:{uid}"),
        (None, MessageIdentity::GraphImmutableId(id)) => format!("graph:{id}"),
    };
    Message {
        identity,
        fields: received.fields,
        received_unix: received.internal_date,
        seen: received.seen,
        content: received.content,
    }
}

/// How a stored load ended, with warnings for what the reader cannot show.
fn log_received_batch(account: &str, messages: &[Message], incomplete: Option<&IncompleteList>) {
    // Content the reader does not show by design, and content that could not
    // be read, counted once over the batch.
    let (unsupported, unreadable) = messages.iter().fold(
        (0, 0),
        |(unsupported, unreadable), message| match &message.content {
            ReceivedContent::Text(_) => (unsupported, unreadable),
            ReceivedContent::Explained(explanation) if explanation.is_by_design() => {
                (unsupported + 1, unreadable)
            }
            ReceivedContent::Explained(_)
            | ReceivedContent::StructureUnreadable
            | ReceivedContent::TextNotReturned => (unsupported, unreadable + 1),
        },
    );
    tracing::info!(
        account,
        messages = messages.len(),
        unsupported,
        "Inbox load finished"
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
