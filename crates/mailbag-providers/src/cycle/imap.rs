// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
//! research §2, §3, §4).

use super::{CycleEnd, completed, pending::send_imap_changes};
use crate::{
    LoadResult,
    gmail::{gmail_options, log_gmail_rows},
    imap_texts::{imap_account, read_contents},
    store_load::BatchWriter,
};
use goa_adapter::ImapAccess;
use mailbag_content::decode_display_fields;
use mailbag_domain::{FolderBatch, FolderState, IncompleteList, Message, MessageFlags};
use mailbag_imap::{
    FolderListing, ImapError, ImapFailure, ImapStep, MailboxReader, OpenOptions, RowItems,
    ServerReply,
};
use mailbag_store::FolderSync;
use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

/// How many missing messages one batch fetches (research §3).
const BATCH_SIZE: usize = 100;

/// How a listed message is identified in its account (spec FR-005, FR-006).
#[derive(Clone, Copy)]
pub(super) enum IdentityRule {
    /// `imap:<folder>/<UIDVALIDITY>/<UID>`: a Generic IMAP message has no
    /// identity beyond its place, which the numbering version belongs to.
    Generic,
    /// `gmail:<X-GM-MSGID>`, the same in every label.
    Gmail,
}

/// A listed message the cycle knows by its identity.
struct ListedMessage {
    identity: String,
    uid: u32,
    flags: MessageFlags,
}

/// The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
/// research §2, §3): list every message, store what the listing proves,
/// then fetch the missing messages newest first, a batch at a time. The
/// user's pending changes are sent by the listing's UIDs before each batch
/// and once before the cycle ends (specs/011-read-and-star FR-007).
pub(super) async fn synchronize_imap_folder(
    access: ImapAccess,
    identity_rule: IdentityRule,
    batches: &mut BatchWriter<'_>,
) -> Result<LoadResult, CycleEnd> {
    let recent_limit = super::recent_limit(SystemTime::now());
    let mut server = ImapFolder::open(access, identity_rule, &batches.folder.identity).await?;
    let stored = batches.read_folder_sync()?;
    let listing = server.list_messages().await?;
    let listed = server.identify(&listing, &batches.folder.identity)?;
    let listed_uids: HashMap<String, u32> = listed
        .iter()
        .map(|message| (message.identity.clone(), message.uid))
        .collect();
    let missing = missing_messages(&listed, &stored);
    batches.store(&listing_changes(&listed, &stored, &listing, &missing))?;
    send_imap_changes(&mut server.reader, &listed_uids, batches).await?;
    for batch_messages in missing.chunks(BATCH_SIZE) {
        let (batch, refusal) = server
            .fetch_arrivals(batch_messages, recent_limit, batches)
            .await?;
        batches.store(&batch)?;
        send_imap_changes(&mut server.reader, &listed_uids, batches).await?;
        // The rows the server withheld are missing, so the folder stays
        // not completed; the listing's proof is stored.
        if let Some(refusal) = refusal {
            return Ok(batches.finish(listed.len(), Some(short_list(refusal))));
        }
    }
    if let Some(refusal) = listing.refusal {
        return Ok(batches.finish(listed.len(), Some(short_list(refusal))));
    }
    if !missing.is_empty() {
        batches.store(&FolderBatch {
            state: Some(completed(None)),
            ..FolderBatch::default()
        })?;
    }
    Ok(batches.finish(listed.len(), None))
}

/// The listed messages the folder does not hold, highest UID first, so the
/// newest arrive first (spec FR-003).
fn missing_messages<'a>(
    listed: &'a [ListedMessage],
    stored: &FolderSync,
) -> Vec<&'a ListedMessage> {
    let mut missing: Vec<_> = listed
        .iter()
        .filter(|message| !stored.stored.contains_key(&message.identity))
        .collect();
    missing.sort_unstable_by_key(|message| std::cmp::Reverse(message.uid));
    missing
}

/// What the listing proves before anything is fetched: removals when it
/// completed (spec FR-004), changed flags, and the folder's state: not
/// completed while messages are missing, refused listing or not; completed
/// when none are and the listing completed; otherwise as it was.
fn listing_changes(
    listed: &[ListedMessage],
    stored: &FolderSync,
    listing: &FolderListing,
    missing: &[&ListedMessage],
) -> FolderBatch {
    let complete = listing.refusal.is_none();
    let listed_identities: HashSet<&str> = listed
        .iter()
        .map(|message| message.identity.as_str())
        .collect();
    let removed = match complete {
        true => stored
            .stored
            .keys()
            .filter(|identity| !listed_identities.contains(identity.as_str()))
            .cloned()
            .collect(),
        false => Vec::new(),
    };
    let flag_states = listed
        .iter()
        .filter(|message| {
            stored
                .stored
                .get(&message.identity)
                .is_some_and(|flags| *flags != message.flags)
        })
        .map(|message| (message.identity.clone(), message.flags.into()))
        .collect();
    let state = if !missing.is_empty() {
        Some(FolderState::default())
    } else if complete && !stored.state.synchronized {
        Some(completed(None))
    } else {
        None
    };
    FolderBatch {
        removed,
        flag_states,
        state,
        ..FolderBatch::default()
    }
}

fn short_list(refusal: ServerReply) -> IncompleteList {
    IncompleteList::ServerRefused {
        reply: refusal.text,
        code: refusal.code,
    }
}

/// An open IMAP folder for one cycle.
struct ImapFolder {
    reader: MailboxReader,
    identity_rule: IdentityRule,
}

impl ImapFolder {
    async fn open(
        access: ImapAccess,
        identity_rule: IdentityRule,
        folder: &str,
    ) -> Result<Self, ImapError> {
        let reader =
            MailboxReader::open(imap_account(access), options(identity_rule), folder).await?;
        Ok(Self {
            reader,
            identity_rule,
        })
    }

    async fn list_messages(&mut self) -> Result<FolderListing, ImapError> {
        let items = self.row_items();
        self.reader.list_messages(items).await
    }

    /// What the listing and the rows ask for beyond RFC 3501.
    fn row_items(&self) -> RowItems {
        match self.identity_rule {
            IdentityRule::Generic => RowItems::Standard,
            IdentityRule::Gmail => RowItems::WithGmailAttributes,
        }
    }

    /// Each listed message with its identity in the account. A Gmail
    /// message listed without its identifier is left out and written to the
    /// record: it is never guessed from its place (research §4).
    fn identify(
        &self,
        listing: &FolderListing,
        folder: &str,
    ) -> Result<Vec<ListedMessage>, ImapError> {
        let generic_prefix = match self.identity_rule {
            IdentityRule::Generic => {
                // RFC 3501 §2.3.1.1 requires UIDVALIDITY with every opened
                // mailbox.
                let uid_validity = self
                    .reader
                    .uid_validity()
                    .ok_or(ImapFailure::Failed(ImapStep::OpenMailbox))?;
                Some(format!("imap:{folder}/{uid_validity}/"))
            }
            IdentityRule::Gmail => None,
        };
        let listed = listing
            .messages
            .iter()
            .filter_map(|message| {
                let identity = match (&generic_prefix, message.gmail_message_id) {
                    (Some(prefix), _) => format!("{prefix}{}", message.uid),
                    (None, Some(gmail_id)) => format!("gmail:{gmail_id}"),
                    (None, None) => {
                        tracing::warn!(
                            uid = message.uid,
                            "a Gmail message was listed without its identifier and left out"
                        );
                        return None;
                    }
                };
                Some(ListedMessage {
                    identity,
                    uid: message.uid,
                    flags: MessageFlags {
                        seen: message.seen,
                        flagged: message.flagged,
                    },
                })
            })
            .collect();
        Ok(listed)
    }

    /// One batch of missing messages: those the account already holds,
    /// related without fetching them (research §4), and the others with
    /// their rows and previews and, for those received after `recent_limit`,
    /// their text.
    /// A message that disappeared meanwhile is left out. A row fetch the
    /// server refused returns its reason with what it answered.
    async fn fetch_arrivals(
        &mut self,
        messages: &[&ListedMessage],
        recent_limit: i64,
        batches: &BatchWriter<'_>,
    ) -> Result<(FolderBatch, Option<ServerReply>), CycleEnd> {
        let identities: Vec<String> = messages
            .iter()
            .map(|message| message.identity.clone())
            .collect();
        let known = batches.stored_identities(&identities)?;
        let (known, unknown): (Vec<&ListedMessage>, Vec<&ListedMessage>) = messages
            .iter()
            .copied()
            .partition(|message| known.contains(&message.identity));
        let uids: Vec<u32> = unknown.iter().map(|message| message.uid).collect();
        let items = self.row_items();
        let rows = self.reader.fetch_rows_by_uid(&uids, items).await?;
        log_gmail_rows(&rows.rows);
        let recent: Vec<u32> = rows
            .rows
            .iter()
            .filter(|row| row.internal_date.is_some_and(|date| date >= recent_limit))
            .map(|row| row.uid)
            .collect();
        let mut contents = read_contents(&mut self.reader, &rows.rows, &recent).await?;
        let arrived = rows
            .rows
            .into_iter()
            .filter_map(|row| {
                // A message missing here disappeared meanwhile.
                let (content, preview) = contents.remove(&row.uid)?;
                let listed = unknown.iter().find(|message| message.uid == row.uid)?;
                Some(Message {
                    identity: listed.identity.clone(),
                    fields: tracing::debug_span!("message", uid = row.uid)
                        .in_scope(|| decode_display_fields(&row.list_headers)),
                    received_unix: row.internal_date,
                    seen: row.seen,
                    flagged: row.flagged,
                    content,
                    preview,
                })
            })
            .collect();
        let batch = FolderBatch {
            known_arrived: known
                .iter()
                .map(|message| (message.identity.clone(), message.flags))
                .collect(),
            arrived,
            ..FolderBatch::default()
        };
        Ok((batch, rows.refusal))
    }
}

/// What the provider asks for beyond RFC 3501 when it opens a folder.
fn options(identity_rule: IdentityRule) -> OpenOptions {
    match identity_rule {
        IdentityRule::Generic => OpenOptions::default(),
        IdentityRule::Gmail => gmail_options(),
    }
}
