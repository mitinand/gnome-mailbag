// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lists the folders of a Microsoft 365 mailbox and reads the messages of one
//! of them from Microsoft Graph.
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
/// The change-tracking listing, which gives the whole folder tree flat, page
/// by page (specs/008-folders/research.md §5).
const FOLDER_LISTING_PATH: &str = "/me/mailFolders/delta";
const FOLDER_FIELDS: &str = "id,displayName,parentFolderId,isHidden";
const LISTED_FIELDS: &str = "id,subject,from,toRecipients,receivedDateTime,isRead,body";
/// Identifiers that survive folder moves, and bodies rendered as text.
const PREFERENCES: &str = r#"IdType="ImmutableId", outlook.body-content-type="text""#;

/// A folder of the mailbox, as the listing names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphFolder {
    pub id: String,
    pub name: String,
    /// The parent's id. Top-level folders name the mailbox's root folder,
    /// which the listing leaves out.
    pub parent_id: Option<String>,
    /// The well-known name that resolves to this folder, if any.
    pub well_known: Option<WellKnownFolder>,
}

/// The folders Microsoft Graph finds by a well-known name in any language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WellKnownFolder {
    Inbox,
    Drafts,
    SentItems,
    DeletedItems,
    JunkEmail,
    Archive,
}

impl WellKnownFolder {
    const ALL: [Self; 6] = [
        Self::Inbox,
        Self::Drafts,
        Self::SentItems,
        Self::DeletedItems,
        Self::JunkEmail,
        Self::Archive,
    ];

    /// The name as a request writes it, such as `sentitems`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Drafts => "drafts",
            Self::SentItems => "sentitems",
            Self::DeletedItems => "deleteditems",
            Self::JunkEmail => "junkemail",
            Self::Archive => "archive",
        }
    }
}

/// The newest messages of a folder, as one answer delivered them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagePage {
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
/// `https://graph.microsoft.com/v1.0`, for every folder of the mailbox except
/// hidden ones: the change-tracking listing page by page, then the folder
/// behind each well-known name. A failed page or lookup fails the whole list,
/// because the caller replaces what it knew with it.
pub async fn list_folders(
    service_url: &str,
    access_token: &str,
) -> Result<Vec<GraphFolder>, GraphError> {
    let session = open_session(WAIT_LIMIT_SECONDS);
    let mut folders = read_folder_listing(&session, service_url, access_token).await?;
    for well_known in WellKnownFolder::ALL {
        let found = find_well_known_folder(&session, service_url, access_token, well_known).await?;
        let resolved = folders
            .iter_mut()
            .find(|folder| Some(&folder.id) == found.as_ref());
        if let Some(folder) = resolved {
            folder.well_known = Some(well_known);
        }
    }
    tracing::debug!(folders = folders.len(), "folder list received");
    Ok(folders)
}

/// Every page of the listing. The service may repeat a folder on a later
/// page or mark one as removed, and warns against assuming an order, so the
/// last entry for a folder decides whether it is listed.
async fn read_folder_listing(
    session: &soup::Session,
    service_url: &str,
    access_token: &str,
) -> Result<Vec<GraphFolder>, GraphError> {
    let mut folders: Vec<GraphFolder> = Vec::new();
    let mut address = format!("{service_url}{FOLDER_LISTING_PATH}?$select={FOLDER_FIELDS}");
    loop {
        let request = build_request(&address, access_token)?;
        let answer = send(session, &request).await?;
        check_status(&request, &answer)?;
        let page = reply::read_folder_page(&answer).map_err(|failure| failed(failure, None))?;
        for (folder_id, listed) in page.entries {
            folders.retain(|folder| folder.id != folder_id);
            folders.extend(listed);
        }
        match page.next_link {
            Some(next_link) => address = next_link,
            None => return Ok(folders),
        }
    }
}

/// The id of the folder a well-known name resolves to, or `None` when the
/// mailbox has no such folder, such as an archive that was never created.
async fn find_well_known_folder(
    session: &soup::Session,
    service_url: &str,
    access_token: &str,
    well_known: WellKnownFolder,
) -> Result<Option<String>, GraphError> {
    let address = format!(
        "{service_url}/me/mailFolders/{}?$select=id",
        well_known.name()
    );
    let request = build_request(&address, access_token)?;
    let answer = send(session, &request).await?;
    if request.status() == soup::Status::NotFound {
        return Ok(None);
    }
    check_status(&request, &answer)?;
    let folder_id = reply::read_folder_id(&answer).map_err(|failure| failed(failure, None))?;
    Ok(Some(folder_id))
}

/// Asks the service at `service_url` for the newest `batch_size` messages of
/// the folder `folder_id` with their text, in one request. A well-known name
/// such as `inbox` serves as the id.
pub async fn list_mailbox_messages(
    service_url: &str,
    access_token: &str,
    folder_id: &str,
    batch_size: u32,
) -> Result<MessagePage, GraphError> {
    list_mailbox_messages_within(
        service_url,
        access_token,
        folder_id,
        batch_size,
        WAIT_LIMIT_SECONDS,
    )
    .await
}

/// Tests shorten the wait limit to observe a silent service quickly.
#[cfg(any(test, feature = "test-support"))]
pub async fn list_mailbox_messages_with_short_wait_limit(
    service_url: &str,
    access_token: &str,
    folder_id: &str,
    batch_size: u32,
    wait_limit_seconds: u32,
) -> Result<MessagePage, GraphError> {
    list_mailbox_messages_within(
        service_url,
        access_token,
        folder_id,
        batch_size,
        wait_limit_seconds,
    )
    .await
}

async fn list_mailbox_messages_within(
    service_url: &str,
    access_token: &str,
    folder_id: &str,
    batch_size: u32,
    wait_limit_seconds: u32,
) -> Result<MessagePage, GraphError> {
    let session = open_session(wait_limit_seconds);
    let request = build_messages_request(service_url, access_token, folder_id, batch_size)?;
    let answer = send(&session, &request).await?;
    let status = check_status(&request, &answer)?;
    let page = reply::read_message_page(&answer).map_err(|failure| failed(failure, None))?;
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

fn build_messages_request(
    service_url: &str,
    access_token: &str,
    folder_id: &str,
    batch_size: u32,
) -> Result<soup::Message, GraphError> {
    let folder_id = glib::Uri::escape_string(folder_id, None, false);
    let address = format!(
        "{service_url}/me/mailFolders/{folder_id}/messages?$top={batch_size}\
         &$orderby=receivedDateTime%20desc&$select={LISTED_FIELDS}"
    );
    let request = build_request(&address, access_token)?;
    let headers = request
        .request_headers()
        .expect("a new request has headers");
    headers.append("Prefer", PREFERENCES);
    Ok(request)
}

/// A request with the token and the JSON answer asked for. Folder ids are the
/// same with or without immutable ids, so only message requests prefer them.
fn build_request(address: &str, access_token: &str) -> Result<soup::Message, GraphError> {
    // A next page's address comes from the service's answer.
    let request = soup::Message::new("GET", address)
        .map_err(|error| failed(GraphFailure::InvalidReply, Some(error.to_string())))?;
    let headers = request
        .request_headers()
        .expect("a new request has headers");
    headers.append("Authorization", &format!("Bearer {access_token}"));
    headers.append("Accept", "application/json");
    Ok(request)
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
