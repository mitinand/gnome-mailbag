// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
//! research §2, §3, §4, §13).

use super::{CycleEnd, completed};
use crate::{
    LoadResult,
    gmail::{gmail_options, log_gmail_rows},
    imap_texts::{imap_account, read_contents},
    renewal::AccessRenewal,
    store_load::PortionWriter,
};
use goa_adapter::{ImapAccess, ImapCredential};
use mailbag_content::decode_display_fields;
use mailbag_domain::{FolderPortion, FolderState, IncompleteList, Message, ReceivedContent};
use mailbag_imap::{
    FolderListing, ImapError, ImapFailure, ImapStep, MailboxReader, OpenOptions, RowItems,
    ServerReply,
};
use mailbag_store::FolderSync;
use std::{collections::HashSet, time::SystemTime};

/// How many missing messages one portion fetches (research §3).
const PORTION_SIZE: usize = 100;

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
    seen: bool,
}

/// The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
/// research §2, §3): list every message, store what the listing proves,
/// then fetch the missing messages newest first, a portion at a time.
pub(super) async fn synchronize_imap_folder(
    access: ImapAccess,
    identify: IdentityRule,
    renewal: Option<AccessRenewal<ImapAccess>>,
    portions: &mut PortionWriter<'_>,
) -> Result<LoadResult, CycleEnd> {
    let recent_limit = super::recent_limit(SystemTime::now());
    let mut server = ImapFolder::open(access, identify, renewal, &portions.folder.identity).await?;
    let stored = portions.read_folder_sync()?;
    let listing = server.list_messages().await?;
    let listed = server.identify(&listing, &portions.folder.identity)?;
    let missing = missing_messages(&listed, &stored);
    portions.store(&listing_changes(&listed, &stored, &listing, &missing))?;
    for portion_messages in missing.chunks(PORTION_SIZE) {
        let (portion, refusal) = server
            .fetch_arrivals(portion_messages, recent_limit, portions)
            .await?;
        portions.store(&portion)?;
        // The rows the server withheld are missing, so the folder stays
        // not completed; the listing's proof is stored.
        if let Some(refusal) = refusal {
            return Ok(portions.finish(listed.len(), Some(short_list(refusal))));
        }
    }
    if let Some(refusal) = listing.refusal {
        return Ok(portions.finish(listed.len(), Some(short_list(refusal))));
    }
    if !missing.is_empty() {
        portions.store(&FolderPortion {
            state: Some(completed(None)),
            ..FolderPortion::default()
        })?;
    }
    Ok(portions.finish(listed.len(), None))
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
/// completed (spec FR-004), changed read states, and the folder's state: not
/// completed while messages are missing, completed when none are and the
/// listing completed; a refused listing leaves the state as it was.
fn listing_changes(
    listed: &[ListedMessage],
    stored: &FolderSync,
    listing: &FolderListing,
    missing: &[&ListedMessage],
) -> FolderPortion {
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
    let read_states = listed
        .iter()
        .filter(|message| {
            stored
                .stored
                .get(&message.identity)
                .is_some_and(|seen| *seen != message.seen)
        })
        .map(|message| (message.identity.clone(), message.seen))
        .collect();
    let state = if !missing.is_empty() {
        Some(FolderState::default())
    } else if complete && !stored.state.synchronized {
        Some(completed(None))
    } else {
        None
    };
    FolderPortion {
        removed,
        read_states,
        state,
        ..FolderPortion::default()
    }
}

fn short_list(refusal: ServerReply) -> IncompleteList {
    IncompleteList::ServerRefused {
        reply: refusal.text,
        code: refusal.code,
    }
}

/// An open IMAP folder for one cycle, and on Gmail the one renewal of its
/// access the cycle may use (research §13).
struct ImapFolder {
    reader: MailboxReader,
    identify: IdentityRule,
    renewal: Option<SessionRenewal>,
}

/// What reopening the folder with a renewed access needs.
struct SessionRenewal {
    renewal: AccessRenewal<ImapAccess>,
    folder: String,
    /// The token the session signed in with, to tell a renewed one apart.
    token: String,
}

impl ImapFolder {
    async fn open(
        access: ImapAccess,
        identify: IdentityRule,
        renewal: Option<AccessRenewal<ImapAccess>>,
        folder: &str,
    ) -> Result<Self, ImapError> {
        let renewal = match (renewal, &access.credential) {
            (Some(renewal), ImapCredential::AccessToken(token)) => Some(SessionRenewal {
                renewal,
                folder: folder.to_owned(),
                token: token.clone(),
            }),
            _ => None,
        };
        let reader = MailboxReader::open(imap_account(access), options(identify), folder).await?;
        Ok(Self {
            reader,
            identify,
            renewal,
        })
    }

    async fn list_messages(&mut self) -> Result<FolderListing, ImapError> {
        let items = self.row_items();
        self.request(async |reader| reader.list_messages(items).await)
            .await
    }

    /// What the listing and the rows ask for beyond RFC 3501.
    fn row_items(&self) -> RowItems {
        match self.identify {
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
        let generic_prefix = match self.identify {
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
                    seen: message.seen,
                })
            })
            .collect();
        Ok(listed)
    }

    /// One portion of missing messages: those the account already holds,
    /// related without fetching them (research §4), and the others with
    /// their rows and, for those received after `recent_limit`, their text.
    /// A message that disappeared meanwhile is left out. A row fetch the
    /// server refused returns its reason with what it answered.
    async fn fetch_arrivals(
        &mut self,
        messages: &[&ListedMessage],
        recent_limit: i64,
        portions: &PortionWriter<'_>,
    ) -> Result<(FolderPortion, Option<ServerReply>), CycleEnd> {
        let identities: Vec<String> = messages
            .iter()
            .map(|message| message.identity.clone())
            .collect();
        let known = portions.stored_identities(&identities)?;
        let (known, unknown): (Vec<&ListedMessage>, Vec<&ListedMessage>) = messages
            .iter()
            .copied()
            .partition(|message| known.contains(&message.identity));
        let uids: Vec<u32> = unknown.iter().map(|message| message.uid).collect();
        let items = self.row_items();
        let rows = self
            .request(async |reader| reader.fetch_rows_by_uid(&uids, items).await)
            .await?;
        log_gmail_rows(&rows.rows);
        let recent: Vec<u32> = rows
            .rows
            .iter()
            .filter(|row| row.internal_date.is_some_and(|date| date >= recent_limit))
            .map(|row| row.uid)
            .collect();
        let mut contents = self
            .request(async |reader| read_contents(reader, &recent).await)
            .await?;
        let arrived = rows
            .rows
            .into_iter()
            .filter_map(|row| {
                let content = match recent.contains(&row.uid) {
                    // A message missing here disappeared meanwhile.
                    true => contents.remove(&row.uid)?,
                    false => ReceivedContent::NotDownloaded,
                };
                let listed = unknown.iter().find(|message| message.uid == row.uid)?;
                Some(Message {
                    identity: listed.identity.clone(),
                    fields: tracing::debug_span!("message", uid = row.uid)
                        .in_scope(|| decode_display_fields(&row.list_headers)),
                    received_unix: row.internal_date,
                    seen: row.seen,
                    content,
                })
            })
            .collect();
        let portion = FolderPortion {
            known_arrived: known
                .iter()
                .map(|message| (message.identity.clone(), message.seen))
                .collect(),
            arrived,
            ..FolderPortion::default()
        };
        Ok((portion, rows.refusal))
    }

    /// Runs one request of the cycle. When the server ended a Gmail session,
    /// asks Online Accounts for the access once and, only with a different
    /// token, opens the folder again and repeats the request once; otherwise
    /// the server's end stands with its own words (research §13).
    async fn request<T>(
        &mut self,
        mut request: impl AsyncFnMut(&mut MailboxReader) -> Result<T, ImapError>,
    ) -> Result<T, ImapError> {
        let ended = match request(&mut self.reader).await {
            Err(error) if error.ended_by_server && self.renewal.is_some() => error,
            answered => return answered,
        };
        self.reopen_with_renewed_access(ended).await?;
        request(&mut self.reader).await
    }

    async fn reopen_with_renewed_access(&mut self, ended: ImapError) -> Result<(), ImapError> {
        let renewal = self.renewal.take().expect("asked only with a renewal");
        let Some(access) = renewal.renewal.renew().await else {
            return Err(ended);
        };
        let renewed = matches!(&access.credential,
            ImapCredential::AccessToken(token) if *token != renewal.token);
        if !renewed {
            tracing::info!("Online Accounts gave the same access, so the session's end stands");
            return Err(ended);
        }
        tracing::info!("the server ended the session; opening the folder again");
        let reader = MailboxReader::open(
            imap_account(access),
            options(self.identify),
            &renewal.folder,
        )
        .await?;
        // Another UIDVALIDITY means the UIDs the cycle holds name other
        // messages.
        if reader.uid_validity() != self.reader.uid_validity() {
            return Err(ImapFailure::MailboxChanged.into());
        }
        self.reader = reader;
        Ok(())
    }
}

/// What the provider asks for beyond RFC 3501 when it opens a folder.
fn options(identify: IdentityRule) -> OpenOptions {
    match identify {
        IdentityRule::Generic => OpenOptions::default(),
        IdentityRule::Gmail => gmail_options(),
    }
}
