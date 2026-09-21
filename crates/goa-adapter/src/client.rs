// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountId, AccountUpdate,
    accounts::{
        GOA_ACCOUNT_PATH_PREFIX, GOA_BUS_NAME, GOA_ROOT_PATH, OBJECT_MANAGER_INTERFACE,
        map_glib_error, parse_accounts,
    },
};
use gio::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

/// One bus signal that means the account list may have changed.
struct AccountChangeSignal {
    sender: &'static str,
    interface: &'static str,
    member: &'static str,
    /// Exact object path, when every interesting signal arrives on one path.
    path: Option<&'static str>,
    first_argument: Option<&'static str>,
    /// Accept only paths below this prefix when signals arrive on many paths.
    accepted_path_prefix: Option<&'static str>,
}

/// GOA property changes arrive on individual account paths, so they are
/// subscribed without a path and filtered by prefix instead.
const ACCOUNT_CHANGE_SIGNALS: [AccountChangeSignal; 4] = [
    AccountChangeSignal {
        sender: "org.freedesktop.DBus",
        interface: "org.freedesktop.DBus",
        member: "NameOwnerChanged",
        path: Some("/org/freedesktop/DBus"),
        first_argument: Some(GOA_BUS_NAME),
        accepted_path_prefix: None,
    },
    AccountChangeSignal {
        sender: GOA_BUS_NAME,
        interface: OBJECT_MANAGER_INTERFACE,
        member: "InterfacesAdded",
        path: Some(GOA_ROOT_PATH),
        first_argument: None,
        accepted_path_prefix: None,
    },
    AccountChangeSignal {
        sender: GOA_BUS_NAME,
        interface: OBJECT_MANAGER_INTERFACE,
        member: "InterfacesRemoved",
        path: Some(GOA_ROOT_PATH),
        first_argument: None,
        accepted_path_prefix: None,
    },
    AccountChangeSignal {
        sender: GOA_BUS_NAME,
        interface: "org.freedesktop.DBus.Properties",
        member: "PropertiesChanged",
        path: None,
        first_argument: None,
        accepted_path_prefix: Some(GOA_ACCOUNT_PATH_PREFIX),
    },
];

/// A local handle to GOA observation on the calling thread's GLib main context.
/// Clones share one observer; dropping the last handle stops it.
#[derive(Clone)]
pub struct GoaAdapter(pub(crate) Rc<AccountObserver>);

pub(crate) struct AccountObserver {
    on_update: Box<dyn Fn(&AccountUpdate)>,
    update: RefCell<Rc<AccountUpdate>>,
    pub(crate) connection: RefCell<Option<gio::DBusConnection>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    read_cancellable: RefCell<Option<gio::Cancellable>>,
    refetch_needed: Cell<bool>,
    stopped: Cell<bool>,
    #[cfg(test)]
    bus_address: Option<String>,
    pub(crate) timeout_msec: i32,
}

impl GoaAdapter {
    /// Subscribe before the initial full read. Updates run on the current GLib
    /// context; keep that context running and use this handle on the same thread.
    pub fn start(on_update: impl Fn(&AccountUpdate) + 'static) -> Self {
        let observer = Rc::new(AccountObserver::new(on_update));
        observer.request_read();
        Self(observer)
    }

    /// Request an explicit Retry, preserving the previous result until completion.
    pub fn refresh_accounts(&self) {
        if self.0.stopped.get() {
            return;
        }
        let was_pending = {
            let mut update = self.0.update.borrow_mut();
            std::mem::replace(&mut Rc::make_mut(&mut update).retry_pending, true)
        };
        self.0.request_read();
        if !was_pending {
            self.0.publish_update();
        }
    }

    /// Cancel the current read and unsubscribe. Repeated calls are harmless.
    pub fn stop(&self) {
        self.0.stop();
    }

    #[cfg(test)]
    fn start_for_test(
        address: String,
        timeout_msec: i32,
        on_update: impl Fn(&AccountUpdate) + 'static,
    ) -> Self {
        let mut observer = AccountObserver::new(on_update);
        observer.bus_address = Some(address);
        observer.timeout_msec = timeout_msec;
        let observer = Rc::new(observer);
        observer.request_read();
        Self(observer)
    }
}

impl AccountObserver {
    fn new(on_update: impl Fn(&AccountUpdate) + 'static) -> Self {
        Self {
            on_update: Box::new(on_update),
            update: RefCell::new(Rc::new(AccountUpdate::default())),
            connection: RefCell::new(None),
            subscriptions: RefCell::new(Vec::new()),
            read_cancellable: RefCell::new(None),
            refetch_needed: Cell::new(false),
            stopped: Cell::new(false),
            #[cfg(test)]
            bus_address: None,
            timeout_msec: -1,
        }
    }

    fn request_read(self: &Rc<Self>) {
        if self.stopped.get() {
            return;
        }
        if self.read_cancellable.borrow().is_some() {
            self.refetch_needed.set(true);
            return;
        }
        let cancellable = gio::Cancellable::new();
        self.read_cancellable.replace(Some(cancellable.clone()));
        let connection = self.connection.borrow().clone();
        if let Some(connection) = connection {
            self.read_accounts(&connection, &cancellable);
        } else {
            self.connect_to_bus(&cancellable);
        }
    }

    fn connect_to_bus(self: &Rc<Self>, cancellable: &gio::Cancellable) {
        let weak = Rc::downgrade(self);
        let connected = move |result: Result<gio::DBusConnection, glib::Error>| {
            let Some(observer) = weak.upgrade().filter(|observer| !observer.stopped.get()) else {
                return;
            };
            match result {
                Ok(connection) => {
                    observer.subscribe_to_changes(&connection);
                    observer.connection.replace(Some(connection.clone()));
                    let cancellable = observer.read_cancellable.borrow().as_ref().unwrap().clone();
                    observer.read_accounts(&connection, &cancellable);
                }
                Err(error) => {
                    observer.finish_read(Err(map_glib_error("connect to session bus", error)))
                }
            }
        };
        #[cfg(test)]
        if let Some(address) = &self.bus_address {
            gio::DBusConnection::for_address(
                address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None::<&gio::DBusAuthObserver>,
                Some(cancellable),
                connected,
            );
            return;
        }
        gio::bus_get(gio::BusType::Session, Some(cancellable), connected);
    }

    fn read_accounts(
        self: &Rc<Self>,
        connection: &gio::DBusConnection,
        cancellable: &gio::Cancellable,
    ) {
        let weak = Rc::downgrade(self);
        connection.call(
            Some(GOA_BUS_NAME),
            GOA_ROOT_PATH,
            OBJECT_MANAGER_INTERFACE,
            "GetManagedObjects",
            None,
            Some(glib::VariantTy::new("(a{oa{sa{sv}}})").unwrap()),
            gio::DBusCallFlags::NONE,
            self.timeout_msec,
            Some(cancellable),
            move |reply| {
                if let Some(observer) = weak.upgrade().filter(|observer| !observer.stopped.get()) {
                    observer.finish_read(
                        reply
                            .map_err(|error| map_glib_error("read accounts", error))
                            .and_then(|reply| parse_accounts(&reply)),
                    );
                }
            },
        );
    }

    fn subscribe_to_changes(self: &Rc<Self>, connection: &gio::DBusConnection) {
        for signal in ACCOUNT_CHANGE_SIGNALS {
            let weak = Rc::downgrade(self);
            let accepted_path_prefix = signal.accepted_path_prefix;
            let member = signal.member;
            self.subscriptions
                .borrow_mut()
                .push(connection.subscribe_to_signal(
                    Some(signal.sender),
                    Some(signal.interface),
                    Some(signal.member),
                    signal.path,
                    signal.first_argument,
                    gio::DBusSignalFlags::NONE,
                    move |received| {
                        if accepted_path_prefix
                            .is_none_or(|prefix| received.object_path.starts_with(prefix))
                            && let Some(observer) = weak.upgrade()
                        {
                            tracing::debug!(signal = member, "Online Accounts signalled a change");
                            observer.request_read();
                        }
                    },
                ));
        }
    }

    fn finish_read(
        self: &Rc<Self>,
        result: Result<BTreeMap<AccountId, AccountDetails>, AccountCheckError>,
    ) {
        self.read_cancellable.borrow_mut().take();
        {
            let mut update = self.update.borrow_mut();
            let update = Rc::make_mut(&mut update);
            match result {
                Ok(accounts) => {
                    tracing::info!(accounts = accounts.len(), "account list read");
                    update.accounts = accounts;
                    update.last_check = AccountCheckResult::Complete;
                }
                Err(error) => {
                    tracing::error!(
                        step = error.operation,
                        cause = ?error.cause,
                        "account list could not be read"
                    );
                    update.last_check = AccountCheckResult::Failed(error);
                }
            }
            if !self.refetch_needed.get() {
                update.retry_pending = false;
            }
        }
        // Settle scheduling before calling application code, which may retry or stop.
        if self.refetch_needed.replace(false) {
            self.request_read();
        }
        self.publish_update();
    }

    fn publish_update(&self) {
        let update = self.update.borrow().clone();
        (self.on_update)(&update);
    }

    fn stop(&self) {
        self.stopped.set(true);
        if let Some(cancellable) = self.read_cancellable.borrow_mut().take() {
            cancellable.cancel();
        }
        self.subscriptions.borrow_mut().clear();
        self.connection.borrow_mut().take();
    }
}

impl Drop for AccountObserver {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod event_tests;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
pub(crate) mod tests;
