// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

fn make_details() -> AccountDetails {
    AccountDetails {
        provider: Some(AccountProvider::Other),
        mail_enabled: Some(false),
        needs_attention: Some(true),
        mail_service_available: false,
        provider_name: None,
        display_name: None,
        email_address: None,
        icon_name: None,
    }
}

#[test]
fn unknown_fields_are_distinct_from_other_provider_and_disabled_mail() {
    let mut details = make_details();
    assert!(details.invalid_fields().is_empty());
    details.provider = None;
    details.mail_enabled = None;
    details.needs_attention = None;
    assert_eq!(
        details.invalid_fields(),
        vec![
            AccountField::Provider,
            AccountField::MailEnabled,
            AccountField::Attention
        ]
    );
    details.mail_enabled = Some(false);
    assert_eq!(
        details.invalid_fields(),
        vec![AccountField::Provider, AccountField::Attention]
    );
}

#[test]
fn ids_validate_text_and_never_expose_it_in_debug() {
    for invalid in ["", "line\nbreak", "nul\0", &"x".repeat(4097)] {
        assert!(AccountId::try_from(invalid).is_err());
    }
    let id = AccountId::try_from("synthetic-private-id").unwrap();
    assert_eq!(id, AccountId::try_from("synthetic-private-id").unwrap());
    assert_eq!(id.byte_len(), 20);
    assert!(!format!("{id:?}").contains("synthetic-private-id"));
    assert!(AccountId::try_from("x".repeat(4096).as_str()).is_ok());
}

#[test]
fn invalid_display_values_and_icon_paths_are_rejected() {
    assert!(is_valid_text("Synthetic account"));
    assert!(!is_valid_text("\n"));
    assert!(!is_valid_text(&"x".repeat(4097)));
    assert!(is_valid_icon_name("mail-unread-symbolic"));
    for invalid in [
        "",
        "/tmp/image",
        "https://example.invalid/icon",
        "icon name",
    ] {
        assert!(!is_valid_icon_name(invalid));
    }
}

#[test]
fn initial_list_does_not_confirm_absence_or_hide_diagnostics() {
    let update = AccountUpdate::default();
    assert_eq!(update.last_check, AccountCheckResult::NotChecked);
    assert!(!update.check_pending);
    assert!(!update.last_check.is_complete());
    let error = AccountCheckError {
        operation: "read accounts",
        cause: ErrorCause::AccessDenied,
        domain: Some("safe-source-domain".into()),
        code: Some(7),
    };
    assert!(format!("{error:?}").contains("safe-source-domain"));
    assert_eq!(error.code, Some(7));
    let mut details = make_details();
    details.display_name = Some("Private label".into());
    assert!(!format!("{details:?}").contains("Private label"));
}
