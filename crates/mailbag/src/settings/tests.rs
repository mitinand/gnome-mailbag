// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::test_bus::TestBus;
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum SettingsReply {
    Accepted,
    AccessDenied,
    WrongType,
}

#[test]
fn settings_requests_share_one_launch_and_use_the_exact_panel_parameters() {
    run_in_context(async {
        let bus = TestBus::new();
        let service = FakeSettings::new(&bus.address);
        let (launcher, errors) = test_launcher();
        launcher.open_with_connection(connect_to_bus(&bus.address));
        launcher.open_with_connection(connect_to_bus(&bus.address));
        wait_for_completion(&launcher).await;
        assert_eq!(service.request_count.get(), 1);
        assert!(
            errors.borrow().is_empty(),
            "success produces no notification"
        );
        launcher.open_with_connection(connect_to_bus(&bus.address));
        wait_for_completion(&launcher).await;
        assert_eq!(service.request_count.get(), 2);
        assert!(errors.borrow().is_empty());
    });
}

#[test]
fn settings_errors_are_reported_once_and_allow_another_attempt() {
    run_in_context(async {
        let bus = TestBus::new();
        let service = FakeSettings::new(&bus.address);
        let (launcher, errors) = test_launcher();
        for (reply, expected) in [
            (SettingsReply::AccessDenied, LaunchError::AccessDenied),
            (SettingsReply::WrongType, LaunchError::Unavailable),
        ] {
            service.reply.set(reply);
            launcher.open_with_connection(connect_to_bus(&bus.address));
            wait_for_completion(&launcher).await;
            assert_eq!(*errors.borrow(), [expected]);
            assert!(!format!("{:?}", errors.borrow()).contains("private diagnostic"));
            errors.borrow_mut().clear();
        }
        service.release_name();
        launcher.open_with_connection(connect_to_bus(&bus.address));
        wait_for_completion(&launcher).await;
        assert_eq!(*errors.borrow(), [LaunchError::Unavailable]);
        errors.borrow_mut().clear();
        service.request_name();
        service.reply.set(SettingsReply::Accepted);
        launcher.open_with_connection(connect_to_bus(&bus.address));
        wait_for_completion(&launcher).await;
        assert!(errors.borrow().is_empty());
    });
}

fn run_in_context(test: impl Future<Output = ()>) {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(test))
        .unwrap();
}

fn connect_to_bus(
    address: &str,
) -> impl Future<Output = Result<gio::DBusConnection, glib::Error>> + use<> {
    gio::DBusConnection::for_address_future(
        address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None::<&gio::DBusAuthObserver>,
    )
}

fn test_launcher() -> (Rc<SettingsLauncher>, Rc<RefCell<Vec<LaunchError>>>) {
    let errors = Rc::new(RefCell::new(Vec::new()));
    let recorded_errors = errors.clone();
    let launcher = SettingsLauncher::new(move |error| recorded_errors.borrow_mut().push(error));
    (launcher, errors)
}

async fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline, "Settings test deadline");
        glib::timeout_future(Duration::from_millis(1)).await;
    }
}

async fn wait_for_completion(launcher: &SettingsLauncher) {
    wait_until(|| !launcher.launch_pending.get()).await;
}

struct FakeSettings {
    connection: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    reply: Rc<Cell<SettingsReply>>,
    request_count: Rc<Cell<usize>>,
}

impl FakeSettings {
    fn new(address: &str) -> Self {
        let connection = gio::DBusConnection::for_address_sync(
            address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None::<&gio::DBusAuthObserver>,
            None::<&gio::Cancellable>,
        )
        .unwrap();
        let reply = Rc::new(Cell::new(SettingsReply::Accepted));
        let request_count = Rc::new(Cell::new(0));
        let info = gio::DBusNodeInfo::for_xml(
            "<node><interface name='org.gtk.Actions'><method name='Activate'>
                <arg type='s' direction='in'/><arg type='av' direction='in'/>
                <arg type='a{sv}' direction='in'/>
            </method></interface></node>",
        )
        .unwrap();
        let registration = connection
            .register_object("/org/gnome/Settings", &info.interfaces()[0])
            .method_call({
                let reply = reply.clone();
                let request_count = request_count.clone();
                move |connection, _, path, interface, method, body, invocation| {
                    assert_eq!(path, "/org/gnome/Settings");
                    assert_eq!(interface, Some("org.gtk.Actions"));
                    assert_eq!(method, "Activate");
                    assert_online_accounts_parameters(&body);
                    request_count.set(request_count.get() + 1);
                    match reply.get() {
                        SettingsReply::Accepted => invocation.return_value(Some(&().to_variant())),
                        SettingsReply::AccessDenied => invocation.return_dbus_error(
                            "org.freedesktop.DBus.Error.AccessDenied",
                            "private diagnostic",
                        ),
                        SettingsReply::WrongType => {
                            let message = invocation.message().new_method_reply();
                            message.set_body(&("wrong",).to_variant());
                            connection
                                .send_message(&message, gio::DBusSendMessageFlags::NONE)
                                .unwrap();
                        }
                    }
                }
            })
            .build()
            .unwrap();
        let service = Self {
            connection,
            registration: Some(registration),
            reply,
            request_count,
        };
        service.request_name();
        service
    }

    fn request_name(&self) {
        self.connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&("org.gnome.Settings", 0_u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                None::<&gio::Cancellable>,
            )
            .unwrap();
    }

    fn release_name(&self) {
        self.connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ReleaseName",
                Some(&("org.gnome.Settings",).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                None::<&gio::Cancellable>,
            )
            .unwrap();
    }
}

impl Drop for FakeSettings {
    fn drop(&mut self) {
        self.connection
            .unregister_object(self.registration.take().unwrap())
            .unwrap();
        self.connection
            .close_sync(None::<&gio::Cancellable>)
            .unwrap();
    }
}

fn assert_online_accounts_parameters(body: &glib::Variant) {
    assert_eq!(body.type_().as_str(), "(sava{sv})");
    assert_eq!(body.child_value(0).str(), Some("launch-panel"));
    let parameters = body.child_value(1);
    assert_eq!(parameters.n_children(), 1);
    let panel = parameters.child_value(0).as_variant().unwrap();
    assert_eq!(panel.type_().as_str(), "(sav)");
    assert_eq!(panel.child_value(0).str(), Some("online-accounts"));
    assert_eq!(panel.child_value(1).n_children(), 0);
    assert_eq!(body.child_value(2).n_children(), 0);
}
