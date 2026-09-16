// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use goa_adapter::{
    AccountCheckResult, AccountDetails, AccountId, AccountProvider, AccountUpdate, ErrorCause,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const GENERIC_ACCOUNT_ICON: &str = "mail-unread-symbolic";

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
        let mut hidden_notices = Vec::new();
        self.visible_accounts.retain(|id, row| {
            let details = update.accounts.get(id);
            let disabled = details.is_some_and(|details| !details.mail_enabled);
            let removed = details.is_none();
            if disabled || removed {
                hidden_notices.push(AccountHiddenNotice {
                    label: row.label.clone(),
                });
                false
            } else {
                true
            }
        });
        for (id, details) in &update.accounts {
            if details.provider == AccountProvider::Other {
                self.visible_accounts.remove(id);
                self.excluded_reasons
                    .insert(ExclusionReason::UnsupportedProvider);
                continue;
            }
            if !details.mail_enabled {
                self.excluded_reasons.insert(ExclusionReason::MailDisabled);
                continue;
            }
            if let Some(row) = self.visible_accounts.get_mut(id) {
                row.update_details(details);
            } else if details.mail_service_available {
                let mut row = AccountRow {
                    label: String::new(),
                    base_label: String::new(),
                    icon_name: GENERIC_ACCOUNT_ICON,
                    problems: vec![],
                    label_number: self.next_label_number,
                };
                self.next_label_number += 1;
                row.update_details(details);
                self.visible_accounts.insert(id.clone(), row);
            } else {
                self.excluded_reasons
                    .insert(ExclusionReason::MailUnavailable);
            }
        }
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
impl AccountRow {
    fn mark_check_unconfirmed(&mut self) {
        if !self.problems.contains(&AccountProblem::CheckUnconfirmed) {
            self.problems.push(AccountProblem::CheckUnconfirmed);
        }
    }

    fn update_details(&mut self, details: &AccountDetails) {
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
        self.icon_name = match details.provider {
            AccountProvider::Google => "mailbag-account-google-symbolic",
            AccountProvider::Microsoft365 => "mailbag-account-ms365-symbolic",
            AccountProvider::ImapSmtp | AccountProvider::Other => GENERIC_ACCOUNT_ICON,
        };
    }
}
