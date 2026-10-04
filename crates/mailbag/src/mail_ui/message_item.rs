// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The messages list's row object: one stored message as its row shows it.
//! The row template in message-row.ui binds its labels, its unread dot and
//! its star to these properties, so a changed read state or star updates
//! the shown row in place (specs/009-synchronization/research.md §9); the
//! star's icon and colour follow the star and the pointer over the row
//! (specs/011-read-and-star FR-004). The texts are made when a shown row
//! reads them, since a folder may list 100 000 messages and only a
//! screenful is shown. `shown` and `transition-ms` drive the row's
//! revealer, which animates its arrival and leaving
//! (specs/010-message-list/research.md §10).

use super::{row_date_text, sender_text, subject_text};
use adw::{glib, prelude::*, subclass::prelude::*};
use mailbag_domain::MessageListRow;
use std::cell::{Cell, OnceCell};

mod imp {
    use super::*;
    use std::marker::PhantomData;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::MessageItem)]
    pub struct MessageItem {
        /// The stored row the item was made from; its read state and star
        /// may be older than `unread` and `starred`.
        pub(super) listed: OnceCell<MessageListRow>,
        #[property(get = Self::sender)]
        sender: PhantomData<String>,
        #[property(get = Self::subject)]
        subject: PhantomData<String>,
        #[property(get = Self::date_text)]
        date_text: PhantomData<String>,
        #[property(get = Self::preview)]
        preview: PhantomData<String>,
        #[property(get, set = Self::set_unread)]
        pub(super) unread: Cell<bool>,
        #[property(get, set = Self::set_starred)]
        pub(super) starred: Cell<bool>,
        /// Whether the pointer is over the row.
        #[property(get, set = Self::set_pointed)]
        pointed: Cell<bool>,
        /// The row's star: filled while starred, the outline while the
        /// pointer is over the row, none otherwise.
        #[property(get = Self::star_icon)]
        star_icon: PhantomData<String>,
        /// The star's colour: the warning colour while starred, dimmed as
        /// the trash icon is otherwise.
        #[property(get = Self::star_style)]
        star_style: PhantomData<glib::StrV>,
        /// "Read" or "Unread", and "starred", which the row speaks in place
        /// of the decorative dot and star.
        #[property(get = Self::row_state_text)]
        row_state_text: PhantomData<String>,
        /// Whether the row is open; a row arriving or leaving is closed.
        #[property(get, set, default = true)]
        pub(super) shown: Cell<bool>,
        /// How long the row takes to open or close; 0 changes it at once.
        #[property(get, set)]
        transition_ms: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageItem {
        const NAME: &'static str = "MessageItem";
        type Type = super::MessageItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageItem {}

    impl MessageItem {
        fn listed(&self) -> &MessageListRow {
            self.listed.get().expect("made from a row")
        }

        fn sender(&self) -> String {
            sender_text(&self.listed().fields)
        }

        fn subject(&self) -> String {
            subject_text(&self.listed().fields)
        }

        fn date_text(&self) -> String {
            row_date_text(self.listed().received_unix)
        }

        /// The stored preview; the row's two lines cut it further.
        fn preview(&self) -> String {
            self.listed().preview.clone()
        }

        fn set_unread(&self, unread: bool) {
            if self.unread.replace(unread) != unread {
                self.obj().notify_row_state_text();
            }
        }

        fn set_starred(&self, starred: bool) {
            if self.starred.replace(starred) != starred {
                self.obj().notify_row_state_text();
                self.notify_star();
            }
        }

        fn set_pointed(&self, pointed: bool) {
            if self.pointed.replace(pointed) != pointed {
                self.notify_star();
            }
        }

        fn notify_star(&self) {
            self.obj().notify_star_icon();
            self.obj().notify_star_style();
        }

        fn star_icon(&self) -> String {
            match (self.starred.get(), self.pointed.get()) {
                (true, _) => "starred-symbolic",
                (false, true) => "non-starred-symbolic",
                (false, false) => "",
            }
            .to_owned()
        }

        fn star_style(&self) -> glib::StrV {
            match self.starred.get() {
                true => glib::StrV::from(["warning"]),
                false => glib::StrV::from(["dim-label"]),
            }
        }

        fn row_state_text(&self) -> String {
            match (self.unread.get(), self.starred.get()) {
                (true, false) => "Unread",
                (false, false) => "Read",
                (true, true) => "Unread, starred",
                (false, true) => "Read, starred",
            }
            .to_owned()
        }
    }
}

glib::wrapper! {
    pub struct MessageItem(ObjectSubclass<imp::MessageItem>);
}

impl MessageItem {
    /// The row object of a stored message, with the list's text rules.
    pub fn new(row: MessageListRow) -> Self {
        let item: Self = glib::Object::new();
        item.imp().shown.set(true);
        item.imp().unread.set(!row.seen);
        item.imp().starred.set(row.flagged);
        item.imp()
            .listed
            .set(row)
            .expect("a new item holds no row yet");
        item
    }

    /// The stored row the item was made from, for the reader's envelope and
    /// for comparing with a newer read.
    pub fn listed(&self) -> &MessageListRow {
        self.imp().listed.get().expect("made from a row")
    }

    /// Whether the item shows `row` apart from its read state and star,
    /// which change in place; a new preview, such as a draft's edited
    /// elsewhere, needs a new item.
    pub fn lists_same_message(&self, row: &MessageListRow) -> bool {
        let listed = self.listed();
        listed.identity == row.identity
            && listed.fields == row.fields
            && listed.received_unix == row.received_unix
            && listed.preview == row.preview
    }

    /// Opens or closes the row over `transition_ms`.
    pub fn show_over(&self, shown: bool, transition_ms: u32) {
        self.set_transition_ms(transition_ms);
        self.set_shown(shown);
    }
}
