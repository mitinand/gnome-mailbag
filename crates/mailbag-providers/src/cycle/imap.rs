// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
//! research §2, §3, §4, §15).

use super::{
    CycleEnd,
    pending::{SentChanges, end_changes_the_listing_shows, send_imap_changes, settle_sent_changes},
};
use crate::{
    LoadResult,
    gmail::{gmail_options, log_gmail_rows},
    imap_texts::{imap_account, read_contents},
    store_load::BatchWriter,
};
use goa_adapter::ImapAccess;
use mailbag_content::decode_display_fields;
use mailbag_domain::{
    FolderBatch, FolderNumbers, FolderState, IncompleteList, Message, MessageFlags,
};
use mailbag_imap::{
    FolderListing, ImapError, ImapFailure, ImapStep, MailboxNumbers, MailboxReader, OpenOptions,
    RowItems, ServerReply,
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
pub(super) struct ListedMessage {
    pub(super) identity: String,
    pub(super) uid: u32,
    pub(super) flags: MessageFlags,
}

/// The cycle of an IMAP folder, Generic IMAP or Gmail (spec FR-005, FR-006;
/// research §2, §3, §15): a state pass learns the folder's state from the
/// numbers its opening returns and lists what they call for; the missing
/// messages are fetched newest first, a batch at a time; the user's pending
/// changes are sent by the listing's UIDs after the listing and after each
/// batch (specs/011-read-and-star FR-007); a cycle that fetched messages or
/// sent commands ends with a second pass, so that the folder is as the
/// server has it now.
pub(super) async fn synchronize_imap_folder(
    access: ImapAccess,
    identity_rule: IdentityRule,
    batches: &mut BatchWriter<'_>,
) -> Result<LoadResult, CycleEnd> {
    let recent_limit = super::recent_limit(SystemTime::now());
    let mut server = ImapFolder::open(access, identity_rule, &batches.folder.identity).await?;
    let stored = batches.read_folder_sync()?;
    let pending = batches.pending_changes()?;
    // A synchronized folder's stored numbers describe the listing the store
    // reflects; an unfinished fill's do not.
    let reference = stored
        .state
        .synchronized
        .then_some(stored.state.numbers)
        .flatten();
    let plan = pass_plan(server.reader.numbers(), reference, !pending.is_empty());
    let first = run_state_pass(&mut server, &stored, batches, plan).await?;
    let listed_by_identity: HashMap<&str, &ListedMessage> = first
        .listed
        .iter()
        .map(|message| (message.identity.as_str(), message))
        .collect();
    end_changes_the_listing_shows(&listed_by_identity, &pending, batches)?;
    let mut sent_changes = SentChanges::new();
    send_imap_changes(
        &mut server.reader,
        &listed_by_identity,
        &mut sent_changes,
        batches,
    )
    .await?;
    let missing = missing_messages(&first.listed, &stored);
    for batch_messages in missing.chunks(BATCH_SIZE) {
        let (batch, refusal) = server
            .fetch_arrivals(batch_messages, recent_limit, batches)
            .await?;
        batches.store(&batch)?;
        send_imap_changes(
            &mut server.reader,
            &listed_by_identity,
            &mut sent_changes,
            batches,
        )
        .await?;
        // The rows the server withheld are missing, so the folder stays
        // not completed; the listing's proof is stored.
        if let Some(refusal) = refusal {
            return Ok(batches.finish(first.listed.len(), Some(short_list(refusal))));
        }
    }
    let first_listing_complete = first.refusal.is_none();
    let mut refusal = first.refusal;
    if !missing.is_empty() || !sent_changes.is_empty() {
        // The second pass: the folder as the server has it after the fill
        // and the cycle's own commands. Its reference is the first pass's
        // numbers, whose listing the store reflects once every missing
        // message is stored; a refused first listing gives none.
        server.reader.reopen().await?;
        let stored_now = batches.read_folder_sync()?;
        let reference = first_listing_complete.then_some(first.numbers);
        let plan = pass_plan(server.reader.numbers(), reference, false);
        let second = run_state_pass(&mut server, &stored_now, batches, plan).await?;
        settle_sent_changes(&second.listed, &sent_changes, batches)?;
        refusal = refusal.or(second.refusal);
    }
    let listed = match first.listed.is_empty() {
        true => first.numbers.message_count as usize,
        false => first.listed.len(),
    };
    Ok(batches.finish(listed, refusal.map(short_list)))
}

/// What a state pass lists, told by the opening's numbers against the
/// numbers of the listing the store reflects (spec FR-005(b)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassPlan {
    /// Every number equal under the same numbering: the folder is as stored.
    Nothing,
    /// The count and the next UID equal, the highest mod-sequence not: only
    /// flags changed, listed with `CHANGEDSINCE` the earlier one.
    ChangedFlags { since: u64 },
    /// Every message with its number and flags, as the base method lists.
    Everything,
}

/// Chooses what the pass lists. Without a reference, under another
/// numbering, with a number missing on either side (a server without
/// CONDSTORE gives no mod-sequence) or with pending changes, which the
/// listing's UIDs address (specs/011-read-and-star FR-007), every message
/// is listed.
fn pass_plan(
    numbers: MailboxNumbers,
    reference: Option<FolderNumbers>,
    pending_changes: bool,
) -> PassPlan {
    let Some(reference) = reference else {
        return PassPlan::Everything;
    };
    let (Some(uid_validity), Some(uid_next), Some(highest_modseq)) = (
        numbers.uid_validity,
        numbers.uid_next,
        numbers.highest_modseq,
    ) else {
        return PassPlan::Everything;
    };
    let (Some(reference_uid_next), Some(reference_modseq)) =
        (reference.uid_next, reference.highest_modseq)
    else {
        return PassPlan::Everything;
    };
    if pending_changes
        || Some(uid_validity) != reference.uid_validity
        || numbers.message_count != reference.message_count
        || uid_next != reference_uid_next
    {
        return PassPlan::Everything;
    }
    match highest_modseq == reference_modseq {
        true => PassPlan::Nothing,
        false => PassPlan::ChangedFlags {
            since: reference_modseq,
        },
    }
}

/// What a state pass found: the messages its listing showed (none when
/// nothing changed), the numbers it started from and the server's refusal
/// to finish the listing.
struct StatePass {
    listed: Vec<ListedMessage>,
    numbers: FolderNumbers,
    refusal: Option<ServerReply>,
}

/// Runs the pass's listing and stores what it proves with the opening's
/// numbers, in one batch (spec FR-005(c)): removals only from a complete
/// listing of every message (FR-004), the changed flags, and the folder's
/// state (`pass_state`). A pass that lists nothing writes the completed
/// state when the folder is not yet marked so.
async fn run_state_pass(
    server: &mut ImapFolder,
    stored: &FolderSync,
    batches: &mut BatchWriter<'_>,
    plan: PassPlan,
) -> Result<StatePass, CycleEnd> {
    let numbers = folder_numbers(server.reader.numbers());
    let listing = match plan {
        PassPlan::Nothing => {
            tracing::info!(
                messages = numbers.message_count,
                "state pass: nothing changed"
            );
            if let Some(state) = pass_state(&stored.state, numbers, true, false) {
                batches.store(&FolderBatch {
                    state: Some(state),
                    ..FolderBatch::default()
                })?;
            }
            return Ok(StatePass {
                listed: Vec::new(),
                numbers,
                refusal: None,
            });
        }
        PassPlan::ChangedFlags { since } => server.list_changed_flags(since).await?,
        PassPlan::Everything => server.list_messages().await?,
    };
    let listed = server.identify(&listing, &batches.folder.identity)?;
    let missing = missing_messages(&listed, stored);
    let complete = listing.refusal.is_none();
    let removals_proven = complete && plan == PassPlan::Everything;
    let mut batch = listing_changes(&listed, stored, removals_proven);
    batch.state = pass_state(&stored.state, numbers, complete, !missing.is_empty());
    tracing::info!(
        outcome = match plan {
            PassPlan::Everything => "folder listed",
            _ => "changed flags listed",
        },
        listed = listed.len(),
        missing = missing.len(),
        removed = batch.removed.len(),
        flag_changes = batch.flag_states.len(),
        "state pass"
    );
    batches.store(&batch)?;
    Ok(StatePass {
        listed,
        numbers,
        refusal: listing.refusal,
    })
}

/// The folder's state after a pass (spec FR-005(c), FR-008): not completed
/// while messages are missing, with the pass's numbers when its listing
/// completed and the stored ones otherwise; completed with the numbers when
/// none are missing and the listing completed, unless the folder is marked
/// so with these numbers already; nothing to write otherwise.
fn pass_state(
    stored: &FolderState,
    numbers: FolderNumbers,
    complete: bool,
    missing: bool,
) -> Option<FolderState> {
    let state = |synchronized, numbers| FolderState {
        synchronized,
        numbers,
        ..FolderState::default()
    };
    match (missing, complete) {
        (true, true) => Some(state(false, Some(numbers))),
        (true, false) => Some(state(false, stored.numbers)),
        (false, true) if !stored.synchronized || stored.numbers != Some(numbers) => {
            Some(state(true, Some(numbers)))
        }
        _ => None,
    }
}

/// The opening's numbers as the folder stores them.
fn folder_numbers(numbers: MailboxNumbers) -> FolderNumbers {
    FolderNumbers {
        uid_validity: numbers.uid_validity,
        message_count: numbers.message_count,
        uid_next: numbers.uid_next,
        highest_modseq: numbers.highest_modseq,
    }
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

/// What the listing proves: removals, when it completed and listed every
/// message (spec FR-004), and the flags that differ from the stored ones.
fn listing_changes(
    listed: &[ListedMessage],
    stored: &FolderSync,
    removals_proven: bool,
) -> FolderBatch {
    let listed_identities: HashSet<&str> = listed
        .iter()
        .map(|message| message.identity.as_str())
        .collect();
    let removed = match removals_proven {
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
    FolderBatch {
        removed,
        flag_states,
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

    async fn list_changed_flags(&mut self, since: u64) -> Result<FolderListing, ImapError> {
        let items = self.row_items();
        self.reader.list_changed_flags(since, items).await
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opening(
        uid_validity: u32,
        message_count: u32,
        uid_next: u32,
        highest_modseq: Option<u64>,
    ) -> MailboxNumbers {
        MailboxNumbers {
            uid_validity: Some(uid_validity),
            message_count,
            uid_next: Some(uid_next),
            highest_modseq,
        }
    }

    /// Spec FR-005(b): the opening's numbers against the listing the store
    /// reflects decide what the pass lists.
    #[test]
    fn the_pass_lists_what_the_numbers_call_for() {
        let stored = folder_numbers(opening(1, 5, 20, Some(40)));
        let plan = |numbers, pending| pass_plan(numbers, Some(stored), pending);
        assert_eq!(plan(opening(1, 5, 20, Some(40)), false), PassPlan::Nothing);
        assert_eq!(
            plan(opening(1, 5, 20, Some(44)), false),
            PassPlan::ChangedFlags { since: 40 }
        );
        // An arrival, a removal, a renumbered folder with the same count and
        // next UID, a server without mod-sequences, pending changes, and no
        // listing to compare with: every message.
        assert_eq!(
            plan(opening(1, 5, 21, Some(44)), false),
            PassPlan::Everything
        );
        assert_eq!(
            plan(opening(1, 4, 20, Some(44)), false),
            PassPlan::Everything
        );
        assert_eq!(
            plan(opening(2, 5, 20, Some(40)), false),
            PassPlan::Everything
        );
        assert_eq!(plan(opening(1, 5, 20, None), false), PassPlan::Everything);
        assert_eq!(
            plan(opening(1, 5, 20, Some(40)), true),
            PassPlan::Everything
        );
        assert_eq!(
            pass_plan(opening(1, 5, 20, Some(40)), None, false),
            PassPlan::Everything
        );
        let without_mod_sequence = folder_numbers(opening(1, 5, 20, None));
        assert_eq!(
            pass_plan(opening(1, 5, 20, None), Some(without_mod_sequence), false),
            PassPlan::Everything
        );
    }
}
