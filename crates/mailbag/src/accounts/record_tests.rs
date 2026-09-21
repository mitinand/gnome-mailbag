// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What applying an account update writes to the record (specs/003-logging).

use super::account_tests::{make_account_details, make_account_id, make_failed_list};
use crate::accounts::*;
use crate::logging::{LogLevel, capture::start_record};
use goa_adapter::{AccountCheckResult, AccountDetails, AccountProvider, AccountUpdate};

/// A display name and an address that must never reach the record.
fn marked_details(provider: AccountProvider) -> AccountDetails {
    AccountDetails {
        display_name: Some("Marker Display Name".into()),
        email_address: Some("marker@example.invalid".into()),
        ..make_account_details(provider)
    }
}

fn checked(accounts: &[(&str, AccountDetails)]) -> AccountUpdate {
    AccountUpdate {
        accounts: accounts
            .iter()
            .map(|(id, details)| (make_account_id(id), details.clone()))
            .collect(),
        last_check: AccountCheckResult::Complete,
        retry_pending: false,
    }
}

/// The lines at `level`, such as `" WARN "`, that name the account by its
/// Online Accounts identifier.
fn lines_about(text: &str, level: &str, id: &str) -> Vec<String> {
    let account = format!(r#"account="{id}""#);
    text.lines()
        .filter(|line| line.contains(level) && line.contains(&account))
        .map(str::to_owned)
        .collect()
}

fn assert_no_private_markers(text: &str) {
    for marker in ["Marker Display Name", "marker@example.invalid"] {
        assert!(
            !text.contains(marker),
            "{marker} reached the record:\n{text}"
        );
    }
}

#[test]
fn every_account_is_shown_or_not_shown_with_its_provider_and_reason() {
    let record = start_record(LogLevel::Debug);
    let mut attention = marked_details(AccountProvider::Google);
    attention.needs_attention = true;
    let mut disabled = marked_details(AccountProvider::ImapSmtp);
    disabled.mail_enabled = false;
    let mut without_mail = marked_details(AccountProvider::Microsoft365);
    without_mail.mail_service_available = false;
    let update = checked(&[
        (
            "account_1726920000_0",
            marked_details(AccountProvider::ImapSmtp),
        ),
        ("account_1726920000_1", attention),
        (
            "account_1726920000_2",
            marked_details(AccountProvider::Other),
        ),
        ("account_1726920000_3", disabled),
        ("account_1726920000_4", without_mail),
    ]);
    AccountList::default().apply_update(&update);
    let text = record.text();

    for (id, provider) in [
        ("account_1726920000_0", "imap"),
        ("account_1726920000_1", "google"),
    ] {
        let shown = lines_about(&text, " INFO ", id);
        assert_eq!(shown.len(), 1, "{text}");
        assert!(shown[0].contains("account shown"));
        assert!(shown[0].contains(&format!(r#"provider="{provider}""#)));
    }
    let problem = lines_about(&text, " WARN ", "account_1726920000_1");
    assert_eq!(problem.len(), 1, "{text}");
    assert!(problem[0].contains(r#"problem="attention needed""#));
    for (id, provider, reason) in [
        ("account_1726920000_2", "other", "unsupported provider"),
        ("account_1726920000_3", "imap", "mail disabled"),
        (
            "account_1726920000_4",
            "microsoft365",
            "mail service unavailable",
        ),
    ] {
        let not_shown = lines_about(&text, " INFO ", id);
        assert_eq!(not_shown.len(), 1, "{text}");
        assert!(not_shown[0].contains("account not shown"));
        assert!(not_shown[0].contains(&format!(r#"provider="{provider}""#)));
        assert!(not_shown[0].contains(&format!(r#"reason="{reason}""#)));
    }
    assert_eq!(text.matches(" WARN ").count(), 1, "{text}");
    assert!(!text.contains(" ERROR "), "{text}");
    assert_no_private_markers(&text);
}

#[test]
fn a_row_logs_its_problems_when_they_change_and_its_removal_once() {
    let record = start_record(LogLevel::Debug);
    let mut accounts = AccountList::default();
    accounts.apply_update(&checked(&[
        ("account_row", marked_details(AccountProvider::ImapSmtp)),
        ("account_removed", marked_details(AccountProvider::Google)),
    ]));

    let mut attention = marked_details(AccountProvider::ImapSmtp);
    attention.needs_attention = true;
    let with_problem = checked(&[("account_row", attention)]);
    accounts.apply_update(&with_problem);
    // A failed read and the same accepted list again change nothing to log.
    accounts.apply_update(&make_failed_list(&with_problem));
    accounts.apply_update(&with_problem);
    let mut without_mail = marked_details(AccountProvider::ImapSmtp);
    without_mail.mail_service_available = false;
    accounts.apply_update(&checked(&[("account_row", without_mail)]));
    let text = record.text();

    let warnings = lines_about(&text, " WARN ", "account_row");
    assert_eq!(warnings.len(), 2, "{text}");
    assert!(warnings[0].contains(r#"problem="attention needed""#));
    assert!(warnings[1].contains(r#"problem="mail service unavailable""#));
    let gone: Vec<String> = lines_about(&text, " INFO ", "account_row")
        .into_iter()
        .filter(|line| line.contains("account problem is gone"))
        .collect();
    assert_eq!(gone.len(), 1, "{text}");
    assert!(gone[0].contains(r#"problem="attention needed""#));
    let removed: Vec<String> = lines_about(&text, " INFO ", "account_removed")
        .into_iter()
        .filter(|line| line.contains("account not shown"))
        .collect();
    assert_eq!(removed.len(), 1, "{text}");
    assert!(removed[0].contains(r#"provider="google""#));
    assert!(removed[0].contains(r#"reason="removed from Online Accounts""#));
    assert!(!text.contains(" ERROR "), "{text}");
    assert_no_private_markers(&text);
}
