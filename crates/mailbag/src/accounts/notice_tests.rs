// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{account_tests::*, accounts::*};
use goa_adapter::AccountProvider;

#[test]
fn explicit_disable_wins_over_unrelated_errors_and_reappearance_does_not_select() {
    let mut accounts = AccountList::default();
    let enabled = make_checked_list(&[("one", make_account_details(AccountProvider::Google))]);
    accounts.apply_update(&enabled);
    accounts.select_account(Some(make_account_id("one")));
    let mut disabled = make_failed_list();
    let mut details = make_account_details(AccountProvider::Google);
    details.mail_enabled = Some(false);
    details.provider = None;
    details.needs_attention = None;
    disabled.accounts.insert(make_account_id("one"), details);
    assert_eq!(
        accounts.apply_update(&disabled),
        Some(AccountHiddenNotice::Single("Synthetic account".into()))
    );
    assert!(accounts.selected_id().is_none());
    assert!(accounts.visible_accounts().is_empty());
    assert_eq!(accounts.page(), AccountPage::Unavailable);
    assert!(accounts.apply_update(&disabled).is_none());
    assert!(accounts.apply_update(&make_failed_list()).is_none());
    assert!(accounts.apply_update(&enabled).is_none());
    assert!(accounts.selected_id().is_none());
    assert_eq!(accounts.page(), AccountPage::SelectAccount);
    assert!(accounts.apply_update(&make_checked_list(&[])).is_some());
    assert_eq!(accounts.page(), AccountPage::NoAccounts);
}

#[test]
fn removal_and_disable_share_one_notice_including_unselected_accounts() {
    let mut accounts = AccountList::default();
    let enabled = make_checked_list(&[
        ("one", make_account_details(AccountProvider::Google)),
        ("two", make_account_details(AccountProvider::ImapSmtp)),
        ("three", make_account_details(AccountProvider::Microsoft365)),
    ]);
    accounts.apply_update(&enabled);
    accounts.select_account(Some(make_account_id("three")));
    let mut update = enabled;
    update.accounts.remove(&make_account_id("one"));
    update
        .accounts
        .get_mut(&make_account_id("two"))
        .unwrap()
        .mail_enabled = Some(false);
    assert_eq!(
        accounts.apply_update(&update),
        Some(AccountHiddenNotice::Group(2))
    );
    assert_eq!(accounts.selected_id(), Some(&make_account_id("three")));
    assert!(accounts.apply_update(&update).is_none());
}

#[test]
fn cold_start_and_incomplete_absence_never_produce_removal_notices() {
    let mut accounts = AccountList::default();
    let mut disabled = make_account_details(AccountProvider::Google);
    disabled.mail_enabled = Some(false);
    assert!(
        accounts
            .apply_update(&make_checked_list(&[("one", disabled)]))
            .is_none()
    );
    assert!(accounts.apply_update(&make_checked_list(&[])).is_none());
    accounts.apply_update(&make_checked_list(&[(
        "one",
        make_account_details(AccountProvider::Google),
    )]));
    assert!(accounts.apply_update(&make_failed_list()).is_none());
    assert_eq!(accounts.visible_accounts().len(), 1);
    let mut restarted = AccountList::default();
    assert!(restarted.apply_update(&make_failed_list()).is_none());
    assert!(restarted.visible_accounts().is_empty());
}

#[test]
fn single_notice_uses_the_displayed_numbered_label() {
    let mut accounts = AccountList::default();
    let enabled = make_checked_list(&[
        ("one", make_account_details(AccountProvider::Google)),
        ("two", make_account_details(AccountProvider::Google)),
    ]);
    accounts.apply_update(&enabled);
    let label = accounts.visible_accounts()[&make_account_id("two")]
        .label
        .clone();
    let mut update = enabled;
    update.accounts.remove(&make_account_id("two"));
    assert_eq!(
        accounts.apply_update(&update),
        Some(AccountHiddenNotice::Single(label))
    );
}
