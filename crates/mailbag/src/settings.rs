// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use adw::{gio, glib, prelude::*};
use std::{cell::Cell, collections::BTreeMap, future::Future, rc::Rc};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchError {
    Unavailable,
    AccessDenied,
    Timeout,
    InvalidReply,
}
impl LaunchError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unavailable => {
                "Could not open Online Accounts. Open Settings and choose Online Accounts, or try again."
            }
            Self::AccessDenied => {
                "Mailbag was denied access to Settings. Open Online Accounts from Settings."
            }
            Self::Timeout => "Settings did not respond in time. Try opening Online Accounts again.",
            Self::InvalidReply => {
                "Settings could not open Online Accounts. Open the panel from Settings or try again."
            }
        }
    }
    fn from_error(error: &glib::Error) -> Self {
        if error.matches(gio::DBusError::AccessDenied)
            || error.matches(gio::IOErrorEnum::PermissionDenied)
        {
            Self::AccessDenied
        } else if error.matches(gio::DBusError::Timeout)
            || error.matches(gio::DBusError::NoReply)
            || error.matches(gio::IOErrorEnum::TimedOut)
        {
            Self::Timeout
        } else if error.matches(gio::DBusError::InvalidArgs)
            || error.matches(gio::IOErrorEnum::InvalidArgument)
        {
            Self::InvalidReply
        } else {
            Self::Unavailable
        }
    }
}

/// Shares one pending launch across the window's Online Accounts actions.
pub struct SettingsLauncher {
    launch_pending: Cell<bool>,
    on_error: Box<dyn Fn(LaunchError)>,
}

impl SettingsLauncher {
    pub fn new(on_error: impl Fn(LaunchError) + 'static) -> Rc<Self> {
        Rc::new(Self {
            launch_pending: Cell::new(false),
            on_error: Box::new(on_error),
        })
    }

    pub fn open(self: &Rc<Self>) {
        self.open_with_connection(gio::bus_get_future(gio::BusType::Session));
    }

    fn open_with_connection(
        self: &Rc<Self>,
        connection_request: impl Future<Output = Result<gio::DBusConnection, glib::Error>> + 'static,
    ) {
        if self.launch_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::MainContext::ref_thread_default().spawn_local(async move {
            let result = async {
                let connection = connection_request.await?;
                activate_online_accounts(&connection).await
            }
            .await
            .map_err(|error| LaunchError::from_error(&error));
            match result {
                Ok(()) => tracing::info!("Online Accounts opened in Settings"),
                Err(error) => tracing::error!(
                    cause = ?error,
                    "Online Accounts could not be opened in Settings"
                ),
            }
            if let Some(launcher) = weak.upgrade() {
                launcher.launch_pending.set(false);
                if let Err(error) = result {
                    (launcher.on_error)(error);
                }
            }
        });
    }
}

async fn activate_online_accounts(connection: &gio::DBusConnection) -> Result<(), glib::Error> {
    let panel = ("online-accounts", Vec::<glib::Variant>::new()).to_variant();
    let body = (
        "launch-panel",
        vec![panel],
        BTreeMap::<String, glib::Variant>::new(),
    )
        .to_variant();
    connection
        .call_future(
            Some("org.gnome.Settings"),
            "/org/gnome/Settings",
            "org.gtk.Actions",
            "Activate",
            Some(&body),
            Some(glib::VariantTy::UNIT),
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .map(|_| ())
}
