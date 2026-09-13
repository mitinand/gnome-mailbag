// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use account_source::{
    AccountCheckError, AccountDetails, AccountId, AccountProvider, AccountUpdate, CheckStatus,
    ErrorCause, MAX_STRING_BYTES, is_valid_icon_name, is_valid_text,
};
use gio::prelude::*;
use glib::Variant;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const GOA_ROOT_PATH: &str = "/org/gnome/OnlineAccounts";
pub(crate) const GOA_BUS_NAME: &str = "org.gnome.OnlineAccounts";
pub(crate) const ACCOUNT_INTERFACE: &str = "org.gnome.OnlineAccounts.Account";
pub(crate) const MAIL_INTERFACE: &str = "org.gnome.OnlineAccounts.Mail";
pub(crate) const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";
const MAX_ACCOUNTS: usize = 4096;
const MAX_ACCOUNT_STRING_BYTES: usize = 16 * 1024 * 1024;

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
    is_valid_text(text).then(|| text.to_owned())
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
        .find(|name| is_valid_icon_name(name))
        .map(ToString::to_string)
}

#[cfg(test)]
pub(crate) fn parse_account_list(reply: &Variant) -> Result<AccountUpdate, AccountCheckError> {
    parse_account_snapshot(reply, &BTreeMap::new()).map(|snapshot| snapshot.account_list)
}

pub(crate) struct AccountSnapshot {
    pub account_list: AccountUpdate,
    /// Maps D-Bus object paths to account IDs. An incomplete reply keeps known
    /// mappings unless the reply shows conflicting paths or IDs.
    pub account_paths: BTreeMap<String, AccountId>,
}

pub(crate) fn parse_account_snapshot(
    reply: &Variant,
    known_paths: &BTreeMap<String, AccountId>,
) -> Result<AccountSnapshot, AccountCheckError> {
    let error = |cause| AccountCheckError::new("GetManagedObjects", cause);
    if reply.type_().as_str() != "(a{oa{sa{sv}}})" {
        return Err(error(ErrorCause::InvalidReply));
    }
    let mut account_list = AccountUpdate {
        status: CheckStatus::Ready,
        membership_confirmed: true,
        ..AccountUpdate::default()
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
        let id = match read_valid_text(&account_properties, "Id")
            .and_then(|id| AccountId::try_from(id.as_str()).ok())
        {
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
        let details = AccountDetails {
            provider: read_provider(&account_properties, "ProviderType"),
            mail_enabled: read_mail_enabled(&account_properties, "MailDisabled"),
            needs_attention: read_bool(&account_properties, "AttentionNeeded"),
            mail_service_available: mail_properties.is_some(),
            provider_name: read_valid_text(&account_properties, "ProviderName"),
            display_name: read_valid_text(&account_properties, "PresentationIdentity"),
            email_address: mail_properties
                .as_ref()
                .and_then(|m| read_valid_text(m, "EmailAddress")),
            icon_name: read_icon_name(&account_properties),
        };
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
        account_list.error = Some(error(ErrorCause::InvalidList));
    }
    Ok(AccountSnapshot {
        account_list,
        account_paths,
    })
}

/// Update fields listed in the GOA signal; keep all other account fields unchanged.
/// Return true if required fields are unknown and need a full account check.
pub(crate) fn apply_account_properties(
    account: &mut AccountDetails,
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
                account.$field = None;
            } else if is_changed($property) {
                account.$field = $read(changed_properties, $property);
            }
        };
    }
    if interface == ACCOUNT_INTERFACE {
        update_field!("ProviderType", provider, read_provider);
        update_field!("MailDisabled", mail_enabled, read_mail_enabled);
        update_field!("AttentionNeeded", needs_attention, read_bool);
        update_field!("ProviderName", provider_name, read_valid_text);
        update_field!("PresentationIdentity", display_name, read_valid_text);
        if is_invalidated("ProviderIcon") {
            account.icon_name = None;
        } else if is_changed("ProviderIcon") {
            account.icon_name = read_icon_name(changed_properties);
        }
    } else if interface == MAIL_INTERFACE {
        update_field!("EmailAddress", email_address, read_valid_text);
    }
    !account.invalid_fields().is_empty()
}

fn account_string_bytes(id: &AccountId, account: &AccountDetails) -> usize {
    id.byte_len() + account.string_bytes()
}

pub(crate) fn fits_account_limits(
    account_list: &AccountUpdate,
    account_paths: &BTreeMap<String, AccountId>,
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
    current_list: &mut AccountUpdate,
    checked_list: AccountUpdate,
    account_paths: &BTreeMap<String, AccountId>,
) -> Result<(), AccountCheckError> {
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
        return Err(AccountCheckError::new(
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

fn read_provider(dict: &Variant, key: &str) -> Option<AccountProvider> {
    read_valid_text(dict, key).map(|provider| match provider.as_str() {
        "imap_smtp" => AccountProvider::ImapSmtp,
        "google" => AccountProvider::Google,
        "ms_graph" => AccountProvider::Microsoft365,
        _ => AccountProvider::Other,
    })
}
fn read_mail_enabled(dict: &Variant, key: &str) -> Option<bool> {
    read_bool(dict, key).map(|disabled| !disabled)
}

pub(crate) fn map_glib_error(operation: &'static str, error: glib::Error) -> AccountCheckError {
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
    AccountCheckError {
        operation,
        cause,
        domain,
        code: Some(error.code()),
    }
}

#[cfg(test)]
mod tests;
