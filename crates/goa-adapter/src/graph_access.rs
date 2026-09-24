// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The access token of a Microsoft 365 account. Its object in Online Accounts
//! holds no server settings, only the credential
//! (specs/002-imap-integration/contracts/goa-access.md, amendment by 005).

use crate::{
    AccountId, GoaAdapter,
    access_calls::{
        AccessError, AccessRequest, access_error, find_account_object, read_access_token,
        read_account_objects, report_without_connection,
    },
    accounts::OAUTH2_BASED_INTERFACE,
};
use glib::variant::ObjectPath;

/// The access token of one Microsoft 365 account, owned by the load that asked
/// for it. No Debug, Display or Clone: a token is as sensitive as a password.
pub struct GraphAccess {
    pub account_id: AccountId,
    pub access_token: String,
}

impl GoaAdapter {
    /// Reads the current access token of one Microsoft 365 account for a load.
    ///
    /// Call on the adapter's GLib context; `on_complete` runs on it exactly
    /// once and never inside this call, also after `cancel()` or dropping the
    /// request. Without the observer's bus connection there is no account
    /// list, and the request fails as Settings. Observed accounts never change.
    pub fn request_graph_access(
        &self,
        account_id: &AccountId,
        on_complete: impl FnOnce(Result<GraphAccess, AccessError>) + 'static,
    ) -> AccessRequest {
        let cancellable = gio::Cancellable::new();
        let attempt = GraphAccessAttempt {
            account_id: account_id.clone(),
            cancellable: cancellable.clone(),
            timeout_msec: self.0.timeout_msec,
            on_complete: Box::new(on_complete),
        };
        let connection = self.0.connection.borrow().clone();
        match connection {
            Some(connection) => attempt.read_objects(connection),
            None => report_without_connection(attempt.cancellable, attempt.on_complete),
        }
        AccessRequest(cancellable)
    }
}

/// One request in progress. Each step's callback owns it, so it completes once.
struct GraphAccessAttempt {
    account_id: AccountId,
    cancellable: gio::Cancellable,
    timeout_msec: i32,
    on_complete: Box<dyn FnOnce(Result<GraphAccess, AccessError>)>,
}

impl GraphAccessAttempt {
    fn read_objects(self, connection: gio::DBusConnection) {
        let cancellable = self.cancellable.clone();
        read_account_objects(
            &connection.clone(),
            self.timeout_msec,
            &cancellable,
            move |reply| {
                let object_path = reply
                    .map_err(|error| access_error(&error, AccessError::Settings))
                    .and_then(|reply| find_account_object(&reply, &self.account_id))
                    .and_then(|(object_path, interfaces)| {
                        // GOA lists an object's interfaces whether or not they
                        // carry properties.
                        if interfaces.contains_key(OAUTH2_BASED_INTERFACE) {
                            Ok(object_path)
                        } else {
                            Err(AccessError::Settings)
                        }
                    });
                match object_path {
                    Ok(object_path) => self.read_token(&connection, &object_path),
                    Err(error) => (self.on_complete)(Err(error)),
                }
            },
        );
    }

    fn read_token(self, connection: &gio::DBusConnection, object_path: &ObjectPath) {
        let cancellable = self.cancellable.clone();
        read_access_token(
            connection,
            object_path,
            self.timeout_msec,
            &cancellable,
            move |reply| {
                let access = reply
                    .map(|access_token| GraphAccess {
                        account_id: self.account_id,
                        access_token,
                    })
                    .map_err(|error| access_error(&error, AccessError::AccessToken));
                (self.on_complete)(access);
            },
        );
    }
}

#[cfg(test)]
mod tests;
