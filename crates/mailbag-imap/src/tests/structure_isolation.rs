// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_failure, expect_success, fetch_all_rows, open_reader, run, wait_until};
use crate::{
    ImapFailure, ImapStep, MailboxReader, MessageRow, MessageText, OpenOptions, RowItems,
    TextParts, TextRequest,
    test_server::{FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture},
};

/// Deeper than the 32 levels the IMAP parser accepts.
const UNREADABLE_DEPTH: usize = 40;

fn fetch_rows(reader: &mut MailboxReader, uids: &[u32]) -> Vec<MessageRow> {
    expect_success(run(reader.fetch_rows_by_uid(uids, RowItems::Standard))).rows
}

fn structure_is_read(rows: &[MessageRow], uid: u32) -> bool {
    let row = rows.iter().find(|row| row.uid == uid);
    row.unwrap_or_else(|| panic!("UID {uid} has a row"))
        .structure
        .is_some()
}

#[test]
fn text_is_received_after_the_last_isolated_structure_fails() {
    for interleave_flag_changes in [false, true] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: messages(3, &[20]),
            interleave_flag_changes,
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let rows = expect_success(run(fetch_all_rows(&mut reader))).rows;
        assert_eq!(rows.len(), 3);
        assert!(!structure_is_read(&rows, 20));
        assert!(structure_is_read(&rows, 10) && structure_is_read(&rows, 30));
        // UID 10 was read before the bulk failure; the rows of 30 and 20 are
        // read again, then their structures apart.
        assert_eq!(fixture.log().fetches.last().unwrap().message_set, "20");
        let connections = fixture.log().connections;
        wait_until(|| fixture.log().closed_connections == connections);

        let requests = [10, 30].map(|uid| TextRequest {
            uid,
            parts: TextParts::SinglePartBody,
            limit: None,
        });
        let mut received = Vec::new();
        expect_success(run(reader.fetch_text(requests.to_vec(), |uid, _, text| {
            let MessageText::Received(parts) = text else {
                panic!("UID {uid} must keep its text after isolation");
            };
            assert_eq!(parts[0].body, b"text");
            received.push(uid);
        })));
        assert_eq!(received, [10, 30]);
        assert_eq!(fixture.log().connections, connections + 1);
    }
}

#[test]
fn no_reconnection_is_needed_when_no_text_remains_after_isolation() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages(1, &[10]),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = fetch_rows(&mut reader, &[10]);
    assert!(!structure_is_read(&rows, 10));
    assert_eq!(fixture.log().connections, 2);
    wait_until(|| fixture.log().closed_connections == 2);
    expect_success(run(reader.fetch_text(Vec::new(), |_, _, _| {
        panic!("no text was requested");
    })));
    assert_eq!(fixture.log().connections, 2);
}

/// Messages with UIDs 10, 20 and so on; the listed ones cannot be parsed.
fn messages(count: u32, unreadable: &[u32]) -> Vec<FixtureMessage> {
    (1..=count)
        .map(|number| number * 10)
        .map(|uid| match unreadable.contains(&uid) {
            true => FixtureMessage::deeply_nested(uid, UNREADABLE_DEPTH),
            false => FixtureMessage::plain_text(uid, "text"),
        })
        .collect()
}

#[test]
fn unreadable_structures_keep_their_rows_and_the_others_are_read() {
    for unreadable in [vec![20], vec![20, 40], vec![10, 20, 30, 40, 50]] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: messages(5, &unreadable),
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let rows = expect_success(run(fetch_all_rows(&mut reader))).rows;
        // Rows never depend on structures.
        assert_eq!(rows.len(), 5);
        for row in &rows {
            assert_eq!(
                row.structure.is_none(),
                unreadable.contains(&row.uid),
                "UID {}",
                row.uid
            );
        }

        // After each parse failure the next command that asks for structures
        // runs on a fresh connection.
        let fetches: Vec<_> = fixture
            .log()
            .fetches
            .into_iter()
            .filter(|fetch| fetch.items.contains(&"BODYSTRUCTURE".to_owned()))
            .collect();
        for (fetch, next) in fetches.iter().zip(&fetches[1..]) {
            let failed = fetch.message_set.contains(',')
                || unreadable.contains(&fetch.message_set.parse().unwrap_or_default());
            if failed {
                assert_eq!(next.connection, fetch.connection + 1, "{unreadable:?}");
            } else {
                assert_eq!(next.connection, fetch.connection, "{unreadable:?}");
            }
        }
    }
}

/// A server may answer a message's row before its structure, in separate
/// responses: after the structure fails to parse, the row is not taken for
/// an answer, and the structure is asked for again on its own.
#[test]
fn a_row_answered_before_its_unreadable_structure_is_asked_for_again_apart() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages(3, &[20]),
        split_fetch_responses: true,
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = expect_success(run(fetch_all_rows(&mut reader))).rows;
    assert_eq!(rows.len(), 3);
    assert!(!structure_is_read(&rows, 20));
    assert!(structure_is_read(&rows, 10) && structure_is_read(&rows, 30));
    let structures_apart: Vec<String> = fixture
        .log()
        .fetches
        .into_iter()
        .filter(|fetch| fetch.items == ["UID", "BODYSTRUCTURE"])
        .map(|fetch| fetch.message_set)
        .collect();
    assert_eq!(structures_apart, ["30", "20"]);
}

#[test]
fn a_changed_uidvalidity_on_reconnection_stops_the_load() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages(2, &[10]),
        uid_validity_after_reconnect: Some(2),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let error = expect_failure(run(reader.fetch_rows_by_uid(&[20, 10], RowItems::Standard)));
    assert_eq!(error.failure, ImapFailure::MailboxChanged);
}

#[test]
fn network_failures_timeouts_and_the_response_limit_are_not_isolated() {
    for (fault, expected) in [
        (
            FaultKind::Close,
            ImapFailure::Failed(ImapStep::FetchMessages),
        ),
        (
            FaultKind::TruncatedLiteral,
            ImapFailure::Failed(ImapStep::FetchMessages),
        ),
        (
            FaultKind::HugeLiteral,
            ImapFailure::Failed(ImapStep::FetchMessages),
        ),
        (
            FaultKind::Stall,
            ImapFailure::TimedOut(ImapStep::FetchMessages),
        ),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: messages(2, &[]),
            fault: Some((FaultyCommand::Rows, fault)),
            ..FixtureSetup::default()
        });
        let error = expect_failure(run(async {
            let mut reader = expect_success(
                MailboxReader::open_with_short_socket_timeout(
                    fixture.account(),
                    OpenOptions::default(),
                    "INBOX",
                    1,
                )
                .await,
            );
            reader
                .fetch_rows_by_uid(&[20, 10], RowItems::Standard)
                .await
        }));
        assert_eq!(error.failure, expected, "{fault:?}");
        assert_eq!(fixture.log().connections, 1, "{fault:?} must not reconnect");
    }
}

#[test]
fn a_bye_during_the_load_ends_it_with_the_server_text() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages(2, &[]),
        fault: Some((FaultyCommand::Rows, FaultKind::Bye)),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let error = expect_failure(run(reader.fetch_rows_by_uid(&[20, 10], RowItems::Standard)));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchMessages));
    assert_eq!(
        error.server_reply.map(|reply| reply.text),
        Some("Server is restarting".to_owned())
    );
    assert_eq!(fixture.log().connections, 1);
}

#[test]
fn a_nil_encoding_is_read_on_the_same_connection() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::nil_encoding(10, "text")],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = fetch_rows(&mut reader, &[10]);
    assert!(structure_is_read(&rows, 10));
    let request = TextRequest {
        uid: 10,
        parts: TextParts::SinglePartBody,
        limit: None,
    };
    expect_success(run(reader.fetch_text(vec![request], |_, _, text| {
        let MessageText::Received(parts) = text else {
            panic!("the text must be received");
        };
        assert_eq!(parts[0].body, b"text");
    })));
    assert_eq!(fixture.log().connections, 1);
}

#[test]
fn an_extremely_deep_structure_leaves_the_process_running() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::deeply_nested(10, 10_000)],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = fetch_rows(&mut reader, &[10]);
    assert!(!structure_is_read(&rows, 10));
}
