// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a Gmail account needs from the protocol: a sign-in that carries an
//! access token instead of a password, UTF-8 names, a named client and the
//! two Gmail attributes on every row.

use super::test_record::CapturedRecord;
use super::{expect_failure, expect_success, run};
use crate::{
    ClientIdentity, Credential, GmailRow, ImapFailure, ImapStep, InboxReader, OpenOptions,
    RowItems,
    test_server::{
        FixtureMessage, FixtureSetup, ID_CONNECTION_TOKEN, ID_REMOTE_HOST, ImapFixture,
        TEST_ACCESS_TOKEN,
    },
};

/// A server that offers the token mechanism and holds one labelled message.
fn gmail_setup() -> FixtureSetup {
    FixtureSetup {
        access_token: Some(TEST_ACCESS_TOKEN.to_owned()),
        messages: vec![
            FixtureMessage::plain_text(10, "Text")
                .with_gmail_attributes(1_278_455_344_230_334_865, &["\\Important", "Работа/Счета"]),
        ],
        ..FixtureSetup::default()
    }
}

/// What the Gmail load asks for: readable names and a named client.
fn gmail_options() -> OpenOptions {
    OpenOptions {
        readable_names: true,
        client_identity: Some(ClientIdentity {
            name: "Mailbag".to_owned(),
            version: "0.1.0-dev".to_owned(),
            vendor: "Andrey Mitin".to_owned(),
            contact: "tests@mailbag.invalid".to_owned(),
            support_url: "https://github.com/mitinand/gnome-mailbag".to_owned(),
        }),
    }
}

#[test]
fn a_token_signs_in_with_xoauth2_and_never_with_login() {
    let fixture = ImapFixture::start(gmail_setup());
    let reader = expect_success(run(InboxReader::open(
        fixture.account_with_token(),
        OpenOptions::default(),
    )));
    drop(reader);
    let log = fixture.log();
    assert_eq!(log.commands, ["CAPABILITY", "AUTHENTICATE", "EXAMINE"]);
    assert_eq!(log.sign_in_mechanisms, ["XOAUTH2"]);
    assert_eq!(log.credentials_received, 1);
    assert_eq!(log.empty_challenge_replies, 0);
}

#[test]
fn a_refused_token_is_acknowledged_before_the_server_explains_it() {
    let fixture = ImapFixture::start(gmail_setup());
    let account =
        fixture.account_with_credential(Credential::AccessToken("expired-token".to_owned()));
    let error = expect_failure(run(InboxReader::open(account, OpenOptions::default())));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::SignIn));
    let reply = error.server_reply.expect("the server gave a reason");
    assert_eq!(reply.code.as_deref(), Some("AUTHENTICATIONFAILED"));
    // Google's exchange: the error challenge is answered with an empty line.
    assert_eq!(fixture.log().empty_challenge_replies, 1);
}

#[test]
fn a_server_without_xoauth2_leaves_no_sign_in_method() {
    // The default setup offers AUTH=PLAIN and no token mechanism.
    let fixture = ImapFixture::start(FixtureSetup::default());
    let error = expect_failure(run(InboxReader::open(
        fixture.account_with_token(),
        OpenOptions::default(),
    )));
    assert_eq!(error.failure, ImapFailure::NoSignInMethod);
    let log = fixture.log();
    assert_eq!(log.credentials_received, 0);
    assert_eq!(log.commands, ["CAPABILITY"]);
}

#[test]
fn readable_names_are_offered_before_the_inbox_whether_they_are_accepted_or_refused() {
    for refused in [false, true] {
        let fixture = ImapFixture::start(FixtureSetup {
            enable_refused: refused,
            ..gmail_setup()
        });
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let reader = expect_success(run(InboxReader::open(
            fixture.account_with_token(),
            gmail_options(),
        )));
        drop(reader);
        assert_eq!(
            fixture.log().commands,
            ["CAPABILITY", "AUTHENTICATE", "ENABLE", "ID", "EXAMINE"],
            "refused: {refused}"
        );
        let expected = match refused {
            false => "the server accepted UTF-8 names",
            true => "the server refused UTF-8 names",
        };
        assert!(record.text().contains(expected), "{}", record.text());
    }
}

#[test]
fn the_identification_reply_reaches_the_record_without_its_private_fields() {
    for refused in [false, true] {
        let fixture = ImapFixture::start(FixtureSetup {
            id_refused: refused,
            ..gmail_setup()
        });
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let reader = expect_success(run(InboxReader::open(
            fixture.account_with_token(),
            gmail_options(),
        )));
        drop(reader);
        // A refusal does not stop the Inbox from opening.
        assert!(fixture.log().commands.contains(&"EXAMINE".to_owned()));
        // Google asks for the vendor and a contact address beside the name.
        let sent = fixture
            .log()
            .client_identification
            .expect("the client named itself");
        assert_eq!(
            sent,
            concat!(
                r#"("name" "Mailbag" "version" "0.1.0-dev" "#,
                r#""vendor" "Andrey Mitin" "contact" "tests@mailbag.invalid" "#,
                r#""support-url" "https://github.com/mitinand/gnome-mailbag")"#
            ),
            "refused: {refused}"
        );
        let identifications = debug_lines_about(&record, "the server identified itself");
        let refusals = debug_lines_about(&record, "the server refused the identification");
        if refused {
            // A NO is told apart from a reply that carried no fields.
            assert!(identifications.is_empty(), "{}", record.text());
            assert_eq!(refusals.len(), 1, "{}", record.text());
        } else {
            assert!(refusals.is_empty(), "{}", record.text());
            assert_eq!(identifications.len(), 1, "{}", record.text());
            assert_contains_all(
                &identifications[0],
                &[
                    r#"name="Scripted""#,
                    r#"vendor="Mailbag tests""#,
                    r#"version="1""#,
                ],
            );
        }
        assert!(!record.text().contains(ID_REMOTE_HOST), "{}", record.text());
        assert!(
            !record.text().contains(ID_CONNECTION_TOKEN),
            "{}",
            record.text()
        );
    }
}

#[test]
fn gmail_attributes_arrive_only_when_the_row_fetch_asks_for_them() {
    let fixture = ImapFixture::start(gmail_setup());
    let mut reader = expect_success(run(InboxReader::open(
        fixture.account_with_token(),
        gmail_options(),
    )));
    let with_attributes =
        expect_success(run(reader.fetch_rows(RowItems::WithGmailAttributes, 100)));
    assert_eq!(
        with_attributes.rows[0].gmail,
        Some(GmailRow {
            message_id: 1_278_455_344_230_334_865,
            labels: vec!["\\Important".to_owned(), "Работа/Счета".to_owned()],
        })
    );
    let standard = expect_success(run(reader.fetch_rows(RowItems::Standard, 100)));
    assert_eq!(standard.rows[0].gmail, None);
    let items = fixture.log().fetches;
    assert!(
        items[0].items.contains(&"X-GM-MSGID".to_owned())
            && items[0].items.contains(&"X-GM-LABELS".to_owned()),
        "{items:?}"
    );
    assert!(
        !items[1].items.contains(&"X-GM-MSGID".to_owned()),
        "{items:?}"
    );
}

/// A row the server answered without the attributes is not invented.
#[test]
fn a_row_without_gmail_attributes_keeps_none() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::plain_text(10, "Text")],
        ..gmail_setup()
    });
    let mut reader = expect_success(run(InboxReader::open(
        fixture.account_with_token(),
        gmail_options(),
    )));
    let listed = expect_success(run(reader.fetch_rows(RowItems::WithGmailAttributes, 100)));
    assert_eq!(listed.rows[0].gmail, None);
}

#[test]
fn the_token_never_reaches_the_record_accepted_or_refused() {
    for token in [TEST_ACCESS_TOKEN, "expired-token"] {
        let fixture = ImapFixture::start(gmail_setup());
        let account = fixture.account_with_credential(Credential::AccessToken(token.to_owned()));
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        if let Ok(mut reader) = run(InboxReader::open(account, gmail_options())) {
            expect_success(run(reader.fetch_rows(RowItems::WithGmailAttributes, 100)));
        }
        assert!(!record.text().contains(token), "{}", record.text());
    }
}

/// The debug lines of the record that carry this message.
fn debug_lines_about(record: &CapturedRecord, message: &str) -> Vec<String> {
    record
        .lines_at("DEBUG")
        .into_iter()
        .filter(|line| line.contains(message))
        .collect()
}

fn assert_contains_all(line: &str, fields: &[&str]) {
    for field in fields {
        assert!(line.contains(field), "{field} is missing from {line}");
    }
}
