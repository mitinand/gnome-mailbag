// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{account_tests::*, accounts::*};
use goa_adapter::AccountProvider;

#[test]
fn removal_and_disable_notify_once_and_superseded_toggles_stay_silent() {
    let enabled = make_checked_list(&[
        ("one", make_account_details(AccountProvider::Google)),
        ("two", make_account_details(AccountProvider::ImapSmtp)),
    ]);
    for selected in ["one", "two"] {
        let mut accounts = AccountList::default();
        accounts.apply_update(&enabled);
        accounts.select_account(make_account_id(selected));
        let expected: Vec<_> = accounts
            .visible_accounts()
            .values()
            .map(|row| AccountHiddenNotice {
                label: row.label.clone(),
            })
            .collect();
        let mut update = enabled.clone();
        update.accounts.remove(&make_account_id("one"));
        update
            .accounts
            .get_mut(&make_account_id("two"))
            .unwrap()
            .mail_enabled = false;
        // A toggle replaced before apply_update never reaches the displayed list.
        let mut superseded = update.clone();
        superseded.accounts = enabled.accounts.clone();
        assert!(accounts.apply_update(&superseded).is_empty());
        assert_eq!(accounts.selected_id(), Some(&make_account_id(selected)));
        assert_eq!(accounts.apply_update(&update), expected);
        assert!(accounts.selected_id().is_none());
        assert!(accounts.visible_accounts().is_empty());
        assert!(accounts.apply_update(&update).is_empty());
        assert!(accounts.apply_update(&enabled).is_empty());
        assert!(accounts.selected_id().is_none());
    }
}
