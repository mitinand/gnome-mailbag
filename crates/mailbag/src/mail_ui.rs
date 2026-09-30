// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shows one mailbox's stored mail in the approved list and reader.
//!
//! The list builds only its visible rows and follows each new read of the
//! stored rows by their difference, so the open message stays open while it
//! is listed (specs/009-synchronization FR-013). Opening a message asks the
//! window for its stored content; it sends no request and changes nothing
//! on the server.

mod message_item;
#[cfg(test)]
mod tests;

use crate::failure_declarations::{DeclaredFailure, declare_content, declare_failure};
use crate::failure_dialog::{RetriedOperation, show_action_button, status_description};
use adw::{gio, glib, gtk, prelude::*};
use mailbag_domain::{AccountId, DisplayFields, Failure, MessageListRow, ReceivedContent};
use message_item::MessageItem;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

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

/// Asks the window for the content of an account's message.
type ContentRequest = Box<dyn Fn(&AccountId, &str)>;

pub struct MailUi {
    /// The list's row objects, in the order the stored rows were read.
    items: gio::ListStore,
    /// The list's selection: the open message's row, or none.
    selection: gtk::SingleSelection,
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
    /// Whose stored rows the list shows, as the latest read found them, to
    /// update the list only when a new read answered.
    listed_rows: RefCell<Option<(AccountId, Rc<[MessageListRow]>)>>,
    /// The identity of the message the reader shows.
    open_message: RefCell<Option<String>>,
    content_request: RefCell<Option<ContentRequest>>,
}

impl MailUi {
    pub fn new(builder: &gtk::Builder) -> Rc<Self> {
        let messages: gtk::ListView = builder.object("messages").expect("mailbag.ui: messages");
        let reader = build_reader(builder);
        // The row template names the row object's type.
        MessageItem::ensure_type();
        let factory = gtk::BuilderListItemFactory::from_bytes(
            None::<&gtk::BuilderScope>,
            &glib::Bytes::from_static(include_bytes!("../resources/ui/message-row.ui")),
        );
        messages.set_factory(Some(&factory));
        let items = gio::ListStore::new::<MessageItem>();
        let selection = gtk::SingleSelection::builder()
            .model(&items)
            .autoselect(false)
            .can_unselect(true)
            .build();
        messages.set_model(Some(&selection));
        let mail = Rc::new(Self {
            items,
            selection,
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
            listed_rows: RefCell::new(None),
            open_message: RefCell::new(None),
            content_request: RefCell::new(None),
        });
        let weak = Rc::downgrade(&mail);
        messages.connect_activate(move |_, position| {
            if let Some(mail) = weak.upgrade() {
                mail.open_message(position);
            }
        });
        mail.close_reader();
        mail
    }

    /// Sets how the reader asks for an opened message's stored content; the
    /// answer comes back through `show_content`.
    pub fn connect_content_request(&self, request: impl Fn(&AccountId, &str) + 'static) {
        *self.content_request.borrow_mut() = Some(Box::new(request));
    }

    /// Shows the rows of a stored mailbox. A new read updates the list by its
    /// difference with the rows shown and keeps the open message while it is
    /// listed, with its envelope from the new row; the same read changes
    /// nothing. The content is not read again: a text a cycle replaced, such
    /// as an edited draft's, shows when the message is opened again.
    pub fn show_rows(&self, account_id: &AccountId, rows: &Rc<[MessageListRow]>) {
        let (same_account, same_read) = match &*self.listed_rows.borrow() {
            Some((listed_account, listed)) => {
                let same_account = listed_account == account_id;
                (same_account, same_account && Rc::ptr_eq(listed, rows))
            }
            None => (false, false),
        };
        if same_read {
            return;
        }
        // Identities name messages within one account.
        if !same_account {
            self.clear();
        }
        update_list_by_difference(&self.items, rows);
        *self.listed_rows.borrow_mut() = Some((account_id.clone(), rows.clone()));
        let Some(identity) = self.open_message.borrow().clone() else {
            return;
        };
        match position_of(&self.items, &identity) {
            Some(position) => {
                self.selection.set_selected(position);
                self.show_envelope(&rows[position as usize]);
            }
            None => self.close_reader(),
        }
    }

    /// Empties the list and the reader, as a mailbox without stored rows
    /// does.
    pub fn clear(&self) {
        if self.listed_rows.borrow().is_none() {
            return;
        }
        self.items.remove_all();
        *self.listed_rows.borrow_mut() = None;
        self.close_reader();
    }

    /// Names what the list shows: the mailbox and its account, an account
    /// without a folder list, or nothing selected.
    pub fn show_title(&self, mailbox: Option<&str>, account: Option<&str>) {
        let (title, subtitle) = match (mailbox, account) {
            (Some(mailbox), Some(account)) => (mailbox, account),
            (None, Some(account)) => (account, ""),
            _ => ("Mailbag", ""),
        };
        self.list_title.set_title(title);
        self.list_title.set_subtitle(subtitle);
        self.list_page.set_title(title);
    }

    /// Reads the open message's content again, as the Retry of a failed
    /// read does.
    pub fn read_open_content_again(&self) {
        if let Some((account_id, identity)) = self.open_message_of() {
            self.request_content(&account_id, &identity);
        }
    }

    /// The account and identity of the message the reader shows.
    fn open_message_of(&self) -> Option<(AccountId, String)> {
        let identity = self.open_message.borrow().clone()?;
        let (account_id, _) = self.listed_rows.borrow().clone()?;
        Some((account_id, identity))
    }

    /// Shows a read of the open message's stored content: its text, why it
    /// has none, or the read's failure with Retry reading the stored mail
    /// again. An answer for a message no longer open is dropped; a message
    /// the store no longer holds closes the reader.
    pub fn show_content(
        &self,
        account_id: &AccountId,
        identity: &str,
        content: Result<Option<ReceivedContent>, Failure>,
    ) {
        if self.open_message_of() != Some((account_id.clone(), identity.to_owned())) {
            return;
        }
        match content {
            Ok(Some(content)) => {
                if let ReceivedContent::Text(text) = &content {
                    show_inert_text(&self.reader_body, &inert_text(text));
                }
                let failure = declare_content(&content);
                self.show_body_or_failure(
                    failure
                        .as_ref()
                        .map(|failure| (failure, RetriedOperation::RefreshMailbox)),
                );
            }
            Ok(None) => self.close_reader(),
            Err(failure) => {
                let retried = RetriedOperation::ReadStoredMail;
                self.show_body_or_failure(Some((&declare_failure(&failure, retried), retried)));
            }
        }
    }

    /// Opens the row's message: its envelope from the row at once, its
    /// content when the window has read it.
    fn open_message(&self, position: u32) {
        let Some(item) = self.items.item(position).and_downcast::<MessageItem>() else {
            return;
        };
        let Some((account_id, _)) = self.listed_rows.borrow().clone() else {
            return;
        };
        let listed = item.listed();
        tracing::debug!(
            account = account_id.as_str(),
            identity = listed.identity.as_str(),
            "message opened"
        );
        *self.open_message.borrow_mut() = Some(listed.identity.clone());
        self.selection.set_selected(position);
        self.show_envelope(listed);
        // The body stays empty until the content is read.
        self.reader_body.set_text("");
        self.show_body_or_failure(None);
        self.singleton_slot.set_visible(true);
        self.reader_stack.set_visible_child_name("message");
        self.mail_split.set_show_content(true);
        self.request_content(&account_id, &listed.identity);
    }

    /// The open message's subject, sender, recipients and date.
    fn show_envelope(&self, listed: &MessageListRow) {
        show_inert_text(&self.reader_subject, &subject_text(&listed.fields));
        self.reader_sender.set_text(&sender_text(&listed.fields));
        self.sender_avatar
            .set_text(Some(&sender_text(&listed.fields)));
        match &listed.fields.to {
            Some(recipients) => {
                self.reader_to.set_text(&inert_text(recipients));
                self.reader_to.set_visible(true);
            }
            None => self.reader_to.set_visible(false),
        }
        self.reader_date
            .set_text(&received_date_text(listed.received_unix, "%c"));
    }

    fn request_content(&self, account_id: &AccountId, identity: &str) {
        if let Some(request) = &*self.content_request.borrow() {
            request(account_id, identity);
        }
    }

    /// Shows why the message has no text in the body's place, with the
    /// operation its Retry repeats, or the body when it has one.
    fn show_body_or_failure(&self, failure: Option<(&DeclaredFailure, RetriedOperation)>) {
        self.body_slot.set_visible(failure.is_none());
        self.content_status.set_visible(failure.is_some());
        let Some((failure, retried)) = failure else {
            return;
        };
        self.content_status.set_title(failure.title);
        self.content_status
            .set_description(Some(&status_description(failure)));
        show_action_button(&self.content_action, failure.action, retried);
    }

    fn close_reader(&self) {
        *self.open_message.borrow_mut() = None;
        self.selection.set_selected(gtk::INVALID_LIST_POSITION);
        self.singleton_slot.set_visible(false);
        self.reader_stack.set_visible_child_name("unselected");
        self.mail_split.set_show_content(false);
    }
}

/// Makes the list's row objects follow `rows`: the common beginning and end
/// stay, with their read state changed in place; one splice replaces the
/// middle, keeping the object of a message still listed there unchanged, so
/// the arrival or removal of a few messages rebuilds no other row
/// (specs/009-synchronization/research.md §9).
fn update_list_by_difference(items: &gio::ListStore, rows: &[MessageListRow]) {
    let item_at = |position: usize| {
        items
            .item(position as u32)
            .and_downcast::<MessageItem>()
            .expect("the list holds message items")
    };
    let shown = items.n_items() as usize;
    let mut same_start = 0;
    while same_start < shown.min(rows.len())
        && item_at(same_start).lists_same_message(&rows[same_start])
    {
        same_start += 1;
    }
    let mut same_end = 0;
    while same_end < (shown - same_start).min(rows.len() - same_start)
        && item_at(shown - 1 - same_end).lists_same_message(&rows[rows.len() - 1 - same_end])
    {
        same_end += 1;
    }
    let removed: HashMap<String, MessageItem> = (same_start..shown - same_end)
        .map(|position| {
            let item = item_at(position);
            (item.listed().identity.clone(), item)
        })
        .collect();
    let added: Vec<MessageItem> = rows[same_start..rows.len() - same_end]
        .iter()
        .map(|row| match removed.get(&row.identity) {
            Some(item) if item.lists_same_message(row) => item.clone(),
            _ => MessageItem::new(row.clone()),
        })
        .collect();
    if !removed.is_empty() || !added.is_empty() {
        items.splice(same_start as u32, removed.len() as u32, &added);
    }
    for (position, row) in rows.iter().enumerate() {
        let item = item_at(position);
        if item.unread() == row.seen {
            item.set_unread(!row.seen);
        }
    }
}

/// Where the message with this identity is listed.
fn position_of(items: &gio::ListStore, identity: &str) -> Option<u32> {
    (0..items.n_items()).find(|position| {
        items
            .item(*position)
            .and_downcast::<MessageItem>()
            .is_some_and(|item| item.listed().identity == identity)
    })
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
/// no usable date.
fn received_date_text(received_unix: Option<i64>, format: &str) -> String {
    let Some(received) =
        received_unix.and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok())
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

/// Text for a label whose wrapping cannot be chosen, such as a status page's
/// description, which wraps by word: every run with no place to break a line
/// keeps its first `LONGEST_WORD_WRAPPED_RUN` characters, so a name the sender
/// chose cannot freeze the window.
pub fn cut_unbroken_runs(text: &str) -> String {
    let mut current = 0;
    text.chars()
        .filter(|character| {
            current = match character.is_whitespace() {
                true => 0,
                false => current + 1,
            };
            current <= LONGEST_WORD_WRAPPED_RUN
        })
        .collect()
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
