// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use account_model::{
    AccountCheckError, AccountDetails, AccountId, AccountProvider, ErrorCause, MAX_STRING_BYTES,
    is_valid_icon_name, is_valid_text,
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

/// One decoded reply. No previous account state participates in decoding.
pub(crate) struct AccountSnapshot {
    pub accounts: BTreeMap<AccountId, AccountDetails>,
    pub account_paths: BTreeMap<String, AccountId>,
    pub list_error: Option<AccountCheckError>,
}

pub(crate) fn parse_account_snapshot(
    reply: &Variant,
) -> Result<AccountSnapshot, AccountCheckError> {
    let error = |cause| AccountCheckError::new("GetManagedObjects", cause);
    if reply.type_().as_str() != "(a{oa{sa{sv}}})" {
        return Err(error(ErrorCause::InvalidReply));
    }
    let mut accounts = BTreeMap::new();
    let mut account_paths: BTreeMap<String, AccountId> = BTreeMap::new();
    let mut seen_paths = BTreeSet::new();
    let mut duplicate_ids = BTreeSet::new();
    let mut duplicate_paths = BTreeSet::new();
    let mut list_error = None;
    let mut record_count = 0;
    for entry in reply.child_value(0).iter() {
        let path_value = entry.child_value(0);
        let path = path_value.str().expect("validated object-path type");
        let interfaces = entry.child_value(1);
        let account_properties = lookup_unique_value(&interfaces, ACCOUNT_INTERFACE);
        if !path.starts_with("/org/gnome/OnlineAccounts/")
            || (account_properties.is_none()
                && !path.starts_with("/org/gnome/OnlineAccounts/Accounts/"))
        {
            continue;
        }
        record_count += 1;
        if record_count > MAX_ACCOUNTS || path.len() > MAX_STRING_BYTES {
            return Err(error(ErrorCause::DataLimit));
        }
        if !seen_paths.insert(path.to_owned()) {
            duplicate_paths.insert(path.to_owned());
            if let Some(id) = account_paths.get(path) {
                duplicate_ids.insert(id.clone());
            }
            list_error = Some(error(ErrorCause::InvalidList));
        }
        let Some(account_properties) = account_properties else {
            list_error = Some(error(ErrorCause::InvalidList));
            continue;
        };
        let Some(id) = read_valid_text(&account_properties, "Id")
            .and_then(|id| AccountId::try_from(id.as_str()).ok())
        else {
            list_error = Some(error(ErrorCause::InvalidList));
            continue;
        };
        if duplicate_paths.contains(path) || accounts.contains_key(&id) {
            duplicate_ids.insert(id.clone());
            list_error = Some(error(ErrorCause::InvalidList));
        }
        account_paths.insert(path.to_owned(), id.clone());
        let mail_properties = lookup_unique_value(&interfaces, MAIL_INTERFACE);
        accounts.insert(
            id,
            AccountDetails {
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
            },
        );
    }
    account_paths.retain(|path, id| !duplicate_paths.contains(path) && !duplicate_ids.contains(id));
    accounts.retain(|id, _| !duplicate_ids.contains(id));
    validate_account_limits(accounts.iter(), &account_paths)?;
    Ok(AccountSnapshot {
        accounts,
        account_paths,
        list_error,
    })
}

pub(crate) fn has_account_properties(
    interface: &str,
    changed: &Variant,
    invalidated: &Variant,
) -> bool {
    let properties: &[&str] = match interface {
        ACCOUNT_INTERFACE => &[
            "Id",
            "ProviderType",
            "MailDisabled",
            "AttentionNeeded",
            "ProviderName",
            "PresentationIdentity",
            "ProviderIcon",
        ],
        MAIL_INTERFACE => &["EmailAddress"],
        _ => return false,
    };
    changed.iter().any(|entry| {
        entry
            .child_value(0)
            .str()
            .is_some_and(|name| properties.contains(&name))
    }) || invalidated
        .iter()
        .any(|field| field.str().is_some_and(|name| properties.contains(&name)))
}

/// Update fields listed in the GOA signal; keep all other account fields unchanged.
pub(crate) fn apply_account_properties(
    account: &mut AccountDetails,
    interface: &str,
    changed_properties: &Variant,
    invalidated_properties: &Variant,
) {
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
}

/// Validate the resulting collection, including an uncommitted replacement record.
pub(crate) fn validate_account_limits<'a>(
    accounts: impl Iterator<Item = (&'a AccountId, &'a AccountDetails)>,
    account_paths: &BTreeMap<String, AccountId>,
) -> Result<(), AccountCheckError> {
    let mut string_bytes = account_paths.keys().map(String::len).sum::<usize>();
    let mut account_count = 0;
    for (id, account) in accounts {
        account_count += 1;
        string_bytes += id.byte_len() + account.string_bytes();
    }
    if account_count > MAX_ACCOUNTS || string_bytes > MAX_ACCOUNT_STRING_BYTES {
        Err(AccountCheckError::new(
            "account data",
            ErrorCause::DataLimit,
        ))
    } else {
        Ok(())
    }
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
