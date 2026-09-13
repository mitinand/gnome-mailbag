// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::test_goa::{ACCOUNT_INTERFACE, MAIL_INTERFACE, make_account, make_account_reply};
use account_model::AccountField;

#[test]
fn conflicting_paths_cannot_supply_account_facts() {
    use crate::test_goa::make_object_map;
    let mut disabled = make_account("one");
    disabled
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("MailDisabled".into(), true.to_variant());
    let first = make_object_map(vec![disabled]).to_variant().child_value(0);
    let second = make_object_map(vec![make_account("two")])
        .to_variant()
        .child_value(0);
    let duplicated = Variant::array_from_iter_with_type(first.type_(), [&first, &second]);
    let snapshot = parse_account_snapshot(&Variant::tuple_from_iter([duplicated])).unwrap();
    assert!(snapshot.list_error.is_some());
    assert!(
        snapshot.accounts.is_empty(),
        "neither conflicting record can identify an account"
    );
}

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
            let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
            assert!(snapshot.list_error.is_none());
            let account_details = snapshot.accounts.values().next().unwrap();
            assert!(!account_details.invalid_fields().is_empty());
            if name != "MailDisabled" {
                assert_eq!(account_details.mail_enabled, Some(false));
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
        let snapshot =
            parse_account_snapshot(&make_account_reply(vec![account, make_account("two")]))
                .unwrap();
        assert!(snapshot.list_error.is_some());
        assert_eq!(snapshot.accounts.len(), 1);
    }
    let snapshot = parse_account_snapshot(&make_account_reply(vec![
        make_account("same"),
        make_account("same"),
    ]))
    .unwrap();
    assert!(snapshot.list_error.is_some());
    assert!(snapshot.accounts.is_empty());
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
    let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
    let account_details = snapshot.accounts.values().next().unwrap();
    assert!(account_details.invalid_fields().is_empty());
    assert!(account_details.email_address.is_none());
    assert!(account_details.display_name.is_none());
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
        let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
        let account_details = snapshot.accounts.values().next().unwrap();
        assert!(account_details.icon_name.is_none());
        assert!(account_details.display_name.is_none());
        assert!(account_details.invalid_fields().is_empty());
    }
    let mut account = make_account("one");
    account
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("ProviderIcon".into(), "goa-account-google".to_variant());
    let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
    assert_eq!(
        snapshot
            .accounts
            .values()
            .next()
            .unwrap()
            .icon_name
            .as_deref(),
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
                let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
                let account_details = snapshot.accounts.values().next().unwrap();
                assert_eq!(account_details.mail_enabled, Some(!mail_disabled));
                assert_eq!(account_details.needs_attention, Some(attention_needed));
                assert_eq!(account_details.mail_service_available, mail_present);
            }
        }
    }
}
#[test]
fn empty_protocol_and_record_limits_are_distinct() {
    assert!(
        parse_account_snapshot(&make_account_reply(vec![]))
            .unwrap()
            .list_error
            .is_none()
    );
    assert_eq!(
        parse_account_snapshot(&("invalid",).to_variant())
            .err()
            .expect("invalid snapshot accepted")
            .cause,
        ErrorCause::InvalidReply
    );
    assert_eq!(
        parse_account_snapshot(&make_account_reply(
            (0..4097).map(|i| make_account(&i.to_string())).collect()
        ))
        .err()
        .expect("invalid snapshot accepted")
        .cause,
        ErrorCause::DataLimit
    );
}
#[test]
fn malformed_account_membership_and_duplicate_labels() {
    let mut broken = make_account("one");
    broken.remove(ACCOUNT_INTERFACE);
    assert!(
        parse_account_snapshot(&make_account_reply(vec![broken]))
            .unwrap()
            .list_error
            .is_some()
    );
    let snapshot = parse_account_snapshot(&make_account_reply(
        (0..30).map(|i| make_account(&i.to_string())).collect(),
    ))
    .unwrap();
    assert!(snapshot.list_error.is_none());
    assert_eq!(snapshot.accounts.len(), 30);
    let debug = format!("{:?}", snapshot.accounts);
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
    let snapshot =
        parse_account_snapshot(&make_account_reply(vec![broken, make_account("two")])).unwrap();
    assert!(snapshot.list_error.is_none());
    assert_eq!(
        snapshot
            .accounts
            .values()
            .filter(|account| !account.invalid_fields().is_empty())
            .count(),
        1
    );
    let large = "x".repeat(4096).to_variant();
    let accounts = (0..1100)
        .map(|i| {
            let mut account = make_account(&i.to_string());
            for name in ["ProviderIcon", "ProviderName", "PresentationIdentity"] {
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
        parse_account_snapshot(&make_account_reply(accounts))
            .err()
            .expect("invalid snapshot accepted")
            .cause,
        ErrorCause::DataLimit
    );
}

#[test]
fn invalid_identity_records_have_no_signal_mapping() {
    let mut missing_id = make_account("damaged");
    missing_id.get_mut(ACCOUNT_INTERFACE).unwrap().remove("Id");
    let snapshot = parse_account_snapshot(&make_account_reply(vec![
        make_account("one"),
        make_account("one"),
        missing_id,
        make_account("valid"),
    ]))
    .unwrap();
    assert!(snapshot.list_error.is_some());
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(snapshot.account_paths.len(), 1);
    assert!(
        snapshot
            .accounts
            .contains_key(&AccountId::try_from("valid").unwrap())
    );
}

#[test]
fn goa_provider_keys_translate_without_deciding_application_support() {
    for (key, expected) in [
        ("imap_smtp", AccountProvider::ImapSmtp),
        ("google", AccountProvider::Google),
        ("ms_graph", AccountProvider::Microsoft365),
        ("windows_live", AccountProvider::Other),
        ("microsoft", AccountProvider::Other),
        ("outlook", AccountProvider::Other),
        ("MS_GRAPH", AccountProvider::Other),
        ("exchange", AccountProvider::Other),
        ("unknown", AccountProvider::Other),
    ] {
        let mut account = make_account("one");
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("ProviderType".into(), key.to_variant());
        let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
        let details = snapshot.accounts.values().next().unwrap();
        assert_eq!(details.provider, Some(expected));
        assert!(details.invalid_fields().is_empty());
    }
    for invalid in [
        "".to_variant(),
        7i32.to_variant(),
        "x".repeat(4097).to_variant(),
    ] {
        let mut account = make_account("one");
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("ProviderType".into(), invalid);
        let snapshot = parse_account_snapshot(&make_account_reply(vec![account])).unwrap();
        assert!(snapshot.list_error.is_none());
        assert_eq!(snapshot.accounts.values().next().unwrap().provider, None);
    }
}

#[test]
fn property_signals_use_the_same_translation_as_full_replies() {
    let mut snapshot =
        parse_account_snapshot(&make_account_reply(vec![make_account("one")])).unwrap();
    let account = snapshot.accounts.values_mut().next().unwrap();
    let changed = BTreeMap::from([
        ("ProviderType", "ms_graph".to_variant()),
        ("MailDisabled", true.to_variant()),
        ("AttentionNeeded", true.to_variant()),
    ])
    .to_variant();
    apply_account_properties(
        account,
        ACCOUNT_INTERFACE,
        &changed,
        &Vec::<String>::new().to_variant(),
    );
    assert_eq!(account.provider, Some(AccountProvider::Microsoft365));
    assert_eq!(account.mail_enabled, Some(false));
    assert_eq!(account.needs_attention, Some(true));
    assert!(account.mail_service_available);
    apply_account_properties(
        account,
        ACCOUNT_INTERFACE,
        &BTreeMap::<String, Variant>::new().to_variant(),
        &vec!["ProviderType", "MailDisabled", "AttentionNeeded"].to_variant(),
    );
    assert_eq!(
        account.invalid_fields(),
        vec![
            AccountField::Provider,
            AccountField::MailEnabled,
            AccountField::Attention
        ]
    );
}

#[test]
fn glib_errors_keep_safe_diagnostics_without_remote_messages() {
    for (source_cause, expected) in [
        (gio::DBusError::AccessDenied, ErrorCause::AccessDenied),
        (gio::DBusError::NoReply, ErrorCause::Timeout),
        (gio::DBusError::InvalidArgs, ErrorCause::InvalidReply),
        (gio::DBusError::ServiceUnknown, ErrorCause::Unavailable),
    ] {
        let source = glib::Error::new(source_cause, "synthetic-private-error-detail");
        let code = source.code();
        let mapped = map_glib_error("GetManagedObjects", source);
        assert_eq!(mapped.cause, expected);
        assert_eq!(mapped.operation, "GetManagedObjects");
        assert_eq!(mapped.domain.as_deref(), Some("g-dbus-error-quark"));
        assert_eq!(mapped.code, Some(code));
        assert!(!format!("{mapped:?}").contains("synthetic-private-error-detail"));
    }
    let mapped = map_glib_error(
        "connect",
        glib::Error::new(glib::FileError::Failed, "private detail"),
    );
    assert!(mapped.domain.is_none());
    assert!(!format!("{mapped:?}").contains("private detail"));
}
