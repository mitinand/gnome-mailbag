// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shows one account's received mail in the approved list and reader.
//!
//! Opening a message uses the text the load already received; it sends no
//! request and changes nothing on the server.

#[cfg(test)]
mod tests;

use crate::failure_dialog::{show_action_button, status_description};
use adw::{gio, glib, gtk, prelude::*};
use mailbag_content::DisplayFields;
use mailbag_providers::{DeclaredFailure, ReceivedBatch, ReceivedContent};
use std::{cell::RefCell, rc::Rc};

/// How much text a GTK label shows, in UTF-8 bytes. Longer text is cut at a
/// character boundary without an explanation; the stored text keeps its
/// full length.
const DISPLAY_LIMIT_BYTES: usize = 65_536;

/// The longest run of characters without a place to break a line that the
/// reader still wraps by word.
///
/// Wrapping by word searches each line for a break; in a run that has none
/// the search costs time proportional to the square of its length, so a
/// 64 KiB line takes minutes and freezes the window, while wrapping by
/// character takes under a second. Mail is conventionally wrapped near 72
/// columns, so a longer run means the sender wrapped nothing and breaking
/// inside it is expected anyway.
const LONGEST_WORD_WRAPPED_RUN: usize = 100;

pub struct MailUi {
    messages: gtk::ListBox,
    rows: gio::ListStore,
    list_title: adw::WindowTitle,
    list_page: adw::NavigationPage,
    mail_split: adw::NavigationSplitView,
    reader_stack: gtk::Stack,
    /// Holds the reader's message widgets, which exist once for the window.
    singleton_slot: gtk::Box,
    reader_subject: gtk::Label,
    reader_sender: gtk::Label,
    reader_to: gtk::Label,
    reader_date: gtk::Label,
    reader_body: gtk::Label,
    /// Holds the body; hidden while a content problem takes its place.
    body_slot: gtk::Box,
    /// Why this message shows no text, in the body's place.
    content_status: adw::StatusPage,
    content_action: gtk::Button,
    sender_avatar: adw::Avatar,
    /// The batch the rows were built from, to rebuild them only when the
    /// shown account or its mail changed.
    shown_batch: RefCell<Option<Rc<ReceivedBatch>>>,
}

/// One row's message: the batch it belongs to and its place in it.
struct ListedMessage {
    batch: Rc<ReceivedBatch>,
    position: usize,
}

impl MailUi {
    pub fn new(builder: &gtk::Builder) -> Rc<Self> {
        let messages: gtk::ListBox = builder.object("messages").expect("mailbag.ui: messages");
        let reader = build_reader(builder);
        let rows = gio::ListStore::new::<glib::BoxedAnyObject>();
        messages.bind_model(Some(&rows), |listed| build_message_row(listed).upcast());
        let mail = Rc::new(Self {
            messages: messages.clone(),
            rows,
            list_title: builder
                .object("list_title")
                .expect("mailbag.ui: list_title"),
            list_page: builder.object("list_page").expect("mailbag.ui: list_page"),
            mail_split: builder
                .object("mail_split")
                .expect("mailbag.ui: mail_split"),
            reader_stack: builder
                .object("reader_stack")
                .expect("mailbag.ui: reader_stack"),
            singleton_slot: builder
                .object("singleton_slot")
                .expect("mailbag.ui: singleton_slot"),
            reader_subject: builder
                .object("reader_subject")
                .expect("mailbag.ui: reader_subject"),
            reader_sender: reader.sender,
            reader_to: reader.to,
            reader_date: reader.date,
            reader_body: reader.body,
            body_slot: reader.body_slot,
            content_status: reader.content_status,
            content_action: reader.content_action,
            sender_avatar: reader.avatar,
            shown_batch: RefCell::new(None),
        });
        let weak = Rc::downgrade(&mail);
        messages.connect_row_activated(move |_, row| {
            if let Some(mail) = weak.upgrade() {
                mail.open_message(row.index());
            }
        });
        mail.close_reader();
        mail
    }

    /// Shows the rows of a received batch, keeping the open message when the
    /// same batch is shown again.
    pub fn show_batch(&self, batch: &Rc<ReceivedBatch>) {
        let already_shown = self
            .shown_batch
            .borrow()
            .as_ref()
            .is_some_and(|shown| Rc::ptr_eq(shown, batch));
        if already_shown {
            return;
        }
        self.rows.remove_all();
        for position in 0..batch.messages.len() {
            self.rows.append(&glib::BoxedAnyObject::new(ListedMessage {
                batch: batch.clone(),
                position,
            }));
        }
        *self.shown_batch.borrow_mut() = Some(batch.clone());
        self.close_reader();
    }

    /// Empties the list and the reader, as a refresh and an account without
    /// mail do.
    pub fn clear(&self) {
        if self.shown_batch.borrow().is_none() {
            return;
        }
        self.rows.remove_all();
        *self.shown_batch.borrow_mut() = None;
        self.close_reader();
    }

    /// Names the account whose Inbox the list shows.
    pub fn show_account(&self, label: Option<String>) {
        match label {
            Some(label) => {
                self.list_title.set_title("Inbox");
                self.list_title.set_subtitle(&label);
                self.list_page.set_title("Inbox");
            }
            None => {
                self.list_title.set_title("Mailbag");
                self.list_title.set_subtitle("");
                self.list_page.set_title("Mailbag");
            }
        }
    }

    /// Opens the row's message from the received batch.
    fn open_message(&self, row_position: i32) {
        let Some(listed) = self.rows.item(row_position as u32) else {
            return;
        };
        let listed = listed
            .downcast::<glib::BoxedAnyObject>()
            .expect("message row item");
        let listed = listed.borrow::<ListedMessage>();
        let message = &listed.batch.messages[listed.position];
        tracing::debug!(
            account = listed.batch.account_id.as_str(),
            identity = ?message.identity,
            "message opened"
        );
        show_inert_text(&self.reader_subject, &subject_text(&message.fields));
        self.reader_sender.set_text(&sender_text(&message.fields));
        self.sender_avatar
            .set_text(Some(&sender_text(&message.fields)));
        match &message.fields.to {
            Some(recipients) => {
                self.reader_to.set_text(&inert_text(recipients));
                self.reader_to.set_visible(true);
            }
            None => self.reader_to.set_visible(false),
        }
        self.reader_date
            .set_text(&received_date_text(message.internal_date, "%c"));
        if let ReceivedContent::Text(text) = &message.content {
            show_inert_text(&self.reader_body, &inert_text(text));
        }
        self.show_content_failure(message.content.declare().as_ref());
        self.singleton_slot.set_visible(true);
        self.reader_stack.set_visible_child_name("message");
        self.mail_split.set_show_content(true);
    }

    /// Shows why the message has no text in the body's place, or the body
    /// when it has one.
    fn show_content_failure(&self, failure: Option<&DeclaredFailure>) {
        self.body_slot.set_visible(failure.is_none());
        self.content_status.set_visible(failure.is_some());
        let Some(failure) = failure else {
            return;
        };
        self.content_status.set_title(failure.title);
        self.content_status
            .set_description(Some(&status_description(failure)));
        show_action_button(&self.content_action, failure.action);
    }

    fn close_reader(&self) {
        self.show_content_failure(None);
        self.messages.unselect_all();
        self.singleton_slot.set_visible(false);
        self.reader_stack.set_visible_child_name("unselected");
        self.mail_split.set_show_content(false);
    }
}

/// The reader widgets that exist once for the window.
struct ReaderWidgets {
    sender: gtk::Label,
    to: gtk::Label,
    date: gtk::Label,
    body: gtk::Label,
    body_slot: gtk::Box,
    content_status: adw::StatusPage,
    content_action: gtk::Button,
    avatar: adw::Avatar,
}

/// Puts the approved message and envelope forms into the reader once, and
/// leaves every control that would change mail unavailable.
fn build_reader(window: &gtk::Builder) -> ReaderWidgets {
    let content = gtk::Builder::from_string(include_str!("../resources/ui/message-content.ui"));
    let envelope = gtk::Builder::from_string(include_str!("../resources/ui/envelope.ui"));
    let envelope_slot: gtk::Box = content
        .object("envelope_slot")
        .expect("message-content.ui: envelope_slot");
    envelope_slot.append(
        &envelope
            .object::<gtk::Box>("envelope_group")
            .expect("envelope.ui: envelope_group"),
    );
    let singleton_slot: gtk::Box = window
        .object("singleton_slot")
        .expect("mailbag.ui: singleton_slot");
    singleton_slot.append(
        &content
            .object::<gtk::Box>("main_message")
            .expect("message-content.ui: main_message"),
    );
    // Attachments and folders are outside this feature.
    for (builder, name) in [
        (&envelope, "attachment_button"),
        (&envelope, "reader_location"),
    ] {
        builder
            .object::<gtk::Widget>(name)
            .expect("reader widget")
            .set_visible(false);
    }
    for (builder, name) in [
        (&envelope, "star_button"),
        (&envelope, "message_menu"),
        (window, "demo_button"),
        (window, "reader_trash"),
        (window, "reader_folders"),
        (window, "reader_move"),
        (window, "reader_archive"),
    ] {
        builder
            .object::<gtk::Widget>(name)
            .expect("reader control")
            .set_sensitive(false);
    }
    ReaderWidgets {
        sender: envelope
            .object("reader_sender")
            .expect("envelope.ui: reader_sender"),
        to: envelope
            .object("reader_to")
            .expect("envelope.ui: reader_to"),
        date: envelope
            .object("single_date")
            .expect("envelope.ui: single_date"),
        body: content
            .object("reader_body")
            .expect("message-content.ui: reader_body"),
        body_slot: content
            .object("body_slot")
            .expect("message-content.ui: body_slot"),
        content_status: content
            .object("content_status")
            .expect("message-content.ui: content_status"),
        content_action: content
            .object("content_action")
            .expect("message-content.ui: content_action"),
        avatar: envelope.object("avatar").expect("envelope.ui: avatar"),
    }
}

/// Builds one list row from the approved row form.
fn build_message_row(listed: &glib::Object) -> gtk::ListBoxRow {
    let listed = listed
        .downcast_ref::<glib::BoxedAnyObject>()
        .expect("message row item");
    let listed = listed.borrow::<ListedMessage>();
    let message = &listed.batch.messages[listed.position];
    let builder = gtk::Builder::from_string(include_str!("../resources/ui/message-row.ui"));
    let row: gtk::ListBoxRow = builder.object("row").expect("message-row.ui: row");
    label(&builder, "sender").set_text(&sender_text(&message.fields));
    label(&builder, "subject").set_text(&subject_text(&message.fields));
    label(&builder, "time").set_text(&received_date_text(message.internal_date, "%x"));
    // Previews and conversations are outside this feature.
    label(&builder, "preview").set_visible(false);
    builder
        .object::<gtk::Image>("dot")
        .expect("message-row.ui: dot")
        .set_visible(!message.seen);
    // The dot is decorative, so the row itself speaks the read state.
    row.update_property(
        &[gtk::accessible::Property::Description(match message.seen {
            true => "Read",
            false => "Unread",
        })],
    );
    row
}

fn label(builder: &gtk::Builder, name: &str) -> gtk::Label {
    builder.object(name).expect("message-row.ui: label")
}

fn sender_text(fields: &DisplayFields) -> String {
    match &fields.from {
        Some(sender) => inert_text(sender),
        None => "Unknown sender".to_owned(),
    }
}

fn subject_text(fields: &DisplayFields) -> String {
    match &fields.subject {
        Some(subject) => inert_text(subject),
        None => "No subject".to_owned(),
    }
}

/// The received date in local presentation, or nothing when the server sent
/// no usable INTERNALDATE.
fn received_date_text(internal_date: Option<i64>, format: &str) -> String {
    let Some(received) =
        internal_date.and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok())
    else {
        return String::new();
    };
    received
        .format(format)
        .map(|text| text.to_string())
        .unwrap_or_default()
}

/// Shows text from a message or a mail server in a wrapping label, choosing
/// the wrapping that keeps the window responsive whatever the sender wrote.
pub fn show_inert_text(label: &gtk::Label, text: &str) {
    let wrapping = match longest_unbroken_run(text) > LONGEST_WORD_WRAPPED_RUN {
        true => gtk::pango::WrapMode::Char,
        false => gtk::pango::WrapMode::WordChar,
    };
    label.set_wrap_mode(wrapping);
    label.set_text(text);
}

/// The longest run of characters with no place to break a line.
fn longest_unbroken_run(text: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        // A line break or a space is where wrapping can happen.
        current = match character.is_whitespace() {
            true => 0,
            false => current + 1,
        };
        longest = longest.max(current);
    }
    longest
}

/// Prepares text that came from a message or a mail server for a GTK label:
/// at most the first 64 KiB, cut at a character boundary and without an
/// explanation, and no NUL, which GTK's string APIs cannot carry.
pub fn inert_text(text: &str) -> String {
    let mut end = text.len().min(DISPLAY_LIMIT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].replace('\0', "\u{FFFD}")
}
