// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A scripted Microsoft Graph service for tests, on its own thread and GLib
//! context. It answers the Inbox list with one configured answer over plain
//! HTTP on loopback and records the path, query and headers of every request.
//! Its messages and addresses are synthetic.

use crate::{INBOX_PATH, service_thread::ServiceThread};
use soup::prelude::*;
use std::{
    io::Read,
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};

pub const TEST_ACCESS_TOKEN: &str = "synthetic-graph-access-token";

/// When message 1 of `ScriptedAnswer::inbox` arrived; each later-numbered
/// message arrived an hour earlier.
const NEWEST_RECEIVED_UNIX: i64 = 1_790_150_400;

/// The status and body the service answers the Inbox list with.
#[derive(Clone, Debug)]
pub struct ScriptedAnswer {
    pub status: u32,
    pub body: Vec<u8>,
}

impl ScriptedAnswer {
    /// `message_count` messages as the service documents them, newest first,
    /// with text bodies. Message 2 has no recipients and message 3 no body.
    pub fn inbox(message_count: u32) -> Self {
        Self::ok(serde_json::json!({ "value": inbox_messages(message_count) }))
    }

    /// `message_count` messages as `inbox` gives them, and a link to a
    /// further page.
    pub fn page_with_more(message_count: u32) -> Self {
        Self::ok(serde_json::json!({
            "value": inbox_messages(message_count),
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages?$skip=1",
        }))
    }

    /// The service's answer to a token it does not accept.
    pub fn sign_in_refused() -> Self {
        Self::error(
            401,
            "InvalidAuthenticationToken",
            "Access token has expired.",
        )
    }

    /// The service's answer when the shared allowance is used up.
    pub fn throttled() -> Self {
        Self::error(
            429,
            "ApplicationThrottled",
            "Application is over its request limit.",
        )
    }

    fn ok(answer: serde_json::Value) -> Self {
        Self {
            status: 200,
            body: answer.to_string().into_bytes(),
        }
    }

    fn error(status: u32, code: &str, message: &str) -> Self {
        let answer = serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "innerError": { "date": "2026-09-23T08:00:00", "request-id": "synthetic-request" },
            }
        });
        Self {
            status,
            body: answer.to_string().into_bytes(),
        }
    }
}

/// The immutable identifier of message `message_number` of `ScriptedAnswer::inbox`.
pub fn fixture_immutable_id(message_number: u32) -> String {
    format!("synthetic-immutable-id-{message_number}")
}

/// When message `message_number` of `ScriptedAnswer::inbox` arrived.
pub fn fixture_received_unix(message_number: u32) -> i64 {
    NEWEST_RECEIVED_UNIX - i64::from(message_number - 1) * 3600
}

fn inbox_messages(message_count: u32) -> Vec<serde_json::Value> {
    (1..=message_count).map(inbox_message).collect()
}

/// Odd-numbered messages are read.
fn inbox_message(number: u32) -> serde_json::Value {
    let received = glib::DateTime::from_unix_utc(fixture_received_unix(number))
        .and_then(|time| time.format_iso8601())
        .expect("a valid time");
    let recipients = if number == 2 {
        serde_json::json!([])
    } else {
        serde_json::json!([mailbox("Recipient", "recipient@example.org")])
    };
    let mut message = serde_json::json!({
        "id": fixture_immutable_id(number),
        "subject": format!("Subject {number}"),
        "from": mailbox(&format!("Sender {number}"), &format!("sender{number}@example.org")),
        "toRecipients": recipients,
        "receivedDateTime": received.as_str(),
        "isRead": number % 2 == 1,
        "body": { "contentType": "text", "content": format!("Text {number}") },
    });
    if number == 3 {
        message.as_object_mut().expect("an object").remove("body");
    }
    message
}

fn mailbox(name: &str, address: &str) -> serde_json::Value {
    serde_json::json!({ "emailAddress": { "name": name, "address": address } })
}

/// What the service saw of one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedRequest {
    pub path: String,
    pub query: String,
    pub authorization: Option<String>,
    pub prefer: Option<String>,
    pub accept: Option<String>,
}

pub struct ScriptedService {
    url: String,
    received: Arc<Mutex<Vec<ReceivedRequest>>>,
    /// Stopped when the service is dropped.
    _service: ServiceThread,
}

impl ScriptedService {
    /// Listens on a free loopback port. Any path other than the Inbox list
    /// gets 404.
    pub fn start(answer: ScriptedAnswer) -> Self {
        let received = Arc::new(Mutex::new(Vec::new()));
        let service_received = received.clone();
        let (service, port) = ServiceThread::start(move |_, _| {
            let server: soup::Server = glib::Object::builder().build();
            server.add_handler(None, move |_, request, path, _| {
                service_received
                    .lock()
                    .unwrap()
                    .push(received_request(request, path));
                if path == INBOX_PATH {
                    request.set_status(answer.status, None);
                    request.set_response(
                        Some("application/json"),
                        soup::MemoryUse::Copy,
                        &answer.body,
                    );
                } else {
                    request.set_status(404, None);
                }
            });
            server
                .listen_local(0, soup::ServerListenOptions::IPV4_ONLY)
                .expect("listen on loopback");
            let port = server.uris()[0].port();
            // The server lives as long as its thread's loop.
            Ok::<_, ()>((port, Box::new(move || drop(server)) as Box<dyn FnOnce()>))
        })
        .expect("the scripted service starts");
        Self {
            url: format!("http://127.0.0.1:{port}"),
            received,
            _service: service,
        }
    }

    /// The service address to list from, in place of Microsoft Graph's.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn received_requests(&self) -> Vec<ReceivedRequest> {
        self.received.lock().unwrap().clone()
    }

    /// A port that takes connections and never answers.
    pub fn stalled() -> StalledService {
        StalledService {
            listener: TcpListener::bind("127.0.0.1:0").expect("listen on loopback"),
        }
    }
}

fn received_request(request: &soup::ServerMessage, path: &str) -> ReceivedRequest {
    let headers = request.request_headers().expect("a request has headers");
    let header = |name| headers.one(name).map(|value| value.to_string());
    ReceivedRequest {
        path: path.to_owned(),
        query: request
            .uri()
            .and_then(|address| address.query())
            .map(|query| query.to_string())
            .unwrap_or_default(),
        authorization: header("Authorization"),
        prefer: header("Prefer"),
        accept: header("Accept"),
    }
}

/// The operating system completes the connection to this port, but nothing
/// reads or answers until a test asks.
pub struct StalledService {
    listener: TcpListener,
}

impl StalledService {
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}",
            self.listener.local_addr().unwrap().port()
        )
    }

    /// Takes the first connection and returns what the client sent before it
    /// closed the connection. Fails if the client keeps it open.
    pub fn read_first_connection(&self) -> String {
        let (mut connection, _) = self.listener.accept().unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut sent = String::new();
        connection
            .read_to_string(&mut sent)
            .expect("the client closes the connection");
        sent
    }
}
