// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::tests::*;
use crate::{AccountCheckResult, AccountId};
use crate::{test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use glib::variant::ObjectPath;
use std::collections::BTreeMap;
use std::time::Duration;

#[test]
fn all_goa_signals_trigger_full_reads() {
    run_in_context(|| {
        let bus = TestBus::new();
        let initial = make_account_reply(vec![make_account("one")]);
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(initial));
        let (_client, updates) = start_test_client(&bus);
        updates.completed();
        let mut renamed = make_account("one");
        renamed.get_mut(ACCOUNT_INTERFACE).unwrap().insert(
            "PresentationIdentity".into(),
            "Read from reply".to_variant(),
        );
        goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
            renamed.clone(),
        ])));
        // Signal contents are only a trigger; the accepted fact comes from the reply.
        goa.change_properties(
            ACCOUNT_INTERFACE,
            BTreeMap::new(),
            vec!["PresentationIdentity".into()],
        );
        let update = updates.completed();
        assert_eq!(
            update
                .accounts
                .values()
                .next()
                .unwrap()
                .display_name
                .as_deref(),
            Some("Read from reply")
        );
        assert!(!update.retry_pending);
        let path = ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0")).unwrap();
        renamed.remove(MAIL_INTERFACE);
        goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
            renamed.clone(),
        ])));
        goa.emit_signal(
            GOA_ROOT_PATH,
            OBJECT_MANAGER_INTERFACE,
            "InterfacesRemoved",
            &(path.clone(), vec![MAIL_INTERFACE]).to_variant(),
        );
        let update = updates.completed();
        assert!(
            !update
                .accounts
                .values()
                .next()
                .unwrap()
                .mail_service_available
        );
        let restored = make_account("one");
        goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
            restored.clone(),
        ])));
        goa.emit_signal(
            GOA_ROOT_PATH,
            OBJECT_MANAGER_INTERFACE,
            "InterfacesAdded",
            &(
                path,
                BTreeMap::from([(MAIL_INTERFACE, restored[MAIL_INTERFACE].clone())]),
            )
                .to_variant(),
        );
        assert!(
            updates
                .completed()
                .accounts
                .values()
                .next()
                .unwrap()
                .mail_service_available
        );
        assert_eq!(goa.read_count(), 4);
        let replacement = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("replacement")])),
        );
        let update = updates.completed();
        assert!(
            update
                .accounts
                .contains_key(&AccountId::try_from("replacement").unwrap())
        );
        assert_eq!(update.accounts.len(), 1);
        assert_eq!(replacement.read_count(), 1);
    });
}

#[test]
fn signals_during_a_read_request_one_followup_with_the_latest_reply() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Hang);
        let (client, updates) = start_test_client(&bus);
        wait_until(|| goa.read_count() == 1);
        for _ in 0..6 {
            goa.change_properties(
                ACCOUNT_INTERFACE,
                BTreeMap::new(),
                vec!["PresentationIdentity".into()],
            );
        }
        // Dispatch the triggers while the method is pending, before making its snapshot.
        wait_until(|| client.0.refetch_needed.get());
        dispatch_for(Duration::from_millis(30));
        assert_eq!(goa.read_count(), 1);
        let first_reply = make_account_reply(vec![make_account("one")]);
        goa.complete_held_reply(&first_reply);
        assert!(
            updates
                .completed()
                .accounts
                .contains_key(&AccountId::try_from("one").unwrap())
        );
        wait_until(|| goa.read_count() == 2);
        goa.complete_held_reply(&make_account_reply(vec![make_account("latest")]));
        let latest = updates.completed();
        assert_eq!(latest.accounts.len(), 1);
        assert!(
            latest
                .accounts
                .contains_key(&AccountId::try_from("latest").unwrap())
        );
        dispatch_for(Duration::from_millis(30));
        assert_eq!(goa.read_count(), 2);
        assert!(updates.is_empty());
    });
}

#[test]
fn repeated_retry_during_a_read_requests_one_followup() {
    run_in_context(|| {
        let bus = TestBus::new();
        let reply = make_account_reply(vec![make_account("one")]);
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Hang);
        let (client, updates) = start_test_client(&bus);
        wait_until(|| goa.read_count() == 1);
        assert!(!client.0.update.borrow().retry_pending);
        goa.change_properties(ACCOUNT_INTERFACE, BTreeMap::new(), vec![]);
        wait_until(|| client.0.refetch_needed.get());
        assert!(!client.0.update.borrow().retry_pending);
        assert!(updates.is_empty());
        for _ in 0..6 {
            client.refresh_accounts();
        }
        let pending = updates.next();
        assert!(pending.retry_pending);
        assert_eq!(pending.last_check, AccountCheckResult::NotChecked);
        goa.complete_held_reply(&reply);
        let intermediate = updates.next();
        assert!(intermediate.last_check.is_complete());
        assert!(intermediate.retry_pending, "Retry covers the follow-up too");
        wait_until(|| goa.read_count() == 2);
        goa.complete_held_reply(&reply);
        assert!(updates.completed().last_check.is_complete());
        dispatch_for(Duration::from_millis(30));
        assert_eq!(goa.read_count(), 2);
        assert!(updates.is_empty());
    });
}
