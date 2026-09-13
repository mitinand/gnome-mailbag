// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use std::cell::Cell;
#[allow(dead_code)]
#[path = "../../../../tests/support/bus.rs"]
mod bus;

#[test]
fn settings_protocol_retry_timeout_and_shutdown() {
    let context = glib::MainContext::new();
    context.with_thread_default(|| context.block_on(async {
        let bus = bus::TestBus::new();
        let connect = || gio::DBusConnection::for_address_sync(&bus.address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None::<&gio::DBusAuthObserver>, None::<&gio::Cancellable>).unwrap();
        let service = connect();
        service.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
            "org.freedesktop.DBus", "RequestName", Some(&("org.gnome.Settings", 0_u32).to_variant()),
            None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
        let mode = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        let held = Rc::new(RefCell::new(Vec::new()));
        let info = gio::DBusNodeInfo::for_xml("<node><interface name='org.gtk.Actions'><method name='Activate'><arg type='s' direction='in'/><arg type='av' direction='in'/><arg type='a{sv}' direction='in'/></method></interface></node>").unwrap();
        let registration = service.register_object("/org/gnome/Settings", &info.interfaces()[0])
            .method_call({ let mode=mode.clone(); let calls=calls.clone(); let held=held.clone();
                move |_, _, path, interface, method, body, invocation| {
                    assert_eq!(path, "/org/gnome/Settings");
                    assert_eq!(interface, Some("org.gtk.Actions"));
                    assert_eq!(method, "Activate");
                    assert_eq!(body.type_().as_str(), "(sava{sv})");
                    assert_eq!(body.child_value(0).str(), Some("launch-panel"));
                    let args=body.child_value(1);
                    assert_eq!(args.n_children(), 1);
                    let panel=args.child_value(0).as_variant().unwrap();
                    assert_eq!(panel.type_().as_str(), "(sav)");
                    assert_eq!(panel.child_value(0).str(), Some("online-accounts"));
                    assert_eq!(panel.child_value(1).n_children(), 0);
                    assert_eq!(body.child_value(2).n_children(), 0);
                    calls.set(calls.get()+1);
                    match mode.get() {
                        0 => invocation.return_value(Some(&().to_variant())),
                        1 => invocation.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "private diagnostic"),
                        3 => invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", "private diagnostic"),
                        _ => held.borrow_mut().push(invocation),
                    }
                }
            }).build().unwrap();
        let outcomes=Rc::new(RefCell::new(Vec::new()));
        let launcher=Rc::new(SettingsLauncher { timeout: Duration::from_millis(200),
            report: { let outcomes=outcomes.clone(); Rc::new(move |pending,error| outcomes.borrow_mut().push((pending,error))) },
            attempt: RefCell::new(LaunchAttempt::default()) });
        let connection=connect();
        launcher.open_on(&connection);
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        assert_eq!(calls.get(),1);
        assert_eq!(outcomes.borrow().last(),Some(&(false,None)));
        mode.set(1);
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        assert_eq!(outcomes.borrow().last(),Some(&(false,Some(LaunchError::AccessDenied))));
        mode.set(3);
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        assert_eq!(outcomes.borrow().last(),Some(&(false,Some(LaunchError::InvalidReply))));
        mode.set(2);
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        assert_eq!(outcomes.borrow().last(),Some(&(false,Some(LaunchError::Timeout))));
        mode.set(0);
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        let count=outcomes.borrow().len();
        for invocation in held.borrow_mut().drain(..) { invocation.return_value(Some(&().to_variant())); }
        glib::timeout_future(Duration::from_millis(10)).await;
        assert_eq!(outcomes.borrow().len(),count);
        service.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "ReleaseName", Some(&("org.gnome.Settings",).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
        launcher.open_on(&connection);
        wait_finished(&launcher).await;
        assert_eq!(outcomes.borrow().last(),Some(&(false,Some(LaunchError::Unavailable))));
        service.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", Some(&("org.gnome.Settings",0_u32).to_variant()), None, gio::DBusCallFlags::NONE, 1000, None::<&gio::Cancellable>).unwrap();
        // The same deadline applies before bus acquisition has completed.
        let calls_before = calls.get();
        let (expired_serial, _) = launcher.begin_launch().unwrap();
        wait_finished(&launcher).await;
        assert_eq!(outcomes.borrow().last(),Some(&(false,Some(LaunchError::Timeout))));
        launcher.call_panel(expired_serial, &connection);
        assert_eq!(calls.get(), calls_before);
        mode.set(2);
        launcher.open_on(&connection);
        glib::timeout_future(Duration::from_millis(10)).await;
        launcher.stop();
        let count=outcomes.borrow().len();
        for invocation in held.borrow_mut().drain(..) { invocation.return_value(Some(&().to_variant())); }
        glib::timeout_future(Duration::from_millis(240)).await;
        assert_eq!(outcomes.borrow().len(),count);
        service.unregister_object(registration).unwrap();
        connection.close_sync(None::<&gio::Cancellable>).unwrap();
        service.close_sync(None::<&gio::Cancellable>).unwrap();
    })).unwrap();
}
async fn wait_finished(launcher: &SettingsLauncher) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while launcher.attempt.borrow().cancellable.is_some() {
        assert!(std::time::Instant::now() < deadline);
        glib::timeout_future(Duration::from_millis(1)).await;
    }
}
