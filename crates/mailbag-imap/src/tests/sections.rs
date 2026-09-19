// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_success, open_reader, run};
use crate::{
    MessageText, ReceivedPart, TextParts, TextRequest,
    test_server::{FixtureMessage, FixtureSetup, ImapFixture},
};
use std::collections::BTreeMap;

fn leaves(sections: &[&[u32]]) -> TextParts {
    TextParts::MultipartLeaves(sections.iter().map(|section| section.to_vec()).collect())
}

fn single_part_requests(uids: &[u32]) -> Vec<TextRequest> {
    uids.iter()
        .map(|&uid| TextRequest {
            uid,
            parts: TextParts::SinglePartBody,
        })
        .collect()
}

fn read_text(
    fixture: &ImapFixture,
    requests: Vec<TextRequest>,
) -> Result<BTreeMap<u32, MessageText>, crate::ImapError> {
    let mut reader = open_reader(fixture);
    let mut results = BTreeMap::new();
    run(reader.fetch_text(requests, |uid, text| {
        assert!(results.insert(uid, text).is_none(), "UID {uid} twice");
    }))?;
    Ok(results)
}

fn received(parts: &[(&[u8], &str)]) -> MessageText {
    MessageText::Received(
        parts
            .iter()
            .map(|(header, body)| ReceivedPart {
                header: header.to_vec(),
                body: body.as_bytes().to_vec(),
            })
            .collect(),
    )
}

fn two_plain_messages() -> Vec<FixtureMessage> {
    vec![
        FixtureMessage::plain_text(10, "first"),
        FixtureMessage::plain_text(20, "second"),
    ]
}

#[test]
fn split_responses_supply_all_single_and_multipart_sections() {
    let messages = vec![
        FixtureMessage::plain_text(10, "first"),
        FixtureMessage::multipart(20, &[("plain", "a"), ("plain", "b")]),
    ];
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages.clone(),
        split_fetch_responses: true,
        reverse_order: true,
        interleave_flag_changes: true,
        ..FixtureSetup::default()
    });
    let results = expect_success(read_text(
        &fixture,
        vec![
            TextRequest {
                uid: 10,
                parts: TextParts::SinglePartBody,
            },
            TextRequest {
                uid: 20,
                parts: leaves(&[&[1], &[2]]),
            },
        ],
    ));
    assert_eq!(results[&10], received(&[(&messages[0].header, "first")]));
    assert_eq!(
        results[&20],
        received(&[
            (&messages[1].sections["1.MIME"], "a"),
            (&messages[1].sections["2.MIME"], "b"),
        ])
    );
}

#[test]
fn split_uid_fetch_responses_keep_their_messages_when_sequence_numbers_shift() {
    let messages = vec![
        FixtureMessage::plain_text(10, "expunged"),
        FixtureMessage::plain_text(20, "second"),
        FixtureMessage::plain_text(30, "third"),
    ];
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages.clone(),
        split_fetch_responses: true,
        expunge_during_text: true,
        ..FixtureSetup::default()
    });
    let results = expect_success(read_text(&fixture, single_part_requests(&[20, 30])));
    assert_eq!(results[&20], received(&[(&messages[1].header, "second")]));
    assert_eq!(results[&30], received(&[(&messages[2].header, "third")]));
}

#[test]
fn one_command_reads_each_request_shape_matched_by_uid_and_section() {
    let messages = vec![
        FixtureMessage::plain_text(10, "first"),
        FixtureMessage::plain_text(20, "second"),
        FixtureMessage::multipart(30, &[("plain", "leaf one"), ("html", "<p>html</p>")]),
        FixtureMessage::multipart(40, &[("plain", "a"), ("plain", "b")]),
        FixtureMessage::multipart(50, &[("plain", "c"), ("plain", "d")]),
    ];
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages.clone(),
        // Responses arrive in reverse order, after flag changes by another client.
        reverse_order: true,
        interleave_flag_changes: true,
        ..FixtureSetup::default()
    });
    let request = |uid, parts| TextRequest { uid, parts };
    let results = expect_success(read_text(
        &fixture,
        vec![
            request(10, TextParts::SinglePartBody),
            request(30, leaves(&[&[1]])),
            request(40, leaves(&[&[1], &[2]])),
            request(20, TextParts::SinglePartBody),
            request(50, leaves(&[&[1], &[2]])),
        ],
    ));

    let text_fetches: Vec<(String, Vec<String>)> = fixture
        .log()
        .fetches
        .into_iter()
        .map(|fetch| (fetch.message_set, fetch.items))
        .collect();
    let items = |items: &[&str]| {
        items
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
    };
    // A single-part body and the first leaf are both section 1, yet need
    // different headers, so they never share a command.
    assert_eq!(
        text_fetches,
        [
            (
                "10,20".to_owned(),
                items(&["UID", "BODY.PEEK[HEADER]", "BODY.PEEK[1]"])
            ),
            (
                "30".to_owned(),
                items(&["UID", "BODY.PEEK[1.MIME]", "BODY.PEEK[1]"])
            ),
            (
                "40,50".to_owned(),
                items(&[
                    "UID",
                    "BODY.PEEK[1.MIME]",
                    "BODY.PEEK[1]",
                    "BODY.PEEK[2.MIME]",
                    "BODY.PEEK[2]"
                ])
            ),
        ]
    );

    let mime_header =
        |message: &FixtureMessage, part: &str| message.sections[&format!("{part}.MIME")].clone();
    assert_eq!(results[&10], received(&[(&messages[0].header, "first")]));
    assert_eq!(results[&20], received(&[(&messages[1].header, "second")]));
    assert_eq!(
        results[&30],
        received(&[(&mime_header(&messages[2], "1"), "leaf one")])
    );
    assert_eq!(
        results[&50],
        received(&[
            (&mime_header(&messages[4], "1"), "c"),
            (&mime_header(&messages[4], "2"), "d"),
        ])
    );
}

#[test]
fn a_missing_or_nil_section_leaves_only_that_text_not_received() {
    for setup in [
        FixtureSetup {
            missing_body_uid: Some(20),
            ..FixtureSetup::default()
        },
        FixtureSetup {
            nil_body_uid: Some(20),
            ..FixtureSetup::default()
        },
    ] {
        let messages = two_plain_messages();
        let fixture = ImapFixture::start(FixtureSetup {
            messages: messages.clone(),
            ..setup
        });
        let results = expect_success(read_text(&fixture, single_part_requests(&[10, 20])));
        assert_eq!(results[&10], received(&[(&messages[0].header, "first")]));
        assert_eq!(results[&20], MessageText::NotReturned);
    }
}

#[test]
fn a_no_completion_keeps_the_text_received_before_it() {
    let messages = two_plain_messages();
    let fixture = ImapFixture::start(FixtureSetup {
        messages: messages.clone(),
        unfetchable_uids: vec![20],
        ..FixtureSetup::default()
    });
    let results = expect_success(read_text(&fixture, single_part_requests(&[10, 20])));
    assert_eq!(results[&10], received(&[(&messages[0].header, "first")]));
    assert_eq!(results[&20], MessageText::NotReturned);
}

#[test]
fn a_message_that_disappears_is_reported_as_gone() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: two_plain_messages(),
        vanishing_uid: Some(20),
        ..FixtureSetup::default()
    });
    let results = expect_success(read_text(&fixture, single_part_requests(&[10, 20])));
    assert!(matches!(results[&10], MessageText::Received(_)));
    assert_eq!(results[&20], MessageText::Disappeared);
}
