// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use gio::prelude::*;
use glib::Variant;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub(crate) const ROOT: &str = "/org/gnome/OnlineAccounts";
pub(crate) const NAME: &str = "org.gnome.OnlineAccounts";
pub(crate) const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
pub(crate) const MAIL: &str = "org.gnome.OnlineAccounts.Mail";
pub(crate) const MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const STRING_LIMIT: usize = 4096;
const RECORD_LIMIT: usize = 4096;
const DATA_LIMIT: usize = 16 * 1024 * 1024;

/// Opaque GOA identity. Debug output deliberately omits the value.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GoaAccountId(String);
impl fmt::Debug for GoaAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GoaAccountId([redacted])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountField {
    ProviderType,
    MailDisabled,
    AttentionNeeded,
}

/// Unknown required fields stay None; optional display fields use None as fallback.
/// Provider eligibility belongs to Mailbag, not this transport crate.
#[derive(Clone, PartialEq, Eq)]
pub struct GoaAccountDetails {
    pub provider_type: Option<String>,
    pub mail_disabled: Option<bool>,
    pub attention_needed: Option<bool>,
    pub mail_present: bool,
    pub provider_name: Option<String>,
    pub presentation_identity: Option<String>,
    pub email_address: Option<String>,
    pub icon_name: Option<String>,
    pub problems: Vec<AccountField>,
}
impl fmt::Debug for GoaAccountDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoaAccountDetails")
            .field("problems", &self.problems)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCause {
    Unavailable,
    AccessDenied,
    Timeout,
    InvalidReply,
    InvalidMembership,
    DataLimit,
    WorkerStopped,
}

/// Safe diagnostic information. Remote error messages are never copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountError {
    pub operation: &'static str,
    pub cause: ErrorCause,
    pub domain: Option<String>,
    pub code: Option<i32>,
}
impl AccountError {
    pub(crate) fn new(operation: &'static str, cause: ErrorCause) -> Self {
        Self {
            operation,
            cause,
            domain: None,
            code: None,
        }
    }
    pub(crate) fn from_glib(operation: &'static str, error: glib::Error) -> Self {
        let cause = if error.matches(gio::IOErrorEnum::TimedOut)
            || error.matches(gio::DBusError::Timeout)
            || error.matches(gio::DBusError::NoReply)
        {
            ErrorCause::Timeout
        } else if error.matches(gio::DBusError::AccessDenied)
            || error.matches(gio::IOErrorEnum::PermissionDenied)
        {
            ErrorCause::AccessDenied
        } else if error.matches(gio::IOErrorEnum::InvalidArgument)
            || error.matches(gio::DBusError::InvalidArgs)
        {
            ErrorCause::InvalidReply
        } else {
            ErrorCause::Unavailable
        };
        // Only expose local GIO domains: unknown remote domains may contain personal text.
        let domain = error.domain().as_str();
        let domain = match domain.as_str() {
            "g-io-error-quark" | "g-dbus-error-quark" => Some(domain.to_string()),
            _ => None,
        };
        Self {
            operation,
            cause,
            domain,
            code: Some(error.code()),
        }
    }
}
impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.operation, self.cause)
    }
}
impl std::error::Error for AccountError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Checking,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoaAccountList {
    pub update_number: u64,
    pub accounts: BTreeMap<GoaAccountId, GoaAccountDetails>,
    /// Only true when the current successful check can establish membership.
    pub complete: bool,
    pub status: CheckStatus,
    pub error: Option<AccountError>,
}
impl GoaAccountList {
    pub(crate) fn initial() -> Self {
        Self {
            update_number: 0,
            accounts: BTreeMap::new(),
            complete: false,
            status: CheckStatus::Checking,
            error: None,
        }
    }
}

fn value(dict: &Variant, key: &str) -> Option<Variant> {
    // Reject duplicate dictionary keys instead of silently choosing one.
    let mut found = None;
    for entry in dict.iter() {
        if entry.child_value(0).str() == Some(key) {
            if found.is_some() {
                return None;
            }
            let value = entry.child_value(1);
            found = Some(if value.type_() == glib::VariantTy::VARIANT {
                value.as_variant().expect("variant wrapper")
            } else {
                value
            });
        }
    }
    found
}
fn text(dict: &Variant, key: &str) -> Option<String> {
    let v = value(dict, key)?;
    if v.type_() != glib::VariantTy::STRING {
        return None;
    }
    let s = v.str()?;
    (!s.is_empty() && s.len() <= STRING_LIMIT && !s.chars().any(char::is_control))
        .then(|| s.to_owned())
}
fn boolean(dict: &Variant, key: &str) -> Option<bool> {
    value(dict, key)?.get()
}
fn icon(dict: &Variant) -> Option<String> {
    let serialized = text(dict, "ProviderIcon")?;
    // GOA serializes GThemedIcon, which may carry several fallback names.
    // Parsing an icon does not load it; reject every non-themed result.
    let icon = gio::Icon::for_string(&serialized)
        .ok()?
        .downcast::<gio::ThemedIcon>()
        .ok()?;
    icon.names()
        .iter()
        .find(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
        .map(ToString::to_string)
}

pub(crate) fn parse_list(reply: &Variant) -> Result<GoaAccountList, AccountError> {
    let error = |cause| AccountError::new("GetManagedObjects", cause);
    if reply.type_().as_str() != "(a{oa{sa{sv}}})" {
        return Err(error(ErrorCause::InvalidReply));
    }
    let mut result = GoaAccountList {
        status: CheckStatus::Ready,
        complete: true,
        ..GoaAccountList::initial()
    };
    let mut duplicate_ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut bytes = 0;
    let mut records = 0;
    for entry in reply.child_value(0).iter() {
        let path_value = entry.child_value(0);
        let path = path_value.str().expect("validated object-path type");
        let interfaces = entry.child_value(1);
        let account = value(&interfaces, ACCOUNT);
        if !path.starts_with(&format!("{ROOT}/")) {
            continue;
        }
        if account.is_none() && !path.starts_with(&format!("{ROOT}/Accounts/")) {
            continue;
        }
        records += 1;
        if records > RECORD_LIMIT {
            return Err(error(ErrorCause::DataLimit));
        }
        if path.len() > STRING_LIMIT {
            return Err(error(ErrorCause::DataLimit));
        }
        bytes += path.len();
        if !paths.insert(path.to_owned()) {
            result.complete = false;
        }
        let Some(fields) = account else {
            result.complete = false;
            continue;
        };
        let Some(id) = text(&fields, "Id").map(GoaAccountId) else {
            result.complete = false;
            continue;
        };
        let mail = value(&interfaces, MAIL);
        let mut details = GoaAccountDetails {
            provider_type: text(&fields, "ProviderType"),
            mail_disabled: boolean(&fields, "MailDisabled"),
            attention_needed: boolean(&fields, "AttentionNeeded"),
            mail_present: mail.is_some(),
            provider_name: text(&fields, "ProviderName"),
            presentation_identity: text(&fields, "PresentationIdentity"),
            email_address: mail.as_ref().and_then(|m| text(m, "EmailAddress")),
            icon_name: icon(&fields),
            problems: Vec::new(),
        };
        if details.provider_type.is_none() {
            details.problems.push(AccountField::ProviderType);
        }
        if details.mail_disabled.is_none() {
            details.problems.push(AccountField::MailDisabled);
        }
        if details.attention_needed.is_none() {
            details.problems.push(AccountField::AttentionNeeded);
        }
        bytes += id.0.len()
            + [
                &details.provider_type,
                &details.provider_name,
                &details.presentation_identity,
                &details.email_address,
                &details.icon_name,
            ]
            .into_iter()
            .flatten()
            .map(String::len)
            .sum::<usize>();
        if bytes > DATA_LIMIT {
            return Err(error(ErrorCause::DataLimit));
        }
        if result.accounts.contains_key(&id) {
            duplicate_ids.insert(id.clone());
            result.complete = false;
        }
        result.accounts.insert(id, details);
    }
    for id in duplicate_ids {
        result.accounts.remove(&id);
    }
    if !result.complete {
        result.status = CheckStatus::Failed;
        result.error = Some(error(ErrorCause::InvalidMembership));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
