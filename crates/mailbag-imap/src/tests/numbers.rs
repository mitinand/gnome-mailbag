// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The numbers an opening returns, the opening with CONDSTORE, the listing
//! of changed flags and opening the mailbox again, for a cycle's state pass
//! (specs/009-synchronization FR-005, research §15).

use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    ImapFailure, MailboxNumbers, RowItems, StoreFlag,
    test_server::{FixtureSetup, ImapFixture},
};

/// Three messages, 10, 20 and 30, on a server that announces CONDSTORE.
fn condstore_fixture(setup: FixtureSetup) -> ImapFixture {
    ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        condstore: true,
        ..setup
    })
}

#[test]
fn the_opening_returns_the_numbers_and_asks_for_condstore_when_announced() {
    let fixture = condstore_fixture(FixtureSetup::default());
    let reader = open_reader(&fixture);
    assert_eq!(
        reader.numbers(),
        MailboxNumbers {
            uid_validity: Some(1),
            message_count: 3,
            uid_next: Some(31),
            highest_modseq: Some(3),
        }
    );
    assert!((fixture.log().commands.iter()).any(|command| command == "SELECT (CONDSTORE)"));
    // Without the capability the plain SELECT goes, and no mod-sequence comes.
    let plain = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        ..FixtureSetup::default()
    });
    let reader = open_reader(&plain);
    assert_eq!(reader.numbers().highest_modseq, None);
    assert_eq!(reader.numbers().uid_next, Some(31));
    let commands = plain.log().commands;
    assert!(commands.iter().any(|command| command == "SELECT"));
    assert!(
        !commands
            .iter()
            .any(|command| command == "SELECT (CONDSTORE)")
    );
    // A mailbox that keeps no mod-sequences answers NOMODSEQ: none either.
    let nomodseq = condstore_fixture(FixtureSetup {
        nomodseq: true,
        ..FixtureSetup::default()
    });
    assert_eq!(open_reader(&nomodseq).numbers().highest_modseq, None);
}

#[test]
fn only_the_messages_changed_since_a_mod_sequence_are_listed() {
    let fixture = condstore_fixture(FixtureSetup::default());
    let mut reader = open_reader(&fixture);
    let before = reader.numbers().highest_modseq.unwrap();
    expect_success(run(reader.store_flags(&[20], StoreFlag::Seen, true)));
    let changed = expect_success(run(reader.list_changed_flags(before, RowItems::Standard)));
    let listed: Vec<(u32, bool)> = (changed.messages.iter())
        .map(|message| (message.uid, message.seen))
        .collect();
    assert_eq!(listed, [(20, true)]);
    assert!(changed.refusal.is_none());
    let unchanged = expect_success(run(
        reader.list_changed_flags(before + 1, RowItems::Standard)
    ));
    assert!(unchanged.messages.is_empty());
    let every = expect_success(run(reader.list_messages(RowItems::Standard)));
    assert_eq!(every.messages.len(), 3);
    let fetches = fixture.log().fetches;
    assert_eq!(fetches[0].message_set, "1:*");
    assert!(fetches[0].items.contains(&format!("CHANGEDSINCE {before}")));
    assert!(
        !fetches[2]
            .items
            .iter()
            .any(|item| item.starts_with("CHANGEDSINCE"))
    );
}

#[test]
fn opening_again_gives_the_servers_numbers_now() {
    // At the second opening message 30 arrives, 20 leaves and 10 is read.
    let fixture = condstore_fixture(FixtureSetup {
        arriving_from_opening: vec![(2, 30)],
        gone_from_opening: vec![(2, 20)],
        seen_from_opening: vec![(2, 10)],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let first = reader.numbers();
    assert_eq!(
        (first.message_count, first.uid_next, first.highest_modseq),
        (2, Some(21), Some(3))
    );
    expect_success(run(reader.reopen()));
    let second = reader.numbers();
    assert_eq!(second.uid_validity, first.uid_validity);
    assert_eq!(
        (second.message_count, second.uid_next, second.highest_modseq),
        (2, Some(31), Some(5))
    );
    let changed = expect_success(run(reader.list_changed_flags(3, RowItems::Standard)));
    let listed: Vec<(u32, bool)> = (changed.messages.iter())
        .map(|message| (message.uid, message.seen))
        .collect();
    assert_eq!(listed, [(10, true), (30, false)]);
    let openings = (fixture.log().commands.iter())
        .filter(|command| command.starts_with("SELECT"))
        .count();
    assert_eq!(openings, 2);
    // Another numbering at the second opening means other messages: the
    // reader stops, as it does after a reconnection.
    let renumbered = condstore_fixture(FixtureSetup {
        uid_validity_from_second_opening: Some(2),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&renumbered);
    let error = expect_failure(run(reader.reopen()));
    assert_eq!(error.failure, ImapFailure::MailboxChanged);
}
