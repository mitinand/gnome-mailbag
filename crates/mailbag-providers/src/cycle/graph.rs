// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cycle of a Microsoft 365 folder (spec FR-007; research §5, §13): the
//! service's delta reading, one batch per page, from the folder's saved
//! position.

use super::{CycleEnd, completed};
use crate::{
    LoadResult, microsoft365::received_fields, renewal::AccessRenewal, store_load::BatchWriter,
};
use goa_adapter::GraphAccess;
use mailbag_content::preview_of_text;
use mailbag_domain::{FlagChanges, FolderBatch, FolderState, Message, ReceivedContent};
use mailbag_graph::{
    ChangePage, ChangesFrom, GraphError, GraphFailure, GraphMessage, MessageChange, NextPage,
    read_message, read_message_changes, read_message_text, read_texts_received_between,
};
use mailbag_store::FolderSync;
use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

/// What the cycle is reading, which decides how texts are fetched, whether
/// each page saves its place and what completes the cycle.
enum Reading {
    /// A first reading of a folder never synchronized. `continued` when it
    /// goes on from a place an earlier cycle saved: it then reads one more
    /// round before it completes, since changes made during the pause come
    /// only with that round (research §5).
    FirstFill { continued: bool },
    /// The changes since the saved position.
    Round,
    /// A first reading after the service rejected the saved position: at its
    /// end, the stored messages it did not list, by identity, are removed.
    FullRereading { listed: HashSet<String> },
}

/// The cycle of a Microsoft 365 folder: pages of the delta reading, each
/// stored as a batch, until the reading completes with the position the
/// next cycle starts from.
pub(super) async fn synchronize_graph_folder(
    access: GraphAccess,
    service_url: String,
    renewal: AccessRenewal,
    batches: &mut BatchWriter<'_>,
) -> Result<LoadResult, CycleEnd> {
    let recent_limit = super::recent_limit(SystemTime::now());
    let mut stored = batches.read_folder_sync()?;
    let folder_id = batches.folder.identity.clone();
    let mut service = GraphService::new(access, service_url, renewal);
    let (mut from, mut reading) = where_to_start(&stored, &folder_id);
    // Where the current round started: an interrupted round starts there again.
    let mut round_start = stored.state.server_position.clone();
    // Messages the service reported, for the record.
    let mut reported = 0;
    loop {
        let page = match service.read_changes(&from).await {
            // Only a saved link can be rejected into a full reading, once; a
            // rejected first reading fails the cycle.
            Err(error)
                if error.failure == GraphFailure::PositionRejected
                    && matches!(from, ChangesFrom::Link(_))
                    && !matches!(reading, Reading::FullRereading { .. }) =>
            {
                tracing::info!("the service no longer accepts the saved position");
                // What the folder holds now, earlier batches of this cycle
                // included, is what the full reading's end compares with.
                stored = batches.read_folder_sync()?;
                from = ChangesFrom::FirstReading(folder_id.clone());
                reading = Reading::FullRereading {
                    listed: HashSet::new(),
                };
                continue;
            }
            page => page?,
        };
        let changes = merge_per_message(page.changes);
        reported += changes.len();
        if let Reading::FullRereading { listed } = &mut reading {
            listed.extend(changes.keys().map(|id| identity(id)));
        }
        let mut batch = service
            .batch_from_changes(changes, &reading, recent_limit, batches)
            .await?;
        match page.next {
            NextPage::More(next_link) => {
                // Not the reading's last page: the folder is not completed.
                batch.state = Some(match reading {
                    Reading::FirstFill { .. } => FolderState {
                        server_position: None,
                        fill_place: Some(next_link.clone()),
                        synchronized: false,
                    },
                    Reading::Round | Reading::FullRereading { .. } => FolderState {
                        server_position: round_start.clone(),
                        fill_place: None,
                        synchronized: false,
                    },
                });
                batches.store(&batch)?;
                from = ChangesFrom::Link(next_link);
            }
            NextPage::Done(delta_link)
                if matches!(reading, Reading::FirstFill { continued: true }) =>
            {
                batch.state = Some(FolderState {
                    server_position: Some(delta_link.clone()),
                    fill_place: None,
                    synchronized: false,
                });
                batches.store(&batch)?;
                round_start = Some(delta_link.clone());
                from = ChangesFrom::Link(delta_link);
                reading = Reading::Round;
            }
            NextPage::Done(delta_link) => {
                if let Reading::FullRereading { listed } = &reading {
                    batch.removed.extend(
                        stored
                            .stored
                            .keys()
                            .filter(|identity| !listed.contains(*identity))
                            .cloned(),
                    );
                }
                batch.state = Some(completed(Some(delta_link)));
                batches.store(&batch)?;
                return Ok(batches.finish(reported, None));
            }
        }
    }
}

/// Where the reading starts: the saved place of an unfinished first fill,
/// the saved position of the next round, or a first reading. A first
/// reading of a folder that holds rows is a stopped full reading, started
/// again in full so that it removes what it does not list.
fn where_to_start(stored: &FolderSync, folder_id: &str) -> (ChangesFrom, Reading) {
    match (&stored.state.fill_place, &stored.state.server_position) {
        (Some(place), _) => (
            ChangesFrom::Link(place.clone()),
            Reading::FirstFill { continued: true },
        ),
        (None, Some(position)) => (ChangesFrom::Link(position.clone()), Reading::Round),
        (None, None) if !stored.stored.is_empty() => (
            ChangesFrom::FirstReading(folder_id.to_owned()),
            Reading::FullRereading {
                listed: HashSet::new(),
            },
        ),
        (None, None) => (
            ChangesFrom::FirstReading(folder_id.to_owned()),
            Reading::FirstFill { continued: false },
        ),
    }
}

/// A page's entries merged per message in their order, since the service
/// may repeat and reorder them: a later entry wins, and a partial one never
/// drops an earlier read state or star. An entry marking the message
/// removed, met with another entry for it, is trusted neither way: the
/// message is read as the service holds it now, like an entry that changed
/// other fields, whatever else the page says about it (research §5).
fn merge_per_message(changes: Vec<MessageChange>) -> HashMap<String, MessageChange> {
    let mut merged: HashMap<String, MessageChange> = HashMap::new();
    let mut read_again: HashSet<String> = HashSet::new();
    for change in changes {
        let id = match &change {
            MessageChange::Removed(id) | MessageChange::Changed { id, .. } => id.clone(),
            MessageChange::Listed(message) => message.immutable_id.clone(),
        };
        if read_again.contains(&id) {
            continue;
        }
        let combined = match (merged.remove(&id), change) {
            (Some(earlier), later) if is_removed(&earlier) != is_removed(&later) => {
                read_again.insert(id.clone());
                MessageChange::Changed {
                    id: id.clone(),
                    is_read: None,
                    flagged: None,
                    other_fields: true,
                }
            }
            (
                Some(MessageChange::Listed(mut message)),
                MessageChange::Changed {
                    is_read,
                    flagged,
                    other_fields: false,
                    ..
                },
            ) => {
                message.is_read = is_read.unwrap_or(message.is_read);
                message.flagged = flagged.unwrap_or(message.flagged);
                MessageChange::Listed(message)
            }
            (
                Some(MessageChange::Changed {
                    is_read: earlier_read,
                    flagged: earlier_flagged,
                    other_fields: earlier_fields,
                    ..
                }),
                MessageChange::Changed {
                    id,
                    is_read,
                    flagged,
                    other_fields,
                },
            ) => MessageChange::Changed {
                id,
                is_read: is_read.or(earlier_read),
                flagged: flagged.or(earlier_flagged),
                other_fields: other_fields || earlier_fields,
            },
            (_, change) => change,
        };
        merged.insert(id, combined);
    }
    merged
}

fn is_removed(change: &MessageChange) -> bool {
    matches!(change, MessageChange::Removed(_))
}

/// A stored Microsoft 365 message's identity.
fn identity(graph_id: &str) -> String {
    format!("graph:{graph_id}")
}

/// Microsoft Graph for one cycle, with the one renewal of its token the
/// cycle may use (research §13).
struct GraphService {
    service_url: String,
    access_token: String,
    renewal: Option<AccessRenewal>,
}

/// A message to store in full, and whether its text is to be fetched.
struct Arrival {
    message: GraphMessage,
    /// The message arrived after the 30-day limit, and the account lacks it
    /// or the service reported its fields again (spec FR-009).
    wants_text: bool,
}

impl GraphService {
    fn new(access: GraphAccess, service_url: String, renewal: AccessRenewal) -> Self {
        Self {
            service_url,
            access_token: access.access_token,
            renewal: Some(renewal),
        }
    }

    async fn read_changes(&mut self, from: &ChangesFrom) -> Result<ChangePage, GraphError> {
        self.request(async |service_url, token| {
            read_message_changes(service_url, token, from).await
        })
        .await
    }

    /// One page's changes as a batch. A message another folder of the
    /// account holds, a partial entry that changed other list fields and an
    /// entry for a message the account lacks are read again; a message read
    /// again is kept only if it is in this folder now, and leaves the folder
    /// otherwise (research §5).
    async fn batch_from_changes(
        &mut self,
        changes: HashMap<String, MessageChange>,
        reading: &Reading,
        recent_limit: i64,
        batches: &BatchWriter<'_>,
    ) -> Result<FolderBatch, CycleEnd> {
        let identities: Vec<String> = changes.keys().map(|id| identity(id)).collect();
        let held_elsewhere = batches.identities_in_other_folders(&identities)?;
        let held_by_account = batches.stored_identities(&identities)?;
        let mut batch = FolderBatch::default();
        let mut arrivals = Vec::new();
        for (id, change) in changes {
            let stored_identity = identity(&id);
            let known = held_by_account.contains(&stored_identity);
            // The service reported the message's list fields again: a draft
            // edited elsewhere keeps its identity, so its text is read again
            // (spec FR-009).
            let fields_reported = matches!(
                change,
                MessageChange::Listed(_)
                    | MessageChange::Changed {
                        other_fields: true,
                        ..
                    }
            );
            let wants_text = |message: &GraphMessage| {
                (!known || fields_reported) && is_recent(message, recent_limit)
            };
            let read_again = match &change {
                // Not checked against the folder as the cycle found it: an
                // earlier page of this cycle may have stored the message, and
                // removing one the folder lacks changes nothing.
                MessageChange::Removed(_) => {
                    batch.removed.push(stored_identity);
                    continue;
                }
                _ if held_elsewhere.contains(&stored_identity) => true,
                MessageChange::Listed(_) => false,
                MessageChange::Changed { other_fields, .. } => *other_fields || !known,
            };
            if read_again {
                match self.read_in_folder(&id, &batches.folder.identity).await? {
                    Some(message) => arrivals.push(Arrival {
                        wants_text: wants_text(&message),
                        message,
                    }),
                    // The service places the message elsewhere or no longer
                    // finds it: it is not in this folder (spec FR-004).
                    None => batch.removed.push(stored_identity),
                }
                continue;
            }
            match change {
                MessageChange::Listed(message) => arrivals.push(Arrival {
                    wants_text: wants_text(&message),
                    message,
                }),
                // Only the flags the entry names: one the entry leaves out
                // keeps what the store holds, which an earlier page of this
                // round may have written (specs/011-read-and-star/research.md §14).
                MessageChange::Changed {
                    is_read, flagged, ..
                } if is_read.is_some() || flagged.is_some() => {
                    let changes = FlagChanges {
                        seen: is_read,
                        flagged,
                    };
                    batch.flag_states.push((stored_identity, changes));
                }
                _ => {}
            }
        }
        let texts = self
            .read_texts(&arrivals, reading, &batches.folder.identity)
            .await?;
        batch.arrived = arrivals
            .into_iter()
            .map(|arrival| stored_message(arrival, &texts))
            .collect();
        Ok(batch)
    }

    /// The message as it is now, if it is in `folder_id`.
    async fn read_in_folder(
        &mut self,
        id: &str,
        folder_id: &str,
    ) -> Result<Option<GraphMessage>, GraphError> {
        let found = self
            .request(async |service_url, token| read_message(service_url, token, id).await)
            .await?;
        Ok(found
            .filter(|(_, parent)| parent == folder_id)
            .map(|(message, _)| message))
    }

    /// The texts of the arrivals that want one: for a page
    /// of a first reading, by the range of their dates in one request; for a
    /// round of changes, whose arrivals are scattered in time, one by one
    /// (research §5).
    async fn read_texts(
        &mut self,
        arrivals: &[Arrival],
        reading: &Reading,
        folder_id: &str,
    ) -> Result<HashMap<String, Option<String>>, GraphError> {
        let recent: Vec<&GraphMessage> = arrivals
            .iter()
            .filter(|arrival| arrival.wants_text)
            .map(|arrival| &arrival.message)
            .collect();
        if recent.is_empty() {
            return Ok(HashMap::new());
        }
        if matches!(reading, Reading::Round) {
            let mut texts = HashMap::new();
            for message in recent {
                let id = &message.immutable_id;
                let text = self
                    .request(async |service_url, token| {
                        read_message_text(service_url, token, id).await
                    })
                    .await?;
                texts.insert(id.clone(), text);
            }
            return Ok(texts);
        }
        let dates = recent.iter().filter_map(|message| message.received_unix);
        let (from, to) = (dates.clone().min(), dates.max());
        let (Some(from), Some(to)) = (from, to) else {
            return Ok(HashMap::new());
        };
        let texts = self
            .request(async |service_url, token| {
                read_texts_received_between(service_url, token, folder_id, from, to).await
            })
            .await?;
        Ok(texts.into_iter().collect())
    }

    /// Runs one request of the cycle. When the service refuses the token, asks
    /// Online Accounts for the access once and, only with a different token,
    /// repeats the request once; otherwise the refusal stands, as for a token
    /// just handed out, which Online Accounts gives again (research §13).
    async fn request<T>(
        &mut self,
        mut request: impl AsyncFnMut(&str, &str) -> Result<T, GraphError>,
    ) -> Result<T, GraphError> {
        let refused = match request(&self.service_url, &self.access_token).await {
            Err(error) if is_refused_token(&error) && self.renewal.is_some() => error,
            answer => return answer,
        };
        let renewal = self.renewal.take().expect("asked only with a renewal");
        match renewal.renew().await {
            Some(access) if access.access_token != self.access_token => {
                tracing::info!("the service refused the token; repeating with a renewed one");
                self.access_token = access.access_token;
                request(&self.service_url, &self.access_token).await
            }
            _ => Err(refused),
        }
    }
}

fn is_refused_token(error: &GraphError) -> bool {
    matches!(error.failure, GraphFailure::Refused { status: 401, .. })
}

fn is_recent(message: &GraphMessage, recent_limit: i64) -> bool {
    message
        .received_unix
        .is_some_and(|received| received >= recent_limit)
}

/// An arrival as the store keeps it: with its text when it wanted one,
/// otherwise without one, which keeps a stored content.
fn stored_message(arrival: Arrival, texts: &HashMap<String, Option<String>>) -> Message {
    let message = arrival.message;
    let content = match arrival.wants_text {
        true => match texts.get(&message.immutable_id) {
            Some(Some(text)) => ReceivedContent::Text(text.clone()),
            _ => ReceivedContent::TextNotReturned,
        },
        false => ReceivedContent::NotDownloaded,
    };
    Message {
        identity: identity(&message.immutable_id),
        fields: received_fields(&message),
        received_unix: message.received_unix,
        seen: message.is_read,
        flagged: message.flagged,
        content,
        preview: preview_of_text(message.body_preview.as_deref().unwrap_or_default()),
    }
}
