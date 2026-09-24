// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountId, GoaAdapter,
    access_calls::{
        AccessError, AccessRequest, access_error, find_account_object, read_access_token,
        read_account_objects, report_without_connection,
    },
    accounts::{
        GOA_BUS_NAME, MAIL_INTERFACE, OAUTH2_BASED_INTERFACE, PASSWORD_BASED_INTERFACE, Properties,
        read_property,
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

impl GoaAdapter {
    /// Reads the current IMAP settings and credential of one account for a load.
    ///
    /// Call on the adapter's GLib context; `on_complete` runs on it exactly
    /// once and never inside this call, also after `cancel()` or dropping the
    /// request. Without the observer's bus connection there is no account
    /// list, and the request fails as Settings. Observed accounts never change.
    pub fn request_imap_access(
        &self,
        account_id: &AccountId,
        on_complete: impl FnOnce(Result<ImapAccess, AccessError>) + 'static,
    ) -> AccessRequest {
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
            None => report_without_connection(attempt.cancellable, attempt.on_complete),
        }
        AccessRequest(cancellable)
    }
}

/// One request in progress. Each step's callback owns it, so it completes once.
struct AccessAttempt {
    account_id: AccountId,
    cancellable: gio::Cancellable,
    timeout_msec: i32,
    on_complete: Box<dyn FnOnce(Result<ImapAccess, AccessError>)>,
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
enum CredentialInterface {
    OAuth2Based,
    PasswordBased,
}

impl AccessAttempt {
    fn read_settings(self, connection: gio::DBusConnection) {
        let cancellable = self.cancellable.clone();
        read_account_objects(
            &connection.clone(),
            self.timeout_msec,
            &cancellable,
            move |reply| {
                let settings = reply
                    .map_err(|error| access_error(&error, AccessError::Settings))
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
        let object_path = settings.object_path.clone();
        match settings.credential_interface {
            CredentialInterface::OAuth2Based => read_access_token(
                &connection,
                &object_path,
                self.timeout_msec,
                &cancellable,
                move |reply| {
                    let credential = reply
                        .map(ImapCredential::AccessToken)
                        .map_err(|error| access_error(&error, AccessError::AccessToken));
                    self.finish(settings, credential);
                },
            ),
            CredentialInterface::PasswordBased => connection.call(
                Some(GOA_BUS_NAME),
                object_path.as_str(),
                PASSWORD_BASED_INTERFACE,
                "GetPassword",
                Some(&(IMAP_PASSWORD_KEY,).to_variant()),
                Some(glib::VariantTy::new("(s)").unwrap()),
                gio::DBusCallFlags::NONE,
                self.timeout_msec,
                Some(&cancellable),
                move |reply| {
                    // GIO checked the reply against the signature above.
                    let credential = reply
                        .map(|reply| ImapCredential::Password(reply.get::<(String,)>().unwrap().0))
                        .map_err(|error| access_error(&error, AccessError::Password));
                    self.finish(settings, credential);
                },
            ),
        }
    }

    fn finish(self, settings: ImapSettings, credential: Result<ImapCredential, AccessError>) {
        (self.on_complete)(credential.map(|credential| ImapAccess {
            account_id: self.account_id,
            host: settings.host,
            login: settings.login,
            encryption: settings.encryption,
            credential,
        }));
    }
}

fn find_imap_settings(
    reply: &glib::Variant,
    account_id: &AccountId,
) -> Result<ImapSettings, AccessError> {
    let (object_path, interfaces) = find_account_object(reply, account_id)?;
    let mail = interfaces
        .get(MAIL_INTERFACE)
        .ok_or(AccessError::Settings)?;
    // ImapAcceptSslErrors is ignored: certificates are always verified.
    let encryption = match (
        read_setting(mail, "ImapUseSsl")?,
        read_setting(mail, "ImapUseTls")?,
    ) {
        (true, _) => ImapEncryption::ImplicitTls,
        (false, true) => ImapEncryption::StartTls,
        (false, false) => return Err(AccessError::NoEncryption),
    };
    // GOA lists an object's interfaces whether or not they carry properties.
    let credential_interface = if interfaces.contains_key(OAUTH2_BASED_INTERFACE) {
        CredentialInterface::OAuth2Based
    } else if interfaces.contains_key(PASSWORD_BASED_INTERFACE) {
        CredentialInterface::PasswordBased
    } else {
        return Err(AccessError::Settings);
    };
    Ok(ImapSettings {
        object_path,
        host: read_setting(mail, "ImapHost")?,
        login: read_setting(mail, "ImapUserName")?,
        encryption,
        credential_interface,
    })
}

fn read_setting<T: FromVariant>(mail: &Properties, name: &str) -> Result<T, AccessError> {
    read_property(mail, name).ok_or(AccessError::Settings)
}

#[cfg(test)]
mod tests;
