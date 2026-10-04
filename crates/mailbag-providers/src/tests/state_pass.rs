// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A cycle learns the folder's state in a state pass and lists only what
//! the opening's numbers call for (specs/009-synchronization FR-005,
//! SC-011, SC-012).

use super::flags::{
    IMAP_ACCOUNT, assert_stored, pending_in, store_commands, synchronized_imap_inbox, want,
};
use super::*;
use mailbag_domain::MessageFlag;

/// How many listings (`UID FETCH 1:*`) and openings (`SELECT`) the server
/// saw so far.
fn listings_and_openings(fixture: &ImapFixture) -> (usize, usize) {
    let log = fixture.log();
    let listings = (log.fetches.iter())
        .filter(|fetch| fetch.message_set == "1:*")
        .count();
    let openings = (log.commands.iter())
        .filter(|command| command.starts_with("SELECT"))
        .count();
    (listings, openings)
}

/// Whether the server's latest listing asked for the changed flags only.
fn latest_listing_asked_changed_flags(fixture: &ImapFixture) -> bool {
    (fixture.log().fetches.into_iter().rev())
        .find(|fetch| fetch.message_set == "1:*")
        .is_some_and(|fetch| {
            fetch
                .items
                .iter()
                .any(|item| item.starts_with("CHANGEDSINCE"))
        })
}

fn message_named(stored: &[Message], uid: u32) -> &Message {
    (stored.iter())
        .find(|message| message.identity == imap_identity(uid))
        .expect("the stored message")
}

/// SC-011 on one server that announces CONDSTORE, cycle after cycle.
#[test]
fn a_synchronized_folder_lists_only_what_its_numbers_call_for() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        condstore: true,
        // Openings 1 and 2 are the first fill and its second pass. Before
        // the later openings: nothing; message 10 read elsewhere; message
        // 30 arriving; message 20 leaving.
        seen_from_opening: vec![(4, 10)],
        arriving_from_opening: vec![(5, 30)],
        gone_from_opening: vec![(7, 20)],
        ..FixtureSetup::default()
    });
    let (store, inbox) = synchronized_imap_inbox(&fixture);
    let state = || store.read_folder_sync(&inbox).unwrap().state;
    assert_eq!(listings_and_openings(&fixture), (1, 2));
    assert!(state().synchronized);
    assert_eq!(state().numbers.unwrap().highest_modseq, Some(3));
    // Nothing changed: the opening alone, nothing written.
    let (outcome, _, store_changes) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(
        (listings_and_openings(&fixture), store_changes),
        ((1, 3), 0)
    );
    // A flag changed elsewhere: the changed flags alone.
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(listings_and_openings(&fixture), (2, 4));
    assert!(latest_listing_asked_changed_flags(&fixture));
    assert!(message_named(&stored, 10).seen);
    assert_eq!(state().numbers.unwrap().highest_modseq, Some(4));
    // An arrival: every message listed, the arrival fetched, and a second
    // pass of the opening alone.
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(identities(&stored).len(), 3);
    assert_eq!(listings_and_openings(&fixture), (3, 6));
    assert!(!latest_listing_asked_changed_flags(&fixture));
    assert!(state().synchronized);
    // A removal: every message listed, the message gone, no second pass.
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(identities(&stored), [imap_identity(30), imap_identity(10)]);
    assert_eq!(listings_and_openings(&fixture), (4, 7));
    // A pending star with nothing changed on the server: every message
    // listed, since the listing addresses the star; the second pass lists
    // the flags the cycle's own command changed and settles it.
    want(
        &store,
        IMAP_ACCOUNT,
        &imap_identity(30),
        MessageFlag::Flagged,
        true,
    );
    let (outcome, stored, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(
        store_commands(&fixture),
        [r"UID STORE 30 +FLAGS.SILENT (\Flagged)"]
    );
    assert_eq!(listings_and_openings(&fixture), (6, 9));
    assert!(latest_listing_asked_changed_flags(&fixture));
    assert_eq!(pending_in(&store, &inbox), []);
    assert!(message_named(&stored, 30).flagged);
}

/// SC-012: the second pass stores what changed during the fill; a fill
/// nothing disturbed ends with the opening alone; an arrival the second
/// pass lists leaves the folder not completed for the next cycle.
#[test]
fn a_first_fill_ends_with_the_folder_as_the_server_has_it_now() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(300),
        condstore: true,
        // Between the first opening and the second pass: 10 read, 20 deleted.
        seen_from_opening: vec![(2, 10)],
        gone_from_opening: vec![(2, 20)],
        ..FixtureSetup::default()
    });
    let (store, inbox) = synchronized_imap_inbox(&fixture);
    let stored = stored_messages(&store, &inbox);
    assert_eq!(stored.len(), 299);
    assert!(!identities(&stored).contains(&imap_identity(20).as_str()));
    assert!(message_named(&stored, 10).seen);
    assert_eq!(listings_and_openings(&fixture), (2, 2));
    assert!(store.read_folder_sync(&inbox).unwrap().state.synchronized);

    let quiet = ImapFixture::start(FixtureSetup {
        messages: plain_messages(3),
        condstore: true,
        ..FixtureSetup::default()
    });
    synchronized_imap_inbox(&quiet);
    assert_eq!(listings_and_openings(&quiet), (1, 2));

    let mut messages = plain_messages(3);
    messages.push(FixtureMessage::plain_text(40, "Arrived during the fill"));
    let late = ImapFixture::start(FixtureSetup {
        messages,
        condstore: true,
        arriving_from_opening: vec![(2, 40)],
        ..FixtureSetup::default()
    });
    let (store, inbox) = synchronized_imap_inbox(&late);
    assert!(!store.read_folder_sync(&inbox).unwrap().state.synchronized);
    assert_eq!(stored_messages(&store, &inbox).len(), 3);
    assert_eq!(listings_and_openings(&late), (2, 2));
    // The next cycle lists every message, fetches the arrival and ends
    // with a second pass of the opening alone.
    let (outcome, stored, _) = synchronize_again(&late, &store);
    assert_stored(&outcome);
    assert!(identities(&stored).contains(&imap_identity(40).as_str()));
    assert!(store.read_folder_sync(&inbox).unwrap().state.synchronized);
    assert_eq!(listings_and_openings(&late), (3, 4));
    assert!(!latest_listing_asked_changed_flags(&late));
}

/// Without CONDSTORE the opening gives no mod-sequence, so every pass
/// lists every message, as the base method always did.
#[test]
fn a_server_without_condstore_lists_every_message_at_every_pass() {
    let fixture = imap_server(plain_messages(2));
    let (store, _) = synchronized_imap_inbox(&fixture);
    assert_eq!(listings_and_openings(&fixture), (2, 2));
    let (outcome, _, _) = synchronize_again(&fixture, &store);
    assert_stored(&outcome);
    assert_eq!(listings_and_openings(&fixture), (3, 3));
    assert!(!latest_listing_asked_changed_flags(&fixture));
}
