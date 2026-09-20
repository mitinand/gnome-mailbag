// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    ImapFailure, ImapStep, InboxReader, MessagePart, ServerReply, TextParts, TextRequest,
    test_server::{
        FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture, RecordedFetch,
    },
};

fn row_uids(fixture: &ImapFixture) -> Vec<u32> {
    let mut reader = open_reader(fixture);
    let rows = expect_success(run(reader.fetch_rows())).rows;
    rows.iter().map(|row| row.uid).collect()
}

#[test]
fn the_newest_hundred_messages_are_listed_by_descending_uid() {
    for count in [1, 100, 101] {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(count),
            ..FixtureSetup::default()
        });
        let expected: Vec<u32> = (1..=count)
            .rev()
            .take(100)
            .map(|number| number * 10)
            .collect();
        assert_eq!(row_uids(&fixture), expected, "{count} messages");
        let first = count.saturating_sub(99).max(1);
        assert_eq!(
            fixture.log().fetches,
            [RecordedFetch {
                connection: 1,
                by_uid: false,
                message_set: format!("{first}:{count}"),
                items: [
                    "UID",
                    "FLAGS",
                    "INTERNALDATE",
                    "BODY.PEEK[HEADER.FIELDS (FROM TO SUBJECT)]"
                ]
                .map(str::to_owned)
                .to_vec(),
            }]
        );
    }
}

#[test]
fn an_empty_inbox_has_no_rows() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    assert!(row_uids(&fixture).is_empty());
    assert!(fixture.log().fetches.is_empty());
}

#[test]
fn rows_keep_their_flags_date_and_headers_despite_flag_changes() {
    for (split_fetch_responses, reverse_order) in [(false, false), (true, false), (true, true)] {
        let mut messages = plain_messages(2);
        messages[0].seen = true;
        let fixture = ImapFixture::start(FixtureSetup {
            messages: messages.clone(),
            // Another client marks every message read during the load.
            interleave_flag_changes: true,
            split_fetch_responses,
            reverse_order,
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let rows = expect_success(run(reader.fetch_rows())).rows;
        assert_eq!(rows.len(), 2);
        for (row, message) in rows.iter().zip(messages.iter().rev()) {
            assert_eq!(row.uid, message.uid);
            assert_eq!(row.seen, message.seen);
            // 17 September 2026 10:00 at +03:00.
            assert_eq!(row.internal_date, Some(1_789_628_400));
            assert_eq!(row.list_headers, message.header);
        }
    }
}

#[test]
fn an_alert_from_a_successful_fetch_explains_a_later_failure() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        fetch_completion: Some("OK [ALERT] Maintenance tonight".to_owned()),
        fault: Some((FaultyCommand::Text, FaultKind::Close)),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    assert_eq!(expect_success(run(reader.fetch_rows())).rows.len(), 1);
    let error = expect_failure(run(reader.fetch_text(
        vec![TextRequest {
            uid: 10,
            parts: TextParts::SinglePartBody,
        }],
        |_, _| panic!("the text transfer must fail"),
    )));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchText));
    assert_eq!(error.alerts, ["Maintenance tonight"]);
}

#[test]
fn examine_alerts_explain_a_later_text_failure() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        examine_completion: "* OK [ALERT] Maintenance tonight\r\n\
                             {tag} OK [ALERT] Backup in progress\r\n"
            .to_owned(),
        fault: Some((FaultyCommand::Text, FaultKind::Close)),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    assert_eq!(expect_success(run(reader.fetch_rows())).rows.len(), 1);
    let error = expect_failure(run(reader.fetch_text(
        vec![TextRequest {
            uid: 10,
            parts: TextParts::SinglePartBody,
        }],
        |_, _| panic!("the text transfer must fail"),
    )));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchText));
    assert_eq!(error.alerts, ["Maintenance tonight", "Backup in progress"]);
}

#[test]
fn an_inbox_emptied_after_examine_is_not_reported_as_empty() {
    let fixture = ImapFixture::start(FixtureSetup::default());
    let mut reader = open_reader(&fixture);
    // EXAMINE counted three messages that another client deleted before FETCH.
    reader.inbox.message_count = 3;
    let error = expect_failure(run(reader.fetch_rows()));
    assert_eq!(error.failure, ImapFailure::InboxChanged);
}

#[test]
fn rows_received_before_a_no_completion_are_kept_with_the_refusal() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        // A damaged message the server cannot return.
        unfetchable_uids: vec![20],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let listed = expect_success(run(reader.fetch_rows()));
    assert_eq!(
        listed.rows.iter().map(|row| row.uid).collect::<Vec<_>>(),
        [30, 10]
    );
    // The missing row explains nothing by itself, so the reason comes along.
    assert_eq!(
        listed.refusal,
        Some(ServerReply {
            code: None,
            text: "Some messages could not be FETCHed".to_owned(),
        })
    );
}

#[test]
fn a_complete_list_carries_no_refusal() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    assert_eq!(expect_success(run(reader.fetch_rows())).refusal, None);
}

#[test]
fn a_no_completion_without_rows_fails_with_the_server_text() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        unfetchable_uids: vec![10, 20],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let error = expect_failure(run(reader.fetch_rows()));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchMessages));
    assert_eq!(
        error.server_reply,
        Some(ServerReply {
            code: None,
            text: "Some messages could not be FETCHed".to_owned(),
        })
    );
}

#[test]
fn a_structure_missing_after_a_no_completion_keeps_its_row() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        unfetchable_uids: vec![20],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let structures = expect_success(run(reader.fetch_structures(&[30, 20, 10])));
    assert_eq!(structures.keys().copied().collect::<Vec<_>>(), [10, 20, 30]);
    assert_eq!(structures[&20], None);
    assert!(structures[&10].is_some() && structures[&30].is_some());
}

#[test]
fn structures_are_read_for_the_listed_uids() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        interleave_flag_changes: true,
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let structures = expect_success(run(reader.fetch_structures(&[30, 20, 10])));
    assert_eq!(structures.keys().copied().collect::<Vec<_>>(), [10, 20, 30]);
    for structure in structures.values() {
        let part = structure.as_ref().expect("readable structure");
        assert_eq!(
            (part.section.as_slice(), part.media_type.as_str()),
            ([1].as_slice(), "text")
        );
    }
    let structure_fetch = &fixture.log().fetches[0];
    assert!(structure_fetch.by_uid);
    assert_eq!(structure_fetch.message_set, "30,20,10");
    assert_eq!(structure_fetch.items, ["UID", "BODYSTRUCTURE"]);
}

#[test]
fn a_message_that_disappears_during_the_load_is_left_out() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        vanishing_uid: Some(20),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let structures = expect_success(run(reader.fetch_structures(&[30, 20, 10])));
    assert_eq!(structures.keys().copied().collect::<Vec<_>>(), [10, 30]);
    // When every listed message disappears, the Inbox changed.
    let error = expect_failure(run(reader.fetch_structures(&[20])));
    assert_eq!(error.failure, ImapFailure::InboxChanged);
}

#[test]
fn the_structure_keeps_sections_types_parameters_and_dispositions() {
    let structure = "((\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\" \"NAME\" \"notes.txt\") NIL NIL \"7BIT\" 5 1 NIL \
         (\"ATTACHMENT\" (\"FILENAME\" \"notes.txt\")) NIL NIL)\
         ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 5 1 NIL NIL NIL NIL)\
         (\"TEXT\" \"HTML\" NIL NIL NIL \"7BIT\" 5 1 NIL NIL NIL NIL) \"ALTERNATIVE\" NIL NIL NIL NIL)\
         (\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 10 (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL) \
         (\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1) 1 NIL NIL NIL NIL) \"MIXED\" NIL NIL NIL NIL)";
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage {
            structure: structure.to_owned(),
            ..FixtureMessage::plain_text(10, "x")
        }],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let root = expect_success(run(reader.fetch_structures(&[10])))
        .remove(&10)
        .flatten()
        .expect("readable structure");
    let part = |section: Vec<u32>, media: &str, subtype: &str| MessagePart {
        section,
        media_type: media.to_owned(),
        media_subtype: subtype.to_owned(),
        parameters: Vec::new(),
        disposition: None,
        children: Vec::new(),
    };
    let expected = MessagePart {
        children: vec![
            MessagePart {
                parameters: vec![
                    ("charset".to_owned(), "UTF-8".to_owned()),
                    ("name".to_owned(), "notes.txt".to_owned()),
                ],
                disposition: Some("attachment".to_owned()),
                ..part(vec![1], "text", "plain")
            },
            MessagePart {
                children: vec![
                    part(vec![2, 1], "text", "plain"),
                    part(vec![2, 2], "text", "html"),
                ],
                ..part(vec![2], "multipart", "alternative")
            },
            // A nested message is not expanded.
            part(vec![3], "message", "rfc822"),
        ],
        ..part(Vec::new(), "multipart", "mixed")
    };
    assert_eq!(root, expected);
}

#[test]
fn loading_sends_only_read_only_commands() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    run(async {
        let rows = expect_success(reader.fetch_rows().await).rows;
        let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
        expect_success(reader.fetch_structures(&uids).await);
        let requests = uids
            .iter()
            .map(|&uid| TextRequest {
                uid,
                parts: TextParts::SinglePartBody,
            })
            .collect();
        expect_success(reader.fetch_text(requests, |_, _| {}).await);
    });
    let log = fixture.log();
    assert_eq!(
        log.commands,
        [
            "CAPABILITY",
            "AUTHENTICATE",
            "EXAMINE",
            "FETCH",
            "UID FETCH",
            "UID FETCH"
        ]
    );
    // BODY.PEEK leaves the \Seen flag unchanged.
    for item in log.fetches.iter().flat_map(|fetch| &fetch.items) {
        assert!(!item.starts_with("BODY["), "{item}");
    }
}

/// An untagged NO is a warning: the tagged completion decides.
#[test]
fn a_warning_before_the_inbox_completion_does_not_fail_it() {
    let fixture = ImapFixture::start(FixtureSetup {
        examine_completion: "* NO [ALERT] Mailbox is almost full\r\n\
                             {tag} OK [READ-ONLY] done\r\n"
            .to_owned(),
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    assert_eq!(expect_success(run(reader.fetch_rows())).rows.len(), 1);
}

/// A server that closes the connection while opening the Inbox says why.
#[test]
fn a_bye_while_opening_the_inbox_keeps_its_reason() {
    let fixture = ImapFixture::start(FixtureSetup {
        examine_completion: "* BYE Server is shutting down for maintenance\r\n".to_owned(),
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(InboxReader::open(fixture.account())));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::OpenInbox));
    assert_eq!(
        error.server_reply,
        Some(ServerReply {
            code: None,
            text: "Server is shutting down for maintenance".to_owned(),
        })
    );
}

/// System flag names are atoms, which a server may write in any case.
#[test]
fn the_read_flag_is_recognized_in_any_case() {
    let fixture = ImapFixture::start(FixtureSetup {
        lowercase_protocol_names: true,
        messages: vec![FixtureMessage {
            seen: true,
            ..FixtureMessage::plain_text(10, "Text")
        }],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = expect_success(run(reader.fetch_rows())).rows;
    assert!(rows[0].seen);
}
