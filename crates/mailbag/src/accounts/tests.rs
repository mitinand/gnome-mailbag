// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::accounts::*;
use account_model::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountField, AccountId,
    AccountProvider, AccountUpdate,
};
use std::collections::BTreeSet;

#[test]
fn pending_check_preserves_confirmed_rows_and_last_result() {
    let mut accounts = AccountList::default();
    let mut ready = make_checked_list(&[("one", make_account_details(AccountProvider::Google))]);
    accounts.apply_update(&ready);
    ready.check_pending = true;
    accounts.apply_update(&ready);
    let row = &accounts.visible_accounts()[&make_account_id("one")];
    assert_eq!(row.availability(), AccountAvailability::Confirmed);
    assert!(row.problems.is_empty());
}

pub(super) fn make_account_id(id: &str) -> AccountId {
    AccountId::try_from(id).unwrap()
}

pub(super) fn make_account_details(provider: AccountProvider) -> AccountDetails {
    AccountDetails {
        provider: Some(provider),
        mail_enabled: Some(true),
        needs_attention: Some(false),
        mail_service_available: true,
        provider_name: None,
        display_name: Some("Synthetic account".into()),
        email_address: None,
        icon_name: None,
    }
}

pub(super) fn make_checked_list(accounts: &[(&str, AccountDetails)]) -> AccountUpdate {
    AccountUpdate {
        accounts: accounts
            .iter()
            .map(|(id, details)| (make_account_id(id), details.clone()))
            .collect(),
        last_check: AccountCheckResult::Complete,
        check_pending: false,
    }
}

pub(super) fn make_failed_list() -> AccountUpdate {
    AccountUpdate {
        last_check: AccountCheckResult::Failed(AccountCheckError::new(
            "read accounts",
            account_model::ErrorCause::Timeout,
        )),
        ..make_checked_list(&[])
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
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("one")].availability(),
            AccountAvailability::Confirmed
        );
        assert_eq!(accounts.page(), AccountPage::SelectAccount);
        assert!(accounts.selected_id().is_none());
    }
    {
        let mut details = make_account_details(AccountProvider::Other);
        details.provider_name = Some("Microsoft 365".into());
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
fn empty_checking_and_failed_lists_have_distinct_statuses() {
    let mut accounts = AccountList::default();
    assert_eq!(accounts.page(), AccountPage::Checking);
    assert!(accounts.visible_accounts().is_empty());
    accounts.apply_update(&make_failed_list());
    assert_eq!(accounts.page(), AccountPage::Unavailable);
    assert!(accounts.check_error().is_some());
    let mut checking = make_failed_list();
    checking.check_pending = true;
    accounts.apply_update(&checking);
    assert_eq!(accounts.page(), AccountPage::Unavailable);
    assert!(accounts.check_pending());
    assert!(
        accounts.check_error().is_some(),
        "retry must keep the unresolved explanation"
    );
    accounts.apply_update(&make_checked_list(&[]));
    assert_eq!(accounts.page(), AccountPage::NoAccounts);
    assert!(accounts.check_error().is_none());
}

#[test]
fn exclusions_explain_all_reasons_and_do_not_invent_rows() {
    let mut disabled = make_account_details(AccountProvider::ImapSmtp);
    disabled.mail_enabled = Some(false);
    let mut missing_mail = make_account_details(AccountProvider::Google);
    missing_mail.mail_service_available = false;
    let mut invalid = make_account_details(AccountProvider::Microsoft365);
    invalid.needs_attention = None;
    let mut accounts = AccountList::default();
    accounts.apply_update(&make_checked_list(&[
        ("disabled", disabled),
        ("unsupported", make_account_details(AccountProvider::Other)),
        ("missing", missing_mail),
        ("invalid", invalid),
    ]));
    assert!(accounts.visible_accounts().is_empty());
    assert_eq!(
        accounts.excluded_reasons(),
        &BTreeSet::from([
            ExclusionReason::MailDisabled,
            ExclusionReason::UnsupportedProvider,
            ExclusionReason::MailUnavailable,
            ExclusionReason::InvalidDetails,
        ])
    );
    assert_eq!(accounts.page(), AccountPage::NoEligibleAccounts);
}

#[test]
fn individual_problems_preserve_selection_without_affecting_other_accounts() {
    let original = make_checked_list(&[
        ("one", make_account_details(AccountProvider::Google)),
        ("two", make_account_details(AccountProvider::Microsoft365)),
    ]);
    let mut accounts = AccountList::default();
    accounts.apply_update(&original);
    assert!(accounts.select_account(Some(make_account_id("one"))));
    let mut damaged = original.clone();
    let details = damaged.accounts.get_mut(&make_account_id("one")).unwrap();
    details.mail_service_available = false;
    details.mail_enabled = None;
    accounts.apply_update(&damaged);
    let damaged_row = &accounts.visible_accounts()[&make_account_id("one")];
    assert_eq!(damaged_row.availability(), AccountAvailability::Unconfirmed);
    assert!(
        damaged_row
            .problems
            .contains(&AccountProblem::MailUnavailable)
    );
    assert!(
        damaged_row
            .problems
            .contains(&AccountProblem::InvalidField(AccountField::MailEnabled))
    );
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("two")].availability(),
        AccountAvailability::Confirmed
    );
    assert_eq!(accounts.selected_id(), Some(&make_account_id("one")));
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
    accounts.apply_update(&original);
    assert!(
        accounts.visible_accounts()[&make_account_id("one")]
            .problems
            .is_empty()
    );
}

#[test]
fn outage_and_partial_recovery_keep_rows_but_cannot_add_unconfirmed_accounts() {
    let mut accounts = AccountList::default();
    let original = make_checked_list(&[("one", make_account_details(AccountProvider::Google))]);
    accounts.apply_update(&original);
    accounts.select_account(Some(make_account_id("one")));
    assert!(accounts.apply_update(&make_failed_list()).is_none());
    assert_eq!(accounts.selected_id(), Some(&make_account_id("one")));
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].availability(),
        AccountAvailability::Unconfirmed
    );
    let mut partial = make_failed_list();
    partial.accounts =
        make_checked_list(&[("new", make_account_details(AccountProvider::Google))]).accounts;
    accounts.apply_update(&partial);
    assert_eq!(accounts.visible_accounts().len(), 1);
    accounts.apply_update(&original);
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].availability(),
        AccountAvailability::Confirmed
    );
    assert!(accounts.check_error().is_none());
}

#[test]
fn repeated_incomplete_updates_mark_present_and_omitted_rows_once() {
    let mut accounts = AccountList::default();
    let mut attention_needed = make_account_details(AccountProvider::Google);
    attention_needed.needs_attention = Some(true);
    accounts.apply_update(&make_checked_list(&[
        ("omitted", attention_needed),
        (
            "present",
            make_account_details(AccountProvider::Microsoft365),
        ),
    ]));
    accounts.select_account(Some(make_account_id("omitted")));

    let mut incomplete = make_failed_list();
    let mut damaged = make_account_details(AccountProvider::Microsoft365);
    damaged.mail_enabled = None;
    incomplete
        .accounts
        .insert(make_account_id("present"), damaged);
    for _ in 0..2 {
        assert!(accounts.apply_update(&incomplete).is_none());
        assert_eq!(accounts.selected_id(), Some(&make_account_id("omitted")));
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("omitted")].problems,
            vec![
                AccountProblem::AttentionNeeded,
                AccountProblem::CheckUnconfirmed
            ]
        );
        assert_eq!(
            accounts.visible_accounts()[&make_account_id("present")].problems,
            vec![
                AccountProblem::InvalidField(AccountField::MailEnabled),
                AccountProblem::CheckUnconfirmed,
            ]
        );
    }
}

#[test]
fn attention_is_separate_from_availability_and_survives_successful_checks() {
    let mut details = make_account_details(AccountProvider::Google);
    details.needs_attention = Some(true);
    let list = make_checked_list(&[("one", details)]);
    let mut accounts = AccountList::default();
    accounts.apply_update(&list);
    let row = &accounts.visible_accounts()[&make_account_id("one")];
    assert_eq!(row.availability(), AccountAvailability::Confirmed);
    assert_eq!(row.problems, vec![AccountProblem::AttentionNeeded]);
    accounts.apply_update(&make_failed_list());
    assert!(
        accounts.visible_accounts()[&make_account_id("one")]
            .problems
            .contains(&AccountProblem::AttentionNeeded)
    );
    accounts.apply_update(&list);
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].problems,
        vec![AccountProblem::AttentionNeeded]
    );
}

#[test]
fn thirty_duplicate_labels_keep_distinct_ids_and_stable_selection() {
    let mut update = make_checked_list(&[]);
    for index in 0..30 {
        update.accounts.insert(
            make_account_id(&format!("synthetic-{index}")),
            make_account_details(AccountProvider::Google),
        );
    }
    let mut accounts = AccountList::default();
    accounts.apply_update(&update);
    assert_eq!(accounts.visible_accounts().len(), 30);
    let labels: BTreeSet<_> = accounts
        .visible_accounts()
        .values()
        .map(|row| row.label.clone())
        .collect();
    assert_eq!(labels.len(), 30);
    assert!(labels.iter().all(|label| !label.contains("synthetic-")));
    accounts.select_account(Some(make_account_id("synthetic-17")));
    let previous_labels: Vec<_> = accounts
        .visible_accounts()
        .values()
        .map(|row| row.label.clone())
        .collect();
    accounts.apply_update(&update);
    assert_eq!(
        previous_labels,
        accounts
            .visible_accounts()
            .values()
            .map(|row| row.label.clone())
            .collect::<Vec<_>>()
    );
    update
        .accounts
        .get_mut(&make_account_id("synthetic-17"))
        .unwrap()
        .display_name = Some("Renamed account".into());
    accounts.apply_update(&update);
    assert_eq!(
        accounts.selected_id(),
        Some(&make_account_id("synthetic-17"))
    );
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("synthetic-17")].label,
        "Renamed account"
    );
    assert!(!accounts.select_account(Some(make_account_id("unknown"))));
    assert_eq!(
        accounts.selected_id(),
        Some(&make_account_id("synthetic-17"))
    );
    assert!(accounts.select_account(None));
}

#[test]
fn optional_display_data_falls_back_and_errors_preserve_usable_labels() {
    let mut accounts = AccountList::default();
    let original =
        make_checked_list(&[("one", make_account_details(AccountProvider::Microsoft365))]);
    accounts.apply_update(&original);
    let mut failure = original;
    failure.last_check = make_failed_list().last_check;
    failure
        .accounts
        .get_mut(&make_account_id("one"))
        .unwrap()
        .display_name = None;
    accounts.apply_update(&failure);
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].label,
        "Synthetic account"
    );
    let mut details = make_account_details(AccountProvider::Microsoft365);
    details.display_name = None;
    accounts.apply_update(&make_checked_list(&[("one", details)]));
    let row = &accounts.visible_accounts()[&make_account_id("one")];
    assert_eq!(row.label, "Mail account");
    assert_eq!(row.provider_name, "Microsoft 365");
    assert!(row.email_address.is_none());
    assert_eq!(row.icon_name, "mail-unread-symbolic");
}

#[test]
fn each_unknown_required_field_keeps_known_rows_and_excludes_new_rows() {
    for field in [
        AccountField::Provider,
        AccountField::MailEnabled,
        AccountField::Attention,
    ] {
        let mut accounts = AccountList::default();
        accounts.apply_update(&make_checked_list(&[(
            "known",
            make_account_details(AccountProvider::Google),
        )]));
        accounts.select_account(Some(make_account_id("known")));
        let mut invalid = make_account_details(AccountProvider::Google);
        match field {
            AccountField::Provider => invalid.provider = None,
            AccountField::MailEnabled => invalid.mail_enabled = None,
            AccountField::Attention => invalid.needs_attention = None,
        }
        accounts.apply_update(&make_checked_list(&[
            ("known", invalid.clone()),
            ("new", invalid),
        ]));
        assert_eq!(accounts.visible_accounts().len(), 1);
        let row = &accounts.visible_accounts()[&make_account_id("known")];
        assert_eq!(row.availability(), AccountAvailability::Unconfirmed);
        assert!(row.problems.contains(&AccountProblem::InvalidField(field)));
        assert_eq!(accounts.selected_id(), Some(&make_account_id("known")));
    }
}

#[test]
fn manual_check_keeps_selection_and_shows_pending_without_clearing_attention() {
    let mut accounts = AccountList::default();
    let mut details = make_account_details(AccountProvider::Google);
    details.needs_attention = Some(true);
    let ready = make_checked_list(&[("one", details)]);
    accounts.apply_update(&ready);
    accounts.select_account(Some(make_account_id("one")));
    let mut pending = ready.clone();
    pending.check_pending = true;
    accounts.apply_update(&pending);
    assert!(accounts.check_pending());
    assert_eq!(accounts.page(), AccountPage::SelectedAccount);
    assert_eq!(accounts.selected_id(), Some(&make_account_id("one")));
    assert!(
        accounts.visible_accounts()[&make_account_id("one")]
            .problems
            .contains(&AccountProblem::AttentionNeeded)
    );
    accounts.apply_update(&ready);
    assert!(!accounts.check_pending());
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].problems,
        vec![AccountProblem::AttentionNeeded]
    );
}

#[test]
fn provider_changes_recheck_support_without_claiming_removal_or_disabled_mail() {
    let mut accounts = AccountList::default();
    accounts.apply_update(&make_checked_list(&[(
        "one",
        make_account_details(AccountProvider::Google),
    )]));
    accounts.select_account(Some(make_account_id("one")));
    let mut partial = make_failed_list();
    partial.accounts =
        make_checked_list(&[("one", make_account_details(AccountProvider::Other))]).accounts;
    accounts.apply_update(&partial);
    assert_eq!(accounts.visible_accounts().len(), 1);
    assert_eq!(
        accounts.visible_accounts()[&make_account_id("one")].availability(),
        AccountAvailability::Unconfirmed
    );
    assert!(
        accounts
            .apply_update(&make_checked_list(&[(
                "one",
                make_account_details(AccountProvider::Other)
            )]))
            .is_none()
    );
    assert!(accounts.visible_accounts().is_empty());
    assert!(accounts.selected_id().is_none());
    assert_eq!(accounts.page(), AccountPage::NoEligibleAccounts);
    accounts.apply_update(&make_checked_list(&[(
        "one",
        make_account_details(AccountProvider::ImapSmtp),
    )]));
    assert_eq!(accounts.visible_accounts().len(), 1);
    assert!(accounts.selected_id().is_none());
}

#[test]
fn duplicate_labels_do_not_collide_with_names_that_already_contain_numbers() {
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
fn display_and_notice_debug_output_omit_personal_fields() {
    let mut details = make_account_details(AccountProvider::Google);
    details.provider_name = Some("Private provider label".into());
    details.email_address = Some("synthetic@example.invalid".into());
    let mut accounts = AccountList::default();
    accounts.apply_update(&make_checked_list(&[("private-id", details)]));
    let debug = format!("{:?}", accounts.visible_accounts());
    for private in [
        "private-id",
        "Synthetic account",
        "Private provider label",
        "synthetic@example.invalid",
    ] {
        assert!(!debug.contains(private));
    }
    let notice = accounts.apply_update(&make_checked_list(&[])).unwrap();
    assert!(!format!("{notice:?}").contains("Synthetic account"));
}
