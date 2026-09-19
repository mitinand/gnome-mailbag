// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, fmt};

/// Account ID supplied by the account source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);
impl AccountId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for AccountId {
    type Error = AccountCheckError;

    fn try_from(id: &str) -> Result<Self, Self::Error> {
        if !id.is_empty() {
            Ok(Self(id.to_owned()))
        } else {
            Err(AccountCheckError::new(
                "account ID",
                ErrorCause::InvalidReply,
            ))
        }
    }
}

/// Provider identity recognized by an adapter. Mailbag decides which it supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountProvider {
    ImapSmtp,
    Google,
    Microsoft365,
    Other,
}

/// One decoded GOA account. Mailbag owns eligibility and display fallbacks.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountDetails {
    pub provider: AccountProvider,
    pub mail_enabled: bool,
    pub needs_attention: bool,
    pub mail_service_available: bool,
    pub display_name: Option<String>,
    pub email_address: Option<String>,
}
impl fmt::Debug for AccountDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountDetails")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCause {
    Unavailable,
    AccessDenied,
    Timeout,
    InvalidReply,
}

/// Safe operation and cause, without remote error text or personal data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountCheckError {
    pub operation: &'static str,
    pub cause: ErrorCause,
}
impl AccountCheckError {
    pub fn new(operation: &'static str, cause: ErrorCause) -> Self {
        Self { operation, cause }
    }
}
impl fmt::Display for AccountCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.operation, self.cause)
    }
}
impl std::error::Error for AccountCheckError {}

/// Result of the last observation, independent of any request now in progress.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AccountCheckResult {
    #[default]
    NotChecked,
    Complete,
    Failed(AccountCheckError),
}
impl AccountCheckResult {
    /// Only a complete observation can confirm absence from the account map.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
    pub fn error(&self) -> Option<&AccountCheckError> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountUpdate {
    pub accounts: BTreeMap<AccountId, AccountDetails>,
    pub last_check: AccountCheckResult,
    /// An explicit Retry is running; automatic reads do not set this flag.
    pub retry_pending: bool,
}
