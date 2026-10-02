// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    FolderListing, ImapAccount, ImapError, ImapFailure, ImapStep, MessageList, MessageRow,
    MessageText, OpenOptions, RowItems, ServerReply, TextParts, TextRequest,
    fetch_responses::{
        FetchEnd, FetchResponses, collect_fetches, collect_rows, keep_listed, message_text,
        section_paths, structure_of, uid_set,
    },
    session::{
        self, MailboxSession, SOCKET_TIMEOUT_SECONDS, ServerNotices, StepFailure, command_failure,
        replace_sign_in_name,
    },
    transport,
};
use async_imap::error::{Error, ResponseTooLarge};
use futures_util::TryStreamExt;
use std::{cmp::Reverse, collections::BTreeMap, io};

const ROW_ITEMS: &str = "UID FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO SUBJECT)]";
/// Gmail's message identifier and labels, added to the rows on request.
const GMAIL_ROW_ITEMS: &str = "X-GM-MSGID X-GM-LABELS";
const LISTING_ITEMS: &str = "(UID FLAGS)";
/// Gmail's message identifier, added to the listing on request.
const GMAIL_LISTING_ITEMS: &str = "(UID FLAGS X-GM-MSGID)";
/// The command that reads one structure on its own (`fetch_structures_apart`).
const STRUCTURE_ITEMS: &str = "(UID BODYSTRUCTURE)";

/// A signed-in, read-only session with one mailbox of an account. It never
/// changes mail on the server. Dropping it closes the connection at once.
pub struct MailboxReader {
    account: ImapAccount,
    /// Kept for the reconnection that an unreadable structure forces.
    options: OpenOptions,
    socket_timeout_seconds: u32,
    /// The mailbox name as LIST gave it.
    mailbox_name: String,
    pub(crate) mailbox: MailboxSession,
    notices: ServerNotices,
    needs_reconnect: bool,
}

impl MailboxReader {
    /// Connects securely, signs in and opens the mailbox read-only. `mailbox`
    /// is the name as LIST gave it, or `INBOX`.
    pub async fn open(
        account: ImapAccount,
        options: OpenOptions,
        mailbox: &str,
    ) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, options, mailbox, SOCKET_TIMEOUT_SECONDS).await
    }

    /// Tests shorten the socket timeout to observe stalled servers quickly.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_with_short_socket_timeout(
        account: ImapAccount,
        options: OpenOptions,
        mailbox: &str,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        Self::open_with_socket_timeout(account, options, mailbox, socket_timeout_seconds).await
    }

    async fn open_with_socket_timeout(
        account: ImapAccount,
        options: OpenOptions,
        mailbox_name: &str,
        socket_timeout_seconds: u32,
    ) -> Result<Self, ImapError> {
        let mut notices = ServerNotices::default();
        let opened = session::open_mailbox(
            &account,
            &options,
            socket_timeout_seconds,
            &mut notices,
            mailbox_name,
        )
        .await;
        match opened {
            Ok(mailbox) => Ok(Self {
                account,
                options,
                socket_timeout_seconds,
                mailbox_name: mailbox_name.to_owned(),
                mailbox,
                notices,
                needs_reconnect: false,
            }),
            Err(failure) => Err(notices.error(&account.login, failure)),
        }
    }

    /// The mailbox version the read UIDs belong to.
    pub fn uid_validity(&self) -> Option<u32> {
        self.mailbox.uid_validity
    }

    /// Lists every message of the mailbox by UID with its read state, in
    /// one `UID FETCH 1:*` read as it arrives, so a large mailbox costs a
    /// few bytes per message. A NO or BAD leaves the listing incomplete with
    /// the server's reason, which then proves nothing about the messages it
    /// did not report; a lost connection fails. An empty mailbox is listed
    /// without a command, since servers answer `1:*` there differently.
    pub async fn list_messages(&mut self, row_items: RowItems) -> Result<FolderListing, ImapError> {
        let mut listed = BTreeMap::new();
        if self.mailbox.message_count == 0 {
            return Ok(FolderListing {
                messages: Vec::new(),
                refusal: None,
            });
        }
        if self.needs_reconnect {
            self.reconnect().await?;
        }
        let items = match row_items {
            RowItems::Standard => LISTING_ITEMS,
            RowItems::WithGmailAttributes => GMAIL_LISTING_ITEMS,
        };
        let end = match self.mailbox.session.uid_fetch("1:*", items).await {
            Ok(mut responses) => loop {
                match responses.try_next().await {
                    Ok(Some(fetch)) => keep_listed(&fetch, &mut listed),
                    Ok(None) => break FetchEnd::Completed,
                    Err(Error::No(status) | Error::Bad(status)) => {
                        break FetchEnd::Rejected(ServerReply::from(&status));
                    }
                    Err(error) => break FetchEnd::Failed(error),
                }
            },
            Err(error) => FetchEnd::Failed(error),
        };
        self.notices.collect(&self.account.login);
        let refusal = match end {
            FetchEnd::Completed => None,
            FetchEnd::Rejected(reply) => {
                let reply = self.refusal(reply);
                tracing::debug!(
                    code = reply.code.as_deref(),
                    server_text = reply.text,
                    "the server refused the command"
                );
                Some(reply)
            }
            FetchEnd::Failed(error) => {
                return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
            }
        };
        tracing::info!(messages = listed.len(), "mailbox listed");
        Ok(FolderListing {
            messages: listed.into_values().collect(),
            refusal,
        })
    }

    /// Reads the rows of the given messages with their part structures, in
    /// descending UID order. A message the server did not answer for is left
    /// out: it disappeared, or the server refused it, whose reason travels
    /// with the rows, because a missing row explains nothing by itself.
    ///
    /// One command carries rows and structures: a server spends about as
    /// much on a second command for the same messages as on the first
    /// (specs/009-synchronization/research.md §3). When it does not answer
    /// for every message, because the server refused some or the parser
    /// rejected one structure, which fails the whole answer and leaves the
    /// session unusable, the unanswered messages are read again apart.
    pub async fn fetch_rows_by_uid(
        &mut self,
        uids: &[u32],
        row_items: RowItems,
    ) -> Result<MessageList, ImapError> {
        if uids.is_empty() {
            return Ok(MessageList {
                rows: Vec::new(),
                refusal: None,
            });
        }
        let responses = self
            .fetch(uids, &row_command_items(row_items, true))
            .await?;
        let mut rows = collect_rows(&responses.fetches, uids);
        let refusal = match responses.end {
            FetchEnd::Completed => None,
            FetchEnd::Rejected(reply)
                if rows.len() == uids.len() && rows.iter().all(|row| row.structure.is_some()) =>
            {
                Some(self.refusal(reply))
            }
            FetchEnd::Rejected(_) => {
                self.read_unanswered_apart(uids, row_items, &mut rows)
                    .await?
            }
            FetchEnd::Failed(error) if is_parse_failure(&error) => {
                self.read_unanswered_apart(uids, row_items, &mut rows)
                    .await?
            }
            FetchEnd::Failed(error) => {
                return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
            }
        };
        tracing::info!(
            rows = rows.len(),
            without_structure = rows.iter().filter(|row| row.structure.is_none()).count(),
            "message rows loaded"
        );
        Ok(MessageList { rows, refusal })
    }

    /// Reads the messages the row command did not answer for, or answered
    /// without a structure, since a server may answer the rows of several
    /// messages before their structures (RFC 2683 §3.4.4): their rows in
    /// one command, then each structure on its own, so that one message the
    /// server refuses to describe for good, or whose description the parser
    /// cannot read, keeps its row without a structure. A message gone
    /// between the two commands disappeared. Returns the refusal of its own
    /// row command, which then names messages the server withheld again; the
    /// first command's refusal is dropped, since what it withheld has been
    /// asked for again.
    async fn read_unanswered_apart(
        &mut self,
        uids: &[u32],
        row_items: RowItems,
        rows: &mut Vec<MessageRow>,
    ) -> Result<Option<ServerReply>, ImapError> {
        rows.retain(|row| row.structure.is_some());
        let unanswered: Vec<u32> = uids
            .iter()
            .copied()
            .filter(|uid| !rows.iter().any(|row| row.uid == *uid))
            .collect();
        if unanswered.is_empty() {
            return Ok(None);
        }
        let responses = self
            .fetch(&unanswered, &row_command_items(row_items, false))
            .await?;
        let mut answered = collect_rows(&responses.fetches, &unanswered);
        let refusal = match responses.end {
            FetchEnd::Completed => None,
            FetchEnd::Rejected(reply) => Some(self.refusal(reply)),
            FetchEnd::Failed(error) => {
                return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
            }
        };
        self.fetch_structures_apart(&mut answered).await?;
        rows.append(&mut answered);
        rows.sort_by_key(|row| Reverse(row.uid));
        Ok(refusal)
    }

    /// Reads each row's structure on its own, reconnecting after every
    /// structure the parser rejects, for example one nested deeper than its
    /// limit, since that fails the whole response and leaves the session
    /// unusable. A row whose structure could not be read keeps none; a row
    /// the server no longer answers for is dropped, its message disappeared.
    /// A refusal the server marks temporary (RFC 5530 `UNAVAILABLE`) fails
    /// the read instead, so that nothing is kept as unreadable for a passing
    /// condition.
    async fn fetch_structures_apart(
        &mut self,
        rows: &mut Vec<MessageRow>,
    ) -> Result<(), ImapError> {
        for mut row in std::mem::take(rows) {
            let uid = row.uid;
            let responses = self.fetch(&[uid], STRUCTURE_ITEMS).await?;
            row.structure = structure_of(uid, &responses.fetches);
            match responses.end {
                FetchEnd::Completed if row.structure.is_none() => {
                    tracing::debug!(uid, "message disappeared");
                    continue;
                }
                FetchEnd::Completed => {}
                FetchEnd::Rejected(reply) if is_temporary(&reply) => {
                    return Err(self.error(refused(ImapStep::FetchMessages, reply)));
                }
                FetchEnd::Rejected(_) => {
                    tracing::debug!(uid, "structure could not be read: the server refused it");
                }
                FetchEnd::Failed(error) if is_parse_failure(&error) => {
                    tracing::debug!(
                        uid,
                        "structure could not be read: the description could not be parsed"
                    );
                }
                FetchEnd::Failed(error) => {
                    return Err(self.error(command_failure(ImapStep::FetchMessages, &error)));
                }
            }
            rows.push(row);
        }
        Ok(())
    }

    /// The server's refusal as the caller keeps it, with the sign-in name
    /// replaced; the failing branches replace it in `error`.
    fn refusal(&self, reply: ServerReply) -> ServerReply {
        ServerReply {
            text: replace_sign_in_name(&self.account.login, &reply.text),
            ..reply
        }
    }

    /// Replaces the session with a fresh one on the same mailbox.
    async fn reconnect(&mut self) -> Result<(), ImapError> {
        tracing::info!("reconnecting after a structure that could not be read");
        self.mailbox.connection.close();
        let opened = session::open_mailbox(
            &self.account,
            &self.options,
            self.socket_timeout_seconds,
            &mut self.notices,
            &self.mailbox_name,
        )
        .await;
        let mailbox = match opened {
            Ok(mailbox) => mailbox,
            Err(failure) => return Err(self.error(failure)),
        };
        // Another UIDVALIDITY means the UIDs now name other messages.
        if mailbox.uid_validity != self.mailbox.uid_validity {
            return Err(self.error(ImapFailure::MailboxChanged.into()));
        }
        self.mailbox = mailbox;
        self.needs_reconnect = false;
        Ok(())
    }

    /// Reads text parts, one command per distinct request shape (its parts and
    /// limit), and passes the result for each requested message, with the
    /// request's limit, to `on_message` before the next command.
    /// A refusal the server marks temporary (RFC 5530 `UNAVAILABLE`) fails the
    /// read, as for the structures.
    pub async fn fetch_text(
        &mut self,
        requests: Vec<TextRequest>,
        mut on_message: impl FnMut(u32, Option<u32>, MessageText),
    ) -> Result<(), ImapError> {
        let messages = requests.len();
        let mut uids_by_shape = BTreeMap::<(TextParts, Option<u32>), Vec<u32>>::new();
        for request in requests {
            uids_by_shape
                .entry((request.parts, request.limit))
                .or_default()
                .push(request.uid);
        }
        let commands = uids_by_shape.len();
        for ((parts, limit), uids) in uids_by_shape {
            let paths = section_paths(&parts);
            let partial = limit.map_or(String::new(), |limit| format!("<0.{limit}>"));
            let items = paths
                .iter()
                .map(|(header, body)| format!("BODY.PEEK[{header}] BODY.PEEK[{body}]{partial}"))
                .collect::<Vec<_>>()
                .join(" ");
            let responses = self.fetch(&uids, &format!("(UID {items})")).await?;
            let sections = || {
                paths
                    .iter()
                    .map(|(_, body)| body.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let rejected = match responses.end {
                FetchEnd::Completed => false,
                FetchEnd::Rejected(reply) if is_temporary(&reply) => {
                    return Err(self.error(refused(ImapStep::FetchText, reply)));
                }
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
                on_message(uid, limit, text);
            }
        }
        tracing::info!(messages, commands, "text loaded");
        Ok(())
    }

    /// Runs one UID FETCH command and keeps every response received before
    /// it ended.
    async fn fetch(&mut self, uids: &[u32], items: &str) -> Result<FetchResponses, ImapError> {
        if self.needs_reconnect {
            self.reconnect().await?;
        }
        let session = &mut self.mailbox.session;
        let responses = match session.uid_fetch(uid_set(uids), items).await {
            Ok(responses) => collect_fetches(responses).await,
            Err(error) => FetchResponses::failed(error),
        };
        self.notices.collect(&self.account.login);
        if let FetchEnd::Rejected(reply) = &responses.end {
            tracing::debug!(
                code = reply.code.as_deref(),
                server_text = replace_sign_in_name(&self.account.login, &reply.text),
                "the server refused the command"
            );
        }
        if matches!(&responses.end, FetchEnd::Failed(error) if is_parse_failure(error)) {
            self.mailbox.connection.close();
            self.needs_reconnect = true;
        }
        Ok(responses)
    }

    fn error(&mut self, failure: StepFailure) -> ImapError {
        self.notices.error(&self.account.login, failure)
    }
}

/// The items of a row command: the list fields, Gmail's fields on request
/// and the part structures, unless they are read apart.
fn row_command_items(row_items: RowItems, with_structures: bool) -> String {
    let mut items = vec![ROW_ITEMS];
    if row_items == RowItems::WithGmailAttributes {
        items.push(GMAIL_ROW_ITEMS);
    }
    if with_structures {
        items.push("BODYSTRUCTURE");
    }
    format!("({})", items.join(" "))
}

/// Whether the server marked its refusal temporary (RFC 5530 `UNAVAILABLE`):
/// what it withheld is asked for again by a later cycle rather than kept as
/// unreadable.
fn is_temporary(reply: &ServerReply) -> bool {
    reply
        .code
        .as_deref()
        .is_some_and(|code| code.eq_ignore_ascii_case("UNAVAILABLE"))
}

/// The failure of a command the server refused with `reply`.
fn refused(step: ImapStep, reply: ServerReply) -> StepFailure {
    StepFailure {
        failure: ImapFailure::Failed(step),
        server_reply: Some(reply),
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
