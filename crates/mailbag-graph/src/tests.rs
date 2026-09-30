// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    ChangePage, ChangesFrom, GraphError, GraphFailure, GraphFolder, GraphMessage, Mailbox,
    MessageChange, NextPage, WellKnownFolder, list_folders, read_message, read_message_changes,
    read_message_changes_with_short_wait_limit, read_message_text, read_texts_received_between,
    test_server::{
        ReceivedRequest, ScriptedAnswer, ScriptedChanges, ScriptedFolders, ScriptedNext,
        ScriptedPage, ScriptedService, TEST_ACCESS_TOKEN, delta_entry, fixture_immutable_id,
        fixture_received_unix, folder_entry, stored_message,
    },
};
use std::{
    collections::BTreeMap,
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

/// A service whose delta reading has the given pages, by token.
fn changes_service(pages: Vec<(&str, ScriptedPage)>) -> ScriptedService {
    ScriptedService::start_with_changes(ScriptedChanges {
        pages: pages
            .into_iter()
            .map(|(token, page)| (token.to_owned(), page))
            .collect(),
        ..ScriptedChanges::default()
    })
}

/// A page of the given entries that completes the reading.
fn last_page(entries: Vec<serde_json::Value>) -> ScriptedPage {
    ScriptedPage::Entries {
        entries,
        next: ScriptedNext::Done("round-1"),
    }
}

fn changes_from(service_url: &str, from: &ChangesFrom) -> Result<ChangePage, GraphError> {
    run(read_message_changes(service_url, TEST_ACCESS_TOKEN, from))
}

fn first_reading(service_url: &str) -> Result<ChangePage, GraphError> {
    changes_from(service_url, &ChangesFrom::FirstReading("inbox".to_owned()))
}

fn failure_from(answer: ScriptedAnswer) -> GraphError {
    let service = changes_service(vec![("first", ScriptedPage::Refused(answer))]);
    first_reading(service.url()).expect_err("the reading unexpectedly succeeded")
}

#[test]
fn a_first_reading_reaches_the_service_as_documented() {
    let service = changes_service(vec![("first", last_page(Vec::new()))]);
    first_reading(service.url()).expect("a page");
    assert_eq!(
        service.received_requests(),
        [ReceivedRequest {
            path: "/me/mailFolders/inbox/messages/delta".to_owned(),
            query: "$select=subject,from,toRecipients,receivedDateTime,isRead,bodyPreview\
                    &$orderby=receivedDateTime%20desc"
                .to_owned(),
            authorization: Some(format!("Bearer {TEST_ACCESS_TOKEN}")),
            prefer: Some(r#"IdType="ImmutableId", odata.maxpagesize=500"#.to_owned()),
            accept: Some("application/json".to_owned()),
        }]
    );
}

#[test]
fn listed_messages_arrive_with_their_fields_and_the_next_link_is_followed() {
    let service = changes_service(vec![
        (
            "first",
            ScriptedPage::Entries {
                entries: vec![delta_entry(1), delta_entry(2)],
                next: ScriptedNext::More("page-2"),
            },
        ),
        ("page-2", last_page(vec![delta_entry(3)])),
    ]);
    let first = first_reading(service.url()).expect("a page");
    let sender = |number| {
        Some(Mailbox {
            name: Some(format!("Sender {number}")),
            address: Some(format!("sender{number}@example.org")),
        })
    };
    assert_eq!(
        first.changes[1],
        MessageChange::Listed(GraphMessage {
            immutable_id: fixture_immutable_id(2),
            subject: Some("Subject 2".to_owned()),
            from: sender(2),
            to: Vec::new(),
            received_unix: Some(fixture_received_unix(2)),
            is_read: false,
            body_preview: Some("Preview of\r\n\r\nmessage 2".to_owned()),
        })
    );
    let NextPage::More(next_link) = first.next else {
        panic!("a further page: {:?}", first.next);
    };
    let last = changes_from(service.url(), &ChangesFrom::Link(next_link)).expect("a page");
    assert_eq!(last.changes.len(), 1);
    assert!(matches!(last.next, NextPage::Done(link) if link.ends_with("$deltatoken=round-1")));
}

/// A link the service no longer accepts is told apart from other refusals:
/// the service documents a 410 and "a 40X-series error with error codes
/// such as syncStateNotFound", so any 4xx answering a link stands for the
/// link, except the token's 401 and the throttling 429; a first reading's
/// refusal is its own (research §5).
#[test]
fn a_rejected_position_is_its_own_failure() {
    let saved_link = |service: &ScriptedService| {
        format!(
            "{}/me/mailFolders/inbox/messages/delta?$deltatoken=old",
            service.url()
        )
    };
    let refused_link = |answer: ScriptedAnswer| {
        let service = changes_service(vec![("old", ScriptedPage::Refused(answer))]);
        changes_from(service.url(), &ChangesFrom::Link(saved_link(&service)))
            .expect_err("a refusal")
    };
    let service = changes_service(Vec::new());
    let gone =
        changes_from(service.url(), &ChangesFrom::Link(saved_link(&service))).expect_err("a 410");
    assert_eq!(gone.failure, GraphFailure::PositionRejected);
    let invalid = ScriptedAnswer {
        status: 400,
        body: br#"{"error":{"code":"ErrorInvalidSyncStateData","message":"Invalid."}}"#.to_vec(),
    };
    assert_eq!(
        refused_link(invalid.clone()).failure,
        GraphFailure::PositionRejected
    );
    assert!(matches!(
        refused_link(ScriptedAnswer::sign_in_refused()).failure,
        GraphFailure::Refused { status: 401, .. }
    ));
    assert!(matches!(
        refused_link(ScriptedAnswer::throttled()).failure,
        GraphFailure::Refused { status: 429, .. }
    ));
    // A first reading has no link to reject.
    assert!(matches!(
        failure_from(invalid).failure,
        GraphFailure::Refused { status: 400, .. }
    ));
}

#[test]
fn texts_come_by_their_date_range_or_one_by_one() {
    let service = ScriptedService::start_with_changes(ScriptedChanges {
        messages: vec![
            stored_message(1, "inbox"),
            stored_message(2, "inbox"),
            stored_message(3, "inbox"),
            stored_message(4, "archive"),
        ],
        ..ScriptedChanges::default()
    });
    let texts = run(read_texts_received_between(
        service.url(),
        TEST_ACCESS_TOKEN,
        "inbox",
        fixture_received_unix(3),
        fixture_received_unix(2),
    ))
    .expect("texts");
    // Message 3 has no body.
    assert_eq!(
        texts,
        [
            (fixture_immutable_id(2), Some("Text 2".to_owned())),
            (fixture_immutable_id(3), None),
        ]
    );
    let request = &service.received_requests()[0];
    assert!(request.query.contains("$top=500"), "{}", request.query);
    assert_eq!(
        request.prefer.as_deref(),
        Some(r#"IdType="ImmutableId", outlook.body-content-type="text""#)
    );
    let text = run(read_message_text(
        service.url(),
        TEST_ACCESS_TOKEN,
        &fixture_immutable_id(1),
    ));
    assert_eq!(text, Ok(Some("Text 1".to_owned())));
}

#[test]
fn one_message_comes_with_its_folder_and_a_missing_one_is_none() {
    let service = ScriptedService::start_with_changes(ScriptedChanges {
        messages: vec![stored_message(4, "archive")],
        ..ScriptedChanges::default()
    });
    let (message, folder) = run(read_message(
        service.url(),
        TEST_ACCESS_TOKEN,
        &fixture_immutable_id(4),
    ))
    .expect("an answer")
    .expect("the message");
    assert_eq!(message.immutable_id, fixture_immutable_id(4));
    assert_eq!(folder, "archive");
    let missing = run(read_message(service.url(), TEST_ACCESS_TOKEN, "gone"));
    assert_eq!(missing, Ok(None));
}

#[test]
fn a_token_refused_mid_reading_is_a_401_and_a_new_token_is_accepted() {
    let service = ScriptedService::start_with_changes(ScriptedChanges {
        pages: BTreeMap::from([("first".to_owned(), last_page(Vec::new()))]),
        token_accepted_requests: Some(1),
        ..ScriptedChanges::default()
    });
    first_reading(service.url()).expect("the first request");
    let error = first_reading(service.url()).expect_err("the token expired");
    assert!(matches!(
        error.failure,
        GraphFailure::Refused { status: 401, .. }
    ));
    run(read_message_changes(
        service.url(),
        "renewed-token",
        &ChangesFrom::FirstReading("inbox".to_owned()),
    ))
    .expect("a renewed token");
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
    let error = first_reading("http://127.0.0.1:1").expect_err("no service listens");
    assert_eq!(error.failure, GraphFailure::ConnectionFailed);
    assert!(error.reason.is_some());
}

#[test]
fn a_silent_service_times_out_at_the_wait_limit() {
    let service = ScriptedService::stalled();
    let started = Instant::now();
    let error = run(read_message_changes_with_short_wait_limit(
        &service.url(),
        TEST_ACCESS_TOKEN,
        &ChangesFrom::FirstReading("inbox".to_owned()),
        1,
    ))
    .expect_err("the service never answers");
    assert_eq!(error.failure, GraphFailure::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_access_token_never_reaches_the_record() {
    let read = changes_service(vec![("first", last_page(vec![delta_entry(1)]))]);
    let record = CapturedRecord::start(tracing::Level::TRACE);
    first_reading(read.url()).expect("a page");
    failure_from(ScriptedAnswer::sign_in_refused());
    let debug_lines = record.lines_at("DEBUG");
    for expected in [
        "request sent",
        "changes received",
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

/// Dropping a reading, as quitting drops a cycle, ends its request; the
/// folder is asked for by its escaped id.
#[test]
fn dropping_the_reading_ends_its_request() {
    let service = ScriptedService::stalled();
    let service_url = service.url();
    run(async {
        let from = ChangesFrom::FirstReading("AAMkAD=/folder".to_owned());
        let reading = read_message_changes(&service_url, TEST_ACCESS_TOKEN, &from);
        glib::future_with_timeout(Duration::from_millis(300), reading)
            .await
            .expect_err("the service never answers");
        // The worker's context keeps running after a load is dropped.
        glib::timeout_future(Duration::from_millis(100)).await;
    });
    let sent = service.read_first_connection();
    assert_eq!(
        sent.matches("GET /me/mailFolders/AAMkAD%3D%2Ffolder/messages/delta?")
            .count(),
        1,
        "{sent}"
    );
}

fn folders_from(
    folders: ScriptedFolders,
) -> (Result<Vec<GraphFolder>, GraphError>, ScriptedService) {
    let service = ScriptedService::start_with_folders(ScriptedAnswer::inbox(1), folders);
    (run(list_folders(service.url(), TEST_ACCESS_TOKEN)), service)
}

fn folder(
    id: &str,
    name: &str,
    parent_id: &str,
    well_known: Option<WellKnownFolder>,
) -> GraphFolder {
    GraphFolder {
        id: id.to_owned(),
        name: name.to_owned(),
        parent_id: Some(parent_id.to_owned()),
        well_known,
    }
}

#[test]
fn the_whole_tree_arrives_from_every_page_with_the_well_known_folders_marked() {
    let (listed, service) = folders_from(ScriptedFolders {
        pages: vec![
            Ok(vec![
                folder_entry("inbox-id", "Incoming", "root-id"),
                folder_entry("projects-id", "Projects", "root-id"),
            ]),
            Ok(vec![
                folder_entry("reports-id", "Reports", "projects-id"),
                folder_entry("sent-id", "Outgoing", "root-id"),
            ]),
        ],
        // No archive was ever created, so that name gives no folder.
        well_known: vec![
            ("inbox", ScriptedAnswer::folder_id("inbox-id")),
            ("sentitems", ScriptedAnswer::folder_id("sent-id")),
        ],
    });
    assert_eq!(
        listed.expect("a folder list"),
        [
            folder(
                "inbox-id",
                "Incoming",
                "root-id",
                Some(WellKnownFolder::Inbox)
            ),
            folder("projects-id", "Projects", "root-id", None),
            folder("reports-id", "Reports", "projects-id", None),
            folder(
                "sent-id",
                "Outgoing",
                "root-id",
                Some(WellKnownFolder::SentItems)
            ),
        ]
    );
    let requests = service.received_requests();
    let asked: Vec<(&str, &str)> = requests
        .iter()
        .map(|request| (request.path.as_str(), request.query.as_str()))
        .collect();
    assert_eq!(
        asked,
        [
            (
                "/me/mailFolders/delta",
                "$select=id,displayName,parentFolderId,isHidden"
            ),
            ("/me/mailFolders/delta", "$skiptoken=1"),
            ("/me/mailFolders/inbox", "$select=id"),
            ("/me/mailFolders/drafts", "$select=id"),
            ("/me/mailFolders/sentitems", "$select=id"),
            ("/me/mailFolders/deleteditems", "$select=id"),
            ("/me/mailFolders/junkemail", "$select=id"),
            ("/me/mailFolders/archive", "$select=id"),
        ]
    );
    assert!(
        requests.iter().all(|request| {
            request.authorization == Some(format!("Bearer {TEST_ACCESS_TOKEN}"))
                && request.prefer.is_none()
        }),
        "{requests:?}"
    );
}

#[test]
fn a_folder_repeated_on_a_later_page_counts_once_with_its_last_entry() {
    let (listed, _service) = folders_from(ScriptedFolders {
        pages: vec![
            Ok(vec![folder_entry("projects-id", "Projects", "root-id")]),
            Ok(vec![folder_entry(
                "projects-id",
                "Projects 2026",
                "root-id",
            )]),
        ],
        well_known: Vec::new(),
    });
    assert_eq!(
        listed.expect("a folder list"),
        [folder("projects-id", "Projects 2026", "root-id", None)]
    );
}

#[test]
fn removed_entries_and_hidden_folders_are_left_out() {
    let (listed, _service) = folders_from(ScriptedFolders {
        pages: vec![
            Ok(vec![
                folder_entry("projects-id", "Projects", "root-id"),
                folder_entry("old-id", "Old", "root-id"),
                serde_json::json!({
                    "id": "hidden-id",
                    "displayName": "Hidden",
                    "parentFolderId": "root-id",
                    "isHidden": true,
                }),
            ]),
            Ok(vec![
                serde_json::json!({ "id": "old-id", "@removed": { "reason": "deleted" } }),
            ]),
        ],
        well_known: Vec::new(),
    });
    assert_eq!(
        listed.expect("a folder list"),
        [folder("projects-id", "Projects", "root-id", None)]
    );
}

#[test]
fn a_failing_later_page_fails_the_whole_list() {
    let (listed, _service) = folders_from(ScriptedFolders {
        pages: vec![
            Ok(vec![folder_entry("projects-id", "Projects", "root-id")]),
            Err(ScriptedAnswer::throttled()),
        ],
        well_known: Vec::new(),
    });
    assert_eq!(
        listed.expect_err("a failed page").failure,
        GraphFailure::Refused {
            status: 429,
            code: Some("ApplicationThrottled".to_owned()),
        }
    );
}

#[test]
fn a_refused_well_known_name_fails_the_list_unlike_a_missing_folder() {
    let (listed, _service) = folders_from(ScriptedFolders {
        pages: vec![Ok(vec![folder_entry("inbox-id", "Inbox", "root-id")])],
        well_known: vec![("inbox", ScriptedAnswer::throttled())],
    });
    assert_eq!(
        listed.expect_err("a refused lookup").failure,
        GraphFailure::Refused {
            status: 429,
            code: Some("ApplicationThrottled".to_owned()),
        }
    );
}
