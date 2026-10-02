// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lists the folders of a Microsoft 365 mailbox and reads the changes of one
//! of them, and its messages' texts, from Microsoft Graph.
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

/// How long the service may stay silent. Single pages of a delta reading took
/// up to 15 seconds (specs/009-synchronization/research.md §11); libsoup has
/// no limit by default.
const WAIT_LIMIT_SECONDS: u32 = 60;
/// The change-tracking listing, which gives the whole folder tree flat, page
/// by page (specs/008-folders/research.md §5).
const FOLDER_LISTING_PATH: &str = "/me/mailFolders/delta";
const FOLDER_FIELDS: &str = "id,displayName,parentFolderId,isHidden";
/// A message's list fields, which a delta entry carries in full for a listed
/// message and in part for a change. `bodyPreview` is the service's text
/// preview (specs/010-message-list/research.md §7).
pub(crate) const CHANGE_FIELDS: [&str; 6] = [
    "subject",
    "from",
    "toRecipients",
    "receivedDateTime",
    "isRead",
    "bodyPreview",
];
/// Identifiers that survive folder moves, and bodies rendered as text.
const PREFERENCES: &str = r#"IdType="ImmutableId", outlook.body-content-type="text""#;
/// A delta reading's pages hold at most 500 entries; the service caps them at
/// 512 whatever is asked (specs/009-synchronization/research.md §5).
const DELTA_PREFERENCES: &str = r#"IdType="ImmutableId", odata.maxpagesize=500"#;

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
    /// The beginning of the message's text as the service gives it.
    pub body_preview: Option<String>,
}

/// Leaves the received mail out of the record.
impl fmt::Debug for GraphMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphMessage")
            .field("immutable_id", &self.immutable_id)
            .field("is_read", &self.is_read)
            .finish_non_exhaustive()
    }
}

/// Messages' texts by identifier; `None` for a message the service gave no
/// text for.
pub type MessageTexts = Vec<(String, Option<String>)>;

/// Where a delta reading of a folder's messages starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangesFrom {
    /// A first reading of the folder with this id, newest first.
    FirstReading(String),
    /// A link the service gave: a next page, or the round after a completed
    /// reading.
    Link(String),
}

/// One page of a delta reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangePage {
    /// In the service's order; the same message may appear more than once.
    pub changes: Vec<MessageChange>,
    pub next: NextPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextPage {
    /// The reading goes on at this link.
    More(String),
    /// The reading is complete; the next round of changes starts at this link.
    Done(String),
}

/// One entry of a delta page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageChange {
    /// The message left the folder.
    Removed(String),
    /// The message with every list field: an arrival or a full update.
    Listed(GraphMessage),
    /// Only what changed: the read state when it did, and whether other list
    /// fields changed too (specs/009-synchronization/research.md §5).
    Changed {
        id: String,
        is_read: Option<bool>,
        other_fields: bool,
    },
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
    /// The service no longer accepts a saved delta link: status 410, or any
    /// other 4xx answering the link but the token's 401 and the throttling
    /// 429 (specs/009-synchronization/research.md §5).
    PositionRejected,
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

/// Reads one page of a delta reading of a folder's messages, the service's
/// changes since a saved link or a first reading newest first
/// (specs/009-synchronization/research.md §5). A link is followed as the
/// service gave it. A link the service no longer accepts fails with
/// `GraphFailure::PositionRejected`.
pub async fn read_message_changes(
    service_url: &str,
    access_token: &str,
    from: &ChangesFrom,
) -> Result<ChangePage, GraphError> {
    read_message_changes_within(service_url, access_token, from, WAIT_LIMIT_SECONDS).await
}

/// Tests shorten the wait limit to observe a silent service quickly.
#[cfg(any(test, feature = "test-support"))]
pub async fn read_message_changes_with_short_wait_limit(
    service_url: &str,
    access_token: &str,
    from: &ChangesFrom,
    wait_limit_seconds: u32,
) -> Result<ChangePage, GraphError> {
    read_message_changes_within(service_url, access_token, from, wait_limit_seconds).await
}

async fn read_message_changes_within(
    service_url: &str,
    access_token: &str,
    from: &ChangesFrom,
    wait_limit_seconds: u32,
) -> Result<ChangePage, GraphError> {
    let session = open_session(wait_limit_seconds);
    let address = match from {
        ChangesFrom::FirstReading(folder_id) => format!(
            "{service_url}/me/mailFolders/{}/messages/delta?$select={}\
             &$orderby=receivedDateTime%20desc",
            escaped(folder_id),
            CHANGE_FIELDS.join(",")
        ),
        ChangesFrom::Link(link) => link.clone(),
    };
    let request = build_request(&address, access_token)?;
    prefer(&request, DELTA_PREFERENCES);
    let answer = send(&session, &request).await?;
    check_status(&request, &answer).map_err(|error| rejected_position(error, from))?;
    let page = reply::read_change_page(&answer).map_err(|failure| failed(failure, None))?;
    tracing::debug!(
        bytes = answer.len(),
        changes = page.changes.len(),
        last_page = matches!(page.next, NextPage::Done(_)),
        "changes received"
    );
    Ok(page)
}

/// A refusal of a saved delta link, told apart from other refusals. The
/// service documents a 410 and "a 40X-series error with error codes such as
/// `syncStateNotFound`" for a token it no longer holds, so the code is not
/// relied on: on a link, any 4xx is the link's, except the token's 401 and
/// the throttling 429. A first reading has no link to reject.
fn rejected_position(error: GraphError, from: &ChangesFrom) -> GraphError {
    match &error.failure {
        GraphFailure::Refused { status, .. }
            if matches!(from, ChangesFrom::Link(_))
                && (400..500).contains(status)
                && !matches!(status, 401 | 429) =>
        {
            failed(GraphFailure::PositionRejected, error.reason)
        }
        _ => error,
    }
}

/// The texts of the folder's messages received from `from_unix` to
/// `to_unix`, both seconds included, by identifier: one request for a first
/// reading's page, whose messages lie between those dates
/// (specs/009-synchronization/research.md §5), paged by the service.
pub async fn read_texts_received_between(
    service_url: &str,
    access_token: &str,
    folder_id: &str,
    from_unix: i64,
    to_unix: i64,
) -> Result<MessageTexts, GraphError> {
    let session = open_session(WAIT_LIMIT_SECONDS);
    // The service keeps dates finer than the seconds it shows, so a message
    // shown at `to_unix` may lie after it: the range ends a second later.
    let filter = format!(
        "receivedDateTime%20ge%20{}%20and%20receivedDateTime%20lt%20{}",
        iso_8601(from_unix),
        iso_8601(to_unix + 1)
    );
    let mut address = format!(
        "{service_url}/me/mailFolders/{}/messages?$filter={filter}&$select=id,body&$top=500",
        escaped(folder_id)
    );
    let mut texts = Vec::new();
    loop {
        let request = build_request(&address, access_token)?;
        prefer(&request, PREFERENCES);
        let answer = send(&session, &request).await?;
        check_status(&request, &answer)?;
        let (page, next_link) =
            reply::read_text_page(&answer).map_err(|failure| failed(failure, None))?;
        texts.extend(page);
        match next_link {
            Some(next_link) => address = next_link,
            None => return Ok(texts),
        }
    }
}

/// One message's text as the service renders it; `None` when it has none or
/// is gone.
pub async fn read_message_text(
    service_url: &str,
    access_token: &str,
    message_id: &str,
) -> Result<Option<String>, GraphError> {
    let address = format!(
        "{service_url}/me/messages/{}?$select=body",
        escaped(message_id)
    );
    let Some(answer) = read_one(&address, access_token).await? else {
        return Ok(None);
    };
    reply::read_message_text(&answer).map_err(|failure| failed(failure, None))
}

/// One message with its list fields and the id of the folder it is in now;
/// `None` when it is gone.
pub async fn read_message(
    service_url: &str,
    access_token: &str,
    message_id: &str,
) -> Result<Option<(GraphMessage, String)>, GraphError> {
    let address = format!(
        "{service_url}/me/messages/{}?$select={},parentFolderId",
        escaped(message_id),
        CHANGE_FIELDS.join(",")
    );
    let Some(answer) = read_one(&address, access_token).await? else {
        return Ok(None);
    };
    reply::read_one_message(&answer)
        .map(Some)
        .map_err(|failure| failed(failure, None))
}

/// The answer about one message, or `None` for a 404.
async fn read_one(address: &str, access_token: &str) -> Result<Option<glib::Bytes>, GraphError> {
    let session = open_session(WAIT_LIMIT_SECONDS);
    let request = build_request(address, access_token)?;
    prefer(&request, PREFERENCES);
    let answer = send(&session, &request).await?;
    if request.status() == soup::Status::NotFound {
        return Ok(None);
    }
    check_status(&request, &answer)?;
    Ok(Some(answer))
}

fn prefer(request: &soup::Message, preferences: &str) {
    request
        .request_headers()
        .expect("a new request has headers")
        .append("Prefer", preferences);
}

fn escaped(path_part: &str) -> glib::GString {
    glib::Uri::escape_string(path_part, None, false)
}

/// A time as the service's filters write it, in UTC.
fn iso_8601(unix: i64) -> String {
    glib::DateTime::from_unix_utc(unix)
        .and_then(|time| time.format("%Y-%m-%dT%H:%M:%SZ"))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

/// A session for one load. It verifies the service's certificate against the
/// system's trust, as libsoup does by default.
fn open_session(wait_limit_seconds: u32) -> soup::Session {
    let session = soup::Session::new();
    session.set_timeout(wait_limit_seconds);
    session.set_user_agent(concat!("Mailbag/", env!("CARGO_PKG_VERSION")));
    session
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
