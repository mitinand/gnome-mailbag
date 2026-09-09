// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use adw::{gio, glib, gtk, prelude::*};

const APP_ID: &str = "io.github.mitinand.Mailbag";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_window);
    app.run()
}

fn build_window(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let menu = gio::Menu::new();
    menu.append(Some("About Mailbag"), Some("app.about"));
    menu.append(Some("Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&menu)
        .build();
    let header = adw::HeaderBar::new();
    header.pack_end(&menu_button);
    let status = adw::StatusPage::builder()
        .icon_name("mail-unread-symbolic")
        .title("Mailbag")
        .description(
            "A personal email reader for GNOME.
Development has just started.",
        )
        .build();
    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.set_content(Some(&status));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Mailbag")
        .default_width(960)
        .default_height(640)
        .content(&content)
        .build();

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
                .application_icon("mail-unread-symbolic")
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
}
