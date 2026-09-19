// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod acquisition;
mod sections;
mod secure_session;
mod structure_isolation;
mod timeouts;

use crate::{
    Encryption, ImapError, InboxReader,
    test_server::{FixtureMessage, FixtureSetup, ImapFixture},
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
#[test]
#[ignore = "serves IMAP on fixed ports until stopped"]
fn serve_fixture() {
    let scenario = std::env::var("MAILBAG_IMAP_SCENARIO").unwrap_or_else(|_| "basic-101".into());
    let count = scenario
        .strip_prefix("basic-")
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("unknown scenario {scenario}; use basic-<message count>"));
    let start = |encryption, port| {
        let setup = FixtureSetup {
            encryption,
            credentials: None,
            messages: plain_messages(count),
            ..FixtureSetup::default()
        };
        ImapFixture::start_on_port(setup, port)
            .unwrap_or_else(|error| panic!("cannot listen on port {port}: {error}"))
    };
    let _implicit_tls = start(Encryption::ImplicitTls, 1993);
    let _starttls = start(Encryption::StartTls, 1143);
    println!(
        "Scenario {scenario}: implicit TLS on localhost:1993, STARTTLS on localhost:1143. \
         Any login and password are accepted. Stop with Ctrl+C."
    );
    loop {
        std::thread::park();
    }
}
