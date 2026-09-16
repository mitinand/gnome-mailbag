// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::accounts::*;
use goa_adapter::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountId, AccountProvider,
    AccountUpdate, ErrorCause,
};
use std::collections::BTreeSet;

pub(super) fn make_account_id(id: &str) -> AccountId {
    AccountId::try_from(id).unwrap()
}

pub(super) fn make_account_details(provider: AccountProvider) -> AccountDetails {
    AccountDetails {
        provider,
        mail_enabled: true,
        needs_attention: false,
        mail_service_available: true,
        display_name: Some("Synthetic account".into()),
        email_address: None,
    }
}

pub(super) fn make_checked_list(accounts: &[(&str, AccountDetails)]) -> AccountUpdate {
    AccountUpdate {
        accounts: accounts
            .iter()
            .map(|(id, details)| (make_account_id(id), details.clone()))
            .collect(),
        last_check: AccountCheckResult::Complete,
        retry_pending: false,
    }
}

pub(super) fn make_failed_list(accepted: &AccountUpdate) -> AccountUpdate {
    AccountUpdate {
        last_check: AccountCheckResult::Failed(AccountCheckError::new(
            "read accounts",
            goa_adapter::ErrorCause::Timeout,
        )),
        ..accepted.clone()
    }
}

#[test]
fn recognized_providers_define_support_without_imap_or_address_requirements() {
    for provider in [
        AccountProvider::ImapSmtp,
        AccountProvider::Google,
        AccountProvider::Microsoft365,
    ] {
        let mut accounts = AccountList::default();
        accounts.apply_update(&make_checked_list(&[(
            "one",
            make_account_details(provider),
        )]));
        assert_eq!(accounts.visible_accounts().len(), 1);
        assert!(
            accounts.visible_accounts()[&make_account_id("one")]
                .problems
                .is_empty()
        );
        assert_eq!(accounts.page(), AccountPage::SelectAccount);
        assert!(accounts.selected_id().is_none());
        let mut details = make_account_details(provider);
        details.mail_service_available = false;
        assert!(
            accounts
                .apply_update(&make_checked_list(&[("one", details.clone())]))
                .is_empty()
        );
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("one")].problems,
            vec![AccountProblem::MailUnavailable]
        );
        details.mail_enabled = false;
        accounts.apply_update(&make_checked_list(&[("one", details)]));
        assert!(accounts.visible_accounts().is_empty());
    }
    for mail_enabled in [true, false] {
        let mut details = make_account_details(AccountProvider::Other);
        details.mail_enabled = mail_enabled;
        details.email_address = Some("synthetic@google.example.invalid".into());
        let mut accounts = AccountList::default();
        accounts.apply_update(&make_checked_list(&[("one", details)]));
        assert!(accounts.visible_accounts().is_empty());
        assert_eq!(accounts.page(), AccountPage::NoEligibleAccounts);
        assert!(
            accounts
                .excluded_reasons()
                .contains(&ExclusionReason::UnsupportedProvider)
        );
    }
}

#[test]
fn label_fallbacks_and_duplicate_names_are_distinguishable() {
    let mut update = make_checked_list(&[]);
    for index in 0..3 {
        update.accounts.insert(
            make_account_id(&format!("synthetic-{index}")),
            make_account_details(AccountProvider::Google),
        );
    }
    let mut accounts = AccountList::default();
    accounts.apply_update(&update);
    assert_eq!(accounts.visible_accounts().len(), 3);
    let labels: BTreeSet<_> = accounts
        .visible_accounts()
        .values()
        .map(|row| row.label.clone())
        .collect();
    assert_eq!(labels.len(), 3);
    assert!(labels.iter().all(|label| !label.contains("synthetic-")));
    let mut accounts = AccountList::default();
    let mut details = make_account_details(AccountProvider::Microsoft365);
    details.email_address = Some("synthetic@example.invalid".into());
    accounts.apply_update(&make_checked_list(&[("one", details.clone())]));
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].label,
        "Synthetic account"
    );
    details.display_name = None;
    details.email_address = Some("synthetic@example.invalid".into());
    accounts.apply_update(&make_checked_list(&[("one", details.clone())]));
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].label,
        "synthetic@example.invalid"
    );
    details.email_address = None;
    accounts.apply_update(&make_checked_list(&[("one", details)]));
    let row = &accounts.visible_accounts()[&make_account_id("one")];
    assert_eq!(row.label, "Mail account");

    let mut numbered_account = make_account_details(AccountProvider::Google);
    numbered_account.display_name = Some("Synthetic account (1)".into());
    let mut accounts = AccountList::default();
    accounts.apply_update(&make_checked_list(&[
        ("one", make_account_details(AccountProvider::Google)),
        ("two", make_account_details(AccountProvider::Google)),
        ("three", numbered_account),
    ]));
    assert_eq!(
        accounts
            .visible_accounts()
            .values()
            .map(|row| &row.label)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
}

#[test]
fn failed_read_and_retry_preserve_rows_labels_selection_and_individual_problems() {
    let mut attention = make_account_details(AccountProvider::Google);
    attention.needs_attention = true;
    let accepted = make_checked_list(&[
        ("one", attention),
        ("two", make_account_details(AccountProvider::Microsoft365)),
    ]);
    let mut accounts = AccountList::default();
    accounts.apply_update(&accepted);
    accounts.select_account(make_account_id("one"));
    let label = accounts.visible_accounts()[&make_account_id("one")]
        .label
        .clone();
    let rows = accounts.visible_accounts().clone();
    assert_eq!(
        rows[&make_account_id("one")].problems,
        vec![AccountProblem::AttentionNeeded]
    );
    let mut pending = accepted.clone();
    pending.retry_pending = true;
    assert!(accounts.apply_update(&pending).is_empty());
    assert_eq!(accounts.visible_accounts(), &rows);
    assert_eq!(accounts.selected_id(), Some(&make_account_id("one")));
    assert!(accounts.retry_pending());
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
    let mut failed = make_failed_list(&accepted);
    for retry_pending in [false, true] {
        failed.retry_pending = retry_pending;
        assert!(accounts.apply_update(&failed).is_empty());
        assert_eq!(accounts.visible_accounts().len(), 2);
        assert_eq!(accounts.selected_id(), Some(&make_account_id("one")));
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("one")].label,
            label
        );
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("one")].problems,
            vec![
                AccountProblem::AttentionNeeded,
                AccountProblem::CheckUnconfirmed
            ]
        );
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("two")].problems,
            vec![AccountProblem::CheckUnconfirmed]
        );
        assert_eq!(
            accounts.page(),
            AccountPage::ReadFailed(ErrorCause::Timeout)
        );
        assert_eq!(accounts.retry_pending(), retry_pending);
    }
    accounts.apply_update(&accepted);
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].problems,
        vec![AccountProblem::AttentionNeeded]
    );
}

#[test]
fn page_states_follow_loading_failure_empty_and_selection() {
    let mut accounts = AccountList::default();
    assert_eq!(accounts.page(), AccountPage::Loading);
    let empty = make_checked_list(&[]);
    accounts.apply_update(&make_failed_list(&empty));
    assert_eq!(
        accounts.page(),
        AccountPage::ReadFailed(ErrorCause::Timeout)
    );
    accounts.apply_update(&empty);
    assert_eq!(accounts.page(), AccountPage::NoAccounts);
    accounts.apply_update(&make_checked_list(&[(
        "one",
        make_account_details(AccountProvider::Google),
    )]));
    assert_eq!(accounts.page(), AccountPage::SelectAccount);
    accounts.select_account(make_account_id("one"));
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
}
