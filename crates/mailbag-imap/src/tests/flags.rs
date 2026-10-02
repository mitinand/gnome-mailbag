// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one command that changes mail: `UID STORE` of the read state or the
//! star (specs/011-read-and-star FR-007).

use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    ImapFailure, ImapStep, MailboxReader, RowItems, StoreFlag,
    test_server::{FixtureSetup, ImapFixture, StoreFault, TEST_LOGIN},
};

/// Each listed message's UID, read state and star.
fn listed_flags(reader: &mut MailboxReader) -> Vec<(u32, bool, bool)> {
    let listing = expect_success(run(reader.list_messages(RowItems::Standard)));
    listing
        .messages
        .iter()
        .map(|message| (message.uid, message.seen, message.flagged))
        .collect()
}

#[test]
fn each_flag_and_direction_is_one_silent_command_on_a_uid_set() {
    // Gmail sends the new flags despite `.SILENT`; other servers do not.
    for store_echoes_fetch in [false, true] {
        let mut messages = plain_messages(3);
        messages[1].seen = true;
        messages[2].flagged = true;
        let fixture = ImapFixture::start(FixtureSetup {
            messages,
            store_echoes_fetch,
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let changes = [
            (&[10, 20][..], StoreFlag::Flagged, true),
            (&[20][..], StoreFlag::Seen, false),
            (&[10][..], StoreFlag::Seen, true),
            (&[30][..], StoreFlag::Flagged, false),
        ];
        for (uids, flag, set) in changes {
            let refusal = expect_success(run(reader.store_flags(uids, flag, set)));
            assert_eq!(refusal, None);
        }
        let stores: Vec<String> = (fixture.log().commands.into_iter())
            .filter(|command| command.starts_with("UID STORE"))
            .collect();
        assert_eq!(
            stores,
            [
                r"UID STORE 10,20 +FLAGS.SILENT (\Flagged)",
                r"UID STORE 20 -FLAGS.SILENT (\Seen)",
                r"UID STORE 10 +FLAGS.SILENT (\Seen)",
                r"UID STORE 30 -FLAGS.SILENT (\Flagged)",
            ]
        );
        assert_eq!(
            listed_flags(&mut reader),
            [(10, true, true), (20, false, true), (30, false, false)]
        );
    }
}

#[test]
fn a_no_or_a_bad_is_the_servers_refusal_with_the_sign_in_name_replaced() {
    let completions = [("NO [CANNOT]", "CANNOT"), ("BAD [CLIENTBUG]", "CLIENTBUG")];
    for (status, code) in completions {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(1),
            store_completion: Some(format!("{{tag}} {status} Not for {TEST_LOGIN}\r\n")),
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let refusal = expect_success(run(reader.store_flags(&[10], StoreFlag::Flagged, true)))
            .expect("the refusal");
        assert_eq!(refusal.code.as_deref(), Some(code));
        assert_eq!(refusal.text, "Not for <login>");
        assert_eq!(listed_flags(&mut reader), [(10, false, false)]);
    }
}

/// Whether the server applied the change is unknown to the client; the
/// scripted server's next connection shows it.
#[test]
fn a_connection_lost_before_the_completion_fails_the_command() {
    let faults = [
        (StoreFault::CloseAfterApplying, true),
        (StoreFault::CloseBeforeApplying, false),
    ];
    for (fault, applied) in faults {
        let fixture = ImapFixture::start(FixtureSetup {
            messages: plain_messages(1),
            store_fault: Some(fault),
            ..FixtureSetup::default()
        });
        let mut reader = open_reader(&fixture);
        let error = expect_failure(run(reader.store_flags(&[10], StoreFlag::Flagged, true)));
        assert_eq!(error.failure, ImapFailure::Failed(ImapStep::StoreFlags));
        let mut next_reader = open_reader(&fixture);
        assert_eq!(listed_flags(&mut next_reader), [(10, false, applied)]);
    }
}
