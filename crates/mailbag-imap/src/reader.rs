// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    ImapAccount, ImapError, ImapFailure, ImapStep, MessagePart, MessageRow, MessageText,
    ReceivedPart, ServerReply, TextParts, TextRequest,
    session::{self, InboxSession, ServerNotices, StepFailure, command_failure},
    transport,
};
use async_imap::{
    error::{Error, ResponseTooLarge},
    imap_proto::{MessageSection, SectionPath},
    types::{Fetch, Flag},
};
use futures_util::{Stream, TryStreamExt};
use std::{collections::BTreeMap, fmt, io};

/// Seconds without progress after which connecting, TLS, a read or a write fails.
const SOCKET_TIMEOUT_SECONDS: u32 = 30;
/// A load reads at most this many of the newest Inbox messages.
const MESSAGE_WINDOW: u32 = 100;
const ROW_ITEMS: &str = "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO SUBJECT)])";
const STRUCTURE_ITEMS: &str = "(UID BODYSTRUCTURE)";

/// A signed-in, read-only session with an account's Inbox. It never changes
/// mail on the server. Dropping it closes the connection at once.
pub struct InboxReader {
    account: ImapAccount,
    socket_timeout_seconds: u32,
    pub(crate) inbox: InboxSession,
    notices: ServerNotices,
    needs_reconnect: bool,
}

/// The messages a FETCH command addresses.
enum MessageSet<'a> {
    /// Sequence numbers from the first to the last.
    Sequence(u32, u32),
    Uids(&'a [u32]),
}

/// The responses to one FETCH command and how the command ended.
struct FetchResponses {
    fetches: Vec<Fetch>,
    end: FetchEnd,
}

enum FetchEnd {
    Completed,
    /// The server completed with NO, after answering for some messages or none,
    /// for example when another client expunged a message meanwhile.
    Rejected(ServerReply),
    Failed(Error),
}

impl InboxReader {
    /// Connects securely, signs in and opens the Inbox read-only.
    pub async fn open(account: ImapAccount) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, SOCKET_TIMEOUT_SECONDS).await
    }

    /// Tests shorten the socket timeout to observe stalled servers quickly.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_with_short_socket_timeout(
        account: ImapAccount,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, socket_timeout_seconds).await
    }

    async fn open_with_socket_timeout(
        account: ImapAccount,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        let mut notices = ServerNotices::default();
        match session::open_inbox(&account, socket_timeout_seconds, &mut notices).await {
            Ok(inbox) => Ok(Self {
                account,
                socket_timeout_seconds,
                inbox,
                notices,
                needs_reconnect: false,
            }),
            Err(failure) => Err(notices.error(failure)),
        }
    }

    /// The Inbox version the read UIDs belong to.
    pub fn uid_validity(&self) -> Option<u32> {
        self.inbox.uid_validity
    }

    /// Reads the newest messages, at most 100, in descending UID order. A
    /// message the server did not answer for is left out.
    pub async fn fetch_rows(&mut self) -> Result<Vec<MessageRow>, ImapError> {
        let count = self.inbox.message_count;
        if count == 0 {
            return Ok(Vec::new());
        }
        let first = count.saturating_sub(MESSAGE_WINDOW - 1).max(1);
        let responses = self
            .fetch(MessageSet::Sequence(first, count), ROW_ITEMS)
            .await?;
        let rows = collect_rows(&responses.fetches, first, count);
        match responses.end {
            FetchEnd::Failed(error) => {
                Err(self.error(command_failure(ImapStep::FetchMessages, &error)))
            }
            FetchEnd::Rejected(server_reply) if rows.is_empty() => Err(self.error(StepFailure {
                failure: ImapFailure::Failed(ImapStep::FetchMessages),
                server_reply: Some(server_reply),
            })),
            // Messages deleted since EXAMINE are missing; that is not an empty Inbox.
            FetchEnd::Completed if rows.is_empty() => {
                Err(self.error(ImapFailure::InboxChanged.into()))
            }
            _ => Ok(rows),
        }
    }

    /// Reads part structures for the given UIDs. A structure that could not be
    /// read is `None`; a UID missing from the result has disappeared.
    pub async fn fetch_structures(
        &mut self,
        uids: &[u32],
    ) -> Result<BTreeMap<u32, Option<MessagePart>>, ImapError> {
        let mut structures = BTreeMap::new();
        if uids.is_empty() {
            return Ok(structures);
        }
        let responses = self.fetch(MessageSet::Uids(uids), STRUCTURE_ITEMS).await?;
        keep_structures(&responses.fetches, uids, &mut structures);
        match responses.end {
            FetchEnd::Completed => {}
            FetchEnd::Rejected(_) => keep_rows_without_structure(uids, &mut structures),
            FetchEnd::Failed(error) if is_parse_failure(&error) => {
                self.isolate_unreadable_structures(uids, &mut structures)
                    .await?
            }
            FetchEnd::Failed(error) => {
                return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
            }
        }
        if structures.is_empty() {
            return Err(self.error(ImapFailure::InboxChanged.into()));
        }
        Ok(structures)
    }

    /// One structure the parser rejects, for example nested deeper than its
    /// limit, fails the whole response and leaves the session unusable. Reads
    /// each remaining structure on its own, reconnecting after every failure.
    async fn isolate_unreadable_structures(
        &mut self,
        uids: &[u32],
        structures: &mut BTreeMap<u32, Option<MessagePart>>,
    ) -> Result<(), ImapError> {
        // A flag-only reply before the parse failure did not supply a structure.
        // Let the individual request determine its result, including disappearance.
        structures.retain(|_, structure| structure.is_some());
        let unread: Vec<u32> = uids
            .iter()
            .copied()
            .filter(|uid| !structures.contains_key(uid))
            .collect();
        for uid in unread {
            let responses = self
                .fetch(MessageSet::Uids(&[uid]), STRUCTURE_ITEMS)
                .await?;
            keep_structures(&responses.fetches, &[uid], structures);
            match responses.end {
                FetchEnd::Completed => {}
                FetchEnd::Rejected(_) => keep_rows_without_structure(&[uid], structures),
                FetchEnd::Failed(error) if is_parse_failure(&error) => {
                    structures.insert(uid, None);
                }
                FetchEnd::Failed(error) => {
                    return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
                }
            }
        }
        Ok(())
    }

    /// Replaces the session with a fresh one on the same Inbox.
    async fn reconnect(&mut self) -> Result<(), ImapError> {
        self.inbox.connection.close();
        let opened = session::open_inbox(
            &self.account,
            self.socket_timeout_seconds,
            &mut self.notices,
        )
        .await;
        let inbox = match opened {
            Ok(inbox) => inbox,
            Err(failure) => return Err(self.error(failure)),
        };
        // Another UIDVALIDITY means the UIDs now name other messages.
        if inbox.uid_validity != self.inbox.uid_validity {
            return Err(self.error(ImapFailure::InboxChanged.into()));
        }
        self.inbox = inbox;
        self.needs_reconnect = false;
        Ok(())
    }

    /// Reads text parts, one command per distinct request shape, and passes the
    /// result for each requested message to `on_message` before the next command.
    pub async fn fetch_text(
        &mut self,
        requests: Vec<TextRequest>,
        mut on_message: impl FnMut(u32, MessageText),
    ) -> Result<(), ImapError> {
        let mut uids_by_parts = BTreeMap::<TextParts, Vec<u32>>::new();
        for request in requests {
            uids_by_parts
                .entry(request.parts)
                .or_default()
                .push(request.uid);
        }
        for (parts, uids) in uids_by_parts {
            let paths = section_paths(&parts);
            let items = paths
                .iter()
                .map(|(header, body)| format!("BODY.PEEK[{header}] BODY.PEEK[{body}]"))
                .collect::<Vec<_>>()
                .join(" ");
            let responses = self
                .fetch(MessageSet::Uids(&uids), &format!("(UID {items})"))
                .await?;
            let rejected = match responses.end {
                FetchEnd::Completed => false,
                FetchEnd::Rejected(_) => true,
                FetchEnd::Failed(error) => {
                    return Err(self.error(command_failure(ImapStep::FetchText, &error)));
                }
            };
            for uid in uids {
                on_message(uid, message_text(&responses.fetches, uid, &paths, rejected));
            }
        }
        Ok(())
    }

    /// Runs one FETCH command and keeps every response received before it ended.
    async fn fetch(
        &mut self,
        messages: MessageSet<'_>,
        items: &str,
    ) -> Result<FetchResponses, ImapError> {
        if self.needs_reconnect {
            self.reconnect().await?;
        }
        let session = &mut self.inbox.session;
        let responses = match messages {
            MessageSet::Sequence(first, last) => {
                match session.fetch(format!("{first}:{last}"), items).await {
                    Ok(responses) => collect_fetches(responses).await,
                    Err(error) => FetchResponses::failed(error),
                }
            }
            MessageSet::Uids(uids) => match session.uid_fetch(uid_set(uids), items).await {
                Ok(responses) => collect_fetches(responses).await,
                Err(error) => FetchResponses::failed(error),
            },
        };
        self.collect_notices();
        if matches!(&responses.end, FetchEnd::Failed(error) if is_parse_failure(error)) {
            self.inbox.connection.close();
            self.needs_reconnect = true;
        }
        Ok(responses)
    }

    fn collect_notices(&mut self) {
        let responses = &self.inbox.session.unsolicited_responses;
        self.notices.collect(|| responses.try_recv().ok());
    }

    fn error(&mut self, failure: StepFailure) -> ImapError {
        self.collect_notices();
        self.notices.error(failure)
    }
}

/// A sequence-number FETCH can return each field separately. Sequence numbers
/// stay stable during this command; unsolicited updates outside its window do
/// not establish rows. UID FETCH uses UIDs instead because EXPUNGE is allowed.
fn collect_rows(fetches: &[Fetch], first: u32, last: u32) -> Vec<MessageRow> {
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
            })
        })
        .collect();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.uid));
    rows
}

impl FetchResponses {
    fn failed(error: Error) -> Self {
        Self {
            fetches: Vec::new(),
            end: FetchEnd::Failed(error),
        }
    }
}

async fn collect_fetches(
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
/// another client arrives as a response without a structure; it must not
/// hide the real one.
fn keep_structures(
    fetches: &[Fetch],
    uids: &[u32],
    structures: &mut BTreeMap<u32, Option<MessagePart>>,
) {
    for fetch in fetches {
        let Some(uid) = fetch.uid.filter(|uid| uids.contains(uid)) else {
            continue;
        };
        match fetch.bodystructure() {
            Some(structure) => {
                structures.insert(uid, Some(MessagePart::from_body_structure(structure)));
            }
            None => {
                structures.entry(uid).or_insert(None);
            }
        }
    }
}

/// After a NO completion, a message without a structure keeps its row with an
/// unreadable structure: the server failed to answer, it did not delete it.
fn keep_rows_without_structure(uids: &[u32], structures: &mut BTreeMap<u32, Option<MessagePart>>) {
    for &uid in uids {
        structures.entry(uid).or_insert(None);
    }
}

/// Whether async-imap could not parse a response. Network errors, timeouts,
/// a connection closed inside a literal and the buffer limit are not parse
/// failures; they end the load.
fn is_parse_failure(error: &Error) -> bool {
    let Error::Io(error) = error else {
        return false;
    };
    error.kind() == io::ErrorKind::Other
        && !transport::is_transport_error(error)
        && !error
            .get_ref()
            .is_some_and(|source| source.is::<ResponseTooLarge>())
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Header and body section of each requested part.
fn section_paths(parts: &TextParts) -> Vec<(SectionName, SectionName)> {
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
enum SectionName {
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
fn message_text(
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
