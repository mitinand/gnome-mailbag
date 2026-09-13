// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use account_source::{
    AccountCheckError, AccountDetails, AccountField, AccountId, AccountProvider, AccountUpdate,
    CheckStatus,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Whether the source confirms the account exists and its required fields are usable.
/// This does not report mail authentication or synchronization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountAvailability {
    Confirmed,
    Unconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountProblem {
    CheckUnconfirmed,
    InvalidField(AccountField),
    MailUnavailable,
    UnsupportedProvider,
    AttentionNeeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExclusionReason {
    MailDisabled,
    UnsupportedProvider,
    MailUnavailable,
    InvalidDetails,
}

/// Which explanation to show in the existing account status area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountPage {
    Checking,
    Unavailable,
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
    pub provider_name: String,
    pub email_address: Option<String>,
    pub icon_name: String,
    pub availability: AccountAvailability,
    pub problems: Vec<AccountProblem>,
    /// Account name or address before adding a distinguishing number.
    base_label: String,
    label_number: u64,
}
impl fmt::Debug for AccountRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountRow")
            .field("availability", &self.availability)
            .field("problems", &self.problems)
            .finish_non_exhaustive()
    }
}

/// Returned only when an applied update hides a row for removal or disabled Mail.
/// The caller owns the notice; the account list keeps no notice history.
#[derive(Clone, PartialEq, Eq)]
pub enum AccountHiddenNotice {
    Single(String),
    Group(usize),
}
impl fmt::Debug for AccountHiddenNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single(_) => f.write_str("AccountHiddenNotice::Single([redacted])"),
            Self::Group(count) => f
                .debug_tuple("AccountHiddenNotice::Group")
                .field(count)
                .finish(),
        }
    }
}

/// The rows last applied by the UI and the account chosen by the user.
/// Call apply_update when updating the displayed rows so notices describe
/// changes the user actually sees.
pub struct AccountList {
    visible_accounts: BTreeMap<AccountId, AccountRow>,
    selected_id: Option<AccountId>,
    check_status: CheckStatus,
    check_error: Option<AccountCheckError>,
    membership_confirmed: bool,
    excluded_reasons: BTreeSet<ExclusionReason>,
    next_label_number: u64,
}
impl Default for AccountList {
    fn default() -> Self {
        Self {
            visible_accounts: BTreeMap::new(),
            selected_id: None,
            check_status: CheckStatus::Checking,
            check_error: None,
            membership_confirmed: false,
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
    pub fn check_status(&self) -> CheckStatus {
        self.check_status
    }
    pub fn check_error(&self) -> Option<&AccountCheckError> {
        self.check_error.as_ref()
    }
    pub fn excluded_reasons(&self) -> &BTreeSet<ExclusionReason> {
        &self.excluded_reasons
    }
    /// Select an existing row, or clear selection with None.
    /// Return false without changing selection if the ID has no row.
    pub fn select_account(&mut self, id: Option<AccountId>) -> bool {
        if id
            .as_ref()
            .is_some_and(|id| !self.visible_accounts.contains_key(id))
        {
            return false;
        }
        self.selected_id = id;
        true
    }
    pub fn page(&self) -> AccountPage {
        if self.selected_id.is_some() {
            AccountPage::SelectedAccount
        } else if !self.visible_accounts.is_empty() {
            AccountPage::SelectAccount
        } else if self.check_status == CheckStatus::Checking {
            AccountPage::Checking
        } else if !self.membership_confirmed {
            AccountPage::Unavailable
        } else if self.excluded_reasons.is_empty() {
            AccountPage::NoAccounts
        } else {
            AccountPage::NoEligibleAccounts
        }
    }

    pub fn apply_update(&mut self, update: AccountUpdate) -> Option<AccountHiddenNotice> {
        self.check_status = update.status;
        self.check_error = update.error;
        self.membership_confirmed =
            update.membership_confirmed && update.status == CheckStatus::Ready;
        self.excluded_reasons.clear();
        let mut hidden_notice = None;
        self.visible_accounts.retain(|id, row| {
            let disabled = update
                .accounts
                .get(id)
                .is_some_and(|details| details.mail_enabled == Some(false));
            let removed = self.membership_confirmed && !update.accounts.contains_key(id);
            if disabled || removed {
                extend_hidden_notice(&mut hidden_notice, &row.label);
                false
            } else {
                true
            }
        });
        for (id, details) in &update.accounts {
            let problems = collect_account_problems(details);
            let reasons = collect_exclusion_reasons(details, &problems);
            if details.mail_enabled == Some(false) {
                self.excluded_reasons.extend(reasons);
                continue;
            }
            let provider_name = lookup_provider_name(details.provider);
            // A confirmed change to an unsupported provider hides the row, but is
            // neither account removal nor Mail disablement, so it creates no notice.
            if self.membership_confirmed && details.provider.is_some() && provider_name.is_none() {
                self.visible_accounts.remove(id);
                self.excluded_reasons.extend(reasons);
                continue;
            }
            if let Some(row) = self.visible_accounts.get_mut(id) {
                row.update_details(details, self.membership_confirmed, problems);
            } else if self.membership_confirmed && reasons.is_empty() {
                let mut row = AccountRow {
                    label: String::new(),
                    base_label: "Mail account".into(),
                    provider_name: "Online Accounts".into(),
                    email_address: None,
                    icon_name: "mail-unread-symbolic".into(),
                    availability: AccountAvailability::Unconfirmed,
                    problems: vec![],
                    label_number: self.next_label_number,
                };
                self.next_label_number += 1;
                row.update_details(details, true, problems);
                self.visible_accounts.insert(id.clone(), row);
            } else {
                self.excluded_reasons.extend(reasons);
            }
        }
        if !self.membership_confirmed {
            for row in self.visible_accounts.values_mut() {
                row.availability = AccountAvailability::Unconfirmed;
                if !row.problems.contains(&AccountProblem::CheckUnconfirmed) {
                    row.problems.push(AccountProblem::CheckUnconfirmed);
                }
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
        hidden_notice
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
    fn update_details(
        &mut self,
        details: &AccountDetails,
        membership_confirmed: bool,
        problems: Vec<AccountProblem>,
    ) {
        self.problems = problems;
        if !membership_confirmed {
            self.problems.push(AccountProblem::CheckUnconfirmed);
        }
        let provider_name = lookup_provider_name(details.provider);
        self.availability = if self
            .problems
            .iter()
            .all(|problem| *problem == AccountProblem::AttentionNeeded)
        {
            AccountAvailability::Confirmed
        } else {
            AccountAvailability::Unconfirmed
        };
        // While account availability is unconfirmed, keep previous display text
        // and icon when the source provides no replacement. Once confirmed, use fallbacks
        // for missing fields.
        let keep_previous_display = self.availability == AccountAvailability::Unconfirmed;
        if let Some(name) = details
            .display_name
            .as_ref()
            .or(details.email_address.as_ref())
        {
            self.base_label = name.clone();
        } else if !keep_previous_display {
            self.base_label = "Mail account".into();
        }
        if let Some(name) = &details.provider_name {
            self.provider_name = name.clone();
        } else if !keep_previous_display {
            self.provider_name = provider_name.unwrap_or("Online Accounts").into();
        }
        if details.email_address.is_some() || !keep_previous_display {
            self.email_address = details.email_address.clone();
        }
        if let Some(icon) = &details.icon_name {
            self.icon_name = icon.clone();
        } else if !keep_previous_display {
            self.icon_name = "mail-unread-symbolic".into();
        }
    }
}

/// Return the fallback name for a supported provider; None means unsupported.
fn lookup_provider_name(provider: Option<AccountProvider>) -> Option<&'static str> {
    match provider {
        Some(AccountProvider::ImapSmtp) => Some("IMAP / SMTP"),
        Some(AccountProvider::Google) => Some("Google"),
        Some(AccountProvider::Microsoft365) => Some("Microsoft 365"),
        _ => None,
    }
}

fn collect_account_problems(details: &AccountDetails) -> Vec<AccountProblem> {
    let mut problems: Vec<_> = details
        .invalid_fields()
        .into_iter()
        .map(AccountProblem::InvalidField)
        .collect();
    if details.provider.is_some() && lookup_provider_name(details.provider).is_none() {
        problems.push(AccountProblem::UnsupportedProvider);
    }
    if !details.mail_service_available {
        problems.push(AccountProblem::MailUnavailable);
    }
    if details.needs_attention == Some(true) {
        problems.push(AccountProblem::AttentionNeeded);
    }
    problems
}

fn collect_exclusion_reasons(
    details: &AccountDetails,
    problems: &[AccountProblem],
) -> BTreeSet<ExclusionReason> {
    let mut reasons = BTreeSet::new();
    if details.mail_enabled == Some(false) {
        reasons.insert(ExclusionReason::MailDisabled);
    }
    for problem in problems {
        match problem {
            AccountProblem::UnsupportedProvider => {
                reasons.insert(ExclusionReason::UnsupportedProvider);
            }
            AccountProblem::MailUnavailable => {
                reasons.insert(ExclusionReason::MailUnavailable);
            }
            AccountProblem::InvalidField(_) => {
                reasons.insert(ExclusionReason::InvalidDetails);
            }
            AccountProblem::AttentionNeeded | AccountProblem::CheckUnconfirmed => {}
        }
    }
    reasons
}

fn extend_hidden_notice(notice: &mut Option<AccountHiddenNotice>, label: &str) {
    *notice = Some(match notice.take() {
        None => AccountHiddenNotice::Single(label.to_owned()),
        Some(AccountHiddenNotice::Single(_)) => AccountHiddenNotice::Group(2),
        Some(AccountHiddenNotice::Group(count)) => AccountHiddenNotice::Group(count + 1),
    });
}

#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod tests;
