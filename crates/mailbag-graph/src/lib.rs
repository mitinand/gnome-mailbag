// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the Inbox of a Microsoft 365 mailbox from Microsoft Graph.
//!
//! This crate owns the web request: its address, query and headers, the
//! answer's JSON and the service's refusals. It has no notion of Online
//! Accounts, a mail provider or message display. Its futures must run on one
//! thread with a running GLib main context, as the mail worker's.

mod reply;
#[cfg(any(test, feature = "test-support"))]
#[path = "../../../tests/support/service_thread.rs"]
mod service_thread;
#[cfg(any(test, feature = "test-support"))]
pub mod test_server;
#[cfg(test)]
mod tests;

use glib::translate::IntoGlib;
use soup::prelude::*;
use std::fmt;

/// How long the service may stay silent, as the IMAP wait limit of
/// specs/002-imap-integration. libsoup has no limit by default.
const WAIT_LIMIT_SECONDS: u32 = 30;
/// The Inbox by its well-known name, which does not depend on the mailbox's
/// language.
const INBOX_PATH: &str = "/me/mailFolders/inbox/messages";
const LISTED_FIELDS: &str = "id,subject,from,toRecipients,receivedDateTime,isRead,body";
/// Identifiers that survive folder moves, and bodies rendered as text.
const PREFERENCES: &str = r#"IdType="ImmutableId", outlook.body-content-type="text""#;

/// The newest Inbox messages, as one answer delivered them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxPage {
    /// Newest first.
    pub messages: Vec<GraphMessage>,
    /// The service offered a further page, which was not requested.
    pub more_available: bool,
}

/// One message of the answer. Fields the service left out, or sent in another
/// form, are `None` or empty.
#[derive(Clone, PartialEq, Eq)]
pub struct GraphMessage {
    /// The identifier that stays the same when the message moves between
    /// folders.
    pub immutable_id: String,
    pub subject: Option<String>,
    pub from: Option<Mailbox>,
    pub to: Vec<Mailbox>,
    /// When the message arrived, as seconds since the Unix epoch.
    pub received_unix: Option<i64>,
    pub is_read: bool,
    /// The body as the service rendered it into text.
    pub body_text: Option<String>,
}

/// Leaves the received mail out of the record.
impl fmt::Debug for GraphMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphMessage")
            .field("immutable_id", &self.immutable_id)
            .field("is_read", &self.is_read)
            .field("body_length", &self.body_text.as_ref().map(String::len))
            .finish_non_exhaustive()
    }
}

/// A sender or recipient as the service names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphFailure {
    /// No connection, a refused certificate or a broken transfer.
    ConnectionFailed,
    /// The service stopped responding within the wait limit.
    TimedOut,
    /// The service answered with a status other than 200.
    Refused {
        status: u32,
        /// The service's machine-readable error code, such as
        /// `InvalidAuthenticationToken`.
        code: Option<String>,
    },
    /// The answer is not the documented JSON.
    InvalidReply,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GraphError {
    pub failure: GraphFailure,
    /// The platform's text about a failed connection, or the service's
    /// developer message about a refusal, which the service documents as not
    /// meant for users (specs/005-microsoft-graph-integration/research.md §5).
    pub reason: Option<String>,
}

/// Leaves the reason out: it reaches the record only at debug, where the
/// failure is built (specs/003-logging).
impl fmt::Debug for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphError")
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

/// Asks the service at `service_url`, such as
/// `https://graph.microsoft.com/v1.0`, for the newest `batch_size` Inbox
/// messages with their text, in one request.
pub async fn list_inbox_messages(
    service_url: &str,
    access_token: &str,
    batch_size: u32,
) -> Result<InboxPage, GraphError> {
    list_inbox_messages_within(service_url, access_token, batch_size, WAIT_LIMIT_SECONDS).await
}

/// Tests shorten the wait limit to observe a silent service quickly.
#[cfg(any(test, feature = "test-support"))]
pub async fn list_inbox_messages_with_short_wait_limit(
    service_url: &str,
    access_token: &str,
    batch_size: u32,
    wait_limit_seconds: u32,
) -> Result<InboxPage, GraphError> {
    list_inbox_messages_within(service_url, access_token, batch_size, wait_limit_seconds).await
}

async fn list_inbox_messages_within(
    service_url: &str,
    access_token: &str,
    batch_size: u32,
    wait_limit_seconds: u32,
) -> Result<InboxPage, GraphError> {
    let session = open_session(wait_limit_seconds);
    let request = build_inbox_request(service_url, access_token, batch_size);
    let answer = send(&session, &request).await?;
    let status = check_status(&request, &answer)?;
    let page = reply::read_inbox_page(&answer).map_err(|failure| failed(failure, None))?;
    tracing::debug!(
        status,
        bytes = answer.len(),
        messages = page.messages.len(),
        more_available = page.more_available,
        "answer received"
    );
    Ok(page)
}

/// A session for one load. It verifies the service's certificate against the
/// system's trust, as libsoup does by default.
fn open_session(wait_limit_seconds: u32) -> soup::Session {
    let session = soup::Session::new();
    session.set_timeout(wait_limit_seconds);
    session.set_user_agent(concat!("Mailbag/", env!("CARGO_PKG_VERSION")));
    session
}

fn build_inbox_request(service_url: &str, access_token: &str, batch_size: u32) -> soup::Message {
    let address = format!("{service_url}{INBOX_PATH}?{}", inbox_query(batch_size));
    // The address is Microsoft Graph's constant or a test service's.
    let request = soup::Message::new("GET", &address).expect("the service address is a URL");
    let headers = request
        .request_headers()
        .expect("a new request has headers");
    headers.append("Authorization", &format!("Bearer {access_token}"));
    headers.append("Prefer", PREFERENCES);
    headers.append("Accept", "application/json");
    request
}

fn inbox_query(batch_size: u32) -> String {
    format!("$top={batch_size}&$orderby=receivedDateTime%20desc&$select={LISTED_FIELDS}")
}

async fn send(session: &soup::Session, request: &soup::Message) -> Result<glib::Bytes, GraphError> {
    let address = request.uri().expect("a request has an address");
    tracing::debug!(
        path = address.path().as_str(),
        query = address.query().as_deref(),
        "request sent"
    );
    session
        .send_and_read_future(request, glib::Priority::DEFAULT)
        .await
        .map_err(|error| {
            let failure = if error.matches(gio::IOErrorEnum::TimedOut) {
                GraphFailure::TimedOut
            } else {
                GraphFailure::ConnectionFailed
            };
            failed(failure, Some(error.message().to_owned()))
        })
}

/// Returns the status of a successful answer.
fn check_status(request: &soup::Message, answer: &[u8]) -> Result<u32, GraphError> {
    let status = request.status().into_glib() as u32;
    if status == 200 {
        return Ok(status);
    }
    let (code, message) = reply::read_error(answer).unwrap_or_default();
    Err(failed(GraphFailure::Refused { status, code }, message))
}

/// Every failure passes here, so its reason is logged here.
fn failed(failure: GraphFailure, reason: Option<String>) -> GraphError {
    tracing::debug!(?failure, reason = reason.as_deref(), "the request failed");
    GraphError { failure, reason }
}
