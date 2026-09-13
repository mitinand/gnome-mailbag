// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::tests::{await_check_result, await_with_timeout, start_test_client};
use crate::{CheckStatus, test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use std::collections::BTreeMap;

#[test]
fn changed_fields_apply_while_full_check_is_hanging() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    await_check_result(&mut updates);
    goa.set_reply(ReplyBehavior::Hang);
    client.refresh_accounts();
    goa.wait_for_calls(2);
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([
            ("MailDisabled".into(), true.to_variant()),
            ("ProviderType".into(), "google".to_variant()),
            ("AttentionNeeded".into(), true.to_variant()),
            ("ProviderName".into(), "Synthetic provider".to_variant()),
            ("PresentationIdentity".into(), "New label".to_variant()),
            ("ProviderIcon".into(), "mail-unread-symbolic".to_variant()),
        ]),
        vec![],
    );
    let update = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.accounts.values().next().unwrap().mail_enabled == Some(false) {
                break update;
            }
        }
    });
    let account = update.accounts.values().next().unwrap();
    assert_eq!(
        account.provider,
        Some(account_source::AccountProvider::Google)
    );
    assert_eq!(account.needs_attention, Some(true));
    assert_eq!(account.provider_name.as_deref(), Some("Synthetic provider"));
    assert_eq!(account.display_name.as_deref(), Some("New label"));
    assert_eq!(account.icon_name.as_deref(), Some("mail-unread-symbolic"));
    goa.change_properties(
        MAIL_INTERFACE,
        BTreeMap::from([(
            "EmailAddress".into(),
            "changed@example.invalid".to_variant(),
        )]),
        vec![],
    );
    let update = await_with_timeout(updates.next_account_update()).unwrap();
    assert_eq!(
        update
            .accounts
            .values()
            .next()
            .unwrap()
            .email_address
            .as_deref(),
        Some("changed@example.invalid")
    );
    client.stop();
}

#[test]
fn invalidated_required_field_becomes_unknown_before_recheck_finishes() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    await_check_result(&mut updates);
    goa.set_reply(ReplyBehavior::Hang);
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::new(),
        vec!["MailDisabled".into()],
    );
    let update = await_with_timeout(updates.next_account_update()).unwrap();
    assert_eq!(update.accounts.values().next().unwrap().mail_enabled, None);
    goa.wait_for_calls(2);
    client.stop();
}

#[test]
fn owner_loss_retains_accounts_and_replacement_recovers() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let original = await_check_result(&mut updates);
    drop(goa);
    let failed = await_check_result(&mut updates);
    assert_eq!(failed.status, CheckStatus::Failed);
    assert!(!failed.membership_confirmed);
    assert_eq!(failed.accounts, original.accounts);
    let _replacement = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("two"),
        ]))],
    );
    let recovered = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.status == CheckStatus::Ready {
                break update;
            }
        }
    });
    assert!(recovered.membership_confirmed);
    assert_ne!(
        original.accounts.keys().next(),
        recovered.accounts.keys().next()
    );
    client.stop();
}

#[test]
fn mail_removal_and_disable_work_in_both_orders_without_false_account_removal() {
    for disable_first in [false, true] {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            vec![ReplyBehavior::Value(make_account_reply(vec![
                make_account("one"),
            ]))],
        );
        let (client, mut updates) = start_test_client(&bus);
        await_check_result(&mut updates);
        goa.set_reply(ReplyBehavior::Hang);
        let disable_mail = || {
            goa.change_properties(
                ACCOUNT_INTERFACE,
                BTreeMap::from([("MailDisabled".into(), true.to_variant())]),
                vec![],
            )
        };
        let remove_mail = || {
            goa.emit_signal(
                GOA_ROOT_PATH,
                OBJECT_MANAGER_INTERFACE,
                "InterfacesRemoved",
                &(
                    glib::variant::ObjectPath::try_from(format!(
                        "{GOA_ROOT_PATH}/Accounts/account_0"
                    ))
                    .unwrap(),
                    vec![MAIL_INTERFACE],
                )
                    .to_variant(),
            )
        };
        if disable_first {
            disable_mail();
            remove_mail();
        } else {
            remove_mail();
            disable_mail();
        }
        let update = await_with_timeout(async {
            loop {
                let update = updates.next_account_update().await.unwrap();
                let account = update.accounts.values().next().unwrap();
                if account.mail_enabled == Some(false) && !account.mail_service_available {
                    break update;
                }
            }
        });
        assert_eq!(update.accounts.len(), 1);
        client.stop();
    }
}

#[test]
fn account_removal_requires_complete_list_and_interfaces_added_restore_it() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    await_check_result(&mut updates);
    let path =
        glib::variant::ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0")).unwrap();
    goa.set_reply(ReplyBehavior::AccessDenied);
    goa.emit_signal(
        GOA_ROOT_PATH,
        OBJECT_MANAGER_INTERFACE,
        "InterfacesRemoved",
        &(path.clone(), vec![ACCOUNT_INTERFACE]).to_variant(),
    );
    let failed = await_check_result(&mut updates);
    assert_eq!(failed.accounts.len(), 1);
    assert!(!failed.membership_confirmed);
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![])));
    client.refresh_accounts();
    let empty = await_check_result(&mut updates);
    assert!(empty.membership_confirmed);
    assert!(empty.accounts.is_empty());
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        make_account("one"),
    ])));
    goa.emit_signal(
        GOA_ROOT_PATH,
        OBJECT_MANAGER_INTERFACE,
        "InterfacesAdded",
        &(path, make_account("one")).to_variant(),
    );
    assert_eq!(await_check_result(&mut updates).accounts.len(), 1);
    client.stop();
}

#[test]
fn damaged_identity_on_known_path_preserves_disable_fact_and_does_not_confirm_absence() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let original = await_check_result(&mut updates);
    let mut damaged_account = make_account("one");
    let fields = damaged_account.get_mut(ACCOUNT_INTERFACE).unwrap();
    fields.remove("Id");
    fields.insert("MailDisabled".into(), true.to_variant());
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        damaged_account,
    ])));
    client.refresh_accounts();
    let failed = await_check_result(&mut updates);
    assert!(!failed.membership_confirmed);
    assert_eq!(
        failed.accounts.keys().next(),
        original.accounts.keys().next()
    );
    assert_eq!(
        failed.accounts.values().next().unwrap().mail_enabled,
        Some(false)
    );
    client.stop();
}

#[test]
fn identity_signal_requires_full_check_and_optional_invalidations_clear_display() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let original = await_check_result(&mut updates);
    goa.set_reply(ReplyBehavior::Hang);
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("Id".into(), "two".to_variant())]),
        vec![
            "PresentationIdentity".into(),
            "ProviderName".into(),
            "ProviderIcon".into(),
            "ProviderType".into(),
            "AttentionNeeded".into(),
        ],
    );
    let invalidated = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.accounts.values().next().unwrap().provider.is_none() {
                break update;
            }
        }
    });
    assert_eq!(
        invalidated.accounts.keys().next(),
        original.accounts.keys().next()
    );
    assert!(!invalidated.membership_confirmed);
    let account = invalidated.accounts.values().next().unwrap();
    assert!(account.display_name.is_none());
    assert!(account.needs_attention.is_none());
    assert_eq!(account.invalid_fields().len(), 2);
    goa.wait_for_calls(2);
    client.stop();
}

#[test]
fn obsolete_owner_reply_and_later_signals_cannot_restore_old_accounts() {
    let bus = TestBus::new();
    let old_goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("old"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let original = await_check_result(&mut updates);
    old_goa.set_reply(ReplyBehavior::Hang);
    client.refresh_accounts();
    old_goa.wait_for_calls(2);
    let _new_goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("new"),
        ]))],
    );
    old_goa.complete_held_reply(&make_account_reply(vec![make_account("old")]));
    let recovered = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.status == CheckStatus::Ready
                && update.accounts.keys().next() != original.accounts.keys().next()
            {
                break update;
            }
        }
    });
    old_goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("MailDisabled".into(), true.to_variant())]),
        vec![],
    );
    client.refresh_accounts();
    let checked = await_check_result(&mut updates);
    assert_eq!(checked.accounts, recovered.accounts);
    client.stop();
}

#[test]
fn late_reply_from_stopped_client_does_not_affect_new_client_of_same_process() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (old_client, mut old_updates) = start_test_client(&bus);
    goa.wait_for_calls(1);
    old_client.stop();
    await_with_timeout(async { while old_updates.next_account_update().await.is_some() {} });
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        make_account("new"),
    ])));
    let (new_client, mut new_updates) = start_test_client(&bus);
    let expected = await_check_result(&mut new_updates);
    goa.complete_held_reply(&make_account_reply(vec![make_account("stale")]));
    new_client.refresh_accounts();
    assert_eq!(
        await_check_result(&mut new_updates).accounts,
        expected.accounts
    );
    assert!(await_with_timeout(old_updates.next_account_update()).is_none());
    new_client.stop();
}

#[test]
fn stalled_activation_times_out_and_manual_retry_permits_fresh_activation() {
    let bus = TestBus::with_activation();
    let (client, mut updates) = start_test_client(&bus);
    let failed = await_check_result(&mut updates);
    assert_eq!(failed.error.unwrap().cause, crate::ErrorCause::Timeout);
    assert!(!failed.membership_confirmed);
    bus.set_activation_mode("ready");
    client.refresh_accounts();
    let recovered = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.status == CheckStatus::Ready {
                break update;
            }
        }
    });
    assert_eq!(recovered.accounts.len(), 1);
    assert!(recovered.membership_confirmed);
    client.stop();
}

#[test]
fn reply_from_timed_out_request_cannot_overwrite_a_later_check() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (client, mut updates) = start_test_client(&bus);
    assert_eq!(
        await_check_result(&mut updates).error.unwrap().cause,
        crate::ErrorCause::Timeout
    );
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        make_account("current"),
    ])));
    client.refresh_accounts();
    let current = await_check_result(&mut updates);
    goa.complete_held_reply(&make_account_reply(vec![make_account("obsolete")]));
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("AttentionNeeded".into(), true.to_variant())]),
        vec![],
    );
    let changed = await_with_timeout(updates.next_account_update()).unwrap();
    assert_eq!(
        changed.accounts.keys().next(),
        current.accounts.keys().next()
    );
    assert_eq!(
        changed.accounts.values().next().unwrap().needs_attention,
        Some(true)
    );
    client.stop();
}

#[test]
fn partial_check_keeps_known_paths_for_immediate_disable() {
    for missing_interface in [true, false] {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            vec![ReplyBehavior::Value(make_account_reply(vec![
                make_account("one"),
            ]))],
        );
        let (client, mut updates) = start_test_client(&bus);
        let original = await_check_result(&mut updates);
        let mut partial_objects =
            make_object_map(vec![make_account("one"), make_account("damaged")]);
        let first_path =
            glib::variant::ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0"))
                .unwrap();
        if missing_interface {
            partial_objects
                .get_mut(&first_path)
                .unwrap()
                .remove(ACCOUNT_INTERFACE);
        } else {
            partial_objects.remove(&first_path);
        }
        let second_path =
            glib::variant::ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_1"))
                .unwrap();
        partial_objects
            .get_mut(&second_path)
            .unwrap()
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .remove("Id");
        goa.set_reply(ReplyBehavior::Value((partial_objects,).to_variant()));
        client.refresh_accounts();
        let partial = await_check_result(&mut updates);
        assert!(!partial.membership_confirmed);
        assert_eq!(partial.accounts, original.accounts);
        goa.set_reply(ReplyBehavior::Hang);
        goa.change_properties(
            ACCOUNT_INTERFACE,
            BTreeMap::from([("MailDisabled".into(), true.to_variant())]),
            vec![],
        );
        let disabled = await_with_timeout(async {
            loop {
                let update = updates.next_account_update().await.unwrap();
                if update.accounts.values().next().unwrap().mail_enabled == Some(false) {
                    break update;
                }
            }
        });
        assert!(!disabled.membership_confirmed);
        client.stop();
    }
}
