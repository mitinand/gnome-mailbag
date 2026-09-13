// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use gio::prelude::*;
use glib::{Variant, variant::ObjectPath};
use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::Duration,
};

pub const ROOT: &str = "/org/gnome/OnlineAccounts";
pub const NAME: &str = "org.gnome.OnlineAccounts";
pub const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
pub const MAIL: &str = "org.gnome.OnlineAccounts.Mail";
pub const MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
pub type Properties = BTreeMap<String, Variant>;
pub type Interfaces = BTreeMap<String, Properties>;
pub type Objects = BTreeMap<ObjectPath, Interfaces>;

pub fn account(id: &str) -> Interfaces {
    BTreeMap::from([
        (
            ACCOUNT.into(),
            BTreeMap::from([
                ("Id".into(), id.to_variant()),
                ("ProviderType".into(), "imap_smtp".to_variant()),
                ("MailDisabled".into(), false.to_variant()),
                ("AttentionNeeded".into(), false.to_variant()),
                (
                    "PresentationIdentity".into(),
                    "Synthetic account".to_variant(),
                ),
            ]),
        ),
        (
            MAIL.into(),
            BTreeMap::from([(
                "EmailAddress".into(),
                "synthetic@example.invalid".to_variant(),
            )]),
        ),
    ])
}
pub fn objects(accounts: Vec<Interfaces>) -> Objects {
    accounts
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            (
                ObjectPath::try_from(format!("{ROOT}/Accounts/account_{i}")).unwrap(),
                a,
            )
        })
        .collect()
}
pub fn reply(accounts: Vec<Interfaces>) -> Variant {
    (objects(accounts),).to_variant()
}

#[derive(Clone)]
pub enum Reply {
    Value(Variant),
    Error,
    Hang,
    /// Emit a changed property before completing this now-outdated request.
    ChangeBeforeCompletion {
        old: Variant,
        disabled: bool,
    },
    WrongType,
}
#[derive(Clone, Debug)]
pub struct Call {
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    pub body_type: String,
}

pub struct Goa {
    pub calls: Arc<Mutex<Vec<Call>>>,
    calls_changed: Arc<Condvar>,
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}
impl Goa {
    pub fn wait_for_calls(&self, count: usize) {
        let (calls, _) = self
            .calls_changed
            .wait_timeout_while(
                self.calls.lock().unwrap(),
                Duration::from_secs(2),
                |calls| calls.len() < count,
            )
            .unwrap();
        assert!(calls.len() >= count, "fixture call deadline");
    }

    pub fn new(address: &str, replies: Vec<Reply>) -> Self {
        assert!(!replies.is_empty());
        let address = address.to_owned();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let calls_changed = Arc::new(Condvar::new());
        let notify_calls = calls_changed.clone();
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let context = glib::MainContext::new();
            context.with_thread_default(|| {
                let connection = gio::DBusConnection::for_address_sync(&address,
                    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                    None::<&gio::DBusAuthObserver>, None::<&gio::Cancellable>).unwrap();
                let recorded_filter = recorded.clone();
                let filter = connection.add_filter(move |_, message, incoming| {
                    if incoming && message.message_type() == gio::DBusMessageType::MethodCall {
                        recorded_filter.lock().unwrap().push(Call {
                            destination: message.destination().unwrap_or_default().into(),
                            path: message.path().unwrap_or_default().into(),
                            interface: message.interface().unwrap_or_default().into(),
                            method: message.member().unwrap_or_default().into(),
                            body_type: message.body().map(|v| v.type_().to_string()).unwrap_or_else(|| "()".into()),
                        });
                        notify_calls.notify_all();
                    }
                    Some(message.clone())
                });
                let info = gio::DBusNodeInfo::for_xml(r#"<node><interface name="org.freedesktop.DBus.ObjectManager"><method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method><signal name="InterfacesAdded"><arg type="o"/><arg type="a{sa{sv}}"/></signal><signal name="InterfacesRemoved"><arg type="o"/><arg type="as"/></signal></interface></node>"#).unwrap();
                let mut sequence = replies.into_iter(); let mut last = sequence.next().unwrap();
                let next = std::cell::RefCell::new(move || { let result = last.clone(); if let Some(value) = sequence.next() { last = value; } result });
                let pending = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let held = pending.clone();
                let registration = connection.register_object(ROOT, &info.interfaces()[0]).method_call(move |conn, _, _, _, _, _, invocation| {
                    match next.borrow_mut()() {
                        Reply::Value(value) => invocation.return_value(Some(&value)),
                        Reply::Error => invocation.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "synthetic-private-detail"),
                        Reply::Hang => held.borrow_mut().push(invocation),
                        Reply::ChangeBeforeCompletion { old, disabled } => {
                            let changed = BTreeMap::from([("MailDisabled", disabled.to_variant())]);
                            conn.emit_signal(None, &format!("{ROOT}/Accounts/account_0"), "org.freedesktop.DBus.Properties", "PropertiesChanged", Some(&(ACCOUNT, changed, Vec::<String>::new()).to_variant())).unwrap();
                            invocation.return_value(Some(&old));
                        }
                        Reply::WrongType => {
                            // Bypass invocation's reply validation to exercise the client's wire check.
                            let message = invocation.message().new_method_reply();
                            message.set_body(&("wrong",).to_variant());
                            conn.send_message(&message, gio::DBusSendMessageFlags::NONE).unwrap();
                        }
                    }
                }).build().unwrap();
                connection.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", Some(&(NAME, 0u32).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
                let main_loop = glib::MainLoop::new(Some(&context), false);
                let stop_loop = main_loop.clone();
                let deadline = context.spawn_local(async move { glib::timeout_future(Duration::from_secs(10)).await; stop_loop.quit(); });
                ready.send(main_loop.clone()).unwrap();
                main_loop.run();
                deadline.abort();
                connection.unregister_object(registration).unwrap();
                connection.remove_filter(filter);
                pending.borrow_mut().clear();
                connection.close_sync(None::<&gio::Cancellable>).unwrap();
            }).unwrap();
        });
        Self {
            calls,
            calls_changed,
            main_loop: receiver
                .recv_timeout(Duration::from_secs(3))
                .expect("GOA fixture startup deadline"),
            thread: Some(thread),
        }
    }
}
impl Drop for Goa {
    fn drop(&mut self) {
        let main_loop = self.main_loop.clone();
        self.main_loop.context().invoke(move || main_loop.quit());
        self.thread.take().unwrap().join().unwrap();
    }
}
