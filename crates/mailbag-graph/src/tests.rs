// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    GraphError, GraphFailure, GraphMessage, InboxPage, Mailbox, list_inbox_messages,
    list_inbox_messages_with_short_wait_limit,
    test_server::{
        ReceivedRequest, ScriptedAnswer, ScriptedService, TEST_ACCESS_TOKEN, fixture_immutable_id,
        fixture_received_unix,
    },
};
use std::{
    future::Future,
    time::{Duration, Instant},
};

#[path = "../../../tests/support/record.rs"]
mod test_record;
use test_record::CapturedRecord;

/// Runs a future on a fresh GLib context, as the mail worker does.
fn run<T>(future: impl Future<Output = T>) -> T {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .unwrap()
}

fn list_from(service_url: &str) -> Result<InboxPage, GraphError> {
    run(list_inbox_messages(service_url, TEST_ACCESS_TOKEN, 100))
}

fn failure_from(answer: ScriptedAnswer) -> GraphError {
    let service = ScriptedService::start(answer);
    list_from(service.url()).expect_err("the list unexpectedly succeeded")
}

#[test]
fn the_request_reaches_the_service_as_documented() {
    let service = ScriptedService::start(ScriptedAnswer::inbox(1));
    list_from(service.url()).expect("a page");
    assert_eq!(
        service.received_requests(),
        [ReceivedRequest {
            path: "/me/mailFolders/inbox/messages".to_owned(),
            query: "$top=100&$orderby=receivedDateTime%20desc\
                    &$select=id,subject,from,toRecipients,receivedDateTime,isRead,body"
                .to_owned(),
            authorization: Some(format!("Bearer {TEST_ACCESS_TOKEN}")),
            prefer: Some(r#"IdType="ImmutableId", outlook.body-content-type="text""#.to_owned()),
            accept: Some("application/json".to_owned()),
        }]
    );
}

#[test]
fn messages_arrive_with_their_fields_and_text() {
    let service = ScriptedService::start(ScriptedAnswer::inbox(3));
    let page = list_from(service.url()).expect("a page");
    let sender = |number| {
        Some(Mailbox {
            name: Some(format!("Sender {number}")),
            address: Some(format!("sender{number}@example.org")),
        })
    };
    let recipient = Mailbox {
        name: Some("Recipient".to_owned()),
        address: Some("recipient@example.org".to_owned()),
    };
    assert_eq!(
        page,
        InboxPage {
            messages: vec![
                GraphMessage {
                    immutable_id: fixture_immutable_id(1),
                    subject: Some("Subject 1".to_owned()),
                    from: sender(1),
                    to: vec![recipient.clone()],
                    received_unix: Some(fixture_received_unix(1)),
                    is_read: true,
                    body_text: Some("Text 1".to_owned()),
                },
                GraphMessage {
                    immutable_id: fixture_immutable_id(2),
                    subject: Some("Subject 2".to_owned()),
                    from: sender(2),
                    to: Vec::new(),
                    received_unix: Some(fixture_received_unix(2)),
                    is_read: false,
                    body_text: Some("Text 2".to_owned()),
                },
                GraphMessage {
                    immutable_id: fixture_immutable_id(3),
                    subject: Some("Subject 3".to_owned()),
                    from: sender(3),
                    to: vec![recipient],
                    received_unix: Some(fixture_received_unix(3)),
                    is_read: true,
                    body_text: None,
                },
            ],
            more_available: false,
        }
    );
}

#[test]
fn a_short_page_with_more_offered_is_marked_and_not_followed() {
    let service = ScriptedService::start(ScriptedAnswer::page_with_more(1));
    let page = list_from(service.url()).expect("a page");
    assert_eq!(page.messages.len(), 1);
    assert!(page.more_available);
    assert_eq!(service.received_requests().len(), 1);
}

#[test]
fn a_refused_sign_in_gives_the_status_and_code() {
    let error = failure_from(ScriptedAnswer::sign_in_refused());
    assert_eq!(
        error.failure,
        GraphFailure::Refused {
            status: 401,
            code: Some("InvalidAuthenticationToken".to_owned()),
        }
    );
    assert_eq!(error.reason.as_deref(), Some("Access token has expired."));
}

#[test]
fn throttling_gives_the_status_and_code() {
    let error = failure_from(ScriptedAnswer::throttled());
    assert_eq!(
        error.failure,
        GraphFailure::Refused {
            status: 429,
            code: Some("ApplicationThrottled".to_owned()),
        }
    );
}

#[test]
fn an_answer_without_the_documented_shape_is_invalid() {
    let error = failure_from(ScriptedAnswer {
        status: 200,
        body: br#"{"messages":[]}"#.to_vec(),
    });
    assert_eq!(error.failure, GraphFailure::InvalidReply);
}

#[test]
fn a_closed_port_fails_the_connection() {
    // Nothing listens on port 1 of the loopback address: binding a port below
    // 1024 needs privileges, so the connection is refused at once.
    let error = list_from("http://127.0.0.1:1").expect_err("no service listens");
    assert_eq!(error.failure, GraphFailure::ConnectionFailed);
    assert!(error.reason.is_some());
}

#[test]
fn a_silent_service_times_out_at_the_wait_limit() {
    let service = ScriptedService::stalled();
    let started = Instant::now();
    let error = run(list_inbox_messages_with_short_wait_limit(
        &service.url(),
        TEST_ACCESS_TOKEN,
        100,
        1,
    ))
    .expect_err("the service never answers");
    assert_eq!(error.failure, GraphFailure::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_access_token_never_reaches_the_record() {
    let listed = ScriptedService::start(ScriptedAnswer::inbox(1));
    let refused = ScriptedService::start(ScriptedAnswer::sign_in_refused());
    let record = CapturedRecord::start(tracing::Level::TRACE);
    list_from(listed.url()).expect("a page");
    list_from(refused.url()).expect_err("a refusal");
    let debug_lines = record.lines_at("DEBUG");
    for expected in [
        "request sent",
        "answer received",
        "InvalidAuthenticationToken",
    ] {
        assert!(
            debug_lines.iter().any(|line| line.contains(expected)),
            "{expected} is missing from {}",
            record.text()
        );
    }
    assert!(
        !record.text().contains(TEST_ACCESS_TOKEN),
        "{}",
        record.text()
    );
}

#[test]
fn dropping_the_listing_ends_its_request() {
    let service = ScriptedService::stalled();
    let service_url = service.url();
    run(async {
        let listing = list_inbox_messages(&service_url, TEST_ACCESS_TOKEN, 100);
        glib::future_with_timeout(Duration::from_millis(300), listing)
            .await
            .expect_err("the service never answers");
        // The worker's context keeps running after a load is dropped.
        glib::timeout_future(Duration::from_millis(100)).await;
    });
    let sent = service.read_first_connection();
    assert_eq!(
        sent.matches("GET /me/mailFolders/inbox/messages?").count(),
        1,
        "{sent}"
    );
}
