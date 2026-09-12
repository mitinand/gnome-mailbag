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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a graphical GTK session"]
    fn empty_window_and_about() {
        adw::init().expect("GTK display");
        let app = adw::Application::builder()
            .application_id("io.github.mitinand.Mailbag.LayoutTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        build_window(&app);
        let window = app.windows()[0].clone().downcast::<adw::Window>().unwrap();
        assert!(window.content().unwrap().is::<adw::ToastOverlay>());
        assert_eq!(window.default_width(), 1440);
        app.lookup_action("about").unwrap().activate(None);
        let about = window
            .visible_dialog()
            .unwrap()
            .downcast::<adw::AboutDialog>()
            .unwrap();
        assert_eq!(about.application_name(), "Mailbag");
        assert_eq!(about.application_icon(), APP_ID);
        about.force_close();
        app.lookup_action("shortcuts").unwrap().activate(None);
        assert!(
            window
                .visible_dialog()
                .unwrap()
                .is::<adw::ShortcutsDialog>()
        );
        window.destroy();
    }
}
