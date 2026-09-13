// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use gio::prelude::*;
use glib::Variant;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub(crate) const GOA_ROOT_PATH: &str = "/org/gnome/OnlineAccounts";
pub(crate) const GOA_BUS_NAME: &str = "org.gnome.OnlineAccounts";
pub(crate) const ACCOUNT_INTERFACE: &str = "org.gnome.OnlineAccounts.Account";
pub(crate) const MAIL_INTERFACE: &str = "org.gnome.OnlineAccounts.Mail";
pub(crate) const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";
const MAX_STRING_BYTES: usize = 4096;
const MAX_ACCOUNTS: usize = 4096;
const MAX_ACCOUNT_STRING_BYTES: usize = 16 * 1024 * 1024;

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
    pub invalid_fields: Vec<AccountField>,
}
impl fmt::Debug for GoaAccountDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoaAccountDetails")
            .field("invalid_fields", &self.invalid_fields)
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
    pub membership_confirmed: bool,
    pub status: CheckStatus,
    pub error: Option<AccountError>,
}
impl GoaAccountList {
    pub(crate) fn initial() -> Self {
        Self {
            update_number: 0,
            accounts: BTreeMap::new(),
            membership_confirmed: false,
            status: CheckStatus::Checking,
            error: None,
        }
    }
}

fn lookup_unique_value(dict: &Variant, key: &str) -> Option<Variant> {
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
fn read_valid_text(dict: &Variant, key: &str) -> Option<String> {
    let field_value = lookup_unique_value(dict, key)?;
    if field_value.type_() != glib::VariantTy::STRING {
        return None;
    }
    let text = field_value.str()?;
    (!text.is_empty() && text.len() <= MAX_STRING_BYTES && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}
fn read_bool(dict: &Variant, key: &str) -> Option<bool> {
    lookup_unique_value(dict, key)?.get()
}
fn read_icon_name(dict: &Variant) -> Option<String> {
    let serialized = read_valid_text(dict, "ProviderIcon")?;
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

pub(crate) fn parse_account_list(reply: &Variant) -> Result<GoaAccountList, AccountError> {
    let error = |cause| AccountError::new("GetManagedObjects", cause);
    if reply.type_().as_str() != "(a{oa{sa{sv}}})" {
        return Err(error(ErrorCause::InvalidReply));
    }
    let mut account_list = GoaAccountList {
        status: CheckStatus::Ready,
        membership_confirmed: true,
        ..GoaAccountList::initial()
    };
    let mut duplicate_ids = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    let mut account_string_bytes = 0;
    let mut account_count = 0;
    for entry in reply.child_value(0).iter() {
        let path_value = entry.child_value(0);
        let path = path_value.str().expect("validated object-path type");
        let interfaces = entry.child_value(1);
        let account_properties = lookup_unique_value(&interfaces, ACCOUNT_INTERFACE);
        if !path.starts_with(&format!("{GOA_ROOT_PATH}/")) {
            continue;
        }
        if account_properties.is_none() && !path.starts_with(&format!("{GOA_ROOT_PATH}/Accounts/"))
        {
            continue;
        }
        account_count += 1;
        if account_count > MAX_ACCOUNTS {
            return Err(error(ErrorCause::DataLimit));
        }
        if path.len() > MAX_STRING_BYTES {
            return Err(error(ErrorCause::DataLimit));
        }
        account_string_bytes += path.len();
        if !seen_paths.insert(path.to_owned()) {
            account_list.membership_confirmed = false;
        }
        let Some(account_properties) = account_properties else {
            account_list.membership_confirmed = false;
            continue;
        };
        let Some(id) = read_valid_text(&account_properties, "Id").map(GoaAccountId) else {
            account_list.membership_confirmed = false;
            continue;
        };
        let mail_properties = lookup_unique_value(&interfaces, MAIL_INTERFACE);
        let mut details = GoaAccountDetails {
            provider_type: read_valid_text(&account_properties, "ProviderType"),
            mail_disabled: read_bool(&account_properties, "MailDisabled"),
            attention_needed: read_bool(&account_properties, "AttentionNeeded"),
            mail_present: mail_properties.is_some(),
            provider_name: read_valid_text(&account_properties, "ProviderName"),
            presentation_identity: read_valid_text(&account_properties, "PresentationIdentity"),
            email_address: mail_properties
                .as_ref()
                .and_then(|m| read_valid_text(m, "EmailAddress")),
            icon_name: read_icon_name(&account_properties),
            invalid_fields: Vec::new(),
        };
        if details.provider_type.is_none() {
            details.invalid_fields.push(AccountField::ProviderType);
        }
        if details.mail_disabled.is_none() {
            details.invalid_fields.push(AccountField::MailDisabled);
        }
        if details.attention_needed.is_none() {
            details.invalid_fields.push(AccountField::AttentionNeeded);
        }
        account_string_bytes += id.0.len()
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
        if account_string_bytes > MAX_ACCOUNT_STRING_BYTES {
            return Err(error(ErrorCause::DataLimit));
        }
        if account_list.accounts.contains_key(&id) {
            duplicate_ids.insert(id.clone());
            account_list.membership_confirmed = false;
        }
        account_list.accounts.insert(id, details);
    }
    for id in duplicate_ids {
        account_list.accounts.remove(&id);
    }
    if !account_list.membership_confirmed {
        account_list.status = CheckStatus::Failed;
        account_list.error = Some(error(ErrorCause::InvalidMembership));
    }
    Ok(account_list)
}

#[cfg(test)]
mod tests;
