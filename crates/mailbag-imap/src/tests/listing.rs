// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The listing a cycle proves removals with: complete only when the server
//! finished it, and a session the server ended told apart from other
//! failures (specs/009-synchronization/research.md §2, §13).

use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    Credential, ImapFailure, ImapStep, ListedUid, MailboxReader, OpenOptions, RowItems,
    test_server::{FaultKind, FaultyCommand, FixtureSetup, ImapFixture, TEST_ACCESS_TOKEN},
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
    assert!(!error.ended_by_server);
}

/// Gmail ends an OAuth session with BYE, for example when its token
/// expires; the error says so, and a new token signs in again.
#[test]
fn a_session_the_server_ended_is_told_apart_and_a_renewed_token_signs_in() {
    let fixture = ImapFixture::start(FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        renewed_access_token: Some("renewed-token".to_owned()),
        messages: plain_messages(2),
        fault: Some((FaultyCommand::Listing, FaultKind::Bye)),
        fault_times: 2,
        ..FixtureSetup::default()
    });
    let open = |token: &str| {
        let account = fixture.account_with_credential(Credential::AccessToken(token.to_owned()));
        expect_success(run(MailboxReader::open(
            account,
            OpenOptions::default(),
            "INBOX",
        )))
    };
    let mut reader = open(TEST_ACCESS_TOKEN);
    let error = expect_failure(run(reader.list_messages(RowItems::Standard)));
    assert!(error.ended_by_server);
    assert_eq!(
        error.server_reply.expect("the server's words").text,
        "Server is restarting"
    );
    // The server ends the second session the same way.
    let mut renewed = open("renewed-token");
    let error = expect_failure(run(renewed.list_messages(RowItems::Standard)));
    assert!(error.ended_by_server);
    let mut third = open("renewed-token");
    let listing = expect_success(run(third.list_messages(RowItems::Standard)));
    assert_eq!(listed_uids(&listing.messages), [10, 20]);
}
