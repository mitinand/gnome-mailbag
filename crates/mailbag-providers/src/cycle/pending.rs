// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sending step of a cycle (specs/011-read-and-star FR-007 to FR-010;
//! research §2, §7, §14, §15): the folder's pending changes go to the
//! server; a change ends when the cycle sees the server hold it, a refused
//! one is dropped and fails the cycle, and one whose outcome is unknown
//! stays for the next cycle.

use super::{
    CycleEnd,
    graph::{GraphService, graph_id},
    imap::ListedMessage,
};
use crate::{LoadFailure, store_load::BatchWriter};
use mailbag_domain::MessageFlag;
use mailbag_graph::{FlagUpdate, GraphError, GraphFailure};
use mailbag_imap::{MailboxReader, StoreFlag};
use std::collections::{BTreeMap, HashMap};

/// How many messages one IMAP command changes, which keeps the command
/// within a server's line limit, 64 KiB on Dovecot by default (research §14).
const UIDS_PER_COMMAND: usize = 100;

/// The value an IMAP cycle last sent for each message's flag, by identity.
/// The server's `OK` does not say the message changed, since a UID the
/// mailbox no longer has is ignored (RFC 3501 §6.4.8); the listing after
/// the cycle's commands says it (research §15).
pub(super) type SentChanges = HashMap<(String, MessageFlag), bool>;

/// Ends, without a command, the pending changes whose value the listing
/// shows the server holds: a star set and taken off before the cycle,
/// a change another client made first, or a command of an earlier cycle
/// that was applied though its answer was lost (spec FR-007, FR-009).
/// Called once, right after the listing, while it is current; a later
/// sending step sends instead, since the listing is old by then
/// (research §15).
pub(super) fn end_changes_the_listing_shows(
    listed: &HashMap<&str, &ListedMessage>,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    let mut shown = Vec::new();
    for change in batches.pending_changes()? {
        let Some(message) = listed.get(change.identity.as_str()) else {
            continue;
        };
        let listed_value = match change.flag {
            MessageFlag::Seen => message.flags.seen,
            MessageFlag::Flagged => message.flags.flagged,
        };
        if listed_value == change.wanted {
            shown.push((change.identity, change.flag, change.wanted));
        }
    }
    settle_changes_server_holds(shown, batches)
}

/// Sends the folder's pending changes to the IMAP server by the UIDs the
/// listing gave: one `UID STORE` per flag, wanted value and hundred
/// messages, each recorded in `sent_changes`. A wish equal to the value
/// this cycle last sent waits for the listing after the commands. The
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
            for identity in identities {
                sent_changes.insert((identity, flag), wanted);
            }
        }
    }
    Ok(())
}

/// Ends each change the cycle sent whose value the listing after its
/// commands shows; one it does not show, or shows otherwise, stays for the
/// next cycle of a folder that lists the message (research §15).
pub(super) fn settle_sent_changes(
    listed: &[ListedMessage],
    sent_changes: &SentChanges,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    let mut shown = Vec::new();
    for message in listed {
        let listed_values = [
            (MessageFlag::Seen, message.flags.seen),
            (MessageFlag::Flagged, message.flags.flagged),
        ];
        for (flag, value) in listed_values {
            if sent_changes.get(&(message.identity.clone(), flag)) == Some(&value) {
                shown.push((message.identity.clone(), flag, value));
            }
        }
    }
    settle_changes_server_holds(shown, batches)
}

/// Ends the changes whose value the cycle saw the server hold, one store
/// write per flag and value.
fn settle_changes_server_holds(
    changes: Vec<(String, MessageFlag, bool)>,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    let mut groups: BTreeMap<(MessageFlag, bool), Vec<String>> = BTreeMap::new();
    for (identity, flag, value) in changes {
        groups.entry((flag, value)).or_default().push(identity);
    }
    for ((flag, value), identities) in groups {
        batches.settle(&identities, flag, value)?;
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
