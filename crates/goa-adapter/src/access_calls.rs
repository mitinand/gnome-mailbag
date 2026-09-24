// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the IMAP and the Microsoft 365 access requests share: their error and
//! cancellation types, and the Online Accounts calls both of them make.

use crate::{
    AccountId, ErrorCause,
    accounts::{
        ACCOUNT_INTERFACE, GOA_BUS_NAME, GOA_ROOT_PATH, ManagedObjects, OAUTH2_BASED_INTERFACE,
        OBJECT_MANAGER_INTERFACE, Properties, classify_glib_error, read_property,
    },
};
use gio::prelude::*;
use glib::variant::ObjectPath;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// Online Accounts did not list the account, or its object lacks the
    /// settings or the credential interface the request needs.
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
pub struct AccessRequest(pub(crate) gio::Cancellable);
impl AccessRequest {
    pub fn cancel(&self) {
        self.0.cancel();
    }
}
impl Drop for AccessRequest {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Reads every account object over the observer's connection.
pub(crate) fn read_account_objects(
    connection: &gio::DBusConnection,
    timeout_msec: i32,
    cancellable: &gio::Cancellable,
    on_reply: impl FnOnce(Result<glib::Variant, glib::Error>) + 'static,
) {
    connection.call(
        Some(GOA_BUS_NAME),
        GOA_ROOT_PATH,
        OBJECT_MANAGER_INTERFACE,
        "GetManagedObjects",
        None,
        Some(glib::VariantTy::new("(a{oa{sa{sv}}})").unwrap()),
        gio::DBusCallFlags::NONE,
        timeout_msec,
        Some(cancellable),
        on_reply,
    );
}

/// Asks the account's object for its OAuth access token. `expires_in` is
/// discarded: GOA renews a token close to expiry before it returns one, and a
/// load lasts seconds (specs/004-gmail-integration/research.md §1).
pub(crate) fn read_access_token(
    connection: &gio::DBusConnection,
    object_path: &ObjectPath,
    timeout_msec: i32,
    cancellable: &gio::Cancellable,
    on_reply: impl FnOnce(Result<String, glib::Error>) + 'static,
) {
    connection.call(
        Some(GOA_BUS_NAME),
        object_path.as_str(),
        OAUTH2_BASED_INTERFACE,
        "GetAccessToken",
        None,
        Some(glib::VariantTy::new("(si)").unwrap()),
        gio::DBusCallFlags::NONE,
        timeout_msec,
        Some(cancellable),
        // GIO checked the reply against the signature above.
        move |reply| on_reply(reply.map(|reply| reply.get::<(String, i32)>().unwrap().0)),
    );
}

/// A D-Bus timeout at any step is Timeout; other errors fail their step.
pub(crate) fn access_error(error: &glib::Error, step_failure: AccessError) -> AccessError {
    if error.matches(gio::IOErrorEnum::Cancelled) {
        AccessError::Cancelled
    } else if classify_glib_error(error) == ErrorCause::Timeout {
        AccessError::Timeout
    } else {
        step_failure
    }
}

/// The path GOA returned for the account and the interfaces of its object.
/// An account that is no longer listed is Settings.
pub(crate) fn find_account_object(
    reply: &glib::Variant,
    account_id: &AccountId,
) -> Result<(ObjectPath, BTreeMap<String, Properties>), AccessError> {
    // GIO checked the reply against the signature passed to the call.
    let (objects,) = reply.get::<(ManagedObjects,)>().unwrap();
    objects
        .into_iter()
        .find(|(_, interfaces)| {
            interfaces
                .get(ACCOUNT_INTERFACE)
                .and_then(|account| read_property::<String>(account, "Id"))
                .is_some_and(|id| id == account_id.as_str())
        })
        .ok_or(AccessError::Settings)
}

/// Without the observer's bus connection there is no account list to select
/// from. The failure is reported on the context's next turn, like every other
/// answer, so a caller never sees its completion run inside its own call; a
/// request cancelled before that turn completes as Cancelled.
pub(crate) fn report_without_connection<T: 'static>(
    cancellable: gio::Cancellable,
    on_complete: Box<dyn FnOnce(Result<T, AccessError>)>,
) {
    glib::MainContext::ref_thread_default().spawn_local(async move {
        let error = match cancellable.is_cancelled() {
            true => AccessError::Cancelled,
            false => AccessError::Settings,
        };
        on_complete(Err(error));
    });
}
