// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use gio::prelude::*;
use glib::{Variant, variant::ObjectPath};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

pub const GOA_ROOT_PATH: &str = "/org/gnome/OnlineAccounts";
pub const GOA_BUS_NAME: &str = "org.gnome.OnlineAccounts";
pub const ACCOUNT_INTERFACE: &str = "org.gnome.OnlineAccounts.Account";
pub const MAIL_INTERFACE: &str = "org.gnome.OnlineAccounts.Mail";
pub const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";
pub type Properties = BTreeMap<String, Variant>;
pub type Interfaces = BTreeMap<String, Properties>;
pub type Objects = BTreeMap<ObjectPath, Interfaces>;

pub fn make_account(id: &str) -> Interfaces {
    BTreeMap::from([
        (
            ACCOUNT_INTERFACE.into(),
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
            MAIL_INTERFACE.into(),
            BTreeMap::from([(
                "EmailAddress".into(),
                "synthetic@example.invalid".to_variant(),
            )]),
        ),
    ])
}
pub fn make_object_map(accounts: Vec<Interfaces>) -> Objects {
    accounts
        .into_iter()
        .enumerate()
        .map(|(i, account)| {
            (
                ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_{i}")).unwrap(),
                account,
            )
        })
        .collect()
}
pub fn make_account_reply(accounts: Vec<Interfaces>) -> Variant {
    (make_object_map(accounts),).to_variant()
}

#[derive(Clone)]
pub enum ReplyBehavior {
    Value(Variant),
    AccessDenied,
    Hang,
    /// Emit a changed property before completing this now-outdated request.
    ChangeBeforeCompletion {
        stale_reply: Variant,
        mail_disabled: bool,
    },
    WrongType,
}
#[derive(Clone, Debug)]
pub struct RecordedCall {
    pub received_at: Instant,
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    pub body_type: String,
}

#[derive(Default)]
struct HeldReplies {
    requests: Mutex<Vec<(String, u32)>>,
    call_received: Condvar,
}

pub struct FakeGoaService {
    pub calls: Arc<Mutex<Vec<RecordedCall>>>,
    calls_changed: Arc<Condvar>,
    connection: gio::DBusConnection,
    reply_sequence: Arc<Mutex<VecDeque<ReplyBehavior>>>,
    held_replies: Arc<HeldReplies>,
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}
impl FakeGoaService {
    pub fn complete_held_reply(&self, body: &Variant) {
        let (mut pending_calls, _) = self
            .held_replies
            .call_received
            .wait_timeout_while(
                self.held_replies.requests.lock().unwrap(),
                Duration::from_secs(2),
                |pending_calls| pending_calls.is_empty(),
            )
            .unwrap();
        let (destination, serial) = pending_calls.remove(0);
        let message = gio::DBusMessage::new();
        message.set_message_type(gio::DBusMessageType::MethodReturn);
        message.set_destination(Some(&destination));
        message.set_reply_serial(serial);
        message.set_body(body);
        self.connection
            .send_message(&message, gio::DBusSendMessageFlags::NONE)
            .unwrap();
    }

    pub fn set_reply(&self, reply: ReplyBehavior) {
        *self.reply_sequence.lock().unwrap() = VecDeque::from([reply]);
    }

    pub fn emit_signal(&self, path: &str, interface: &str, member: &str, body: &Variant) {
        self.connection
            .emit_signal(None, path, interface, member, Some(body))
            .unwrap();
    }

    pub fn change_properties(
        &self,
        interface: &str,
        changed_properties: Properties,
        invalidated_properties: Vec<String>,
    ) {
        self.emit_signal(
            &format!("{GOA_ROOT_PATH}/Accounts/account_0"),
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(interface, changed_properties, invalidated_properties).to_variant(),
        );
    }

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

    pub fn new(address: &str, reply_sequence: Vec<ReplyBehavior>) -> Self {
        assert!(!reply_sequence.is_empty());
        let reply_sequence = Arc::new(Mutex::new(VecDeque::from(reply_sequence)));
        let handler_sequence = reply_sequence.clone();
        let held_replies = Arc::new(HeldReplies::default());
        let handler_pending_calls = held_replies.clone();
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
                        let received_at = Instant::now();
                        recorded_filter.lock().unwrap().push(RecordedCall {
                            received_at,
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
                let pending_invocations = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let handler_invocations = pending_invocations.clone();
                let registration = connection.register_object(GOA_ROOT_PATH, &info.interfaces()[0]).method_call(move |connection, _, _, _, _, _, invocation| {
                    let reply = { let mut reply_sequence = handler_sequence.lock().unwrap(); if reply_sequence.len() > 1 { reply_sequence.pop_front().unwrap() } else { reply_sequence.front().unwrap().clone() } };
                    match reply {
                        ReplyBehavior::Value(value) => invocation.return_value(Some(&value)),
                        ReplyBehavior::AccessDenied => invocation.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "synthetic-private-detail"),
                        ReplyBehavior::Hang => {
                            let request = invocation.message();
                            handler_pending_calls.requests.lock().unwrap().push((request.sender().unwrap().into(), request.serial()));
                            handler_pending_calls.call_received.notify_all();
                            handler_invocations.borrow_mut().push(invocation);
                        },
                        ReplyBehavior::ChangeBeforeCompletion { stale_reply, mail_disabled } => {
                            let changed_properties = BTreeMap::from([("MailDisabled", mail_disabled.to_variant())]);
                            connection.emit_signal(None, &format!("{GOA_ROOT_PATH}/Accounts/account_0"), "org.freedesktop.DBus.Properties", "PropertiesChanged", Some(&(ACCOUNT_INTERFACE, changed_properties, Vec::<String>::new()).to_variant())).unwrap();
                            invocation.return_value(Some(&stale_reply));
                        }
                        ReplyBehavior::WrongType => {
                            // Send a reply with the wrong type so the client must reject it.
                            let message = invocation.message().new_method_reply();
                            message.set_body(&("wrong",).to_variant());
                            connection.send_message(&message, gio::DBusSendMessageFlags::NONE).unwrap();
                        }
                    }
                }).build().unwrap();
                connection.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", Some(&(GOA_BUS_NAME, 3u32).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
                let main_loop = glib::MainLoop::new(Some(&context), false);
                let stop_loop = main_loop.clone();
                let deadline = context.spawn_local(async move { glib::timeout_future(Duration::from_secs(10)).await; stop_loop.quit(); });
                ready.send((main_loop.clone(), connection.clone())).unwrap();
                main_loop.run();
                deadline.abort();
                connection.unregister_object(registration).unwrap();
                connection.remove_filter(filter);
                pending_invocations.borrow_mut().clear();
                connection.close_sync(None::<&gio::Cancellable>).unwrap();
            }).unwrap();
        });
        let (main_loop, connection) = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("GOA fixture startup deadline");
        Self {
            connection,
            reply_sequence,
            held_replies,
            calls,
            calls_changed,
            main_loop,
            thread: Some(thread),
        }
    }
}
impl Drop for FakeGoaService {
    fn drop(&mut self) {
        let main_loop = self.main_loop.clone();
        self.main_loop.context().invoke(move || main_loop.quit());
        self.thread.take().unwrap().join().unwrap();
    }
}

/// The private test bus runs this function as the fake GOA service.
#[test]
#[ignore = "private D-Bus activation subprocess; exercised by activation tests"]
fn activated_service_process() {
    let Ok(directory) = std::env::var("MAILBAG_ACTIVATION_DIRECTORY") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let mode = std::fs::read_to_string(directory.join("mode")).unwrap();
    let _service = if mode == "ready" {
        Some(FakeGoaService::new(
            &std::env::var("DBUS_STARTER_ADDRESS").unwrap(),
            vec![ReplyBehavior::Value(make_account_reply(vec![
                make_account("activated"),
            ]))],
        ))
    } else {
        None
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while directory.join("run").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    if mode == "hang" {
        std::process::exit(1);
    }
}
