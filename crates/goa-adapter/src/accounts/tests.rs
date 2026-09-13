// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::test_goa::{ACCOUNT_INTERFACE, MAIL_INTERFACE, make_account, make_account_reply};

#[test]
fn required_fields_are_not_defaulted_and_disable_is_independent() {
    for name in ["ProviderType", "MailDisabled", "AttentionNeeded"] {
        for replacement in [None, Some(42u32.to_variant())] {
            let mut account = make_account("one");
            let account_properties = account.get_mut(ACCOUNT_INTERFACE).unwrap();
            account_properties.insert("MailDisabled".into(), true.to_variant());
            match replacement {
                None => {
                    account_properties.remove(name);
                }
                Some(field_value) => {
                    account_properties.insert(name.into(), field_value);
                }
            }
            let list = parse_account_list(&make_account_reply(vec![account])).unwrap();
            assert!(list.membership_confirmed);
            let account_details = list.accounts.values().next().unwrap();
            assert!(!account_details.invalid_fields.is_empty());
            if name != "MailDisabled" {
                assert_eq!(account_details.mail_disabled, Some(true));
            }
        }
    }
}
#[test]
fn ambiguous_identity_cannot_confirm_absence() {
    for value in [
        None,
        Some(9u32.to_variant()),
        Some("".to_variant()),
        Some("x".repeat(4097).to_variant()),
    ] {
        let mut account = make_account("one");
        let account_properties = account.get_mut(ACCOUNT_INTERFACE).unwrap();
        account_properties.remove("Id");
        if let Some(value) = value {
            account_properties.insert("Id".into(), value);
        }
        let list =
            parse_account_list(&make_account_reply(vec![account, make_account("two")])).unwrap();
        assert!(!list.membership_confirmed);
        assert_eq!(list.accounts.len(), 1);
    }
    let list = parse_account_list(&make_account_reply(vec![
        make_account("same"),
        make_account("same"),
    ]))
    .unwrap();
    assert!(!list.membership_confirmed);
    assert!(list.accounts.is_empty());
}
#[test]
fn optional_fields_fall_back_without_changing_availability() {
    let mut account = make_account("one");
    for name in ["ProviderName", "PresentationIdentity", "ProviderIcon"] {
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert(name.into(), 12u32.to_variant());
    }
    account
        .get_mut(MAIL_INTERFACE)
        .unwrap()
        .insert("EmailAddress".into(), "".to_variant());
    let list = parse_account_list(&make_account_reply(vec![account])).unwrap();
    let account_details = list.accounts.values().next().unwrap();
    assert!(account_details.invalid_fields.is_empty());
    assert!(account_details.email_address.is_none());
    assert!(account_details.presentation_identity.is_none());
    assert!(account_details.provider_name.is_none());
    assert!(account_details.icon_name.is_none());
}
#[test]
fn display_limits_and_icons_do_not_authorize_file_access() {
    for icon in [
        "/tmp/private.png",
        "file:///tmp/private.png",
        "https://example.invalid/icon",
        "../icon",
        ". GFile /tmp/private",
    ] {
        let mut account = make_account("one");
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("ProviderIcon".into(), icon.to_variant());
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("PresentationIdentity".into(), "x".repeat(4097).to_variant());
        let list = parse_account_list(&make_account_reply(vec![account])).unwrap();
        let account_details = list.accounts.values().next().unwrap();
        assert!(account_details.icon_name.is_none());
        assert!(account_details.presentation_identity.is_none());
        assert!(account_details.invalid_fields.is_empty());
    }
    let mut account = make_account("one");
    account
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("ProviderIcon".into(), "goa-account-google".to_variant());
    let list = parse_account_list(&make_account_reply(vec![account])).unwrap();
    assert_eq!(
        list.accounts.values().next().unwrap().icon_name.as_deref(),
        Some("goa-account-google")
    );
}
#[test]
fn mail_presence_disable_and_attention_are_separate() {
    for mail_disabled in [false, true] {
        for attention_needed in [false, true] {
            for mail_present in [false, true] {
                let mut account = make_account("one");
                account
                    .get_mut(ACCOUNT_INTERFACE)
                    .unwrap()
                    .insert("MailDisabled".into(), mail_disabled.to_variant());
                account
                    .get_mut(ACCOUNT_INTERFACE)
                    .unwrap()
                    .insert("AttentionNeeded".into(), attention_needed.to_variant());
                if !mail_present {
                    account.remove(MAIL_INTERFACE);
                }
                let list = parse_account_list(&make_account_reply(vec![account])).unwrap();
                let account_details = list.accounts.values().next().unwrap();
                assert_eq!(account_details.mail_disabled, Some(mail_disabled));
                assert_eq!(account_details.attention_needed, Some(attention_needed));
                assert_eq!(account_details.mail_present, mail_present);
            }
        }
    }
}
#[test]
fn empty_protocol_and_record_limits_are_distinct() {
    assert!(
        parse_account_list(&make_account_reply(vec![]))
            .unwrap()
            .membership_confirmed
    );
    assert_eq!(
        parse_account_list(&("invalid",).to_variant())
            .unwrap_err()
            .cause,
        ErrorCause::InvalidReply
    );
    assert_eq!(
        parse_account_list(&make_account_reply(
            (0..4097).map(|i| make_account(&i.to_string())).collect()
        ))
        .unwrap_err()
        .cause,
        ErrorCause::DataLimit
    );
}
#[test]
fn malformed_account_membership_and_duplicate_labels() {
    let mut broken = make_account("one");
    broken.remove(ACCOUNT_INTERFACE);
    assert!(
        !parse_account_list(&make_account_reply(vec![broken]))
            .unwrap()
            .membership_confirmed
    );
    let list = parse_account_list(&make_account_reply(
        (0..30).map(|i| make_account(&i.to_string())).collect(),
    ))
    .unwrap();
    assert!(list.membership_confirmed);
    assert_eq!(list.accounts.len(), 30);
    let debug = format!("{list:?}");
    assert!(!debug.contains("synthetic@example.invalid"));
    assert!(!debug.contains("Synthetic account"));
}

#[test]
fn oversized_required_field_is_an_account_error_and_total_data_is_bounded() {
    let mut broken = make_account("one");
    broken
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("ProviderType".into(), "x".repeat(4097).to_variant());
    let list = parse_account_list(&make_account_reply(vec![broken, make_account("two")])).unwrap();
    assert!(list.membership_confirmed);
    assert_eq!(
        list.accounts
            .values()
            .filter(|account| !account.invalid_fields.is_empty())
            .count(),
        1
    );
    let large = "x".repeat(4096).to_variant();
    let accounts = (0..1100)
        .map(|i| {
            let mut account = make_account(&i.to_string());
            for name in ["ProviderType", "ProviderName", "PresentationIdentity"] {
                account
                    .get_mut(ACCOUNT_INTERFACE)
                    .unwrap()
                    .insert(name.into(), large.clone());
            }
            account
                .get_mut(MAIL_INTERFACE)
                .unwrap()
                .insert("EmailAddress".into(), large.clone());
            account
        })
        .collect();
    assert_eq!(
        parse_account_list(&make_account_reply(accounts))
            .unwrap_err()
            .cause,
        ErrorCause::DataLimit
    );
}
