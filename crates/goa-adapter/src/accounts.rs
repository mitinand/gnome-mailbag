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

/// Account ID supplied by GOA. Its value is hidden from Debug output.
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

/// Account details read from GOA; missing or invalid fields are None.
/// Mailbag decides which providers it supports and how to display missing fields.
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

/// Operation and error codes, without service error text that could contain personal data.
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
        // Keep only known GIO error-domain names; other names may contain personal data.
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
    /// True when the check confirms which accounts exist, so absent accounts can be removed.
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
    // GOA supplies a GThemedIcon as text, with one or more icon names.
    // Accept only theme icons; parsing this text does not load an image.
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

#[cfg(test)]
pub(crate) fn parse_account_list(reply: &Variant) -> Result<GoaAccountList, AccountError> {
    parse_account_snapshot(reply, &BTreeMap::new()).map(|snapshot| snapshot.account_list)
}

pub(crate) struct AccountSnapshot {
    pub account_list: GoaAccountList,
    /// Maps D-Bus object paths to account IDs. An incomplete reply keeps known
    /// mappings unless the reply shows conflicting paths or IDs.
    pub account_paths: BTreeMap<String, GoaAccountId>,
}

pub(crate) fn parse_account_snapshot(
    reply: &Variant,
    known_paths: &BTreeMap<String, GoaAccountId>,
) -> Result<AccountSnapshot, AccountError> {
    let error = |cause| AccountError::new("GetManagedObjects", cause);
    if reply.type_().as_str() != "(a{oa{sa{sv}}})" {
        return Err(error(ErrorCause::InvalidReply));
    }
    let mut account_list = GoaAccountList {
        status: CheckStatus::Ready,
        membership_confirmed: true,
        ..GoaAccountList::initial()
    };
    let mut account_paths = BTreeMap::new();
    let mut duplicate_paths = BTreeSet::new();
    let mut duplicate_ids = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    let mut total_string_bytes = 0;
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
        total_string_bytes += path.len();
        if !seen_paths.insert(path.to_owned()) {
            duplicate_paths.insert(path.to_owned());
            account_list.membership_confirmed = false;
        }
        let Some(account_properties) = account_properties else {
            account_list.membership_confirmed = false;
            continue;
        };
        let id = match read_valid_text(&account_properties, "Id").map(GoaAccountId) {
            Some(id) => id,
            None => {
                account_list.membership_confirmed = false;
                let Some(id) = known_paths.get(path) else {
                    continue;
                };
                id.clone()
            }
        };
        account_paths.insert(path.to_owned(), id.clone());
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
        details.refresh_invalid_fields();
        total_string_bytes += account_string_bytes(&id, &details);
        if total_string_bytes > MAX_ACCOUNT_STRING_BYTES {
            return Err(error(ErrorCause::DataLimit));
        }
        if account_list.accounts.contains_key(&id) {
            duplicate_ids.insert(id.clone());
            account_list.membership_confirmed = false;
        }
        account_list.accounts.insert(id, details);
    }
    if !account_list.membership_confirmed {
        // An incomplete reply cannot prove that a known account was removed.
        // Keep its old path unless that path conflicts or the account has a new path.
        for (path, id) in known_paths {
            if !duplicate_paths.contains(path)
                && !duplicate_ids.contains(id)
                && !account_list.accounts.contains_key(id)
            {
                account_paths
                    .entry(path.clone())
                    .or_insert_with(|| id.clone());
            }
        }
    }
    account_paths.retain(|path, id| !duplicate_ids.contains(id) && !duplicate_paths.contains(path));
    for id in duplicate_ids {
        account_list.accounts.remove(&id);
    }
    if !account_list.membership_confirmed {
        account_list.status = CheckStatus::Failed;
        account_list.error = Some(error(ErrorCause::InvalidMembership));
    }
    Ok(AccountSnapshot {
        account_list,
        account_paths,
    })
}

impl GoaAccountDetails {
    pub(crate) fn refresh_invalid_fields(&mut self) {
        self.invalid_fields.clear();
        if self.provider_type.is_none() {
            self.invalid_fields.push(AccountField::ProviderType);
        }
        if self.mail_disabled.is_none() {
            self.invalid_fields.push(AccountField::MailDisabled);
        }
        if self.attention_needed.is_none() {
            self.invalid_fields.push(AccountField::AttentionNeeded);
        }
    }

    /// Update fields listed in the GOA signal; keep all other account fields unchanged.
    /// Return true if required fields are unknown and need a full account check.
    pub(crate) fn apply_properties(
        &mut self,
        interface: &str,
        changed_properties: &Variant,
        invalidated_properties: &Variant,
    ) -> bool {
        let is_invalidated = |name: &str| {
            invalidated_properties
                .iter()
                .any(|field| field.str() == Some(name))
        };
        let is_changed = |name: &str| {
            changed_properties
                .iter()
                .any(|entry| entry.child_value(0).str() == Some(name))
        };
        macro_rules! update_field {
            ($property:literal, $field:ident, $read:ident) => {
                if is_invalidated($property) {
                    self.$field = None;
                } else if is_changed($property) {
                    self.$field = $read(changed_properties, $property);
                }
            };
        }
        if interface == ACCOUNT_INTERFACE {
            update_field!("ProviderType", provider_type, read_valid_text);
            update_field!("MailDisabled", mail_disabled, read_bool);
            update_field!("AttentionNeeded", attention_needed, read_bool);
            update_field!("ProviderName", provider_name, read_valid_text);
            update_field!(
                "PresentationIdentity",
                presentation_identity,
                read_valid_text
            );
            if is_invalidated("ProviderIcon") {
                self.icon_name = None;
            } else if is_changed("ProviderIcon") {
                self.icon_name = read_icon_name(changed_properties);
            }
        } else if interface == MAIL_INTERFACE {
            update_field!("EmailAddress", email_address, read_valid_text);
        }
        self.refresh_invalid_fields();
        !self.invalid_fields.is_empty()
    }
}

fn account_string_bytes(id: &GoaAccountId, account: &GoaAccountDetails) -> usize {
    id.0.len()
        + [
            &account.provider_type,
            &account.provider_name,
            &account.presentation_identity,
            &account.email_address,
            &account.icon_name,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum::<usize>()
}

pub(crate) fn fits_account_limits(
    account_list: &GoaAccountList,
    account_paths: &BTreeMap<String, GoaAccountId>,
) -> bool {
    let string_bytes = account_paths.keys().map(String::len).sum::<usize>()
        + account_list
            .accounts
            .iter()
            .map(|(id, account)| account_string_bytes(id, account))
            .sum::<usize>();
    account_list.accounts.len() <= MAX_ACCOUNTS && string_bytes <= MAX_ACCOUNT_STRING_BYTES
}

/// Merge the checked accounts while keeping known accounts omitted from the reply.
/// Leave the current list unchanged if the combined data exceeds the size limits.
pub(crate) fn merge_account_facts(
    current_list: &mut GoaAccountList,
    checked_list: GoaAccountList,
    account_paths: &BTreeMap<String, GoaAccountId>,
) -> Result<(), AccountError> {
    let mut account_count = checked_list.accounts.len();
    let mut string_bytes = account_paths.keys().map(String::len).sum::<usize>()
        + checked_list
            .accounts
            .iter()
            .map(|(id, account)| account_string_bytes(id, account))
            .sum::<usize>();
    for (id, account) in &current_list.accounts {
        if !checked_list.accounts.contains_key(id) {
            account_count += 1;
            string_bytes += account_string_bytes(id, account);
        }
    }
    if account_count > MAX_ACCOUNTS || string_bytes > MAX_ACCOUNT_STRING_BYTES {
        return Err(AccountError::new(
            "retain unconfirmed accounts",
            ErrorCause::DataLimit,
        ));
    }
    current_list.accounts.extend(checked_list.accounts);
    current_list.status = checked_list.status;
    current_list.error = checked_list.error;
    current_list.membership_confirmed = false;
    Ok(())
}

#[cfg(test)]
mod tests;
