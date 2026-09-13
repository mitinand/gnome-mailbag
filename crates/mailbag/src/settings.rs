// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use adw::{gio, glib, prelude::*};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Duration};

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
#[derive(Default)]
struct LaunchAttempt {
    serial: u64,
    cancellable: Option<gio::Cancellable>,
    deadline: Option<glib::JoinHandle<()>>,
    stopped: bool,
}

/// Coalesces Settings requests from every Online Accounts action in the window.
pub struct SettingsLauncher {
    attempt: RefCell<LaunchAttempt>,
    timeout: Duration,
    report: Rc<dyn Fn(bool, Option<LaunchError>)>,
}
impl SettingsLauncher {
    pub fn new(report: impl Fn(bool, Option<LaunchError>) + 'static) -> Rc<Self> {
        Rc::new(Self {
            attempt: RefCell::new(LaunchAttempt::default()),
            timeout: Duration::from_secs(5),
            report: Rc::new(report),
        })
    }
    pub fn open(self: &Rc<Self>) {
        let Some((serial, cancellable)) = self.begin_launch() else {
            return;
        };
        let weak = Rc::downgrade(self);
        gio::bus_get(gio::BusType::Session, Some(&cancellable), move |result| {
            if let Some(launcher) = weak.upgrade() {
                match result {
                    Ok(connection) => launcher.call_panel(serial, &connection),
                    Err(error) => launcher.finish(serial, Some(LaunchError::from_error(&error))),
                }
            }
        });
    }
    fn begin_launch(self: &Rc<Self>) -> Option<(u64, gio::Cancellable)> {
        let mut attempt = self.attempt.borrow_mut();
        if attempt.stopped || attempt.cancellable.is_some() {
            return None;
        }
        attempt.serial += 1;
        let serial = attempt.serial;
        let cancellable = gio::Cancellable::new();
        attempt.cancellable = Some(cancellable.clone());
        let weak = Rc::downgrade(self);
        // Attach to the caller's context, also used by GIO's asynchronous callbacks.
        let timeout = self.timeout;
        attempt.deadline = Some(
            glib::MainContext::ref_thread_default().spawn_local(async move {
                glib::timeout_future(timeout).await;
                if let Some(launcher) = weak.upgrade()
                    && launcher.attempt.borrow().serial == serial
                {
                    launcher.attempt.borrow_mut().deadline.take();
                    launcher.finish(serial, Some(LaunchError::Timeout));
                }
            }),
        );
        drop(attempt);
        (self.report)(true, None);
        Some((serial, cancellable))
    }
    fn call_panel(self: &Rc<Self>, serial: u64, connection: &gio::DBusConnection) {
        let attempt = self.attempt.borrow();
        if attempt.stopped || attempt.serial != serial {
            return;
        }
        let Some(cancellable) = attempt.cancellable.clone() else {
            return;
        };
        drop(attempt);
        let panel = ("online-accounts", Vec::<glib::Variant>::new()).to_variant();
        let body = (
            "launch-panel",
            vec![panel],
            BTreeMap::<String, glib::Variant>::new(),
        )
            .to_variant();
        let weak = Rc::downgrade(self);
        connection.call(
            Some("org.gnome.Settings"),
            "/org/gnome/Settings",
            "org.gtk.Actions",
            "Activate",
            Some(&body),
            Some(glib::VariantTy::UNIT),
            gio::DBusCallFlags::NONE,
            -1,
            Some(&cancellable),
            move |result| {
                if let Some(launcher) = weak.upgrade() {
                    launcher.finish(serial, result.err().as_ref().map(LaunchError::from_error));
                }
            },
        );
    }
    fn finish(&self, serial: u64, error: Option<LaunchError>) {
        let mut attempt = self.attempt.borrow_mut();
        if attempt.stopped || attempt.serial != serial || attempt.cancellable.is_none() {
            return;
        }
        let cancellable = attempt.cancellable.take().unwrap();
        let deadline = attempt.deadline.take();
        drop(attempt);
        if let Some(deadline) = deadline {
            deadline.abort();
        }
        cancellable.cancel();
        (self.report)(false, error);
    }
    pub fn stop(&self) {
        let mut attempt = self.attempt.borrow_mut();
        attempt.stopped = true;
        let cancellable = attempt.cancellable.take();
        let deadline = attempt.deadline.take();
        drop(attempt);
        if let Some(deadline) = deadline {
            deadline.abort();
        }
        if let Some(cancellable) = cancellable {
            cancellable.cancel();
        }
    }
    #[cfg(test)]
    fn open_on(self: &Rc<Self>, connection: &gio::DBusConnection) {
        if let Some((serial, _)) = self.begin_launch() {
            self.call_panel(serial, connection);
        }
    }
}
impl Drop for SettingsLauncher {
    fn drop(&mut self) {
        self.stop();
    }
}
