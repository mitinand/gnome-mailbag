// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use gio::prelude::*;
use glib::{Variant, variant::ObjectPath};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

pub const GOA_ROOT_PATH: &str = "/org/gnome/OnlineAccounts";
pub const GOA_BUS_NAME: &str = "org.gnome.OnlineAccounts";
pub const ACCOUNT_INTERFACE: &str = "org.gnome.OnlineAccounts.Account";
pub const MAIL_INTERFACE: &str = "org.gnome.OnlineAccounts.Mail";
pub const PASSWORD_BASED_INTERFACE: &str = "org.gnome.OnlineAccounts.PasswordBased";
pub const OAUTH2_BASED_INTERFACE: &str = "org.gnome.OnlineAccounts.OAuth2Based";
pub const SYNTHETIC_PASSWORD: &str = "synthetic-password";
pub const SYNTHETIC_ACCESS_TOKEN: &str = "synthetic-access-token";
/// The credential interfaces answer on the paths of the first accounts in
/// make_object_map.
const CREDENTIAL_ACCOUNT_COUNT: usize = 4;
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
            BTreeMap::from([
                (
                    "EmailAddress".into(),
                    "synthetic@example.invalid".to_variant(),
                ),
                ("ImapHost".into(), "imap.example.invalid".to_variant()),
                ("ImapUserName".into(), "synthetic-user".to_variant()),
                ("ImapUseSsl".into(), true.to_variant()),
                ("ImapUseTls".into(), false.to_variant()),
                ("ImapAcceptSslErrors".into(), false.to_variant()),
            ]),
        ),
        // GOA lists an object's interfaces whether or not they hold properties.
        (PASSWORD_BASED_INTERFACE.into(), Properties::new()),
    ])
}

/// A Google account as GOA builds one: OAuth2Based instead of PasswordBased,
/// Gmail's host and implicit TLS (004 research §1).
pub fn make_google_account(id: &str) -> Interfaces {
    let mut account = make_account(id);
    account.remove(PASSWORD_BASED_INTERFACE);
    account.insert(OAUTH2_BASED_INTERFACE.into(), Properties::new());
    let provider = account.get_mut(ACCOUNT_INTERFACE).unwrap();
    provider.insert("ProviderType".into(), "google".to_variant());
    let mail = account.get_mut(MAIL_INTERFACE).unwrap();
    mail.insert("ImapHost".into(), "imap.gmail.com".to_variant());
    mail.insert(
        "ImapUserName".into(),
        "synthetic@gmail.invalid".to_variant(),
    );
    account
}
pub fn account_object_path(index: usize) -> String {
    format!("{GOA_ROOT_PATH}/Accounts/account_{index}")
}
pub fn make_object_map(accounts: Vec<Interfaces>) -> Objects {
    accounts
        .into_iter()
        .enumerate()
        .map(|(i, account)| {
            (
                ObjectPath::try_from(account_object_path(i)).unwrap(),
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
    WrongType,
}

#[derive(Default)]
struct HeldReplies {
    requests: Mutex<Vec<(String, u32)>>,
    call_received: Condvar,
}

/// One GetPassword call as the fixture received it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordRequest {
    pub object_path: String,
    pub password_key: String,
}

/// One GetAccessToken call as the fixture received it. It takes no arguments,
/// so only the object it was sent to distinguishes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessTokenRequest {
    pub object_path: String,
}

fn answer_call(
    behavior: ReplyBehavior,
    connection: &gio::DBusConnection,
    invocation: gio::DBusMethodInvocation,
    held_replies: &HeldReplies,
    held_invocations: &RefCell<Vec<gio::DBusMethodInvocation>>,
) {
    match behavior {
        ReplyBehavior::Value(value) => invocation.return_value(Some(&value)),
        ReplyBehavior::AccessDenied => invocation.return_dbus_error(
            "org.freedesktop.DBus.Error.AccessDenied",
            "synthetic-private-detail",
        ),
        ReplyBehavior::Hang => {
            let request = invocation.message();
            held_replies
                .requests
                .lock()
                .unwrap()
                .push((request.sender().unwrap().into(), request.serial()));
            held_replies.call_received.notify_all();
            held_invocations.borrow_mut().push(invocation);
        }
        ReplyBehavior::WrongType => {
            // Send a reply with the wrong type so the client must reject it.
            let message = invocation.message().new_method_reply();
            message.set_body(&("wrong",).to_variant());
            connection
                .send_message(&message, gio::DBusSendMessageFlags::NONE)
                .unwrap();
        }
    }
}

pub struct FakeGoaService {
    read_count: Arc<AtomicUsize>,
    connection: gio::DBusConnection,
    reply: Arc<Mutex<ReplyBehavior>>,
    password_reply: Arc<Mutex<ReplyBehavior>>,
    password_requests: Arc<Mutex<Vec<PasswordRequest>>>,
    access_token_reply: Arc<Mutex<ReplyBehavior>>,
    access_token_requests: Arc<Mutex<Vec<AccessTokenRequest>>>,
    held_replies: Arc<HeldReplies>,
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}
impl FakeGoaService {
    pub fn read_count(&self) -> usize {
        self.read_count.load(Ordering::Relaxed)
    }

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
        *self.reply.lock().unwrap() = reply;
    }

    pub fn set_password_reply(&self, reply: ReplyBehavior) {
        *self.password_reply.lock().unwrap() = reply;
    }

    pub fn password_requests(&self) -> Vec<PasswordRequest> {
        self.password_requests.lock().unwrap().clone()
    }

    pub fn set_access_token_reply(&self, reply: ReplyBehavior) {
        *self.access_token_reply.lock().unwrap() = reply;
    }

    pub fn access_token_requests(&self) -> Vec<AccessTokenRequest> {
        self.access_token_requests.lock().unwrap().clone()
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

    pub fn new(address: &str, reply: ReplyBehavior) -> Self {
        let reply = Arc::new(Mutex::new(reply));
        let handler_reply = reply.clone();
        let password_reply = Arc::new(Mutex::new(ReplyBehavior::Value(
            (SYNTHETIC_PASSWORD,).to_variant(),
        )));
        let handler_password_reply = password_reply.clone();
        let password_requests = Arc::new(Mutex::new(Vec::new()));
        let handler_password_requests = password_requests.clone();
        // GOA answers GetAccessToken with the token and its remaining seconds.
        let access_token_reply = Arc::new(Mutex::new(ReplyBehavior::Value(
            (SYNTHETIC_ACCESS_TOKEN, 1669_i32).to_variant(),
        )));
        let handler_access_token_reply = access_token_reply.clone();
        let access_token_requests = Arc::new(Mutex::new(Vec::new()));
        let handler_access_token_requests = access_token_requests.clone();
        let held_replies = Arc::new(HeldReplies::default());
        let handler_held_replies = held_replies.clone();
        let address = address.to_owned();
        let read_count = Arc::new(AtomicUsize::new(0));
        let handler_read_count = read_count.clone();
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let context = glib::MainContext::new();
            context.with_thread_default(|| {
                let connection = gio::DBusConnection::for_address_sync(&address,
                    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                    None::<&gio::DBusAuthObserver>, None::<&gio::Cancellable>).unwrap();
                let info = gio::DBusNodeInfo::for_xml(r#"<node><interface name="org.freedesktop.DBus.ObjectManager"><method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method><signal name="InterfacesAdded"><arg type="o"/><arg type="a{sa{sv}}"/></signal><signal name="InterfacesRemoved"><arg type="o"/><arg type="as"/></signal></interface><interface name="org.gnome.OnlineAccounts.PasswordBased"><method name="GetPassword"><arg type="s" direction="in"/><arg type="s" direction="out"/></method></interface><interface name="org.gnome.OnlineAccounts.OAuth2Based"><method name="GetAccessToken"><arg type="s" direction="out"/><arg type="i" direction="out"/></method></interface></node>"#).unwrap();
                let held_invocations = std::rc::Rc::new(RefCell::new(Vec::new()));
                let mut registrations = Vec::new();
                let (held_replies, invocations) = (handler_held_replies.clone(), held_invocations.clone());
                registrations.push(connection.register_object(GOA_ROOT_PATH, &info.interfaces()[0]).method_call(move |connection, _, _, _, _, _, invocation| {
                    handler_read_count.fetch_add(1, Ordering::Relaxed);
                    let reply = handler_reply.lock().unwrap().clone();
                    answer_call(reply, &connection, invocation, &held_replies, &invocations);
                }).build().unwrap());
                for index in 0..CREDENTIAL_ACCOUNT_COUNT {
                    let (password_reply, password_requests) = (handler_password_reply.clone(), handler_password_requests.clone());
                    let (held_replies, invocations) = (handler_held_replies.clone(), held_invocations.clone());
                    registrations.push(connection.register_object(&account_object_path(index), &info.interfaces()[1]).method_call(move |connection, _, object_path, _, _, parameters, invocation| {
                        let (password_key,) = parameters.get::<(String,)>().unwrap();
                        password_requests.lock().unwrap().push(PasswordRequest { object_path: object_path.into(), password_key });
                        let reply = password_reply.lock().unwrap().clone();
                        answer_call(reply, &connection, invocation, &held_replies, &invocations);
                    }).build().unwrap());
                    let (access_token_reply, access_token_requests) = (handler_access_token_reply.clone(), handler_access_token_requests.clone());
                    let (held_replies, invocations) = (handler_held_replies.clone(), held_invocations.clone());
                    registrations.push(connection.register_object(&account_object_path(index), &info.interfaces()[2]).method_call(move |connection, _, object_path, _, _, _, invocation| {
                        access_token_requests.lock().unwrap().push(AccessTokenRequest { object_path: object_path.into() });
                        let reply = access_token_reply.lock().unwrap().clone();
                        answer_call(reply, &connection, invocation, &held_replies, &invocations);
                    }).build().unwrap());
                }
                connection.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", Some(&(GOA_BUS_NAME, 3u32).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
                let main_loop = glib::MainLoop::new(Some(&context), false);
                let stop_loop = main_loop.clone();
                let deadline = context.spawn_local(async move { glib::timeout_future(Duration::from_secs(10)).await; stop_loop.quit(); });
                ready.send((main_loop.clone(), connection.clone())).unwrap();
                main_loop.run();
                deadline.abort();
                for registration in registrations {
                    connection.unregister_object(registration).unwrap();
                }
                held_invocations.borrow_mut().clear();
                connection.close_sync(None::<&gio::Cancellable>).unwrap();
            }).unwrap();
        });
        let (main_loop, connection) = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("GOA fixture startup deadline");
        Self {
            connection,
            reply,
            password_reply,
            password_requests,
            access_token_reply,
            access_token_requests,
            held_replies,
            read_count,
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
