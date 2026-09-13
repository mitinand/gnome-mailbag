// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, fmt};

/// Account ID supplied by the account source. Its value is hidden from Debug output.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);
impl AccountId {
    pub(crate) fn byte_len(&self) -> usize {
        self.0.len()
    }
}
impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountId([redacted])")
    }
}

impl TryFrom<&str> for AccountId {
    type Error = AccountCheckError;

    fn try_from(id: &str) -> Result<Self, Self::Error> {
        if is_valid_text(id) {
            Ok(Self(id.to_owned()))
        } else {
            Err(AccountCheckError::new(
                "account ID",
                ErrorCause::InvalidList,
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountField {
    Provider,
    MailEnabled,
    Attention,
}

/// Provider identity recognized by an adapter. Mailbag decides which it supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountProvider {
    ImapSmtp,
    Google,
    Microsoft365,
    Other,
}

/// Account details supplied by the source; missing or invalid fields are None.
/// Mailbag decides which providers it supports and how to display missing fields.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountDetails {
    pub provider: Option<AccountProvider>,
    pub mail_enabled: Option<bool>,
    pub needs_attention: Option<bool>,
    pub mail_service_available: bool,
    pub provider_name: Option<String>,
    pub display_name: Option<String>,
    pub email_address: Option<String>,
    pub icon_name: Option<String>,
}
impl fmt::Debug for AccountDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountDetails")
            .field("invalid_fields", &self.invalid_fields())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCause {
    Unavailable,
    AccessDenied,
    Timeout,
    InvalidReply,
    InvalidList,
    DataLimit,
    SourceStopped,
}

/// Operation and error codes, without service error text that could contain personal data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountCheckError {
    pub operation: &'static str,
    pub cause: ErrorCause,
    /// Whitelisted source diagnostic name; account rules must not interpret it.
    pub domain: Option<String>,
    /// Source diagnostic code, retained for troubleshooting only.
    pub code: Option<i32>,
}
impl AccountCheckError {
    pub fn new(operation: &'static str, cause: ErrorCause) -> Self {
        Self {
            operation,
            cause,
            domain: None,
            code: None,
        }
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
    /// A request is running; its result has not replaced last_check yet.
    pub check_pending: bool,
}

impl AccountDetails {
    /// Required fields that the source did not supply with valid values.
    pub fn invalid_fields(&self) -> Vec<AccountField> {
        [
            (AccountField::Provider, self.provider.is_none()),
            (AccountField::MailEnabled, self.mail_enabled.is_none()),
            (AccountField::Attention, self.needs_attention.is_none()),
        ]
        .into_iter()
        .filter_map(|(field, missing)| missing.then_some(field))
        .collect()
    }

    /// Bytes retained in optional display strings; enums contain no source strings.
    pub(crate) fn string_bytes(&self) -> usize {
        [
            &self.provider_name,
            &self.display_name,
            &self.email_address,
            &self.icon_name,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum()
    }
}

pub(crate) const MAX_STRING_BYTES: usize = 4096;
pub(crate) fn is_valid_text(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_STRING_BYTES && !text.chars().any(char::is_control)
}
pub(crate) fn is_valid_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_STRING_BYTES
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[cfg(test)]
mod tests;
