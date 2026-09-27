// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listing an account's mailboxes and opening one of them by its listed name.

use super::{expect_failure, expect_success, plain_messages, run};
use crate::{
    ImapFailure, ImapStep, MailboxList, MailboxName, MailboxReader, OpenOptions, RowItems,
    list_mailboxes,
    test_server::{FixtureSetup, ImapFixture},
};

fn list_from(fixture: &ImapFixture) -> Result<MailboxList, crate::ImapError> {
    run(list_mailboxes(fixture.account(), OpenOptions::default()))
}

fn mailbox(attributes: &[&str], name: &str) -> MailboxName {
    MailboxName {
        name: name.to_owned(),
        attributes: attributes
            .iter()
            .map(|&attribute| attribute.to_owned())
            .collect(),
        delimiter: Some("/".to_owned()),
    }
}

#[test]
fn listing_gives_each_name_with_its_attributes_and_delimiter() {
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![
            ("\\HasNoChildren", "/", "INBOX"),
            ("\\HasChildren \\Noselect", "/", "Projects"),
            ("\\HasNoChildren \\Junk \\Sent", "/", "Projects/Sent"),
        ],
        ..FixtureSetup::default()
    });
    let listed = expect_success(list_from(&fixture));
    assert_eq!(
        listed,
        MailboxList {
            names: vec![
                mailbox(&["\\HasNoChildren"], "INBOX"),
                mailbox(&["\\HasChildren", "\\Noselect"], "Projects"),
                // In the server's order: the first role mark decides.
                mailbox(&["\\HasNoChildren", "\\Junk", "\\Sent"], "Projects/Sent"),
            ],
            utf8_names: false,
        }
    );
    let log = fixture.log();
    assert_eq!(
        log.commands,
        ["CAPABILITY", "AUTHENTICATE", "CAPABILITY", "LIST"]
    );
    assert_eq!(log.list_arguments, [r#""" *"#]);
}

#[test]
fn a_list_refused_after_some_names_fails_with_the_servers_reply() {
    for (completion, code, text) in [
        (
            "{tag} NO [UNAVAILABLE] Try later\r\n",
            Some("UNAVAILABLE"),
            "Try later",
        ),
        ("{tag} BAD Invalid pattern\r\n", None, "Invalid pattern"),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            mailboxes: vec![("\\HasNoChildren", "/", "INBOX")],
            list_completion: Some(completion.to_owned()),
            ..FixtureSetup::default()
        });
        let error = expect_failure(list_from(&fixture));
        assert_eq!(error.failure, ImapFailure::Failed(ImapStep::ListMailboxes));
        let reply = error.server_reply.expect("the server gave a reason");
        assert_eq!(reply.code.as_deref(), code);
        assert_eq!(reply.text, text);
    }
}

#[test]
fn a_list_cut_short_by_a_closed_connection_fails() {
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", "INBOX")],
        list_completion: None,
        ..FixtureSetup::default()
    });
    let error = expect_failure(list_from(&fixture));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::ListMailboxes));
}

#[test]
fn a_name_with_a_quote_and_a_backslash_opens_as_listed() {
    let name = r#"Say "hi" \ bye"#;
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", name)],
        ..FixtureSetup::default()
    });
    let listed = expect_success(list_from(&fixture));
    assert_eq!(listed.names[0].name, name);
    drop(expect_success(run(MailboxReader::open(
        fixture.account(),
        OpenOptions::default(),
        &listed.names[0].name,
    ))));
    assert_eq!(fixture.log().examined_mailboxes, [name]);
}

#[test]
fn utf8_names_are_enabled_only_when_the_server_announces_them() {
    for (announced, enabled) in [
        (vec!["UTF8=ACCEPT"], true),
        // UTF8=ONLY includes UTF8=ACCEPT and still needs the ENABLE.
        (vec!["UTF8=ONLY"], true),
        (Vec::new(), false),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            capabilities_after_sign_in: announced.clone(),
            mailboxes: vec![("\\HasNoChildren", "/", "INBOX")],
            ..FixtureSetup::default()
        });
        let listed = expect_success(list_from(&fixture));
        assert_eq!(listed.utf8_names, enabled, "{announced:?}");
        let enable_sent = fixture
            .log()
            .commands
            .iter()
            .any(|command| command == "ENABLE");
        assert_eq!(enable_sent, enabled, "{announced:?}");
    }
}

#[test]
fn special_use_attributes_are_asked_for_only_when_the_server_announces_them() {
    for (announced, arguments) in [
        (vec!["SPECIAL-USE"], r#""" * RETURN (SPECIAL-USE)"#),
        (Vec::new(), r#""" *"#),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            capabilities_after_sign_in: announced.clone(),
            mailboxes: vec![("\\HasNoChildren \\Sent", "/", "Sent")],
            ..FixtureSetup::default()
        });
        expect_success(list_from(&fixture));
        assert_eq!(fixture.log().list_arguments, [arguments], "{announced:?}");
    }
}

#[test]
fn a_listed_mailbox_opens_and_gives_its_rows() {
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", "INBOX"), ("", "/", "Work")],
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let mut reader = expect_success(run(MailboxReader::open(
        fixture.account(),
        OpenOptions::default(),
        "Work",
    )));
    let listed = expect_success(run(reader.fetch_rows(RowItems::Standard, 100)));
    assert_eq!(listed.rows.len(), 2);
    assert_eq!(fixture.log().examined_mailboxes, ["Work"]);
}

#[test]
fn a_mailbox_the_server_does_not_have_is_not_opened() {
    let fixture = ImapFixture::start(FixtureSetup {
        mailboxes: vec![("\\HasNoChildren", "/", "INBOX")],
        ..FixtureSetup::default()
    });
    let error = expect_failure(run(MailboxReader::open(
        fixture.account(),
        OpenOptions::default(),
        "Removed",
    )));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::OpenMailbox));
    let reply = error.server_reply.expect("the server gave a reason");
    assert_eq!(reply.code.as_deref(), Some("NONEXISTENT"));
}
