// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::logging;
use goa_adapter::{AccountCheckResult, AccountDetails, AccountProvider, AccountUpdate, ErrorCause};
use mailbag_domain::AccountId;
use mailbag_providers::MailProvider;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const GENERIC_ACCOUNT_ICON: &str = "mail-unread-symbolic";

/// Which load sequence an account needs, or `None` when Mailbag cannot load
/// its mail yet. This is the only place a provider becomes a load sequence,
/// and an account without one is not shown.
pub fn mail_provider(provider: AccountProvider) -> Option<MailProvider> {
    match provider {
        AccountProvider::ImapSmtp => Some(MailProvider::GenericImap),
        AccountProvider::Google => Some(MailProvider::Gmail),
        AccountProvider::Microsoft365 => Some(MailProvider::Microsoft365),
        AccountProvider::Other => None,
    }
}

/// The Online Accounts provider a shown row came from, for the record's
/// provider field; the inverse of `mail_provider` for the shown providers.
fn account_provider(provider: MailProvider) -> AccountProvider {
    match provider {
        MailProvider::GenericImap => AccountProvider::ImapSmtp,
        MailProvider::Gmail => AccountProvider::Google,
        MailProvider::Microsoft365 => AccountProvider::Microsoft365,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountProblem {
    CheckUnconfirmed,
    MailUnavailable,
    AttentionNeeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExclusionReason {
    MailDisabled,
    UnsupportedProvider,
    MailUnavailable,
}

/// Which explanation to show in the existing account status area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountPage {
    Loading,
    MailUnavailable,
    ReadFailed(ErrorCause),
    NoAccounts,
    NoEligibleAccounts,
    SelectAccount,
    SelectedAccount,
}

/// Display information for one row. Its presence does not confirm mail access.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Final row label, including a number when account names match.
    pub label: String,
    /// The load sequence the account needs. A provider Mailbag cannot load
    /// is never shown, so every row has one.
    pub provider: MailProvider,
    pub icon_name: &'static str,
    pub problems: Vec<AccountProblem>,
    /// Account name or address before adding a distinguishing number.
    base_label: String,
    label_number: u64,
}
impl fmt::Debug for AccountRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountRow")
            .field("problems", &self.problems)
            .finish_non_exhaustive()
    }
}

/// Returned only when an applied update hides a row for removal or disabled Mail.
/// The caller owns the notice; the account list keeps no notice history.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountHiddenNotice {
    pub label: String,
}
impl fmt::Debug for AccountHiddenNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountHiddenNotice")
            .field("label", &"[redacted]")
            .finish()
    }
}

/// The rows last applied by the UI and the account chosen by the user.
/// Call apply_update when updating the displayed rows so notices describe
/// changes the user actually sees.
pub struct AccountList {
    visible_accounts: BTreeMap<AccountId, AccountRow>,
    selected_id: Option<AccountId>,
    last_check: AccountCheckResult,
    retry_pending: bool,
    excluded_reasons: BTreeSet<ExclusionReason>,
    next_label_number: u64,
}
impl Default for AccountList {
    fn default() -> Self {
        Self {
            visible_accounts: BTreeMap::new(),
            selected_id: None,
            last_check: AccountCheckResult::NotChecked,
            retry_pending: false,
            excluded_reasons: BTreeSet::new(),
            next_label_number: 1,
        }
    }
}
impl AccountList {
    pub fn visible_accounts(&self) -> &BTreeMap<AccountId, AccountRow> {
        &self.visible_accounts
    }
    pub fn selected_id(&self) -> Option<&AccountId> {
        self.selected_id.as_ref()
    }
    pub fn retry_pending(&self) -> bool {
        self.retry_pending
    }
    pub fn excluded_reasons(&self) -> &BTreeSet<ExclusionReason> {
        &self.excluded_reasons
    }
    /// Select an existing row; ignore IDs without a visible row.
    pub fn select_account(&mut self, id: AccountId) {
        if self.visible_accounts.contains_key(&id) {
            self.selected_id = Some(id);
        }
    }
    pub fn page(&self) -> AccountPage {
        if let Some(error) = self.last_check.error() {
            AccountPage::ReadFailed(error.cause)
        } else if self.selected_id.is_some() {
            AccountPage::SelectedAccount
        } else if !self.visible_accounts.is_empty() {
            AccountPage::SelectAccount
        } else if self.last_check == AccountCheckResult::NotChecked {
            AccountPage::Loading
        } else if self
            .excluded_reasons
            .contains(&ExclusionReason::MailUnavailable)
        {
            AccountPage::MailUnavailable
        } else if self.excluded_reasons.is_empty() {
            AccountPage::NoAccounts
        } else {
            AccountPage::NoEligibleAccounts
        }
    }

    pub fn apply_update(&mut self, update: &AccountUpdate) -> Vec<AccountHiddenNotice> {
        self.last_check = update.last_check.clone();
        self.retry_pending = update.retry_pending;
        if !update.last_check.is_complete() {
            for row in self.visible_accounts.values_mut() {
                row.mark_check_unconfirmed();
            }
            return Vec::new();
        }
        self.excluded_reasons.clear();
        let hidden_notices = self.hide_removed_or_disabled_accounts(&update.accounts);
        self.update_visible_accounts(&update.accounts);
        if self
            .selected_id
            .as_ref()
            .is_some_and(|id| !self.visible_accounts.contains_key(id))
        {
            self.selected_id = None;
        }
        self.assign_display_labels();
        hidden_notices
    }

    fn hide_removed_or_disabled_accounts(
        &mut self,
        accounts: &BTreeMap<AccountId, AccountDetails>,
    ) -> Vec<AccountHiddenNotice> {
        let mut hidden_notices = Vec::new();
        self.visible_accounts.retain(|id, row| {
            let details = accounts.get(id);
            let disabled = details.is_some_and(|details| !details.mail_enabled);
            let removed = details.is_none();
            if removed {
                // A disabled account is logged with the accounts not shown.
                log_account_not_shown(
                    id,
                    account_provider(row.provider),
                    "removed from Online Accounts",
                );
            }
            if disabled || removed {
                hidden_notices.push(AccountHiddenNotice {
                    label: row.label.clone(),
                });
                false
            } else {
                true
            }
        });
        hidden_notices
    }

    fn update_visible_accounts(&mut self, accounts: &BTreeMap<AccountId, AccountDetails>) {
        for (id, details) in accounts {
            let Some(provider) = mail_provider(details.provider) else {
                self.visible_accounts.remove(id);
                self.excluded_reasons
                    .insert(ExclusionReason::UnsupportedProvider);
                log_account_not_shown(id, details.provider, "unsupported provider");
                continue;
            };
            if !details.mail_enabled {
                self.excluded_reasons.insert(ExclusionReason::MailDisabled);
                log_account_not_shown(id, details.provider, "mail disabled");
                continue;
            }
            if let Some(row) = self.visible_accounts.get_mut(id) {
                let previous_problems = std::mem::take(&mut row.problems);
                row.update_details(details, provider);
                log_problem_changes(id, &previous_problems, &row.problems);
            } else if details.mail_service_available {
                let mut row = AccountRow {
                    label: String::new(),
                    base_label: String::new(),
                    provider,
                    icon_name: GENERIC_ACCOUNT_ICON,
                    problems: vec![],
                    label_number: self.next_label_number,
                };
                self.next_label_number += 1;
                row.update_details(details, provider);
                tracing::info!(
                    account = id.as_str(),
                    provider = logging::provider_type(details.provider),
                    "account shown"
                );
                log_problem_changes(id, &[], &row.problems);
                self.visible_accounts.insert(id.clone(), row);
            } else {
                self.excluded_reasons
                    .insert(ExclusionReason::MailUnavailable);
                log_account_not_shown(id, details.provider, "mail service unavailable");
            }
        }
    }

    fn assign_display_labels(&mut self) {
        let mut label_counts = BTreeMap::new();
        for row in self.visible_accounts.values() {
            *label_counts.entry(row.base_label.clone()).or_insert(0) += 1;
        }
        let mut used_labels: BTreeSet<_> = label_counts.keys().cloned().collect();
        for row in self.visible_accounts.values_mut() {
            row.label = row.base_label.clone();
            if label_counts[&row.base_label] > 1 {
                // The number belongs to the row for its lifetime, not to its position.
                // Avoid colliding with a literal account name such as "Work (2)".
                loop {
                    row.label = format!("{} ({})", row.label, row.label_number);
                    if used_labels.insert(row.label.clone()) {
                        break;
                    }
                }
            }
        }
    }
}
/// Written at every complete read that leaves the account without a row.
fn log_account_not_shown(account_id: &AccountId, provider: AccountProvider, reason: &'static str) {
    tracing::info!(
        account = account_id.as_str(),
        provider = logging::provider_type(provider),
        reason,
        "account not shown"
    );
}

/// Warns when a row gains a problem and notes when one is gone. An unconfirmed
/// check is left to the failed read's own line.
fn log_problem_changes(
    account_id: &AccountId,
    previous_problems: &[AccountProblem],
    problems: &[AccountProblem],
) {
    for (problem, name) in [
        (AccountProblem::AttentionNeeded, "attention needed"),
        (AccountProblem::MailUnavailable, "mail service unavailable"),
    ] {
        match (
            previous_problems.contains(&problem),
            problems.contains(&problem),
        ) {
            (false, true) => tracing::warn!(
                account = account_id.as_str(),
                problem = name,
                "account has a problem"
            ),
            (true, false) => tracing::info!(
                account = account_id.as_str(),
                problem = name,
                "account problem is gone"
            ),
            _ => {}
        }
    }
}

impl AccountRow {
    fn mark_check_unconfirmed(&mut self) {
        if !self.problems.contains(&AccountProblem::CheckUnconfirmed) {
            self.problems.push(AccountProblem::CheckUnconfirmed);
        }
    }

    fn update_details(&mut self, details: &AccountDetails, provider: MailProvider) {
        self.problems.clear();
        if !details.mail_service_available {
            self.problems.push(AccountProblem::MailUnavailable);
        }
        if details.needs_attention {
            self.problems.push(AccountProblem::AttentionNeeded);
        }
        self.base_label = details
            .display_name
            .as_ref()
            .or(details.email_address.as_ref())
            .cloned()
            .unwrap_or_else(|| "Mail account".into());
        self.provider = provider;
        self.icon_name = match provider {
            MailProvider::Gmail => "mailbag-account-google-symbolic",
            MailProvider::Microsoft365 => "mailbag-account-ms365-symbolic",
            MailProvider::GenericImap => GENERIC_ACCOUNT_ICON,
        };
    }
}
