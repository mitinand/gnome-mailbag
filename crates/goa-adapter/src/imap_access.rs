// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountId, ErrorCause, GoaAdapter,
    accounts::{
        ACCOUNT_INTERFACE, GOA_BUS_NAME, GOA_ROOT_PATH, MAIL_INTERFACE, ManagedObjects,
        OAUTH2_BASED_INTERFACE, OBJECT_MANAGER_INTERFACE, PASSWORD_BASED_INTERFACE, Properties,
        classify_glib_error, read_property,
    },
};
use gio::prelude::*;
use glib::variant::{FromVariant, ObjectPath};

/// GOA's PasswordBased key for the IMAP password of a Generic IMAP account.
const IMAP_PASSWORD_KEY: &str = "imap-password";

/// Settings and credential for one IMAP sign-in, owned by the load that asked
/// for them. There is no Debug, Display or Clone, so the credential is not
/// printed or copied by accident.
pub struct ImapAccess {
    pub account_id: AccountId,
    /// Host as Online Accounts stores it, including an explicit port.
    pub host: String,
    pub login: String,
    pub encryption: ImapEncryption,
    pub credential: ImapCredential,
}

/// What the account signs in with, as Online Accounts keeps it. No Debug,
/// Display or Clone: a token is as sensitive as a password.
pub enum ImapCredential {
    Password(String),
    AccessToken(String),
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
    /// Online Accounts did not return the access token of an OAuth account.
    AccessToken,
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

/// Settings read before the credential is requested.
struct ImapSettings {
    /// The path GOA returned for the account; never built from its ID.
    object_path: ObjectPath,
    host: String,
    login: String,
    encryption: ImapEncryption,
    credential_interface: CredentialInterface,
}

/// Which interface of the account's object holds its credential. GOA's own
/// data model decides this, not the provider type: a Google account exports
/// OAuth2Based and no PasswordBased (004 research §1).
#[derive(Clone, Copy)]
enum CredentialInterface {
    OAuth2Based,
    PasswordBased,
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
                    Ok(settings) => self.read_credential(connection, settings),
                    Err(error) => (self.on_complete)(Err(error)),
                }
            },
        );
    }

    /// Asks the interface the account's object exports for its credential.
    fn read_credential(self, connection: gio::DBusConnection, settings: ImapSettings) {
        let cancellable = self.cancellable.clone();
        let credential_interface = settings.credential_interface;
        let (interface, method, arguments, reply_type, step_failure) = match credential_interface {
            CredentialInterface::OAuth2Based => (
                OAUTH2_BASED_INTERFACE,
                "GetAccessToken",
                None,
                "(si)",
                ImapAccessError::AccessToken,
            ),
            CredentialInterface::PasswordBased => (
                PASSWORD_BASED_INTERFACE,
                "GetPassword",
                Some((IMAP_PASSWORD_KEY,).to_variant()),
                "(s)",
                ImapAccessError::Password,
            ),
        };
        connection.call(
            Some(GOA_BUS_NAME),
            settings.object_path.as_str(),
            interface,
            method,
            arguments.as_ref(),
            Some(glib::VariantTy::new(reply_type).unwrap()),
            gio::DBusCallFlags::NONE,
            self.timeout_msec,
            Some(&cancellable),
            move |reply| {
                let access = reply
                    .map_err(|error| access_error(&error, step_failure))
                    .map(|reply| ImapAccess {
                        account_id: self.account_id,
                        host: settings.host,
                        login: settings.login,
                        encryption: settings.encryption,
                        credential: credential_from(&reply, credential_interface),
                    });
                (self.on_complete)(access);
            },
        );
    }
}

/// GIO checked the reply against the signature its call asked for. A token's
/// `expires_in` is discarded: GOA renews a token close to expiry before it
/// returns one, and a load lasts seconds (004 research §1).
fn credential_from(reply: &glib::Variant, interface: CredentialInterface) -> ImapCredential {
    match interface {
        CredentialInterface::OAuth2Based => {
            ImapCredential::AccessToken(reply.get::<(String, i32)>().unwrap().0)
        }
        CredentialInterface::PasswordBased => {
            ImapCredential::Password(reply.get::<(String,)>().unwrap().0)
        }
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
    // GOA lists an object's interfaces whether or not they carry properties.
    let credential_interface = if interfaces.contains_key(OAUTH2_BASED_INTERFACE) {
        CredentialInterface::OAuth2Based
    } else if interfaces.contains_key(PASSWORD_BASED_INTERFACE) {
        CredentialInterface::PasswordBased
    } else {
        return Err(ImapAccessError::Settings);
    };
    Ok(ImapSettings {
        object_path,
        host: read_setting(mail, "ImapHost")?,
        login: read_setting(mail, "ImapUserName")?,
        encryption,
        credential_interface,
    })
}

fn read_setting<T: FromVariant>(mail: &Properties, name: &str) -> Result<T, ImapAccessError> {
    read_property(mail, name).ok_or(ImapAccessError::Settings)
}

#[cfg(test)]
mod tests;
