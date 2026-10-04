// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sending step of a cycle (specs/011-read-and-star FR-007 to FR-010;
//! research §2, §7, §14): the folder's pending changes go to the server; an
//! accepted change settles, a refused one is dropped and fails the cycle,
//! and one whose outcome is unknown stays for the next cycle.

use super::{
    CycleEnd,
    graph::{GraphService, graph_id},
};
use crate::{LoadFailure, store_load::BatchWriter};
use mailbag_domain::{MessageFlag, PendingChange};
use mailbag_graph::{FlagUpdate, GraphError, GraphFailure};
use mailbag_imap::{MailboxReader, StoreFlag};
use std::collections::{BTreeMap, HashMap};

/// How many messages one IMAP command changes, which keeps the command
/// within a server's line limit, 64 KiB on Dovecot by default (research §14).
const UIDS_PER_COMMAND: usize = 100;

/// Sends the folder's pending changes to the IMAP server by the UIDs the
/// listing gave: one `UID STORE` per flag, wanted value and hundred
/// messages. A message the listing lacks keeps its change for a later cycle.
/// Says whether the server accepted a command.
pub(super) async fn send_imap_changes(
    reader: &mut MailboxReader,
    listed_uids: &HashMap<String, u32>,
    batches: &mut BatchWriter<'_>,
) -> Result<bool, CycleEnd> {
    let mut command_accepted = false;
    let mut commands: BTreeMap<(MessageFlag, bool), Vec<(String, u32)>> = BTreeMap::new();
    for change in changes_to_send(batches)? {
        if let Some(uid) = listed_uids.get(&change.identity) {
            let messages = commands.entry((change.flag, change.wanted)).or_default();
            messages.push((change.identity, *uid));
        }
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
            match reader.store_flags(&uids, store_flag, wanted).await? {
                None => {
                    batches.settle(&identities, flag, wanted)?;
                    command_accepted = true;
                }
                Some(refusal) => {
                    batches.drop_pending(&identities, flag, wanted)?;
                    return Err(refusal.into());
                }
            }
        }
    }
    Ok(command_accepted)
}

/// Sends the folder's pending changes to Microsoft 365, one request each.
/// The service refusing a change drops it; a 5xx answer or a lost
/// connection leaves it for the next cycle, which sends it again unless
/// its round reports the value (spec FR-009).
pub(super) async fn send_graph_changes(
    service: &mut GraphService,
    batches: &mut BatchWriter<'_>,
) -> Result<(), CycleEnd> {
    for change in changes_to_send(batches)? {
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

/// The folder's pending changes the server lacks. One whose wanted value
/// the stored server value already has ends here, without a command: a
/// command for it was accepted while the user changed it back (research
/// §14).
fn changes_to_send(batches: &mut BatchWriter<'_>) -> Result<Vec<PendingChange>, CycleEnd> {
    let (agreed, to_send): (Vec<_>, Vec<_>) = batches
        .pending_changes()?
        .into_iter()
        .partition(|change| change.wanted == change.server_value);
    for change in agreed {
        batches.settle(&[change.identity], change.flag, change.wanted)?;
    }
    Ok(to_send)
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
