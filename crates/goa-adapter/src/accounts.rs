// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{AccountCheckError, AccountDetails, AccountId, AccountProvider, ErrorCause};
use glib::{
    Variant,
    variant::{FromVariant, ObjectPath},
};
use std::collections::BTreeMap;

pub(crate) const GOA_ROOT_PATH: &str = "/org/gnome/OnlineAccounts";
/// Account objects live below the root; used to filter their property signals.
pub(crate) const GOA_ACCOUNT_PATH_PREFIX: &str = "/org/gnome/OnlineAccounts/";
pub(crate) const GOA_BUS_NAME: &str = "org.gnome.OnlineAccounts";
pub(crate) const ACCOUNT_INTERFACE: &str = "org.gnome.OnlineAccounts.Account";
pub(crate) const MAIL_INTERFACE: &str = "org.gnome.OnlineAccounts.Mail";
pub(crate) const PASSWORD_BASED_INTERFACE: &str = "org.gnome.OnlineAccounts.PasswordBased";
pub(crate) const OAUTH2_BASED_INTERFACE: &str = "org.gnome.OnlineAccounts.OAuth2Based";
pub(crate) const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";

pub(crate) type Properties = BTreeMap<String, Variant>;
pub(crate) type ManagedObjects = BTreeMap<ObjectPath, BTreeMap<String, Properties>>;

/// Accept the complete response or return one read error; no partial records escape.
pub(crate) fn parse_accounts(
    reply: &Variant,
) -> Result<BTreeMap<AccountId, AccountDetails>, AccountCheckError> {
    let (objects,) = reply.get::<(ManagedObjects,)>().ok_or_else(invalid_reply)?;
    let mut accounts = BTreeMap::new();
    for interfaces in objects.values() {
        let Some(properties) = interfaces.get(ACCOUNT_INTERFACE) else {
            continue;
        };
        let id = AccountId::try_from(read_required::<String>(properties, "Id")?.as_str())?;
        let provider = match read_required::<String>(properties, "ProviderType")?.as_str() {
            "imap_smtp" => AccountProvider::ImapSmtp,
            "google" => AccountProvider::Google,
            "ms_graph" => AccountProvider::Microsoft365,
            _ => AccountProvider::Other,
        };
        let mail = interfaces.get(MAIL_INTERFACE);
        let details = AccountDetails {
            provider,
            mail_enabled: !read_required::<bool>(properties, "MailDisabled")?,
            needs_attention: read_required(properties, "AttentionNeeded")?,
            mail_service_available: mail.is_some(),
            display_name: read_optional_text(properties, "PresentationIdentity")?,
            email_address: mail
                .map(|properties| read_optional_text(properties, "EmailAddress"))
                .transpose()?
                .flatten(),
        };
        accounts.insert(id, details);
    }
    Ok(accounts)
}

/// The property's value, or None when it is absent or has another type.
pub(crate) fn read_property<T: FromVariant>(properties: &Properties, name: &str) -> Option<T> {
    properties.get(name).and_then(Variant::get)
}

fn read_required<T: FromVariant>(
    properties: &Properties,
    name: &str,
) -> Result<T, AccountCheckError> {
    read_property(properties, name).ok_or_else(invalid_reply)
}

fn read_optional_text(
    properties: &Properties,
    name: &str,
) -> Result<Option<String>, AccountCheckError> {
    properties
        .get(name)
        .map(|text| text.get::<String>().ok_or_else(invalid_reply))
        .transpose()
        .map(|text| text.filter(|text| !text.is_empty()))
}

fn invalid_reply() -> AccountCheckError {
    AccountCheckError::new("read accounts", ErrorCause::InvalidReply)
}

pub(crate) fn map_glib_error(operation: &'static str, error: glib::Error) -> AccountCheckError {
    AccountCheckError::new(operation, classify_glib_error(&error))
}

pub(crate) fn classify_glib_error(error: &glib::Error) -> ErrorCause {
    if error.matches(gio::IOErrorEnum::TimedOut)
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
    }
}

#[cfg(test)]
mod tests;
