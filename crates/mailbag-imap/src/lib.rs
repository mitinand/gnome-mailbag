// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the Inbox of an IMAP account over a verified GIO TLS connection.
//!
//! This crate owns the protocol: the secure connection, sign-in, read-only
//! commands and the message part structure with IMAP section numbers. It has no
//! notion of a mail provider and never decodes message content. Its futures
//! must run on one thread with a running GLib main context.

mod part_tree;
mod reader;
mod session;
#[cfg(any(test, feature = "test-support"))]
pub mod test_server;
#[cfg(test)]
mod tests;
mod transport;

pub use part_tree::MessagePart;
pub use reader::InboxReader;

use std::fmt;

/// Where and how to sign in. It holds the password, so it has no Debug,
/// Display or Clone.
pub struct ImapAccount {
    /// Host as Online Accounts stores it, optionally with a port.
    pub host: String,
    pub login: String,
    pub password: String,
    pub encryption: Encryption,
}

/// How the connection is secured. There is no unencrypted option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encryption {
    /// TLS from the first byte, port 993 by default.
    ImplicitTls,
    /// STARTTLS before sign-in, port 143 by default.
    StartTls,
}

/// The step of a load that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImapStep {
    Connect,
    /// TLS, certificate verification or STARTTLS.
    SecureConnection,
    SignIn,
    OpenInbox,
    /// The message list or part structures.
    FetchMessages,
    FetchText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImapFailure {
    /// The step failed: the server refused, the connection broke or a reply
    /// could not be read.
    Failed(ImapStep),
    /// The server stopped responding during the step.
    TimedOut(ImapStep),
    /// The server offers neither AUTHENTICATE PLAIN nor LOGIN.
    NoSignInMethod,
    /// The Inbox was replaced, or all its selected messages disappeared.
    InboxChanged,
}

pub struct ImapError {
    pub failure: ImapFailure,
    /// The server's own reason for the failure, if it gave one.
    pub server_reply: Option<ServerReply>,
    /// ALERT texts the server sent during this attempt.
    pub alerts: Vec<String>,
}

/// Leaves the server's text out: it reaches the record only at debug, with the
/// sign-in name replaced, where the failure is built (specs/003-logging).
impl fmt::Debug for ImapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImapError")
            .field("failure", &self.failure)
            .field(
                "server_code",
                &self.server_reply.as_ref().map(|reply| &reply.code),
            )
            .field("alert_count", &self.alerts.len())
            .finish()
    }
}

/// What the server said about a failure: the text of its NO or BAD reply to
/// the failed command, or of the BYE with which it closed the connection.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerReply {
    /// The response code of a NO or BAD, such as `AUTHENTICATIONFAILED` or
    /// `UNAVAILABLE` from RFC 5530.
    pub code: Option<String>,
    /// Inert text for the failure explanation.
    pub text: String,
}

impl fmt::Debug for ServerReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerReply")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

/// List fields of one message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageRow {
    pub uid: u32,
    pub seen: bool,
    /// INTERNALDATE as seconds since the Unix epoch.
    pub internal_date: Option<i64>,
    /// The raw From, To and Subject header lines.
    pub list_headers: Vec<u8>,
}

/// The message list as one command delivered it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageList {
    /// Newest first.
    pub rows: Vec<MessageRow>,
    /// What the server said when it refused to finish the command, which
    /// means the list is missing messages it did not answer for. `None` when
    /// the command completed.
    pub refusal: Option<ServerReply>,
}

/// The text parts to read from one message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRequest {
    pub uid: u32,
    pub parts: TextParts,
}

/// Which sections to read. Messages with equal values share one command.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextParts {
    /// The body of a single-part message, with the message header.
    SinglePartBody,
    /// Parts of a multipart message by section numbers, each with its MIME header.
    MultipartLeaves(Vec<Vec<u32>>),
}

/// What the server returned for one message's requested text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageText {
    /// The requested parts, in request order.
    Received(Vec<ReceivedPart>),
    /// The server did not return the text: it sent NIL, left out a section or
    /// failed the command without answering for this message. The message
    /// keeps its row.
    NotReturned,
    /// The message is gone from the Inbox.
    Disappeared,
}

/// A received MIME entity: its header and its still-encoded body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedPart {
    pub header: Vec<u8>,
    pub body: Vec<u8>,
}
