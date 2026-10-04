// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_success, fetch_all_rows, open_reader, plain_messages, run};
use crate::{
    Encryption, MessageText, TextParts, TextRequest,
    test_server::{FixtureMessage, FixtureSetup, ImapFixture},
};

/// Longer than one read of the stream, so the text crosses several
/// decompressed chunks.
const LONG_TEXT_LENGTH: usize = 200_000;

#[test]
fn compression_is_asked_for_when_announced_and_carries_commands_and_text() {
    for encryption in [Encryption::ImplicitTls, Encryption::StartTls] {
        let text = "x".repeat(LONG_TEXT_LENGTH);
        let fixture = ImapFixture::start(FixtureSetup {
            encryption,
            capabilities_after_sign_in: vec!["COMPRESS=DEFLATE"],
            messages: vec![FixtureMessage::plain_text(10, &text)],
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let rows = expect_success(run(fetch_all_rows(&mut reader))).rows;
        assert_eq!(rows.len(), 1, "{encryption:?}");
        let request = TextRequest {
            uid: 10,
            parts: TextParts::SinglePartBody,
            limit: None,
        };
        let mut received = false;
        expect_success(run(reader.fetch_text(
            vec![request],
            |_, _, message_text| {
                let MessageText::Received(parts) = message_text else {
                    panic!("the text must be received");
                };
                assert_eq!(parts[0].body, text.as_bytes());
                received = true;
            },
        )));
        assert!(received, "{encryption:?}");

        // Asked for once signed in, before the mailbox; every command from
        // the mailbox on travelled compressed both ways.
        let commands = fixture.log().commands;
        let compress = commands
            .iter()
            .position(|command| command == "COMPRESS")
            .unwrap_or_else(|| panic!("COMPRESS is sent: {commands:?}"));
        assert!(
            commands[..compress].contains(&"AUTHENTICATE".to_owned()),
            "{commands:?}"
        );
        assert!(commands[compress + 1..].contains(&"SELECT".to_owned()));
        assert!(commands[compress + 1..].contains(&"UID FETCH".to_owned()));
    }
}

#[test]
fn compression_is_not_asked_for_when_the_server_does_not_announce_it() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        ..FixtureSetup::default()
    });
    drop(open_reader(&fixture));
    let commands = fixture.log().commands;
    assert!(
        !commands.iter().any(|command| command == "COMPRESS"),
        "{commands:?}"
    );
}

#[test]
fn a_refused_compression_leaves_the_connection_as_it_is() {
    let fixture = ImapFixture::start(FixtureSetup {
        capabilities_after_sign_in: vec!["COMPRESS=DEFLATE"],
        compress_refused: true,
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let rows = expect_success(run(fetch_all_rows(&mut reader))).rows;
    assert_eq!(rows.len(), 2);
    let commands = fixture.log().commands;
    assert!(
        commands.iter().any(|command| command == "COMPRESS"),
        "{commands:?}"
    );
}
