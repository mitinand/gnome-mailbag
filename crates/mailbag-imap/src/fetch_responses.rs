// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the responses to one FETCH command say: the rows, the part structures
//! and the text parts of the messages it addressed, and how the command
//! ended. Nothing here touches the session; the reader runs the commands.

use crate::{GmailRow, MessagePart, MessageRow, MessageText, ReceivedPart, ServerReply, TextParts};
use async_imap::{
    error::Error,
    imap_proto::{MessageSection, SectionPath},
    types::{Fetch, Flag},
};
use futures_util::{Stream, TryStreamExt};
use std::{collections::BTreeMap, fmt};

/// The responses to one FETCH command and how the command ended.
pub(crate) struct FetchResponses {
    pub(crate) fetches: Vec<Fetch>,
    pub(crate) end: FetchEnd,
}

pub(crate) enum FetchEnd {
    Completed,
    /// The server completed with NO, after answering for some messages or none,
    /// for example when another client expunged a message meanwhile.
    Rejected(ServerReply),
    Failed(Error),
}

/// A sequence-number FETCH can return each field separately. Sequence numbers
/// stay stable during this command; unsolicited updates outside its window do
/// not establish rows. UID FETCH uses UIDs instead because EXPUNGE is allowed.
pub(crate) fn collect_rows(fetches: &[Fetch], first: u32, last: u32) -> Vec<MessageRow> {
    let mut by_sequence = BTreeMap::<u32, Vec<&Fetch>>::new();
    for fetch in fetches {
        if (first..=last).contains(&fetch.message) {
            by_sequence.entry(fetch.message).or_default().push(fetch);
        }
    }
    let header_path = SectionPath::Full(MessageSection::Header);
    let mut rows: Vec<_> = by_sequence
        .into_values()
        .filter_map(|responses| {
            let uid = responses.iter().find_map(|fetch| fetch.uid)?;
            let list_headers = responses
                .iter()
                .find_map(|fetch| fetch.section(&header_path))?;
            let seen = responses
                .iter()
                .rev()
                .find(|fetch| fetch.has_flags())
                .is_some_and(|fetch| fetch.flags().any(|flag| matches!(flag, Flag::Seen)));
            let internal_date = responses.iter().find_map(|fetch| fetch.internal_date());
            Some(MessageRow {
                uid,
                seen,
                internal_date: internal_date.map(|date| date.timestamp()),
                list_headers: list_headers.to_vec(),
                gmail: gmail_attributes(&responses),
            })
        })
        .collect();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.uid));
    rows
}

/// Gmail's attributes among one message's responses. A server that answered
/// without them leaves the row without them.
fn gmail_attributes(responses: &[&Fetch]) -> Option<GmailRow> {
    let message_id = *responses.iter().find_map(|fetch| fetch.gmail_msg_id())?;
    let labels = responses.iter().find_map(|fetch| fetch.gmail_labels())?;
    Some(GmailRow {
        message_id,
        labels: labels.iter().map(|label| label.to_string()).collect(),
    })
}

impl FetchResponses {
    pub(crate) fn failed(error: Error) -> Self {
        Self {
            fetches: Vec::new(),
            end: FetchEnd::Failed(error),
        }
    }
}

pub(crate) async fn collect_fetches(
    mut responses: impl Stream<Item = Result<Fetch, Error>> + Unpin,
) -> FetchResponses {
    let mut fetches = Vec::new();
    let end = loop {
        match responses.try_next().await {
            Ok(Some(fetch)) => fetches.push(fetch),
            Ok(None) => break FetchEnd::Completed,
            Err(Error::No(status)) => break FetchEnd::Rejected(ServerReply::from(&status)),
            Err(error) => break FetchEnd::Failed(error),
        }
    };
    FetchResponses { fetches, end }
}

/// Adds the structures among the responses. A flag change made meanwhile by
/// another client arrives as a response without a structure: it neither hides
/// the real one nor stands in for it, so a message that never gets a structure
/// stays unanswered and the command's completion decides what that means.
pub(crate) fn keep_structures(
    fetches: &[Fetch],
    uids: &[u32],
    structures: &mut BTreeMap<u32, Option<MessagePart>>,
) {
    for fetch in fetches {
        let Some(uid) = fetch.uid.filter(|uid| uids.contains(uid)) else {
            continue;
        };
        if let Some(structure) = fetch.bodystructure() {
            // The part tree's debug lines name the message through this span.
            let _message = tracing::debug_span!("message", uid).entered();
            structures.insert(uid, Some(MessagePart::from_body_structure(structure)));
        }
    }
}

/// After a NO completion, a message without a structure keeps its row with an
/// unreadable structure: the server failed to answer, it did not delete it.
pub(crate) fn keep_rows_without_structure(
    uids: &[u32],
    structures: &mut BTreeMap<u32, Option<MessagePart>>,
) {
    for &uid in uids {
        structures.entry(uid).or_insert_with(|| {
            tracing::debug!(uid, "structure could not be read: the server refused it");
            None
        });
    }
}

pub(crate) fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Header and body section of each requested part.
pub(crate) fn section_paths(parts: &TextParts) -> Vec<(SectionName, SectionName)> {
    match parts {
        TextParts::SinglePartBody => vec![(SectionName::MessageHeader, SectionName::Part(vec![1]))],
        TextParts::MultipartLeaves(leaves) => leaves
            .iter()
            .map(|leaf| {
                (
                    SectionName::PartHeader(leaf.clone()),
                    SectionName::Part(leaf.clone()),
                )
            })
            .collect(),
    }
}

/// A body section as FETCH names it.
pub(crate) enum SectionName {
    /// `HEADER`: the header of the whole message.
    MessageHeader,
    /// `2.1.MIME`: the MIME header of a part.
    PartHeader(Vec<u32>),
    /// `2.1`: the body of a part.
    Part(Vec<u32>),
}

impl SectionName {
    fn path(&self) -> SectionPath {
        match self {
            Self::MessageHeader => SectionPath::Full(MessageSection::Header),
            Self::PartHeader(part) => SectionPath::Part(part.clone(), Some(MessageSection::Mime)),
            Self::Part(part) => SectionPath::Part(part.clone(), None),
        }
    }
}

impl fmt::Display for SectionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let numbers = |part: &[u32]| {
            part.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(".")
        };
        match self {
            Self::MessageHeader => formatter.write_str("HEADER"),
            Self::PartHeader(part) => write!(formatter, "{}.MIME", numbers(part)),
            Self::Part(part) => formatter.write_str(&numbers(part)),
        }
    }
}

/// The requested parts of one message among the responses to its command.
pub(crate) fn message_text(
    fetches: &[Fetch],
    uid: u32,
    paths: &[(SectionName, SectionName)],
    rejected: bool,
) -> MessageText {
    let responses: Vec<_> = fetches
        .iter()
        .filter(|fetch| fetch.uid == Some(uid))
        .collect();
    if responses.is_empty() {
        // After OK a message without a response is gone; after NO the
        // server failed to return it.
        return match rejected {
            true => MessageText::NotReturned,
            false => MessageText::Disappeared,
        };
    }
    // Headers and bodies may arrive in separate responses, mixed with flag updates.
    paths
        .iter()
        .map(|(header, body)| {
            let header_path = header.path();
            let body_path = body.path();
            let header = responses
                .iter()
                .find_map(|fetch| fetch.section(&header_path))?;
            let body = responses
                .iter()
                .find_map(|fetch| fetch.section(&body_path))?;
            Some(ReceivedPart {
                header: header.to_vec(),
                body: body.to_vec(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .map_or(MessageText::NotReturned, MessageText::Received)
}
