// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What reading an Inbox writes to the record (specs/003-logging).

use super::test_record::CapturedRecord;
use super::{expect_failure, expect_success, open_reader, plain_messages, run};
use crate::{
    InboxReader, TextParts, TextRequest,
    session::server_text_for_log,
    test_server::{FixtureMessage, FixtureSetup, ImapFixture, TEST_LOGIN, TEST_PASSWORD},
};

/// Reads the message list, the structures and the text, as a load does.
fn read_inbox(fixture: &ImapFixture) {
    let mut reader = open_reader(fixture);
    let rows = expect_success(run(reader.fetch_rows())).rows;
    let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
    expect_success(run(reader.fetch_structures(&uids)));
    let requests = uids
        .iter()
        .map(|&uid| TextRequest {
            uid,
            parts: TextParts::SinglePartBody,
        })
        .collect();
    expect_success(run(reader.fetch_text(requests, |_, _| {})));
}

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
fn a_load_writes_its_steps_at_info_and_the_details_at_debug() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    read_inbox(&fixture);
    let info = record.lines_at("INFO");
    let steps: Vec<&str> = [
        "connected",
        "connection secured encryption=ImplicitTls",
        "server capabilities",
        r#"signed in method="PLAIN""#,
        "Inbox opened messages=2",
        "message list loaded rows=2",
        "part structures loaded messages=2",
        "text loaded messages=2 commands=1",
    ]
    .into_iter()
    .filter(|step| !info.iter().any(|line| line.contains(step)))
    .collect();
    assert!(steps.is_empty(), "missing {steps:?} in\n{}", record.text());
    let debug = record.lines_at("DEBUG");
    let details = debug.join("\n");
    assert_contains_all(
        &details,
        &[
            r#"host="localhost""#,
            r#"folder="INBOX""#,
            "uid_validity=1",
            r#"message{uid=10}"#,
            r#"content_type="TEXT/PLAIN""#,
            "transfer_encoding=",
            r#"sections="1""#,
        ],
    );
    let text = record.text();
    for private in [TEST_LOGIN, TEST_PASSWORD, "Message 10", "Text 1"] {
        assert!(
            !text.contains(private),
            "{private} reached the record:\n{text}"
        );
    }
    assert!(record.lines_at("WARN").is_empty() && record.lines_at("ERROR").is_empty());
}

#[test]
fn info_names_no_host_folder_or_message() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(2),
        ..FixtureSetup::default()
    });
    let record = CapturedRecord::start(tracing::Level::INFO);
    read_inbox(&fixture);
    let text = record.text();
    for detail in ["localhost", "INBOX", "uid", "127.0.0.1", TEST_LOGIN] {
        assert!(
            !text.contains(detail),
            "{detail} is in an info line:\n{text}"
        );
    }
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
    expect_failure(run(InboxReader::open(account)));
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

#[test]
fn a_certificate_that_is_not_accepted_names_the_failed_check() {
    for (certificate, check) in [
        ("unknown-ca", "UNKNOWN_CA"),
        ("wrong-host", "BAD_IDENTITY"),
        ("expired", "EXPIRED"),
    ] {
        let fixture = ImapFixture::start(FixtureSetup {
            certificate,
            ..FixtureSetup::default()
        });
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        expect_failure(run(InboxReader::open(fixture.account())));
        let rejected: Vec<String> = record
            .lines_at("DEBUG")
            .into_iter()
            .filter(|line| line.contains("certificate_errors"))
            .collect();
        assert_eq!(rejected.len(), 1, "{certificate}:\n{}", record.text());
        assert!(
            rejected[0].contains(check),
            "{certificate}: {}",
            rejected[0]
        );
        assert!(
            rejected[0].contains(r#"tls_error="Unacceptable TLS certificate""#),
            "{certificate}: {}",
            rejected[0]
        );
    }
}

#[test]
fn refused_commands_and_unreadable_structures_are_debug_lines() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: vec![
            FixtureMessage::plain_text(10, "readable"),
            FixtureMessage::deeply_nested(20, 40),
            FixtureMessage::plain_text(30, "unfetchable"),
        ],
        unfetchable_uids: vec![30],
        ..FixtureSetup::default()
    });
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    let mut reader = open_reader(&fixture);
    let rows = expect_success(run(reader.fetch_rows())).rows;
    let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
    expect_success(run(reader.fetch_structures(&uids)));
    let debug = record.lines_at("DEBUG");
    assert!(
        debug
            .iter()
            .any(|line| line.contains(r#"server_text="Some messages could not be FETCHed""#)),
        "{}",
        record.text()
    );
    let unreadable = debug
        .iter()
        .find(|line| line.contains("the description could not be parsed"))
        .unwrap_or_else(|| panic!("no unreadable structure line:\n{}", record.text()));
    assert!(unreadable.contains("uid=20"), "{unreadable}");
    assert!(
        record
            .lines_at("INFO")
            .iter()
            .any(|line| line.contains("reconnecting after a structure that could not be read")),
        "{}",
        record.text()
    );
}

#[test]
fn a_port_that_expects_starttls_is_named_by_the_tls_error() {
    let fixture = ImapFixture::start(FixtureSetup {
        encryption: crate::Encryption::StartTls,
        ..FixtureSetup::default()
    });
    let mut account = fixture.account();
    account.encryption = crate::Encryption::ImplicitTls;
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    expect_failure(run(InboxReader::open(account)));
    let failed: Vec<String> = record
        .lines_at("DEBUG")
        .into_iter()
        .filter(|line| line.contains("TLS handshake failed"))
        .collect();
    assert_eq!(failed.len(), 1, "{}", record.text());
    assert!(
        failed[0].contains("An unexpected TLS packet was received"),
        "{}",
        failed[0]
    );
    assert!(!failed[0].contains("certificate_errors"), "{}", failed[0]);
}

#[test]
fn an_alert_is_logged_when_it_arrives_even_if_the_load_succeeds() {
    let fixture = ImapFixture::start(FixtureSetup {
        messages: plain_messages(1),
        examine_completion: format!(
            "* OK [ALERT] {TEST_LOGIN}: your mailbox is almost full\r\n{{tag}} OK [READ-ONLY] done\r\n"
        ),
        ..FixtureSetup::default()
    });
    let record = CapturedRecord::start(tracing::Level::DEBUG);
    read_inbox(&fixture);
    let alerts: Vec<String> = record
        .text()
        .lines()
        .filter(|line| line.contains("the server sent an alert"))
        .map(str::to_owned)
        .collect();
    assert_eq!(alerts.len(), 2, "{}", record.text());
    assert!(alerts[0].trim_start().starts_with("INFO") && !alerts[0].contains("alert="));
    assert!(
        alerts[1].contains(r#"alert="<login>: your mailbox is almost full""#),
        "{}",
        alerts[1]
    );
    assert!(!record.text().contains(TEST_LOGIN), "{}", record.text());
}
