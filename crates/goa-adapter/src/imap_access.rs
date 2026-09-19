// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountId, ErrorCause, GoaAdapter,
    accounts::{
        ACCOUNT_INTERFACE, GOA_BUS_NAME, GOA_ROOT_PATH, MAIL_INTERFACE, ManagedObjects,
        OBJECT_MANAGER_INTERFACE, PASSWORD_BASED_INTERFACE, Properties, classify_glib_error,
        read_property,
    },
};
use gio::prelude::*;
use glib::variant::{FromVariant, ObjectPath};

/// GOA's PasswordBased key for the IMAP password of a Generic IMAP account.
const IMAP_PASSWORD_KEY: &str = "imap-password";

/// Settings and password for one IMAP sign-in, owned by the load that asked
/// for them. There is no Debug, Display or Clone, so the password is not
/// printed or copied by accident.
pub struct ImapAccess {
    pub account_id: AccountId,
    /// Host as Online Accounts stores it, including an explicit port.
    pub host: String,
    pub login: String,
    pub encryption: ImapEncryption,
    pub password: String,
}

/// How the IMAP connection is secured. There is no unencrypted option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImapEncryption {
    /// TLS from the first byte ("SSL on a dedicated port" in Online Accounts).
    ImplicitTls,
    /// A plain connection that must switch to TLS with STARTTLS before sign-in.
    StartTls,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImapAccessError {
    /// Online Accounts did not return the account's IMAP settings.
    Settings,
    /// Neither SSL nor STARTTLS is set, so no password was requested.
    NoEncryption,
    /// Online Accounts did not return the password.
    Password,
    /// Online Accounts did not answer in time, for example while GetPassword
    /// waits for the keyring to be unlocked.
    Timeout,
    Cancelled,
}

/// Cancels its access request when cancelled or dropped. Holds no credentials.
pub struct ImapAccessRequest(gio::Cancellable);
impl ImapAccessRequest {
    pub fn cancel(&self) {
        self.0.cancel();
    }
}
impl Drop for ImapAccessRequest {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl GoaAdapter {
    /// Reads the current IMAP settings and password of one account for a load.
    ///
    /// Call on the adapter's GLib context; `on_complete` runs on it exactly
    /// once, also after `cancel()` or dropping the request. Without the
    /// observer's bus connection there is no account list, and the request
    /// completes at once with Settings. Observed accounts never change.
    pub fn request_imap_access(
        &self,
        account_id: &AccountId,
        on_complete: impl FnOnce(Result<ImapAccess, ImapAccessError>) + 'static,
    ) -> ImapAccessRequest {
        let cancellable = gio::Cancellable::new();
        let attempt = AccessAttempt {
            account_id: account_id.clone(),
            cancellable: cancellable.clone(),
            timeout_msec: self.0.timeout_msec,
            on_complete: Box::new(on_complete),
        };
        let connection = self.0.connection.borrow().clone();
        match connection {
            Some(connection) => attempt.read_settings(connection),
            None => (attempt.on_complete)(Err(ImapAccessError::Settings)),
        }
        ImapAccessRequest(cancellable)
    }
}

/// One request in progress. Each step's callback owns it, so it completes once.
struct AccessAttempt {
    account_id: AccountId,
    cancellable: gio::Cancellable,
    timeout_msec: i32,
    on_complete: Box<dyn FnOnce(Result<ImapAccess, ImapAccessError>)>,
}

/// Settings read before the password is requested.
struct ImapSettings {
    /// The path GOA returned for the account; never built from its ID.
    object_path: ObjectPath,
    host: String,
    login: String,
    encryption: ImapEncryption,
}

impl AccessAttempt {
    fn read_settings(self, connection: gio::DBusConnection) {
        let cancellable = self.cancellable.clone();
        connection.clone().call(
            Some(GOA_BUS_NAME),
            GOA_ROOT_PATH,
            OBJECT_MANAGER_INTERFACE,
            "GetManagedObjects",
            None,
            Some(glib::VariantTy::new("(a{oa{sa{sv}}})").unwrap()),
            gio::DBusCallFlags::NONE,
            self.timeout_msec,
            Some(&cancellable),
            move |reply| {
                let settings = reply
                    .map_err(|error| access_error(&error, ImapAccessError::Settings))
                    .and_then(|reply| find_imap_settings(&reply, &self.account_id));
                match settings {
                    Ok(settings) => self.read_password(connection, settings),
                    Err(error) => (self.on_complete)(Err(error)),
                }
            },
        );
    }

    fn read_password(self, connection: gio::DBusConnection, settings: ImapSettings) {
        let cancellable = self.cancellable.clone();
        connection.call(
            Some(GOA_BUS_NAME),
            settings.object_path.as_str(),
            PASSWORD_BASED_INTERFACE,
            "GetPassword",
            Some(&(IMAP_PASSWORD_KEY,).to_variant()),
            Some(glib::VariantTy::new("(s)").unwrap()),
            gio::DBusCallFlags::NONE,
            self.timeout_msec,
            Some(&cancellable),
            move |reply| {
                let access = reply
                    .map_err(|error| access_error(&error, ImapAccessError::Password))
                    .map(|reply| ImapAccess {
                        account_id: self.account_id,
                        host: settings.host,
                        login: settings.login,
                        encryption: settings.encryption,
                        // GIO checked the reply against the (s) signature.
                        password: reply.get::<(String,)>().unwrap().0,
                    });
                (self.on_complete)(access);
            },
        );
    }
}

/// A D-Bus timeout at either step is Timeout; other errors fail their step.
fn access_error(error: &glib::Error, step_failure: ImapAccessError) -> ImapAccessError {
    if error.matches(gio::IOErrorEnum::Cancelled) {
        ImapAccessError::Cancelled
    } else if classify_glib_error(error) == ErrorCause::Timeout {
        ImapAccessError::Timeout
    } else {
        step_failure
    }
}

fn find_imap_settings(
    reply: &glib::Variant,
    account_id: &AccountId,
) -> Result<ImapSettings, ImapAccessError> {
    // GIO checked the reply against the signature passed to the call.
    let (objects,) = reply.get::<(ManagedObjects,)>().unwrap();
    let (object_path, interfaces) = objects
        .into_iter()
        .find(|(_, interfaces)| {
            interfaces
                .get(ACCOUNT_INTERFACE)
                .and_then(|account| read_property::<String>(account, "Id"))
                .is_some_and(|id| id == account_id.as_str())
        })
        .ok_or(ImapAccessError::Settings)?;
    let mail = interfaces
        .get(MAIL_INTERFACE)
        .ok_or(ImapAccessError::Settings)?;
    // ImapAcceptSslErrors is ignored: certificates are always verified.
    let encryption = match (
        read_setting(mail, "ImapUseSsl")?,
        read_setting(mail, "ImapUseTls")?,
    ) {
        (true, _) => ImapEncryption::ImplicitTls,
        (false, true) => ImapEncryption::StartTls,
        (false, false) => return Err(ImapAccessError::NoEncryption),
    };
    Ok(ImapSettings {
        object_path,
        host: read_setting(mail, "ImapHost")?,
        login: read_setting(mail, "ImapUserName")?,
        encryption,
    })
}

fn read_setting<T: FromVariant>(mail: &Properties, name: &str) -> Result<T, ImapAccessError> {
    read_property(mail, name).ok_or(ImapAccessError::Settings)
}

#[cfg(test)]
mod tests;
