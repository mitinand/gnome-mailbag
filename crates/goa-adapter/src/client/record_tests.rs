// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a read of the account list writes to the record (specs/003-logging).

use super::tests::*;
use crate::{test_bus::TestBus, test_goa::*, test_record::CapturedRecord};
use gio::prelude::*;
use std::collections::BTreeMap;

/// Accounts whose name, address, host and sign-in name are markers that must
/// never reach the record.
fn marked_accounts() -> Vec<Interfaces> {
    ["imap_smtp", "google", "ms_graph", "exchange"]
        .into_iter()
        .enumerate()
        .map(|(index, provider)| {
            let mut account = make_account(&format!("account_marker_id_{index}"));
            account
                .get_mut(ACCOUNT_INTERFACE)
                .unwrap()
                .insert("ProviderType".into(), provider.to_variant());
            account
        })
        .collect()
}

const PRIVATE_MARKERS: [&str; 4] = [
    "Synthetic account",
    "synthetic@example.invalid",
    "imap.example.invalid",
    "synthetic-user",
];

fn assert_no_private_markers(record: &CapturedRecord) {
    let text = record.text();
    for marker in PRIVATE_MARKERS {
        assert!(
            !text.contains(marker),
            "{marker} reached the record:\n{text}"
        );
    }
}

#[test]
fn a_read_is_one_line_with_the_number_of_accounts() {
    run_in_context(|| {
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let bus = TestBus::new();
        let reply = make_account_reply(marked_accounts());
        let _goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply));
        let (_client, updates) = start_test_client(&bus);
        updates.completed();
        let reads = record.lines_at("INFO");
        assert_eq!(reads.len(), 1, "{}", record.text());
        for field in ["account list read", "accounts=4"] {
            assert!(reads[0].contains(field), "{field} is missing: {}", reads[0]);
        }
        assert!(record.lines_at("WARN").is_empty() && record.lines_at("ERROR").is_empty());
        assert_no_private_markers(&record);
    });
}

#[test]
fn each_failed_read_is_one_error_and_a_result_published_again_is_none() {
    run_in_context(|| {
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let bus = TestBus::new();
        let reply = make_account_reply(marked_accounts());
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply.clone()));
        let (client, updates) = start_test_client(&bus);
        updates.completed();
        goa.set_reply(ReplyBehavior::AccessDenied);
        client.refresh_accounts();
        updates.completed();
        let errors = record.lines_at("ERROR");
        assert_eq!(errors.len(), 1, "{}", record.text());
        for field in [
            "account list could not be read",
            r#"step="read accounts""#,
            "cause=AccessDenied",
        ] {
            assert!(
                errors[0].contains(field),
                "{field} is missing: {}",
                errors[0]
            );
        }

        // Retry publishes the failed result again before its own read ends.
        client.refresh_accounts();
        assert!(updates.next().retry_pending);
        assert_eq!(record.lines_at("ERROR").len(), 1, "{}", record.text());
        updates.completed();
        assert_eq!(
            record.lines_at("ERROR").len(),
            2,
            "the retried read failed too"
        );

        goa.set_reply(ReplyBehavior::Value(reply));
        client.refresh_accounts();
        updates.completed();
        let last_line = record.text().lines().last().unwrap().to_owned();
        assert!(last_line.contains("account list read"), "{last_line}");
        assert_eq!(record.lines_at("ERROR").len(), 2, "{}", record.text());
        assert!(record.lines_at("WARN").is_empty());
        assert_no_private_markers(&record);
    });
}

#[test]
fn a_change_signal_is_a_debug_line_before_its_read() {
    run_in_context(|| {
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let bus = TestBus::new();
        let reply = make_account_reply(marked_accounts());
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply));
        let (_client, updates) = start_test_client(&bus);
        updates.completed();
        goa.change_properties(
            ACCOUNT_INTERFACE,
            BTreeMap::new(),
            vec!["PresentationIdentity".into()],
        );
        updates.completed();
        let lines: Vec<String> = record.text().lines().map(str::to_owned).collect();
        let signal = lines
            .iter()
            .position(|line| line.contains(r#"signal="PropertiesChanged""#))
            .unwrap_or_else(|| panic!("no signal line:\n{}", record.text()));
        assert!(lines[signal].starts_with("DEBUG"), "{}", lines[signal]);
        assert!(
            lines[signal + 1..]
                .iter()
                .any(|line| line.contains("account list read"))
        );
        assert_no_private_markers(&record);
    });
}
