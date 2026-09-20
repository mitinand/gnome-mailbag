// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use adw::{gio, glib, gtk, prelude::*};

mod account_ui;
mod accounts;
mod inbox;
mod inbox_load;
mod mail_ui;
mod settings;
mod window_ui;

#[cfg(test)]
#[path = "accounts/notice_tests.rs"]
mod account_notice_tests;
#[cfg(test)]
#[path = "accounts/tests.rs"]
mod account_tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/support/bus.rs"]
mod test_bus;

const APP_ID: &str = "io.github.mitinand.Mailbag";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| register_resources());
    app.connect_activate(build_window);
    app.run()
}

fn register_resources() {
    gio::resources_register_include!("mailbag.gresource").expect("bundled account icons");
    gtk::IconTheme::for_display(&gtk::gdk::Display::default().expect("GTK display"))
        .add_resource_path("/io/github/mitinand/Mailbag/icons");
}

fn build_window(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let builder = create_window(app);
    let window: adw::Window = builder.object("window").expect("mailbag.ui: window");
    connect_account_updates(&builder, &window);
}

fn create_window(app: &adw::Application) -> gtk::Builder {
    let builder = gtk::Builder::from_string(include_str!("../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").expect("mailbag.ui: window");
    app.add_window(&window);

    let split: adw::OverlaySplitView = builder
        .object("folders_split")
        .expect("mailbag.ui: folders_split");
    register_action(app, "folders", Some("<Primary><Shift>s"), move || {
        split.set_show_sidebar(!split.shows_sidebar());
    });

    let window_weak = window.downgrade();
    register_action(app, "shortcuts", Some("<Primary>question"), move || {
        if let Some(window) = window_weak.upgrade() {
            let builder = gtk::Builder::from_string(include_str!("../resources/ui/shortcuts.ui"));
            let dialog: adw::ShortcutsDialog = builder
                .object("shortcuts_dialog")
                .expect("shortcuts.ui: shortcuts_dialog");
            dialog.present(Some(&window));
        }
    });

    // Mail data and operations are not implemented yet.
    for name in ["search_button", "unread_filter"] {
        builder
            .object::<gtk::Widget>(name)
            .expect("mailbag.ui: mail control")
            .set_sensitive(false);
    }

    let app_weak = app.downgrade();
    register_action(app, "quit", Some("<Primary>q"), move || {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
    });

    let window_weak = window.downgrade();
    register_action(app, "about", None, move || {
        if let Some(window) = window_weak.upgrade() {
            adw::AboutDialog::builder()
                .application_name("Mailbag")
                .application_icon(APP_ID)
                .developer_name("Andrey Mitin")
                .version(env!("CARGO_PKG_VERSION"))
                .license_type(gtk::License::Gpl30)
                .website("https://github.com/mitinand/gnome-mailbag")
                .build()
                .present(Some(&window));
        }
    });
    window.present();
    builder
}

fn register_action(
    app: &impl IsA<gtk::Application>,
    name: &str,
    shortcut: Option<&str>,
    activate: impl Fn() + 'static,
) {
    let app = app.as_ref();
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, _| activate());
    app.add_action(&action);
    if let Some(shortcut) = shortcut {
        app.set_accels_for_action(&format!("app.{name}"), &[shortcut]);
    }
}

fn connect_account_updates(builder: &gtk::Builder, window: &adw::Window) {
    // The observer reports to the window, which the adapter it uses belongs
    // to, so the window is connected once both exist.
    let updated_window: std::rc::Rc<std::cell::RefCell<std::rc::Weak<window_ui::WindowUi>>> =
        std::rc::Rc::default();
    let update_target = updated_window.clone();
    let adapter = goa_adapter::GoaAdapter::start(move |update| {
        if let Some(ui) = update_target.borrow().upgrade() {
            ui.apply_account_update(update);
        }
    });
    let window_ui = window_ui::WindowUi::new(
        builder,
        Box::new(inbox_load::MailLoader::new(adapter.clone())),
    );
    *updated_window.borrow_mut() = std::rc::Rc::downgrade(&window_ui);
    let refresh_adapter = adapter.clone();
    window_ui
        .accounts()
        .borrow()
        .connect_retry_check(move || refresh_adapter.refresh_accounts());
    let settings_ui = std::rc::Rc::downgrade(window_ui.accounts());
    let launcher = settings::SettingsLauncher::new(move |error| {
        if let Some(ui) = settings_ui.upgrade() {
            ui.borrow().show_settings_error(error);
        }
    });
    let action_launcher = launcher.clone();
    let app = window.application().expect("application window");
    register_action(&app, "accounts", None, move || action_launcher.open());
    app.add_action(window_ui.refresh_action());
    let held_window = std::cell::RefCell::new(Some(window_ui));
    window.connect_destroy(move |_| {
        app.remove_action("accounts");
        app.remove_action("refresh-inbox");
        if let Some(ui) = held_window.borrow_mut().take() {
            // The worker closes its connection on its own thread.
            ui.cancel_loads();
        }
        adapter.stop();
    });
}
