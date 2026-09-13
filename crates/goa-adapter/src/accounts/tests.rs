// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::test_goa::{ACCOUNT, MAIL, account, reply};

#[test]
fn required_fields_are_not_defaulted_and_disable_is_independent() {
    for name in ["ProviderType", "MailDisabled", "AttentionNeeded"] {
        for replacement in [None, Some(42u32.to_variant())] {
            let mut a = account("one");
            let fields = a.get_mut(ACCOUNT).unwrap();
            fields.insert("MailDisabled".into(), true.to_variant());
            match replacement {
                None => {
                    fields.remove(name);
                }
                Some(v) => {
                    fields.insert(name.into(), v);
                }
            }
            let list = parse_list(&reply(vec![a])).unwrap();
            assert!(list.complete);
            let row = list.accounts.values().next().unwrap();
            assert!(!row.problems.is_empty());
            if name != "MailDisabled" {
                assert_eq!(row.mail_disabled, Some(true));
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
        let mut a = account("one");
        let fields = a.get_mut(ACCOUNT).unwrap();
        fields.remove("Id");
        if let Some(value) = value {
            fields.insert("Id".into(), value);
        }
        let list = parse_list(&reply(vec![a, account("two")])).unwrap();
        assert!(!list.complete);
        assert_eq!(list.accounts.len(), 1);
    }
    let list = parse_list(&reply(vec![account("same"), account("same")])).unwrap();
    assert!(!list.complete);
    assert!(list.accounts.is_empty());
}
#[test]
fn optional_fields_fall_back_without_changing_availability() {
    let mut a = account("one");
    for name in ["ProviderName", "PresentationIdentity", "ProviderIcon"] {
        a.get_mut(ACCOUNT)
            .unwrap()
            .insert(name.into(), 12u32.to_variant());
    }
    a.get_mut(MAIL)
        .unwrap()
        .insert("EmailAddress".into(), "".to_variant());
    let list = parse_list(&reply(vec![a])).unwrap();
    let row = list.accounts.values().next().unwrap();
    assert!(row.problems.is_empty());
    assert!(row.email_address.is_none());
    assert!(row.presentation_identity.is_none());
    assert!(row.provider_name.is_none());
    assert!(row.icon_name.is_none());
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
        let mut a = account("one");
        a.get_mut(ACCOUNT)
            .unwrap()
            .insert("ProviderIcon".into(), icon.to_variant());
        a.get_mut(ACCOUNT)
            .unwrap()
            .insert("PresentationIdentity".into(), "x".repeat(4097).to_variant());
        let list = parse_list(&reply(vec![a])).unwrap();
        let row = list.accounts.values().next().unwrap();
        assert!(row.icon_name.is_none());
        assert!(row.presentation_identity.is_none());
        assert!(row.problems.is_empty());
    }
    let mut a = account("one");
    a.get_mut(ACCOUNT)
        .unwrap()
        .insert("ProviderIcon".into(), "goa-account-google".to_variant());
    let list = parse_list(&reply(vec![a])).unwrap();
    assert_eq!(
        list.accounts.values().next().unwrap().icon_name.as_deref(),
        Some("goa-account-google")
    );
}
#[test]
fn mail_presence_disable_and_attention_are_separate() {
    for disabled in [false, true] {
        for attention in [false, true] {
            for mail in [false, true] {
                let mut a = account("one");
                a.get_mut(ACCOUNT)
                    .unwrap()
                    .insert("MailDisabled".into(), disabled.to_variant());
                a.get_mut(ACCOUNT)
                    .unwrap()
                    .insert("AttentionNeeded".into(), attention.to_variant());
                if !mail {
                    a.remove(MAIL);
                }
                let list = parse_list(&reply(vec![a])).unwrap();
                let row = list.accounts.values().next().unwrap();
                assert_eq!(row.mail_disabled, Some(disabled));
                assert_eq!(row.attention_needed, Some(attention));
                assert_eq!(row.mail_present, mail);
            }
        }
    }
}
#[test]
fn empty_protocol_and_record_limits_are_distinct() {
    assert!(parse_list(&reply(vec![])).unwrap().complete);
    assert_eq!(
        parse_list(&("invalid",).to_variant()).unwrap_err().cause,
        ErrorCause::InvalidReply
    );
    assert_eq!(
        parse_list(&reply((0..4097).map(|i| account(&i.to_string())).collect()))
            .unwrap_err()
            .cause,
        ErrorCause::DataLimit
    );
}
#[test]
fn malformed_account_membership_and_duplicate_labels() {
    let mut broken = account("one");
    broken.remove(ACCOUNT);
    assert!(!parse_list(&reply(vec![broken])).unwrap().complete);
    let list = parse_list(&reply((0..30).map(|i| account(&i.to_string())).collect())).unwrap();
    assert!(list.complete);
    assert_eq!(list.accounts.len(), 30);
    let debug = format!("{list:?}");
    assert!(!debug.contains("synthetic@example.invalid"));
    assert!(!debug.contains("Synthetic account"));
}

#[test]
fn oversized_required_field_is_an_account_error_and_total_data_is_bounded() {
    let mut broken = account("one");
    broken
        .get_mut(ACCOUNT)
        .unwrap()
        .insert("ProviderType".into(), "x".repeat(4097).to_variant());
    let list = parse_list(&reply(vec![broken, account("two")])).unwrap();
    assert!(list.complete);
    assert_eq!(
        list.accounts
            .values()
            .filter(|a| !a.problems.is_empty())
            .count(),
        1
    );
    let large = "x".repeat(4096).to_variant();
    let accounts = (0..1100)
        .map(|i| {
            let mut a = account(&i.to_string());
            for name in ["ProviderType", "ProviderName", "PresentationIdentity"] {
                a.get_mut(ACCOUNT)
                    .unwrap()
                    .insert(name.into(), large.clone());
            }
            a.get_mut(MAIL)
                .unwrap()
                .insert("EmailAddress".into(), large.clone());
            a
        })
        .collect();
    assert_eq!(
        parse_list(&reply(accounts)).unwrap_err().cause,
        ErrorCause::DataLimit
    );
}
