// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod acquisition;
mod record;
mod sections;
mod secure_session;
mod structure_isolation;
#[path = "../../../../tests/support/record.rs"]
mod test_record;
mod timeouts;

use crate::{
    Encryption, ImapAccount, ImapError, ImapFailure, ImapStep, InboxReader,
    test_server::{FixtureMessage, FixtureSetup, ImapFixture, StartTlsBehavior},
};
use std::{
    future::Future,
    time::{Duration, Instant},
};

/// Runs a future on a fresh GLib context, as the mail worker does.
fn run<T>(future: impl Future<Output = T>) -> T {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .unwrap()
}

fn expect_success<T>(result: Result<T, ImapError>) -> T {
    result.unwrap_or_else(|error| panic!("unexpected {error:?}"))
}

fn expect_failure<T>(result: Result<T, ImapError>) -> ImapError {
    match result {
        Ok(_) => panic!("the operation unexpectedly succeeded"),
        Err(error) => error,
    }
}

/// Plain-text messages with UIDs 10, 20, 30 and so on.
fn plain_messages(count: u32) -> Vec<FixtureMessage> {
    (1..=count)
        .map(|number| FixtureMessage::plain_text(number * 10, &format!("Text {number}")))
        .collect()
}

fn open_reader(fixture: &ImapFixture) -> InboxReader {
    expect_success(run(InboxReader::open(fixture.account())))
}

/// Waits for the server thread to observe something.
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "test server deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Manual acceptance with Online Accounts, see quickstart.md:
/// `MAILBAG_IMAP_SCENARIO=basic-101 cargo test --locked -p mailbag-imap serve_fixture -- --ignored --nocapture`
///
/// `MAILBAG_IMAP_SCENARIO` selects `basic-<message count>` or `long-text`,
/// `MAILBAG_IMAP_CERTIFICATE` the server certificate and
/// `MAILBAG_IMAP_STARTTLS` what the unencrypted part of port 1143 does.
#[test]
#[ignore = "serves IMAP on fixed ports until stopped"]
fn serve_fixture() {
    let scenario = std::env::var("MAILBAG_IMAP_SCENARIO").unwrap_or_else(|_| "basic-101".into());
    let messages = fixture_messages(&scenario);
    let certificate = fixture_certificate();
    let starttls = fixture_starttls();
    let start = |encryption, port| {
        let setup = FixtureSetup {
            encryption,
            certificate,
            starttls,
            credentials: None,
            messages: messages.clone(),
            ..FixtureSetup::default()
        };
        ImapFixture::start_on_port(setup, port)
            .unwrap_or_else(|error| panic!("cannot listen on port {port}: {error}"))
    };
    let implicit_tls = start(Encryption::ImplicitTls, 1993);
    let starttls_server = start(Encryption::StartTls, 1143);
    println!(
        "Scenario {scenario} with the {certificate} certificate and STARTTLS {starttls:?}: \
         implicit TLS on localhost:1993, STARTTLS on localhost:1143. Any login and password \
         are accepted. Stop with Ctrl+C."
    );
    report_server_activity(&[
        ("implicit TLS", &implicit_tls),
        ("STARTTLS", &starttls_server),
    ]);
}

/// The messages a scenario serves.
fn fixture_messages(scenario: &str) -> Vec<FixtureMessage> {
    if scenario == "long-text" {
        // The 64 KiB display boundary: one body below it, one exactly on it
        // and one above it, each a single line of two-byte characters.
        return (1..)
            .zip([65_535_usize, 65_536, 65_537])
            .map(|(number, size)| FixtureMessage::plain_text(number * 10, &long_line(size)))
            .collect();
    }
    let count = scenario
        .strip_prefix("basic-")
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| {
            panic!("unknown scenario {scenario}; use basic-<message count> or long-text")
        });
    plain_messages(count)
}

fn long_line(size_bytes: usize) -> String {
    let mut text = format!("Тело в {size_bytes} байт: ");
    while text.len() + 'я'.len_utf8() <= size_bytes {
        text.push('я');
    }
    while text.len() < size_bytes {
        text.push('.');
    }
    text
}

fn fixture_certificate() -> &'static str {
    let name = std::env::var("MAILBAG_IMAP_CERTIFICATE").unwrap_or_else(|_| "localhost".into());
    match name.as_str() {
        "localhost" => "localhost",
        "unknown-ca" => "unknown-ca",
        "wrong-host" => "wrong-host",
        "expired" => "expired",
        other => {
            panic!("unknown certificate {other}; use localhost, unknown-ca, wrong-host or expired")
        }
    }
}

fn fixture_starttls() -> StartTlsBehavior {
    let name = std::env::var("MAILBAG_IMAP_STARTTLS").unwrap_or_else(|_| "offered".into());
    match name.as_str() {
        "offered" => StartTlsBehavior::Offered,
        "not-offered" => StartTlsBehavior::NotOffered,
        "rejected" => StartTlsBehavior::Rejected,
        "inject" => StartTlsBehavior::InjectAfterReply,
        "preauth" => StartTlsBehavior::PreauthGreeting,
        other => panic!(
            "unknown STARTTLS behavior {other}; use offered, not-offered, rejected, inject or \
             preauth"
        ),
    }
}

/// Prints each server's connections, credential transmissions and command
/// names as they change, so a refused connection shows that it carried no
/// password and no plaintext sign-in.
fn report_server_activity(servers: &[(&str, &ImapFixture)]) -> ! {
    let mut reported = vec![(0, 0, 0); servers.len()];
    loop {
        for (index, (name, server)) in servers.iter().enumerate() {
            let log = server.log();
            let current = (
                log.connections,
                log.credentials_received,
                log.commands.len(),
            );
            if current != reported[index] {
                reported[index] = current;
                println!(
                    "{name}: {} connections, {} sign-ins with credentials, commands: {}",
                    current.0,
                    current.1,
                    log.commands.join(" ")
                );
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Manual acceptance of a running `serve_fixture` through the host's own
/// trust store, as the installed application uses it, see quickstart.md:
/// `MAILBAG_IMAP_EXPECT=rejected cargo test --locked -p mailbag-imap host_trust -- --ignored --nocapture`
///
/// `MAILBAG_IMAP_ENDPOINT` names the server (default `localhost:1993`),
/// `MAILBAG_IMAP_ENCRYPTION` is `implicit` or `starttls`, and
/// `MAILBAG_IMAP_EXPECT` is `success` or `rejected`.
#[test]
#[ignore = "connects to a running serve_fixture with the host's trust store"]
fn host_trust_decides_the_connection() {
    assert!(
        !crate::test_server::test_certificates_trusted(),
        "run this test by its own filter: another test replaced the trust database"
    );
    let host = std::env::var("MAILBAG_IMAP_ENDPOINT").unwrap_or_else(|_| "localhost:1993".into());
    let encryption = match std::env::var("MAILBAG_IMAP_ENCRYPTION").as_deref() {
        Ok("starttls") => Encryption::StartTls,
        Ok("implicit") | Err(_) => Encryption::ImplicitTls,
        Ok(other) => panic!("unknown encryption {other}; use implicit or starttls"),
    };
    let account = ImapAccount {
        host: host.clone(),
        login: "acceptance".to_owned(),
        password: "acceptance".to_owned(),
        encryption,
    };
    let opened = run(InboxReader::open(account));
    match std::env::var("MAILBAG_IMAP_EXPECT").as_deref() {
        Ok("success") | Err(_) => {
            let mut reader = expect_success(opened);
            let listed = expect_success(run(reader.fetch_rows()));
            println!(
                "{host} accepted by the host's trust store: {} rows",
                listed.rows.len()
            );
        }
        Ok("rejected") => {
            let error = expect_failure(opened);
            assert_eq!(
                error.failure,
                ImapFailure::Failed(ImapStep::SecureConnection)
            );
            println!("{host} refused by the host's trust store at the secure-connection step");
        }
        Ok(other) => panic!("unknown expectation {other}; use success or rejected"),
    }
}
