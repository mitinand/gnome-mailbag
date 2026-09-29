// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cycle of a Microsoft 365 folder (spec FR-007; research §5, §13): the
//! service's delta reading, one portion per page, from the folder's saved
//! position.

use super::{CycleEnd, completed};
use crate::{
    LoadResult, microsoft365::received_fields, renewal::AccessRenewal, store_load::PortionWriter,
};
use goa_adapter::GraphAccess;
use mailbag_domain::{FolderPortion, FolderState, Message, ReceivedContent};
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
    /// end, the stored messages it did not list are removed.
    FullRereading { listed: HashSet<String> },
}

/// The cycle of a Microsoft 365 folder: pages of the delta reading, each
/// stored as a portion, until the reading completes with the position the
/// next cycle starts from.
pub(super) async fn synchronize_graph_folder(
    access: GraphAccess,
    service_url: String,
    renewal: AccessRenewal<GraphAccess>,
    portions: &mut PortionWriter<'_>,
) -> Result<LoadResult, CycleEnd> {
    let recent_limit = super::recent_limit(SystemTime::now());
    let stored = portions.read_folder_sync()?;
    let folder_id = portions.folder.identity.clone();
    let mut service = GraphService::new(access, service_url, renewal);
    let (mut from, mut reading) = where_to_start(&stored, &folder_id);
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
            listed.extend(changes.keys().cloned());
        }
        let mut portion = service
            .portion_from_changes(changes, &reading, &stored, recent_limit, portions)
            .await?;
        match page.next {
            NextPage::More(next_link) => {
                if matches!(reading, Reading::FirstFill { .. }) {
                    portion.state = Some(FolderState {
                        server_position: Some(next_link.clone()),
                        synchronized: false,
                    });
                }
                portions.store(&portion)?;
                from = ChangesFrom::Link(next_link);
            }
            NextPage::Done(delta_link)
                if matches!(reading, Reading::FirstFill { continued: true }) =>
            {
                portion.state = Some(FolderState {
                    server_position: Some(delta_link.clone()),
                    synchronized: false,
                });
                portions.store(&portion)?;
                from = ChangesFrom::Link(delta_link);
                reading = Reading::Round;
            }
            NextPage::Done(delta_link) => {
                if let Reading::FullRereading { listed } = &reading {
                    portion.removed.extend(
                        stored
                            .stored
                            .keys()
                            .filter(|identity| !listed.contains(graph_id(identity)))
                            .cloned(),
                    );
                }
                portion.state = Some(completed(Some(delta_link)));
                portions.store(&portion)?;
                return Ok(portions.finish(reported, None));
            }
        }
    }
}

/// Where the reading starts: the saved position after a completed cycle, the
/// saved place of an unfinished first fill, or a first reading.
fn where_to_start(stored: &FolderSync, folder_id: &str) -> (ChangesFrom, Reading) {
    match (&stored.state.server_position, stored.state.synchronized) {
        (Some(position), true) => (ChangesFrom::Link(position.clone()), Reading::Round),
        (Some(place), false) => (
            ChangesFrom::Link(place.clone()),
            Reading::FirstFill { continued: true },
        ),
        (None, _) => (
            ChangesFrom::FirstReading(folder_id.to_owned()),
            Reading::FirstFill { continued: false },
        ),
    }
}

/// A page's entries merged per message in their order, since the service
/// may repeat and reorder them: a later entry wins, and a partial one never
/// drops an earlier read state (research §5).
fn merge_per_message(changes: Vec<MessageChange>) -> HashMap<String, MessageChange> {
    let mut merged: HashMap<String, MessageChange> = HashMap::new();
    for change in changes {
        let id = match &change {
            MessageChange::Removed(id) | MessageChange::Changed { id, .. } => id.clone(),
            MessageChange::Listed(message) => message.immutable_id.clone(),
        };
        let combined = match (merged.remove(&id), change) {
            (
                Some(MessageChange::Listed(mut message)),
                MessageChange::Changed {
                    is_read,
                    other_fields: false,
                    ..
                },
            ) => {
                message.is_read = is_read.unwrap_or(message.is_read);
                MessageChange::Listed(message)
            }
            (
                Some(MessageChange::Changed {
                    is_read: earlier_read,
                    other_fields: earlier_fields,
                    ..
                }),
                MessageChange::Changed {
                    id,
                    is_read,
                    other_fields,
                },
            ) => MessageChange::Changed {
                id,
                is_read: is_read.or(earlier_read),
                other_fields: other_fields || earlier_fields,
            },
            (_, change) => change,
        };
        merged.insert(id, combined);
    }
    merged
}

/// A stored Microsoft 365 message's identity.
fn identity(graph_id: &str) -> String {
    format!("graph:{graph_id}")
}

/// The service's identifier inside a stored identity.
fn graph_id(identity: &str) -> &str {
    identity.strip_prefix("graph:").unwrap_or(identity)
}

/// Microsoft Graph for one cycle, with the one renewal of its token the
/// cycle may use (research §13).
struct GraphService {
    service_url: String,
    access_token: String,
    renewal: Option<AccessRenewal<GraphAccess>>,
    /// Whether a request of this cycle succeeded: a refusal before that is
    /// the sign-in's, not an expiry.
    answered: bool,
}

/// A message to store in full, and whether its text is to be fetched.
struct Arrival {
    message: GraphMessage,
    wants_text: bool,
}

impl GraphService {
    fn new(access: GraphAccess, service_url: String, renewal: AccessRenewal<GraphAccess>) -> Self {
        Self {
            service_url,
            access_token: access.access_token,
            renewal: Some(renewal),
            answered: false,
        }
    }

    async fn read_changes(&mut self, from: &ChangesFrom) -> Result<ChangePage, GraphError> {
        self.request(async |service_url, token| {
            read_message_changes(service_url, token, from).await
        })
        .await
    }

    /// One page's changes as a portion. A message another folder of the
    /// account holds, a partial entry that changed other list fields and an
    /// entry for a message the account lacks are read again; a message read
    /// again is kept only if it is in this folder now (research §5).
    async fn portion_from_changes(
        &mut self,
        changes: HashMap<String, MessageChange>,
        reading: &Reading,
        stored: &FolderSync,
        recent_limit: i64,
        portions: &PortionWriter<'_>,
    ) -> Result<FolderPortion, CycleEnd> {
        let identities: Vec<String> = changes.keys().map(|id| identity(id)).collect();
        let held_elsewhere = portions.identities_in_other_folders(&identities)?;
        let held_by_account = portions.stored_identities(&identities)?;
        let mut portion = FolderPortion::default();
        let mut arrivals = Vec::new();
        for (id, change) in changes {
            let stored_identity = identity(&id);
            let held_here = stored.stored.contains_key(&stored_identity);
            let known = held_by_account.contains(&stored_identity);
            let read_again = match &change {
                MessageChange::Removed(_) => {
                    if held_here {
                        portion.removed.push(stored_identity);
                    }
                    continue;
                }
                _ if held_elsewhere.contains(&stored_identity) => true,
                MessageChange::Listed(_) => false,
                MessageChange::Changed { other_fields, .. } => *other_fields || !known,
            };
            if read_again {
                if let Some(message) = self.read_in_folder(&id, &portions.folder.identity).await? {
                    arrivals.push(Arrival {
                        message,
                        wants_text: !known,
                    });
                }
                continue;
            }
            match change {
                MessageChange::Listed(message) => arrivals.push(Arrival {
                    message,
                    wants_text: !known,
                }),
                MessageChange::Changed {
                    is_read: Some(seen),
                    ..
                } => portion.read_states.push((stored_identity, seen)),
                _ => {}
            }
        }
        let texts = self
            .read_texts(&arrivals, reading, recent_limit, &portions.folder.identity)
            .await?;
        portion.arrived = arrivals
            .into_iter()
            .map(|arrival| stored_message(arrival, &texts, recent_limit))
            .collect();
        Ok(portion)
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

    /// The texts of the arrivals received after `recent_limit`: for a page
    /// of a first reading, by the range of their dates in one request; for a
    /// round of changes, whose arrivals are scattered in time, one by one
    /// (research §5).
    async fn read_texts(
        &mut self,
        arrivals: &[Arrival],
        reading: &Reading,
        recent_limit: i64,
        folder_id: &str,
    ) -> Result<HashMap<String, Option<String>>, GraphError> {
        let recent: Vec<&GraphMessage> = arrivals
            .iter()
            .filter(|arrival| arrival.wants_text && is_recent(&arrival.message, recent_limit))
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

    /// Runs one request of the cycle. When the service refuses the token after
    /// the cycle's first successful request, asks Online Accounts for the
    /// access once and, only with a different token, repeats the request once;
    /// otherwise the refusal stands (research §13).
    async fn request<T>(
        &mut self,
        mut request: impl AsyncFnMut(&str, &str) -> Result<T, GraphError>,
    ) -> Result<T, GraphError> {
        let refused = match request(&self.service_url, &self.access_token).await {
            Err(error) if self.answered && is_refused_token(&error) && self.renewal.is_some() => {
                error
            }
            answer => {
                self.answered |= answer.is_ok();
                return answer;
            }
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

/// An arrival as the store keeps it: with its text when it is recent and the
/// account had none, otherwise without one, which keeps a stored content.
fn stored_message(
    arrival: Arrival,
    texts: &HashMap<String, Option<String>>,
    recent_limit: i64,
) -> Message {
    let message = arrival.message;
    let content = match (arrival.wants_text, is_recent(&message, recent_limit)) {
        (true, true) => match texts.get(&message.immutable_id) {
            Some(Some(text)) => ReceivedContent::Text(text.clone()),
            _ => ReceivedContent::TextNotReturned,
        },
        _ => ReceivedContent::NotDownloaded,
    };
    Message {
        identity: identity(&message.immutable_id),
        fields: received_fields(&message),
        received_unix: message.received_unix,
        seen: message.is_read,
        content,
    }
}
