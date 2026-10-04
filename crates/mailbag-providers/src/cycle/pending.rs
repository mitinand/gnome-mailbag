// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sending step of a cycle (specs/011-read-and-star FR-007 to FR-010;
//! research §2, §7, §14, §15): the folder's pending changes go to the
//! server; a change ends when the server reports the message holding it,
//! in the listing at the cycle's start or in the flags read right after
//! the command; a refused one is dropped and fails the cycle, and one whose
//! outcome is unknown stays for the next cycle.

use super::{
    CycleEnd,
    graph::{GraphService, graph_id},
    imap::ListedMessage,
};
use crate::{LoadFailure, store_load::BatchWriter};
use mailbag_domain::{MessageFlag, PendingChange};
use mailbag_graph::{FlagUpdate, GraphError, GraphFailure};
use mailbag_imap::{MailboxReader, StoreFlag};
use std::collections::{BTreeMap, HashMap};

/// How many messages one IMAP command changes, which keeps the command
/// within a server's line limit, 64 KiB on Dovecot by default (research §14).
const UIDS_PER_COMMAND: usize = 100;

/// The value an IMAP cycle sent for each message's flag, by identity: a
/// wish equal to it is not sent again this cycle, whether the reading after
/// the command confirmed it or not, and a cycle that sent anything runs the
/// state pass once more (research §15; 009 FR-005).
pub(super) type SentChanges = HashMap<(String, MessageFlag), bool>;

/// The value of `flag` among a message's flags.
fn flag_of(flag: MessageFlag, seen: bool, flagged: bool) -> bool {
    match flag {
        MessageFlag::Seen => seen,
        MessageFlag::Flagged => flagged,
    }
}

/// Ends, without a command, the pending changes whose value the listing
/// shows the server holds: a star set and taken off before the cycle,
/// a change another client made first, or a command of an earlier cycle
/// that was applied though its answer was lost (spec FR-007, FR-009).
/// Called once, right after the listing, while it is current, with the
/// folder's pending changes read for the pass's plan; a later sending step
/// sends instead, since the listing is old by then (research §15).
pub(super) fn end_changes_the_listing_shows(
    listed: &HashMap<&str, &ListedMessage>,
    pending: &[PendingChange],
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    let mut shown: BTreeMap<(MessageFlag, bool), Vec<String>> = BTreeMap::new();
    for change in pending {
        let Some(message) = listed.get(change.identity.as_str()) else {
            continue;
        };
        if flag_of(change.flag, message.flags.seen, message.flags.flagged) == change.wanted {
            let identities = shown.entry((change.flag, change.wanted)).or_default();
            identities.push(change.identity.clone());
        }
    }
    for ((flag, value), identities) in shown {
        batches.settle(&identities, flag, value)?;
    }
    Ok(())
}

/// Sends the folder's pending changes to the IMAP server by the UIDs the
/// listing gave: one `UID STORE` per flag, wanted value and hundred
/// messages, each recorded in `sent_changes` and confirmed by the flags
/// read right after it (spec FR-007(d)): a message the reading shows with
/// the wanted value is settled; one it does not report has left the
/// folder, one it shows otherwise was changed meanwhile, and a reading the
/// server refuses confirms nothing, so those stay pending for the next
/// cycle. A wish equal to the value this cycle sent is not sent again. The
/// listing's own values are not compared here: by a later sending step
/// they may be minutes old and another client may have changed the flag,
/// and a command for a value the server has is harmless (research §15).
/// A message the listing lacks keeps its change for a later cycle.
pub(super) async fn send_imap_changes(
    reader: &mut MailboxReader,
    listed: &HashMap<&str, &ListedMessage>,
    sent_changes: &mut SentChanges,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    let mut commands: BTreeMap<(MessageFlag, bool), Vec<(String, u32)>> = BTreeMap::new();
    for change in batches.pending_changes()? {
        let Some(message) = listed.get(change.identity.as_str()) else {
            continue;
        };
        if sent_changes.get(&(change.identity.clone(), change.flag)) == Some(&change.wanted) {
            continue;
        }
        let messages = commands.entry((change.flag, change.wanted)).or_default();
        messages.push((change.identity, message.uid));
    }
    for ((flag, wanted), mut messages) in commands {
        messages.sort_unstable_by_key(|(_, uid)| *uid);
        for command_messages in messages.chunks(UIDS_PER_COMMAND) {
            let (identities, uids): (Vec<String>, Vec<u32>) =
                command_messages.iter().cloned().unzip();
            tracing::debug!(?identities, ?flag, wanted, "flag change sent");
            let store_flag = match flag {
                MessageFlag::Seen => StoreFlag::Seen,
                MessageFlag::Flagged => StoreFlag::Flagged,
            };
            if let Some(refusal) = reader.store_flags(&uids, store_flag, wanted).await? {
                batches.drop_pending(&identities, flag, wanted)?;
                return Err(refusal.into());
            }
            for identity in &identities {
                sent_changes.insert((identity.clone(), flag), wanted);
            }
            let reading = reader.fetch_flags(&uids).await?;
            let confirmed: Vec<String> = (reading.messages.iter())
                .filter(|message| flag_of(flag, message.seen, message.flagged) == wanted)
                .filter_map(|message| {
                    let named = command_messages.iter().find(|(_, uid)| *uid == message.uid);
                    named.map(|(identity, _)| identity.clone())
                })
                .collect();
            if !confirmed.is_empty() {
                batches.settle(&confirmed, flag, wanted)?;
            }
        }
    }
    Ok(())
}

/// Sends every pending change of the folder to Microsoft 365, one request
/// each, whatever the delta reported, since a report may come late or be
/// replayed (research §15). The service accepting a change ends it; its
/// refusal drops it; a 5xx answer or a lost connection leaves it for the
/// next cycle, which sends it again (spec FR-009).
pub(super) async fn send_graph_changes(
    service: &mut GraphService,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    for change in batches.pending_changes()? {
        // A Microsoft 365 folder's messages all have such an identity.
        let id = graph_id(&change.identity).expect("a Microsoft 365 identity");
        let update = match change.flag {
            MessageFlag::Seen => FlagUpdate::Read(change.wanted),
            MessageFlag::Flagged => FlagUpdate::Starred(change.wanted),
        };
        tracing::debug!(
            identity = change.identity,
            flag = ?change.flag,
            wanted = change.wanted,
            "flag change sent"
        );
        let identities = [change.identity.clone()];
        match service.update_flags(id, update).await {
            Ok(()) => batches.settle(&identities, change.flag, change.wanted)?,
            Err(error) if refuses_the_change(&error) => {
                batches.drop_pending(&identities, change.flag, change.wanted)?;
                return Err(CycleEnd::Failed(LoadFailure::MicrosoftGraphChangeRefused(
                    error,
                )));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Whether the service refused the change itself: a 4xx answer other than
/// the token's 401. A 5xx answer says the service could not complete the
/// request, which leaves the outcome unknown.
fn refuses_the_change(error: &GraphError) -> bool {
    matches!(
        error.failure,
        GraphFailure::Refused { status, .. } if (400..500).contains(&status) && status != 401
    )
}
