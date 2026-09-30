// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The listing a cycle proves removals with: complete only when the server
//! finished it (specs/009-synchronization/research.md §2).

use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    ImapFailure, ImapStep, ListedUid, RowItems,
    test_server::{FaultKind, FaultyCommand, FixtureSetup, ImapFixture},
};

fn listed_uids(listed: &[ListedUid]) -> Vec<u32> {
    listed.iter().map(|message| message.uid).collect()
}

#[test]
fn the_listing_carries_each_messages_read_state() {
    let mut messages = plain_messages(2);
    messages[1].seen = true;
    let fixture = ImapFixture::start(FixtureSetup {
        messages,
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let listing = expect_success(run(reader.list_messages(RowItems::Standard)));
    let read_states: Vec<(u32, bool)> = listing
        .messages
        .iter()
        .map(|message| (message.uid, message.seen))
        .collect();
    assert_eq!(read_states, [(10, false), (20, true)]);
}

/// RFC 3501 §7.4.1 lets a server expunge during a UID command and leave the
/// message out; the listing is still complete.
#[test]
fn a_message_expunged_during_the_listing_is_left_out_of_a_complete_listing() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        expunged_during_listing: vec![20],
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let listing = expect_success(run(reader.list_messages(RowItems::Standard)));
    assert_eq!(listed_uids(&listing.messages), [10, 30]);
    assert_eq!(listing.refusal, None);
}

#[test]
fn a_listing_the_server_refused_to_finish_says_why() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(4),
        listing_refused: true,
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let listing = expect_success(run(reader.list_messages(RowItems::Standard)));
    assert_eq!(listed_uids(&listing.messages), [10, 20]);
    let refusal = listing.refusal.expect("the refusal");
    assert_eq!(refusal.text, "Listing not available now");
}

#[test]
fn a_connection_lost_during_the_listing_fails_it() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        fault: Some((FaultyCommand::Listing, FaultKind::Close)),
        ..FixtureSetup::default()
    });
    let mut reader = open_reader(&fixture);
    let error = expect_failure(run(reader.list_messages(RowItems::Standard)));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::FetchMessages));
}
