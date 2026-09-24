// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    ImapAccount, ImapError, ImapFailure, ImapStep, MessageList, MessagePart, MessageText,
    OpenOptions, RowItems, TextParts, TextRequest,
    fetch_responses::{
        FetchEnd, FetchResponses, collect_fetches, collect_rows, keep_rows_without_structure,
        keep_structures, message_text, section_paths, uid_set,
    },
    session::{
        self, InboxSession, ServerNotices, StepFailure, command_failure, server_text_for_log,
    },
    transport,
};
use async_imap::error::{Error, ResponseTooLarge};
use std::{collections::BTreeMap, io};

/// Seconds without progress after which connecting, TLS, a read or a write fails.
const SOCKET_TIMEOUT_SECONDS: u32 = 30;
const ROW_ITEMS: &str = "UID FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO SUBJECT)]";
/// Gmail's message identifier and labels, added to the row FETCH on request.
const GMAIL_ROW_ITEMS: &str = "X-GM-MSGID X-GM-LABELS";
const STRUCTURE_ITEMS: &str = "(UID BODYSTRUCTURE)";

/// A signed-in, read-only session with an account's Inbox. It never changes
/// mail on the server. Dropping it closes the connection at once.
pub struct InboxReader {
    account: ImapAccount,
    /// Kept for the reconnection that an unreadable structure forces.
    options: OpenOptions,
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

impl InboxReader {
    /// Connects securely, signs in and opens the Inbox read-only.
    pub async fn open(account: ImapAccount, options: OpenOptions) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, options, SOCKET_TIMEOUT_SECONDS).await
    }

    /// Tests shorten the socket timeout to observe stalled servers quickly.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_with_short_socket_timeout(
        account: ImapAccount,
        options: OpenOptions,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, options, socket_timeout_seconds).await
    }

    async fn open_with_socket_timeout(
        account: ImapAccount,
        options: OpenOptions,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        let mut notices = ServerNotices::default();
        match session::open_inbox(&account, &options, socket_timeout_seconds, &mut notices).await {
            Ok(inbox) => Ok(Self {
                account,
                options,
                socket_timeout_seconds,
                inbox,
                notices,
                needs_reconnect: false,
            }),
            Err(failure) => Err(notices.error(&account.login, failure)),
        }
    }

    /// The Inbox version the read UIDs belong to.
    pub fn uid_validity(&self) -> Option<u32> {
        self.inbox.uid_validity
    }

    /// Reads the rows of the newest `batch_size` Inbox messages, at least one,
    /// in descending UID order. A message the server did not answer for is
    /// left out. A server that answers for some messages and then refuses the
    /// command leaves the list short; its reason travels with the rows,
    /// because a missing row explains nothing by itself.
    pub async fn fetch_rows(
        &mut self,
        row_items: RowItems,
        batch_size: u32,
    ) -> Result<MessageList, ImapError> {
        let count = self.inbox.message_count;
        if count == 0 {
            return Ok(MessageList {
                rows: Vec::new(),
                refusal: None,
            });
        }
        let first = count.saturating_sub(batch_size - 1).max(1);
        let items = match row_items {
            RowItems::Standard => format!("({ROW_ITEMS})"),
            RowItems::WithGmailAttributes => format!("({ROW_ITEMS} {GMAIL_ROW_ITEMS})"),
        };
        let responses = self
            .fetch(MessageSet::Sequence(first, count), &items)
            .await?;
        let rows = collect_rows(&responses.fetches, first, count);
        if !rows.is_empty() {
            tracing::info!(rows = rows.len(), "message list loaded");
        }
        match responses.end {
            FetchEnd::Failed(error) => {
                Err(self.error(command_failure(ImapStep::FetchMessages, &error)))
            }
            FetchEnd::Rejected(server_reply) if rows.is_empty() => Err(self.error(StepFailure {
                failure: ImapFailure::Failed(ImapStep::FetchMessages),
                server_reply: Some(server_reply),
            })),
            FetchEnd::Rejected(server_reply) => Ok(MessageList {
                rows,
                refusal: Some(server_reply),
            }),
            // Messages deleted since EXAMINE are missing; that is not an empty Inbox.
            FetchEnd::Completed if rows.is_empty() => {
                Err(self.error(ImapFailure::InboxChanged.into()))
            }
            FetchEnd::Completed => Ok(MessageList {
                rows,
                refusal: None,
            }),
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
        for uid in uids.iter().filter(|uid| !structures.contains_key(uid)) {
            tracing::debug!(uid, "message disappeared");
        }
        tracing::info!(messages = structures.len(), "part structures loaded");
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
        // Only answered structures are known at this point, so every other
        // requested message gets its own request, which also tells a message
        // that disappeared from one the parser cannot read.
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
                    tracing::debug!(
                        uid,
                        "structure could not be read: the description could not be parsed"
                    );
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
        tracing::info!("reconnecting after a structure that could not be read");
        self.inbox.connection.close();
        let opened = session::open_inbox(
            &self.account,
            &self.options,
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
        let messages = requests.len();
        let mut uids_by_parts = BTreeMap::<TextParts, Vec<u32>>::new();
        for request in requests {
            uids_by_parts
                .entry(request.parts)
                .or_default()
                .push(request.uid);
        }
        let commands = uids_by_parts.len();
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
            let sections = || {
                paths
                    .iter()
                    .map(|(_, body)| body.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let rejected = match responses.end {
                FetchEnd::Completed => false,
                FetchEnd::Rejected(_) => true,
                FetchEnd::Failed(error) => {
                    tracing::debug!(
                        sections = sections(),
                        uids = uid_set(&uids),
                        "text command failed"
                    );
                    return Err(self.error(command_failure(ImapStep::FetchText, &error)));
                }
            };
            for uid in uids {
                let text = message_text(&responses.fetches, uid, &paths, rejected);
                match text {
                    MessageText::NotReturned => {
                        tracing::debug!(uid, "text not returned");
                    }
                    MessageText::Disappeared => {
                        tracing::debug!(uid, "message disappeared");
                    }
                    MessageText::Received(_) => {}
                }
                on_message(uid, text);
            }
        }
        tracing::info!(messages, commands, "text loaded");
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
        self.notices.collect(&self.account.login);
        if let FetchEnd::Rejected(reply) = &responses.end {
            tracing::debug!(
                code = reply.code.as_deref(),
                server_text = server_text_for_log(&self.account.login, &reply.text),
                "the server refused the command"
            );
        }
        if matches!(&responses.end, FetchEnd::Failed(error) if is_parse_failure(error)) {
            self.inbox.connection.close();
            self.needs_reconnect = true;
        }
        Ok(responses)
    }

    fn error(&mut self, failure: StepFailure) -> ImapError {
        self.notices.error(&self.account.login, failure)
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
