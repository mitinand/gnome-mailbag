// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

// Exercise the production application rules with the private GOA client. Keeping
// this import in tests avoids exposing private-bus setup in the adapter's API.
#[allow(dead_code)]
#[path = "../../../mailbag/src/accounts.rs"]
mod application_accounts;

use super::tests::{
    await_check_result, await_published_update, await_with_timeout, start_test_client,
};
use crate::account_model::{AccountField, AccountId, ErrorCause};
use crate::{test_bus::TestBus, test_goa::*};
use application_accounts::{
    AccountAvailability, AccountHiddenNotice, AccountList, AccountPage, AccountProblem,
};
use gio::prelude::*;
use glib::{Variant, variant::ObjectPath};
use std::{collections::BTreeMap, time::Duration};

#[test]
fn removed_account_is_unconfirmed_until_the_next_complete_list() {
    for removed_interfaces in [
        vec![ACCOUNT_INTERFACE],
        vec![ACCOUNT_INTERFACE, MAIL_INTERFACE],
    ] {
        let bus = TestBus::new();
        let enabled_reply = make_account_reply(vec![make_account("one"), make_account("two")]);
        let goa = FakeGoaService::new(
            &bus.address,
            vec![ReplyBehavior::Value(enabled_reply.clone())],
        );
        let (client, mut updates) =
            super::start_for_test(bus.address.clone(), Duration::from_secs(2));
        let mut accounts = AccountList::default();
        accounts.apply_update(&await_check_result(&mut updates));
        let selected_id = AccountId::try_from("one").unwrap();
        let surviving_id = AccountId::try_from("two").unwrap();
        let label = accounts.visible_accounts()[&selected_id].label.clone();
        assert!(accounts.select_account(Some(selected_id.clone())));

        goa.set_reply(ReplyBehavior::Hang);
        let path = ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0")).unwrap();
        goa.emit_signal(
            GOA_ROOT_PATH,
            OBJECT_MANAGER_INTERFACE,
            "InterfacesRemoved",
            &(path, removed_interfaces).to_variant(),
        );
        goa.wait_for_calls(2);
        let pending = await_with_timeout(updates.next_account_update()).unwrap();
        assert!(pending.check_pending);
        assert!(accounts.apply_update(&pending).is_none());
        assert_eq!(accounts.visible_accounts().len(), 2);
        assert_eq!(accounts.selected_id(), Some(&selected_id));
        assert_eq!(
            accounts.check_error().unwrap().cause,
            ErrorCause::InvalidList
        );
        assert!(
            accounts
                .visible_accounts()
                .values()
                .all(|row| { row.availability() == AccountAvailability::Unconfirmed })
        );

        let remaining_reply = make_account_reply(vec![make_account("two")]);
        // Wait for the handler to hold this call before changing future replies.
        goa.complete_held_reply(&remaining_reply);
        goa.set_reply(ReplyBehavior::Value(remaining_reply));
        assert_eq!(
            accounts.apply_update(&await_check_result(&mut updates)),
            Some(AccountHiddenNotice::Single(label))
        );
        assert_eq!(accounts.visible_accounts().len(), 1);
        assert!(accounts.selected_id().is_none());
        assert!(accounts.check_error().is_none());
        assert_eq!(accounts.page(), AccountPage::SelectAccount);
        assert_eq!(
            accounts.visible_accounts()[&surviving_id].availability(),
            AccountAvailability::Confirmed
        );

        goa.set_reply(ReplyBehavior::Value(enabled_reply));
        client.refresh_accounts();
        assert!(
            accounts
                .apply_update(&await_check_result(&mut updates))
                .is_none()
        );
        assert_eq!(
            accounts.visible_accounts()[&selected_id].availability(),
            AccountAvailability::Confirmed
        );
        assert!(accounts.selected_id().is_none());
        client.stop();
    }
}

#[test]
fn pending_checks_preserve_individual_problems_and_failure_until_recovery() {
    let bus = TestBus::new();
    let healthy_reply = make_account_reply(vec![make_account("one"), make_account("two")]);
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(healthy_reply.clone())],
    );
    let (client, mut updates) = super::start_for_test(bus.address.clone(), Duration::from_secs(2));
    let mut accounts = AccountList::default();
    accounts.apply_update(&await_check_result(&mut updates));
    let selected_id = AccountId::try_from("one").unwrap();
    let damaged_id = AccountId::try_from("two").unwrap();
    assert!(accounts.select_account(Some(selected_id.clone())));

    let mut damaged = make_account("two");
    damaged
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .remove("AttentionNeeded");
    let damaged_reply = make_account_reply(vec![make_account("one"), damaged]);
    goa.set_reply(ReplyBehavior::Value(damaged_reply.clone()));
    client.refresh_accounts();
    assert!(
        accounts
            .apply_update(&await_check_result(&mut updates))
            .is_none()
    );
    let damaged_problems = accounts.visible_accounts()[&damaged_id].problems.clone();
    assert_eq!(
        damaged_problems,
        vec![AccountProblem::InvalidField(AccountField::Attention)]
    );

    goa.set_reply(ReplyBehavior::Hang);
    client.refresh_accounts();
    goa.wait_for_calls(3);
    accounts.apply_update(&await_with_timeout(updates.next_account_update()).unwrap());
    assert!(accounts.check_pending());
    assert_eq!(
        accounts.visible_accounts()[&selected_id].availability(),
        AccountAvailability::Confirmed
    );
    assert_eq!(
        accounts.visible_accounts()[&damaged_id].problems,
        damaged_problems
    );
    assert_eq!(accounts.selected_id(), Some(&selected_id));
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
    goa.complete_held_reply(&damaged_reply);
    accounts.apply_update(&await_check_result(&mut updates));

    goa.set_reply(ReplyBehavior::AccessDenied);
    client.refresh_accounts();
    assert!(
        accounts
            .apply_update(&await_check_result(&mut updates))
            .is_none()
    );
    assert_eq!(
        accounts.check_error().unwrap().cause,
        ErrorCause::AccessDenied
    );
    assert!(
        accounts
            .visible_accounts()
            .values()
            .all(|row| row.availability() == AccountAvailability::Unconfirmed)
    );

    goa.set_reply(ReplyBehavior::Hang);
    client.refresh_accounts();
    goa.wait_for_calls(5);
    accounts.apply_update(&await_with_timeout(updates.next_account_update()).unwrap());
    assert!(accounts.check_pending());
    assert_eq!(
        accounts.check_error().unwrap().cause,
        ErrorCause::AccessDenied
    );
    assert_eq!(accounts.selected_id(), Some(&selected_id));
    goa.complete_held_reply(&healthy_reply);
    assert!(
        accounts
            .apply_update(&await_check_result(&mut updates))
            .is_none()
    );
    assert!(!accounts.check_pending());
    assert!(accounts.check_error().is_none());
    assert!(
        accounts
            .visible_accounts()
            .values()
            .all(|row| row.problems.is_empty())
    );
    assert_eq!(accounts.selected_id(), Some(&selected_id));
    client.stop();
}

#[test]
fn conflicting_identities_cannot_hide_rows_but_unambiguous_disable_still_applies() {
    // The second record either reuses the first path with a different ID or
    // repeats its ID on another path. Both records must lose their authority.
    for (conflicting_id, path_index) in [("two", 0), ("one", 1)] {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            vec![ReplyBehavior::Value(make_account_reply(vec![
                make_account("one"),
                make_account("two"),
                make_account("other"),
            ]))],
        );
        let (client, mut updates) = start_test_client(&bus);
        let mut accounts = AccountList::default();
        accounts.apply_update(&await_check_result(&mut updates));
        let selected_id = AccountId::try_from("one").unwrap();
        let other_id = AccountId::try_from("other").unwrap();
        let other_label = accounts.visible_accounts()[&other_id].label.clone();
        assert!(accounts.select_account(Some(selected_id.clone())));

        let entries: Vec<_> = [
            ("one", 0, false),
            (conflicting_id, path_index, true),
            ("other", 2, true),
        ]
        .into_iter()
        .map(|(id, index, mail_disabled)| {
            let mut account = make_account(id);
            account
                .get_mut(ACCOUNT_INTERFACE)
                .unwrap()
                .insert("MailDisabled".into(), mail_disabled.to_variant());
            let path =
                ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_{index}")).unwrap();
            BTreeMap::from([(path, account)])
                .to_variant()
                .child_value(0)
        })
        .collect();
        let objects = Variant::array_from_iter_with_type(entries[0].type_(), &entries);
        goa.set_reply(ReplyBehavior::Value(Variant::tuple_from_iter([objects])));
        client.refresh_accounts();
        let update = await_check_result(&mut updates);
        assert_eq!(
            update.last_check.error().unwrap().cause,
            ErrorCause::InvalidList
        );
        assert_eq!(
            accounts.apply_update(&update),
            Some(AccountHiddenNotice::Single(other_label))
        );
        assert_eq!(accounts.visible_accounts().len(), 2);
        assert_eq!(accounts.selected_id(), Some(&selected_id));
        assert_eq!(
            accounts.visible_accounts()[&selected_id].availability(),
            AccountAvailability::Unconfirmed
        );
        assert!(
            accounts
                .visible_accounts()
                .contains_key(&AccountId::try_from("two").unwrap())
        );
        client.stop();
    }
}

#[test]
fn only_applied_exclusions_clear_selection_and_produce_notices() {
    let bus = TestBus::new();
    let enabled_reply = make_account_reply(vec![make_account("one")]);
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(enabled_reply.clone())],
    );
    let (client, mut updates) = start_test_client(&bus);
    let mut accounts = AccountList::default();
    accounts.apply_update(&await_check_result(&mut updates));
    let selected_id = AccountId::try_from("one").unwrap();
    assert!(accounts.select_account(Some(selected_id.clone())));

    // Do not consume the disabled snapshot. A later signal restores the account.
    for mail_disabled in [true, false] {
        goa.change_properties(
            ACCOUNT_INTERFACE,
            BTreeMap::from([("MailDisabled".into(), mail_disabled.to_variant())]),
            vec![],
        );
        await_published_update(&client, |update| {
            update.accounts[&selected_id].mail_enabled == Some(!mail_disabled)
        });
    }
    assert!(
        accounts
            .apply_update(&await_with_timeout(updates.next_account_update()).unwrap())
            .is_none()
    );
    assert_eq!(accounts.selected_id(), Some(&selected_id));

    // Full checks can supersede removal too, before the UI applies any change.
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![])));
    client.refresh_accounts();
    await_published_update(&client, |update| {
        !update.check_pending && update.accounts.is_empty()
    });
    goa.set_reply(ReplyBehavior::Value(enabled_reply));
    client.refresh_accounts();
    await_published_update(&client, |update| {
        !update.check_pending && update.accounts.contains_key(&selected_id)
    });
    assert!(
        accounts
            .apply_update(&await_with_timeout(updates.next_account_update()).unwrap())
            .is_none()
    );
    assert_eq!(accounts.selected_id(), Some(&selected_id));

    // When the UI applies the exclusion, it clears selection and owns one notice.
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("MailDisabled".into(), true.to_variant())]),
        vec![],
    );
    assert_eq!(
        accounts.apply_update(&await_with_timeout(updates.next_account_update()).unwrap()),
        Some(AccountHiddenNotice::Single("Synthetic account".into()))
    );
    assert!(accounts.selected_id().is_none());
    assert!(accounts.visible_accounts().is_empty());
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("MailDisabled".into(), false.to_variant())]),
        vec![],
    );
    assert!(
        accounts
            .apply_update(&await_with_timeout(updates.next_account_update()).unwrap())
            .is_none()
    );
    assert_eq!(accounts.visible_accounts().len(), 1);
    assert!(accounts.selected_id().is_none());

    assert!(accounts.select_account(Some(selected_id)));
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![])));
    client.refresh_accounts();
    assert_eq!(
        accounts.apply_update(&await_check_result(&mut updates)),
        Some(AccountHiddenNotice::Single("Synthetic account".into()))
    );
    assert!(accounts.selected_id().is_none());
    assert_eq!(accounts.page(), AccountPage::NoAccounts);
    client.stop();
}
