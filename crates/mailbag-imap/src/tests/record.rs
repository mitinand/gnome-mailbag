// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two guarantees of what reading an Inbox writes: a connection names the
//! server it went to even when it fails, and no sign-in name reaches a line
//! (specs/003-logging).

use super::test_record::CapturedRecord;
use super::{expect_failure, run};
use crate::{
    Credential, Encryption, ImapAccount, ImapFailure, ImapStep, InboxReader, OpenOptions,
    session::server_text_for_log,
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
        assert_eq!(server_text_for_log(sign_in_name, server_text), logged_text);
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
