// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two guarantees of what reading an Inbox writes: a connection names the
//! server it went to even when it fails, and no sign-in name reaches a line
//! (specs/003-logging).

use super::test_record::CapturedRecord;
use super::{expect_failure, expect_success, plain_messages, run};
use crate::{
    Credential, Encryption, ImapAccount, ImapFailure, ImapStep, InboxReader, OpenOptions, RowItems,
    session::replace_sign_in_name,
    test_server::{FixtureSetup, ImapFixture, TEST_LOGIN, TEST_PASSWORD},
};

fn assert_contains_all(line: &str, fields: &[&str]) {
    for field in fields {
        assert!(line.contains(field), "{field} is missing from {line}");
    }
}

#[test]
fn the_sign_in_name_is_replaced_in_any_case_and_length() {
    let replaced = [
        (
            "user@example.org",
            "NO user@example.org rejected",
            "NO <login> rejected",
        ),
        (
            "user@example.org",
            "NO USER@Example.org rejected",
            "NO <login> rejected",
        ),
        ("jo", "jo: jo is locked", "<login>: <login> is locked"),
        ("jo", "NO no such mailbox", "NO no such mailbox"),
        // A short name also matches inside ordinary words; that is accepted.
        ("ab", "about ab", "<login>out <login>"),
    ];
    for (sign_in_name, server_text, logged_text) in replaced {
        assert_eq!(replace_sign_in_name(sign_in_name, server_text), logged_text);
    }
}

#[test]
fn a_connection_that_fails_leaves_the_host_in_the_record() {
    // Nothing listens on port 1 of the loopback address: binding a port below
    // 1024 needs privileges, so the connection is refused at once.
    let unreachable = ImapAccount {
        host: "localhost:1".to_owned(),
        login: TEST_LOGIN.to_owned(),
        credential: Credential::Password(TEST_PASSWORD.to_owned()),
        encryption: Encryption::ImplicitTls,
    };
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    let error = expect_failure(run(InboxReader::open(unreachable, OpenOptions::default())));
    assert_eq!(error.failure, ImapFailure::Failed(ImapStep::Connect));
    let attempts: Vec<String> = record
        .lines_at("DEBUG")
        .into_iter()
        .filter(|line| line.contains("connecting"))
        .collect();
    assert_eq!(attempts.len(), 1, "{}", record.text());
    assert_contains_all(&attempts[0], &[r#"host="localhost""#, "port=1"]);
}

#[test]
fn a_refused_sign_in_is_logged_with_a_short_sign_in_name_replaced() {
    let fixture = ImapFixture::start(FixtureSetup {
        credentials: Some(("ab".to_owned(), TEST_PASSWORD.to_owned())),
        rejection: "{tag} NO [AUTHENTICATIONFAILED] AB may not sign in as ab\r\n".to_owned(),
        ..FixtureSetup::default()
    });
    let mut account = fixture.account_with_password("wrong password");
    account.login = "ab".to_owned();
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    expect_failure(run(InboxReader::open(account, OpenOptions::default())));
    let replies: Vec<String> = record
        .lines_at("DEBUG")
        .into_iter()
        .filter(|line| line.contains("server_text"))
        .collect();
    assert_eq!(replies.len(), 1, "{}", record.text());
    assert_contains_all(
        &replies[0],
        &[
            r#"code="AUTHENTICATIONFAILED""#,
            r#"server_text="<login> may not sign in as <login>""#,
        ],
    );
    assert!(
        record.lines_at("ERROR").is_empty(),
        "the load writes the error line"
    );
}

/// The error carries the server's texts as the record shows them, so every
/// channel shows the same masked text; `in` also occurs inside `<login>`,
/// which a second replacement would break.
#[test]
fn a_refused_sign_in_carries_its_texts_with_the_sign_in_name_replaced_once() {
    let fixture = ImapFixture::start(FixtureSetup {
        credentials: Some(("in".to_owned(), TEST_PASSWORD.to_owned())),
        notice_before_sign_in: Some("* OK [ALERT] Password for in expired".to_owned()),
        rejection: "{tag} NO [AUTHENTICATIONFAILED] in may not sign in as in\r\n".to_owned(),
        ..FixtureSetup::default()
    });
    let mut account = fixture.account_with_password("wrong password");
    account.login = "in".to_owned();
    let error = expect_failure(run(InboxReader::open(account, OpenOptions::default())));
    let reply = error.server_reply.expect("the rejection is kept");
    assert_eq!(reply.text, "<login> may not sign <login> as <login>");
    assert_eq!(error.alerts, ["Password for <login> expired"]);
}

#[test]
fn the_refusal_of_a_short_list_carries_the_sign_in_name_replaced() {
    let fixture = ImapFixture::start(FixtureSetup {
        credentials: Some(("some".to_owned(), TEST_PASSWORD.to_owned())),
        messages: plain_messages(3),
        unfetchable_uids: vec![20],
        ..FixtureSetup::default()
    });
    let mut account = fixture.account();
    account.login = "some".to_owned();
    let mut reader = expect_success(run(InboxReader::open(account, OpenOptions::default())));
    let listed = expect_success(run(reader.fetch_rows(RowItems::Standard, 100)));
    let refusal = listed.refusal.expect("the list is short");
    assert_eq!(refusal.text, "<login> messages could not be FETCHed");
}
