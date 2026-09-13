// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::tests::{await_check_result, await_with_timeout, start_test_client};
use crate::{AccountCheckResult, test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use std::collections::BTreeMap;

#[test]
fn unrelated_account_changes_do_not_invalidate_or_publish() {
    let shared = std::sync::Arc::new(crate::SharedClientState::default());
    let mut worker = super::worker_state::GoaWorkerState::new(shared.clone());
    worker.goa_owner = Some(":1.1".into());
    worker.begin_check();
    worker.finish_check(crate::accounts::parse_account_snapshot(
        &make_account_reply(vec![make_account("one")]),
    ));
    shared.lock().update_pending = false;
    let path = format!("{GOA_ROOT_PATH}/Accounts/account_0");
    for (interface, property) in [
        (ACCOUNT_INTERFACE, "CalendarDisabled"),
        (MAIL_INTERFACE, "ImapHost"),
    ] {
        for body in [
            (
                interface,
                BTreeMap::from([(property, true.to_variant())]),
                Vec::<String>::new(),
            )
                .to_variant(),
            (
                interface,
                BTreeMap::<String, glib::Variant>::new(),
                vec![property],
            )
                .to_variant(),
        ] {
            worker.apply_account_signal(":1.1", &path, "PropertiesChanged", &body);
        }
    }
    let object_path = glib::variant::ObjectPath::try_from(path).unwrap();
    let unrelated_interface = "org.gnome.OnlineAccounts.Calendar";
    worker.apply_account_signal(
        ":1.1",
        GOA_ROOT_PATH,
        "InterfacesAdded",
        &(
            &object_path,
            BTreeMap::from([(
                unrelated_interface,
                BTreeMap::<String, glib::Variant>::new(),
            )]),
        )
            .to_variant(),
    );
    worker.apply_account_signal(
        ":1.1",
        GOA_ROOT_PATH,
        "InterfacesRemoved",
        &(&object_path, vec![unrelated_interface]).to_variant(),
    );
    assert_eq!(worker.account_change_number, 0);
    assert!(!worker.recheck_requested);
    assert!(!shared.lock().update_pending);
}

#[test]
fn account_interface_removal_preserves_an_existing_disable_fact() {
    for removed_interfaces in [
        vec![ACCOUNT_INTERFACE],
        vec![ACCOUNT_INTERFACE, MAIL_INTERFACE],
    ] {
        let shared = std::sync::Arc::new(crate::SharedClientState::default());
        let mut worker = super::worker_state::GoaWorkerState::new(shared);
        worker.goa_owner = Some(":1.1".into());
        let mut disabled = make_account("one");
        disabled
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("MailDisabled".into(), true.to_variant());
        worker.finish_check(crate::accounts::parse_account_snapshot(
            &make_account_reply(vec![disabled]),
        ));
        let path =
            glib::variant::ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0"))
                .unwrap();
        worker.apply_account_signal(
            ":1.1",
            GOA_ROOT_PATH,
            "InterfacesRemoved",
            &(path, removed_interfaces).to_variant(),
        );
        worker.begin_check();
        assert_eq!(
            worker
                .account_list
                .accounts
                .values()
                .next()
                .unwrap()
                .mail_enabled,
            Some(false)
        );
        assert_eq!(
            worker.account_list.last_check.error().unwrap().cause,
            crate::ErrorCause::InvalidList
        );
        assert!(worker.account_list.check_pending);
        assert!(worker.account_paths.is_empty());
    }
}

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
        Some(account_model::AccountProvider::Google)
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
    assert!(matches!(failed.last_check, AccountCheckResult::Failed(_)));
    assert!(!failed.last_check.is_complete());
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
            if update.last_check.is_complete() {
                break update;
            }
        }
    });
    assert!(recovered.last_check.is_complete());
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
    // The signal invalidates membership before the full check finishes.
    let failed = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update
                .last_check
                .error()
                .is_some_and(|error| error.cause == crate::ErrorCause::AccessDenied)
            {
                break update;
            }
        }
    });
    assert_eq!(failed.accounts.len(), 1);
    assert!(!failed.last_check.is_complete());
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![])));
    client.refresh_accounts();
    let empty = await_check_result(&mut updates);
    assert!(empty.last_check.is_complete());
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
fn damaged_identity_does_not_attribute_disable_to_a_previous_account() {
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
    assert!(!failed.last_check.is_complete());
    assert_eq!(
        failed.accounts.keys().next(),
        original.accounts.keys().next()
    );
    assert_eq!(
        failed.accounts.values().next().unwrap().mail_enabled,
        Some(true)
    );
    client.stop();
}

#[test]
fn identity_signal_drops_mapping_without_patching_the_previous_account() {
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
        BTreeMap::from([
            ("Id".into(), "two".to_variant()),
            ("MailDisabled".into(), true.to_variant()),
        ]),
        vec![],
    );
    let invalidated = await_with_timeout(updates.next_account_update()).unwrap();
    assert!(!invalidated.last_check.is_complete());
    assert_eq!(invalidated.accounts, original.accounts);
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
            if update.last_check.is_complete()
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
    assert_eq!(
        failed.last_check.error().unwrap().cause,
        crate::ErrorCause::Timeout
    );
    assert!(!failed.last_check.is_complete());
    bus.set_activation_mode("ready");
    client.refresh_accounts();
    let recovered = await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.last_check.is_complete() {
                break update;
            }
        }
    });
    assert_eq!(recovered.accounts.len(), 1);
    assert!(recovered.last_check.is_complete());
    client.stop();
}

#[test]
fn reply_from_timed_out_request_cannot_overwrite_a_later_check() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (client, mut updates) = start_test_client(&bus);
    assert_eq!(
        await_check_result(&mut updates)
            .last_check
            .error()
            .unwrap()
            .cause,
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
fn incomplete_check_does_not_keep_unverified_signal_mappings() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let original = await_check_result(&mut updates);
    let mut missing_id = make_account("damaged");
    missing_id.get_mut(ACCOUNT_INTERFACE).unwrap().remove("Id");
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![missing_id])));
    client.refresh_accounts();
    let partial = await_check_result(&mut updates);
    assert!(!partial.last_check.is_complete());
    assert_eq!(partial.accounts, original.accounts);
    goa.set_reply(ReplyBehavior::AccessDenied);
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("MailDisabled".into(), true.to_variant())]),
        vec![],
    );
    let failed = await_check_result(&mut updates);
    assert_eq!(
        failed.accounts, original.accounts,
        "unverified paths cannot supply disablement facts"
    );
    client.stop();
}
