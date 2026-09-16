// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use adw::{gio, glib, gtk, prelude::*};

mod account_ui;
mod accounts;
mod settings;

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
    let folders = gio::SimpleAction::new("folders", None);
    folders.connect_activate(move |_, _| split.set_show_sidebar(!split.shows_sidebar()));
    app.add_action(&folders);
    app.set_accels_for_action("app.folders", &["<Primary><Shift>s"]);

    let shortcuts = gio::SimpleAction::new("shortcuts", None);
    let window_weak = window.downgrade();
    shortcuts.connect_activate(move |_, _| {
        if let Some(window) = window_weak.upgrade() {
            let builder = gtk::Builder::from_string(include_str!("../resources/ui/shortcuts.ui"));
            let dialog: adw::ShortcutsDialog = builder
                .object("shortcuts_dialog")
                .expect("shortcuts.ui: shortcuts_dialog");
            dialog.present(Some(&window));
        }
    });
    app.add_action(&shortcuts);
    app.set_accels_for_action("app.shortcuts", &["<Primary>question"]);

    // Mail data and operations are not implemented yet.
    for name in ["search_button", "unread_filter"] {
        builder
            .object::<gtk::Widget>(name)
            .expect("mailbag.ui: mail control")
            .set_sensitive(false);
    }

    let quit = gio::SimpleAction::new("quit", None);
    let app_weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);

    let about = gio::SimpleAction::new("about", None);
    let window_weak = window.downgrade();
    about.connect_activate(move |_, _| {
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
    app.add_action(&about);
    window.present();
    builder
}

fn connect_account_updates(builder: &gtk::Builder, window: &adw::Window) {
    let account_ui = account_ui::AccountUi::new(builder);
    let weak_ui = std::rc::Rc::downgrade(&account_ui);
    let adapter = goa_adapter::GoaAdapter::start(move |update| {
        if let Some(ui) = weak_ui.upgrade() {
            ui.borrow_mut().apply_update(update);
        }
    });
    let refresh_adapter = adapter.clone();
    account_ui
        .borrow()
        .connect_retry_check(move || refresh_adapter.refresh_accounts());
    let settings_ui = std::rc::Rc::downgrade(&account_ui);
    let launcher = settings::SettingsLauncher::new(move |error| {
        if let Some(ui) = settings_ui.upgrade() {
            ui.borrow().show_settings_error(error);
        }
    });
    let action = gio::SimpleAction::new("accounts", None);
    let action_launcher = launcher.clone();
    action.connect_activate(move |_, _| action_launcher.open());
    let app = window.application().expect("application window");
    app.add_action(&action);
    let window_ui = std::cell::RefCell::new(Some(account_ui));
    window.connect_destroy(move |_| {
        app.remove_action("accounts");
        window_ui.borrow_mut().take();
        adapter.stop();
    });
}
