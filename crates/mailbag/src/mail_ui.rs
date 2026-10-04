// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shows one mailbox's stored mail in the approved list and reader.
//!
//! The list builds only its visible rows and follows each new read of the
//! stored rows by their difference, so the open message stays open while it
//! is listed (specs/009-synchronization FR-013). Opening a message asks the
//! window for its stored content; it sends no request. A change of the open
//! message's read state or star is handed to the window, which stores it;
//! the rows show it once read again (specs/011-read-and-star FR-001).

mod message_item;
#[cfg(test)]
mod tests;

use crate::failure_declarations::{DeclaredFailure, declare_content, declare_failure};
use crate::failure_dialog::{RetriedOperation, show_action_button, status_description};
use adw::{gio, glib, gtk, prelude::*};
use mailbag_domain::{
    AccountId, DisplayFields, Failure, FolderRef, MessageFlag, MessageListRow, ReceivedContent,
};
use message_item::MessageItem;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
    time::Duration,
};

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

/// How long a row takes to open when it arrives or to close when it leaves
/// (specs/010-message-list FR-006).
const ROW_TRANSITION_MS: u32 = 220;

/// When the rows that close are taken out of the list and the others come
/// in: after the closing, with a margin. A row scrolled out of view while it
/// closes has no widget to report the end of its transition.
const AFTER_CLOSING: Duration = Duration::from_millis(280);

/// How long a message stays open before it is marked read
/// (specs/010-message-list FR-009; specs/011-read-and-star FR-003).
const READ_AFTER_OPENING: Duration = Duration::from_secs(1);

/// How a change of the rows shown reaches the screen: at once, as for a
/// folder shown anew or the filter changed, or with arriving rows opening
/// and leaving rows closing (specs/010-message-list FR-006).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListChange {
    AtOnce,
    Animated,
}

/// What the window alone remembers of the shown folder since the latest
/// read of its stored rows: messages taken out with their trash button
/// (specs/010-message-list FR-010).
#[derive(Default)]
struct InWindow {
    removed: HashSet<String>,
}

/// Asks the window for the content of an account's message.
type ContentRequest = Box<dyn Fn(&AccountId, &str)>;

/// Asks the window to store the user's wanted value of a flag of an
/// account's message.
type FlagChange = Box<dyn Fn(&AccountId, &str, MessageFlag, bool)>;

pub struct MailUi {
    /// The window's handle on itself, for the timers and the row handlers.
    myself: Weak<MailUi>,
    messages: gtk::ListView,
    /// The list's row objects, in the order the stored rows were read; rows
    /// that leave stay, closed, until the change is applied.
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
    /// The envelope's star, which shows the open message's star by its icon
    /// (specs/011-read-and-star FR-002).
    star_button: gtk::ToggleButton,
    /// The open message's star as the latest read of its row found it; the
    /// envelope's star asks to change it.
    star_action: gio::SimpleAction,
    /// The reader header menu's Mark as Read and Mark as Unread, which the
    /// application publishes, for the open message; enabled while one is
    /// open.
    mark_read: gio::SimpleAction,
    mark_unread: gio::SimpleAction,
    /// Which folder's stored rows the list shows, as the latest read found
    /// them, to update the list only when a new read answered.
    listed_rows: RefCell<Option<(FolderRef, Rc<[MessageListRow]>)>>,
    /// Whether the list shows only the unread messages and the open one; the
    /// same for every folder and account (specs/010-message-list FR-008).
    unread_only: Cell<bool>,
    in_window: RefCell<InWindow>,
    /// Applies the list's latest state once the rows that leave are closed.
    pending_change: RefCell<Option<glib::SourceId>>,
    /// The identity of the message the reader shows.
    open_message: RefCell<Option<String>>,
    /// Marks the open message read when it fires.
    pending_read: RefCell<Option<glib::SourceId>>,
    content_request: RefCell<Option<ContentRequest>>,
    flag_change: RefCell<Option<FlagChange>>,
    /// Tells the window that the user took a row out of the list.
    row_removed: RefCell<Option<Box<dyn Fn()>>>,
}

impl MailUi {
    pub fn new(builder: &gtk::Builder) -> Rc<Self> {
        let messages: gtk::ListView = builder.object("messages").expect("mailbag.ui: messages");
        let reader = build_reader(builder);
        // The row template names the row object's type.
        MessageItem::ensure_type();
        let items = gio::ListStore::new::<MessageItem>();
        let selection = gtk::SingleSelection::builder()
            .model(&items)
            .autoselect(false)
            .can_unselect(true)
            .build();
        messages.set_model(Some(&selection));
        let mail = Rc::new_cyclic(|myself: &Weak<Self>| Self {
            myself: myself.clone(),
            messages: messages.clone(),
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
            star_button: reader.star_button,
            star_action: gio::SimpleAction::new_stateful("star", None, &false.to_variant()),
            mark_read: gio::SimpleAction::new("mark-scope-read", None),
            mark_unread: gio::SimpleAction::new("mark-scope-unread", None),
            listed_rows: RefCell::new(None),
            unread_only: Cell::new(false),
            in_window: RefCell::default(),
            pending_change: RefCell::new(None),
            open_message: RefCell::new(None),
            pending_read: RefCell::new(None),
            content_request: RefCell::new(None),
            flag_change: RefCell::new(None),
            row_removed: RefCell::new(None),
        });
        mail.add_message_actions();
        let factory = gtk::BuilderListItemFactory::from_bytes(
            Some(&row_handlers(Rc::downgrade(&mail))),
            &glib::Bytes::from_static(include_bytes!("../resources/ui/message-row.ui")),
        );
        messages.set_factory(Some(&factory));
        let weak = Rc::downgrade(&mail);
        messages.connect_activate(move |_, position| {
            if let Some(mail) = weak.upgrade() {
                mail.open_message(position, true);
                // Under the filter the message open before, if read, leaves
                // at once (specs/010-message-list FR-008).
                if mail.unread_only.get() {
                    mail.update_shown(ListChange::AtOnce);
                }
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

    /// Sets how the window stores the user's change of the open message's
    /// read state or star; the rows show it once read again.
    pub fn connect_flag_change(
        &self,
        change: impl Fn(&AccountId, &str, MessageFlag, bool) + 'static,
    ) {
        *self.flag_change.borrow_mut() = Some(Box::new(change));
    }

    /// Mark as Read and Mark as Unread of the reader header's menu, for the
    /// application to publish.
    pub fn mark_actions(&self) -> [&gio::SimpleAction; 2] {
        [&self.mark_read, &self.mark_unread]
    }

    /// The actions on the open message: the reader's `message` actions,
    /// which the envelope's star and menu name (`star` asks for the
    /// opposite of its star, `mark-unread` marks it unread), and the header
    /// menu's two. The star's state is set only from the stored rows
    /// (specs/011-read-and-star research §9).
    fn add_message_actions(&self) {
        let mail = self.myself.clone();
        self.star_action.connect_change_state(move |_, wanted| {
            if let (Some(mail), Some(starred)) =
                (mail.upgrade(), wanted.and_then(bool::from_variant))
            {
                mail.change_flag(MessageFlag::Flagged, starred);
            }
        });
        let mark_unread = gio::SimpleAction::new("mark-unread", None);
        let mail = self.myself.clone();
        mark_unread.connect_activate(move |_, _| {
            if let Some(mail) = mail.upgrade() {
                mail.change_flag(MessageFlag::Seen, false);
            }
        });
        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&self.star_action);
        actions.add_action(&mark_unread);
        self.reader_stack
            .insert_action_group("message", Some(&actions));
        for (action, wanted) in [(&self.mark_read, true), (&self.mark_unread, false)] {
            let mail = self.myself.clone();
            action.connect_activate(move |_, _| {
                if let Some(mail) = mail.upgrade() {
                    mail.change_flag(MessageFlag::Seen, wanted);
                }
            });
        }
    }

    /// Asks the window to store the user's wanted value of the open
    /// message's flag (specs/011-read-and-star FR-001). Marking it unread
    /// first stops the second after opening from marking it read again
    /// (research §14). Nothing happens without an open message.
    fn change_flag(&self, flag: MessageFlag, wanted: bool) {
        if flag == MessageFlag::Seen && !wanted {
            self.drop_pending_read();
        }
        let Some(identity) = self.open_message.borrow().clone() else {
            return;
        };
        self.change_listed_flag(&identity, flag, wanted);
    }

    /// Asks the window to store the user's wanted value of a listed
    /// message's flag, open or not, as the message's row star does
    /// (specs/011-read-and-star FR-002, FR-004).
    fn change_listed_flag(&self, identity: &str, flag: MessageFlag, wanted: bool) {
        let Some((folder, _)) = self.listed_rows.borrow().clone() else {
            return;
        };
        if let Some(change) = &*self.flag_change.borrow() {
            change(&folder.account, identity, flag, wanted);
        }
    }

    /// Sets how the window hears that the user took a row out of the list,
    /// which may leave the list with no row to show.
    pub fn connect_row_removed(&self, removed: impl Fn() + 'static) {
        *self.row_removed.borrow_mut() = Some(Box::new(removed));
    }

    /// Shows a folder's stored rows. A new read of the folder shown updates
    /// the list by its difference with the rows shown, with animations; a
    /// folder shown anew changes at once (specs/010-message-list FR-006). The
    /// open message stays while it is listed, with its envelope from the new
    /// row, and what the window alone remembered is forgotten; the same read
    /// changes nothing. The content is not read again: a text a cycle
    /// replaced, such as an edited draft's, shows when the message is opened
    /// again.
    pub fn show_rows(&self, folder: &FolderRef, rows: &Rc<[MessageListRow]>) {
        let (same_account, change) = match &*self.listed_rows.borrow() {
            Some((listed, listed_rows)) if listed == folder && Rc::ptr_eq(listed_rows, rows) => {
                return;
            }
            Some((listed, _)) if listed == folder => (true, ListChange::Animated),
            Some((listed, _)) => (listed.account == folder.account, ListChange::AtOnce),
            None => (false, ListChange::AtOnce),
        };
        // Identities name messages within one account.
        if !same_account {
            self.clear();
        }
        *self.listed_rows.borrow_mut() = Some((folder.clone(), rows.clone()));
        *self.in_window.borrow_mut() = InWindow::default();
        self.update_shown(change);
    }

    /// Shows only the unread messages and the open one, or every message.
    pub fn set_unread_filter(&self, unread_only: bool) {
        self.unread_only.set(unread_only);
        self.update_shown(ListChange::AtOnce);
    }

    /// Whether the list is on and shows only the unread messages.
    pub fn unread_filter(&self) -> bool {
        self.unread_only.get()
    }

    /// Whether the filter or the rows taken out in the window leave none of
    /// the stored rows in the list (specs/010-message-list FR-008, FR-010).
    pub fn shows_no_row(&self) -> bool {
        let Some((_, rows)) = self.listed_rows.borrow().clone() else {
            return false;
        };
        let open = self.open_message.borrow();
        let in_window = self.in_window.borrow();
        shown_rows(&rows, self.unread_only.get(), open.as_deref(), &in_window).is_empty()
    }

    /// Takes a message out of the list in the window only, as its trash
    /// button does: nothing is stored or sent, and the next read of the
    /// stored rows lists it again. When it was the open message, the
    /// neighbour the rule names opens, without bringing the reader forward
    /// in a narrow window (specs/010-message-list FR-007, FR-010).
    pub fn remove_in_window(&self, identity: &str) {
        let Some(position) = position_of(&self.items, identity) else {
            return;
        };
        self.in_window
            .borrow_mut()
            .removed
            .insert(identity.to_owned());
        if self.open_message.borrow().as_deref() == Some(identity) {
            match self.next_after_leaving_at(position) {
                Some(next) => self.open_message(next, false),
                None => self.close_reader(),
            }
        }
        self.update_shown(ListChange::Animated);
        if let Some(removed) = &*self.row_removed.borrow() {
            removed();
        }
    }

    /// The row to open after the open row at `position` leaves: one of its
    /// neighbours in the list as shown, closed rows left out.
    fn next_after_leaving_at(&self, position: u32) -> Option<u32> {
        next_after_leaving(
            first_shown(&self.items, (0..position).rev()),
            first_shown(&self.items, position + 1..self.items.n_items()),
        )
    }

    /// Makes the list show what the stored rows, the filter, the open
    /// message and the window's own changes call for, and keeps the open
    /// message while it is listed, with its envelope from its new row.
    /// Animated, the rows that leave close first and the list changes once
    /// they are closed; the rows that arrive then open
    /// (specs/010-message-list/research.md §10).
    fn update_shown(&self, change: ListChange) {
        let Some((_, rows)) = self.listed_rows.borrow().clone() else {
            return;
        };
        // A list off screen draws no frames to open or close its rows in,
        // and with the system's animations off the rows change at once.
        let animates =
            self.messages.is_mapped() && self.messages.settings().is_gtk_enable_animations();
        let change = match animates {
            true => change,
            false => ListChange::AtOnce,
        };
        let open = self.open_message.borrow().clone();
        let in_window = self.in_window.borrow();
        let shown = shown_rows(&rows, self.unread_only.get(), open.as_deref(), &in_window);
        match change {
            ListChange::AtOnce => {
                self.drop_pending_change();
                self.change_list(&shown, change);
            }
            ListChange::Animated => {
                if close_leaving_rows(&self.items, &shown) {
                    self.change_after_closing();
                }
                if self.pending_change.borrow().is_none() {
                    self.change_list(&shown, change);
                }
            }
        }
        drop(in_window);
        let Some(identity) = open else {
            return;
        };
        let still_listed = shown.iter().any(|row| row.identity == identity);
        match position_of(&self.items, &identity).filter(|_| still_listed) {
            Some(position) => {
                self.selection.set_selected(position);
                self.show_envelope(&item_at(&self.items, position));
            }
            None => self.close_reader(),
        }
    }

    /// Changes the list to `shown` by difference. The rows closed or come in
    /// closed open, on the second frame after the change when animated,
    /// since before that their widgets are not on screen and would jump
    /// open. A list at its top stays at its top, so rows arriving there are
    /// seen.
    fn change_list(&self, shown: &[&MessageListRow], change: ListChange) {
        let at_top = self
            .messages
            .vadjustment()
            .is_some_and(|scrolling| scrolling.value() == 0.0);
        let closed = update_list_by_difference(&self.items, shown, change);
        match change {
            ListChange::AtOnce => closed.iter().for_each(|item| item.show_over(true, 0)),
            ListChange::Animated if !closed.is_empty() => {
                let frames = Cell::new(0);
                self.messages.add_tick_callback(move |_, _| {
                    frames.set(frames.get() + 1);
                    if frames.get() < 2 {
                        return glib::ControlFlow::Continue;
                    }
                    for item in &closed {
                        item.show_over(true, ROW_TRANSITION_MS);
                    }
                    glib::ControlFlow::Break
                });
            }
            ListChange::Animated => {}
        }
        if at_top && self.items.n_items() > 0 {
            self.messages
                .activate_action("list.scroll-to-item", Some(&0_u32.to_variant()))
                .expect("a list view scrolls to an item");
        }
    }

    /// Applies the latest state once the rows that leave are closed. A row
    /// that starts closing meanwhile, such as a second row sent to the
    /// trash, starts the wait again, so it closes whole too.
    fn change_after_closing(&self) {
        self.drop_pending_change();
        let mail = self.myself.clone();
        let pending = glib::timeout_add_local_once(AFTER_CLOSING, move || {
            if let Some(mail) = mail.upgrade() {
                mail.pending_change.take();
                mail.update_shown(ListChange::Animated);
            }
        });
        *self.pending_change.borrow_mut() = Some(pending);
    }

    fn drop_pending_change(&self) {
        if let Some(pending) = self.pending_change.take() {
            pending.remove();
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
        let (folder, _) = self.listed_rows.borrow().clone()?;
        Some((folder.account, identity))
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
    /// content when the window has read it; a second later it is marked
    /// read. A narrow window brings the reader forward only when asked.
    /// The list does not change here: the action that opens decides how.
    fn open_message(&self, position: u32, bring_reader_forward: bool) {
        let Some(item) = self.items.item(position).and_downcast::<MessageItem>() else {
            return;
        };
        // A closing row is leaving the list.
        if !item.shown() {
            return;
        }
        let Some((
            FolderRef {
                account: account_id,
                ..
            },
            _,
        )) = self.listed_rows.borrow().clone()
        else {
            return;
        };
        let listed = item.listed();
        tracing::debug!(
            account = account_id.as_str(),
            identity = listed.identity.as_str(),
            "message opened"
        );
        *self.open_message.borrow_mut() = Some(listed.identity.clone());
        for action in self.mark_actions() {
            action.set_enabled(true);
        }
        self.mark_read_after_opening();
        self.selection.set_selected(position);
        // A neighbour opened after the trash may lie outside the visible area.
        self.messages
            .activate_action("list.scroll-to-item", Some(&position.to_variant()))
            .expect("a list view scrolls to an item");
        self.show_envelope(&item);
        // The body stays empty until the content is read.
        self.reader_body.set_text("");
        self.show_body_or_failure(None);
        self.singleton_slot.set_visible(true);
        self.reader_stack.set_visible_child_name("message");
        if bring_reader_forward {
            self.mail_split.set_show_content(true);
        }
        self.request_content(&account_id, &listed.identity);
    }

    /// Marks the open message read a second after it opened, unless
    /// another opens, the reader closes or the user marks it unread first;
    /// a message read by then asks nothing (specs/010-message-list FR-009;
    /// specs/011-read-and-star FR-003).
    fn mark_read_after_opening(&self) {
        self.drop_pending_read();
        let mail = self.myself.clone();
        let pending = glib::timeout_add_local_once(READ_AFTER_OPENING, move || {
            let Some(mail) = mail.upgrade() else {
                return;
            };
            mail.pending_read.take();
            if mail.open_item().is_some_and(|item| item.unread()) {
                mail.change_flag(MessageFlag::Seen, true);
            }
        });
        *self.pending_read.borrow_mut() = Some(pending);
    }

    /// The open message's row object.
    fn open_item(&self) -> Option<MessageItem> {
        let identity = self.open_message.borrow().clone()?;
        position_of(&self.items, &identity).map(|position| item_at(&self.items, position))
    }

    fn drop_pending_read(&self) {
        if let Some(pending) = self.pending_read.take() {
            pending.remove();
        }
    }

    /// The open message's subject, sender, recipients, date and star; the
    /// star's icon is filled while it is starred, since the pressed look
    /// alone is faint (specs/011-read-and-star FR-002).
    fn show_envelope(&self, item: &MessageItem) {
        let starred = item.starred();
        self.star_action.set_state(&starred.to_variant());
        self.star_button.set_icon_name(match starred {
            true => "starred-symbolic",
            false => "non-starred-symbolic",
        });
        let listed = item.listed();
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
        let received = listed
            .received_unix
            .and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok());
        self.reader_date.set_text(
            &received
                .map(|time| formatted(&time, "%c"))
                .unwrap_or_default(),
        );
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
        self.drop_pending_read();
        *self.open_message.borrow_mut() = None;
        for action in self.mark_actions() {
            action.set_enabled(false);
        }
        self.selection.set_selected(gtk::INVALID_LIST_POSITION);
        self.singleton_slot.set_visible(false);
        self.reader_stack.set_visible_child_name("unselected");
        self.mail_split.set_show_content(false);
    }
}

/// Makes the list's row objects follow `rows`: the common beginning and end
/// stay; one splice replaces the middle, keeping the object of a message
/// still listed there unchanged, so the arrival or removal of a few messages
/// rebuilds no other row (specs/009-synchronization/research.md §9). Every
/// row's read state and star then change in place. An animated change
/// brings new rows in closed; the closed rows are returned, to open them.
fn update_list_by_difference(
    items: &gio::ListStore,
    rows: &[&MessageListRow],
    change: ListChange,
) -> Vec<MessageItem> {
    let shown = items.n_items() as usize;
    let mut same_start = 0;
    while same_start < shown.min(rows.len())
        && item_at(items, same_start as u32).lists_same_message(rows[same_start])
    {
        same_start += 1;
    }
    let mut same_end = 0;
    while same_end < (shown - same_start).min(rows.len() - same_start)
        && item_at(items, (shown - 1 - same_end) as u32)
            .lists_same_message(rows[rows.len() - 1 - same_end])
    {
        same_end += 1;
    }
    let removed: HashMap<String, MessageItem> = (same_start..shown - same_end)
        .map(|position| {
            let item = item_at(items, position as u32);
            (item.listed().identity.clone(), item)
        })
        .collect();
    let added: Vec<MessageItem> = rows[same_start..rows.len() - same_end]
        .iter()
        .map(|row| match removed.get(&row.identity) {
            Some(item) if item.lists_same_message(row) => item.clone(),
            _ => {
                let item = MessageItem::new((*row).clone());
                if change == ListChange::Animated {
                    item.show_over(false, ROW_TRANSITION_MS);
                }
                item
            }
        })
        .collect();
    if !removed.is_empty() || !added.is_empty() {
        items.splice(same_start as u32, removed.len() as u32, &added);
    }
    let mut closed = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        let item = item_at(items, position as u32);
        // Setting a property notifies whether or not it changes.
        if item.unread() == row.seen {
            item.set_unread(!row.seen);
        }
        if item.starred() != row.flagged {
            item.set_starred(row.flagged);
        }
        if !item.shown() {
            closed.push(item);
        }
    }
    closed
}

/// Starts closing the rows whose message `rows` no longer lists, and says
/// whether any started.
fn close_leaving_rows(items: &gio::ListStore, rows: &[&MessageListRow]) -> bool {
    let listed: HashMap<&str, &MessageListRow> = rows
        .iter()
        .map(|row| (row.identity.as_str(), *row))
        .collect();
    let mut started = false;
    for item in (0..items.n_items()).map(|position| item_at(items, position)) {
        let stays = listed
            .get(item.listed().identity.as_str())
            .is_some_and(|row| item.lists_same_message(row));
        if !stays && item.shown() {
            item.show_over(false, ROW_TRANSITION_MS);
            started = true;
        }
    }
    started
}

/// The first row at `positions` that is not closed, and whether it is
/// unread.
fn first_shown(
    items: &gio::ListStore,
    positions: impl Iterator<Item = u32>,
) -> Option<(u32, bool)> {
    positions
        .map(|position| (position, item_at(items, position)))
        .find(|(_, item)| item.shown())
        .map(|(position, item)| (position, item.unread()))
}

fn item_at(items: &gio::ListStore, position: u32) -> MessageItem {
    items
        .item(position)
        .and_downcast::<MessageItem>()
        .expect("the list holds message items")
}

/// The rows the list shows: the stored rows but those taken out in the
/// window, and with the unread filter on only the unread ones and the open
/// message (specs/010-message-list FR-008, FR-010).
fn shown_rows<'a>(
    rows: &'a [MessageListRow],
    unread_only: bool,
    open: Option<&str>,
    in_window: &InWindow,
) -> Vec<&'a MessageListRow> {
    rows.iter()
        .filter(|row| !in_window.removed.contains(&row.identity))
        .filter(|row| !unread_only || !row.seen || open == Some(row.identity.as_str()))
        .collect()
}

/// The position that opens after the open message leaves the list, from
/// each neighbour's position and whether it is unread (`None` where there is
/// none): the one below, unless there is none or only the one above is
/// unread (specs/010-message-list FR-007).
fn next_after_leaving(above: Option<(u32, bool)>, below: Option<(u32, bool)>) -> Option<u32> {
    let next = match (above, below) {
        (None, None) => None,
        (Some(_), None) | (Some((_, true)), Some((_, false))) => above,
        (None, Some(_)) | (Some((_, false)), Some(_)) | (Some((_, true)), Some((_, true))) => below,
    };
    next.map(|(position, _)| position)
}

/// The handlers the row template names: the trash icon slides in beside the
/// date while the pointer is over the row, turns red, Adwaita's colour for
/// a destructive action, while the pointer is over the icon itself, and
/// takes its message out of the list in the
/// window (specs/010-message-list FR-002, FR-010); the star under the date
/// shows its outline while the pointer is over the row and stars or
/// unstars its message without opening it (specs/011-read-and-star
/// FR-004). A signal with an object in the form passes that object first;
/// one without passes its emitter.
fn row_handlers(mail: Weak<MailUi>) -> gtk::BuilderRustScope {
    let scope = gtk::BuilderRustScope::new();
    for (handler, reveal) in [("row_entered", true), ("row_left", false)] {
        scope.add_callback(handler, move |values| {
            let revealer = values
                .first()
                .and_then(|value| value.get::<gtk::Revealer>().ok());
            revealer
                .expect("message-row.ui: trash_reveal")
                .set_reveal_child(reveal);
            None
        });
    }
    for (handler, pointed) in [("trash_entered", true), ("trash_left", false)] {
        scope.add_callback(handler, move |values| {
            let icon = values
                .first()
                .and_then(|value| value.get::<gtk::Widget>().ok());
            let icon = icon.expect("message-row.ui: trash");
            let (shown, hidden) = match pointed {
                true => ("error", "dim-label"),
                false => ("dim-label", "error"),
            };
            icon.remove_css_class(hidden);
            icon.add_css_class(shown);
            None
        });
    }
    for (handler, pointed) in [("row_pointed", true), ("row_unpointed", false)] {
        scope.add_callback(handler, move |values| {
            if let Some(item) = row_item(values) {
                item.set_pointed(pointed);
            }
            None
        });
    }
    // The list opens a message when a click on its row is released; a
    // claimed press never reaches it (GTK's list factory widget, bubble
    // phase). A click where no star shows opens the message as before.
    scope.add_callback("star_pressed", move |values| {
        let press = values
            .first()
            .and_then(|value| value.get::<gtk::GestureClick>().ok())
            .expect("message-row.ui: the star's click");
        let star = press.widget().and_downcast::<gtk::Image>();
        if star.is_some_and(|star| star.icon_name().is_some_and(|icon| !icon.is_empty())) {
            press.set_state(gtk::EventSequenceState::Claimed);
        }
        None
    });
    let starring = mail.clone();
    scope.add_callback("star_row", move |values| {
        let item = row_item(values);
        // Where no star shows, the click asks for nothing.
        if let (Some(mail), Some(item)) = (starring.upgrade(), item)
            && !item.star_icon().is_empty()
        {
            let identity = &item.listed().identity;
            mail.change_listed_flag(identity, MessageFlag::Flagged, !item.starred());
        }
        None
    });
    scope.add_callback("trash_row", move |values| {
        if let (Some(mail), Some(item)) = (mail.upgrade(), row_item(values)) {
            mail.remove_in_window(&item.listed().identity);
        }
        None
    });
    scope
}

/// The message of the row whose list item a row handler was given.
fn row_item(values: &[glib::Value]) -> Option<MessageItem> {
    values
        .first()
        .and_then(|value| value.get::<gtk::ListItem>().ok())
        .expect("message-row.ui: the row's list item")
        .item()
        .and_downcast::<MessageItem>()
}

/// Where the message with this identity is listed.
fn position_of(items: &gio::ListStore, identity: &str) -> Option<u32> {
    (0..items.n_items()).find(|position| item_at(items, *position).listed().identity == identity)
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
    star_button: gtk::ToggleButton,
}

/// Puts the approved message and envelope forms into the reader once, and
/// leaves every control that would change mail unavailable but the star,
/// the message menu and the header's menu, whose Mark as Read and Mark as
/// Unread act (specs/011-read-and-star FR-002).
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
        star_button: envelope
            .object("star_button")
            .expect("envelope.ui: star_button"),
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

/// The row's received date as the list words it, by the computer's clock,
/// or nothing when the server sent no usable date
/// (specs/010-message-list FR-004).
fn row_date_text(received_unix: Option<i64>) -> String {
    let received = received_unix.and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok());
    match (received, glib::DateTime::now_local()) {
        (Some(received), Ok(now)) => date_wording(&received, &now),
        _ => String::new(),
    }
}

/// How the row words `received` at `now`, both in local time: today's time
/// in the locale's form, "Yesterday", the weekday within the six days
/// before, the day and month earlier this year, the locale's short date
/// before that.
fn date_wording(received: &glib::DateTime, now: &glib::DateTime) -> String {
    let format = match days_before(received, now) {
        0 => locale_time_form(&formatted(now, "%X"), &formatted(now, "%p")),
        1 => return "Yesterday".to_owned(),
        2..=6 => "%A",
        _ if received.year() == now.year() => {
            let new_years_eve =
                glib::DateTime::from_local(2000, 12, 31, 0, 0, 0.0).expect("a valid time");
            day_month_form(&formatted(&new_years_eve, "%x"))
        }
        _ => "%x",
    };
    formatted(received, format)
}

/// How many local calendar days `received`'s day lies before `now`'s. A
/// day changing to or from summer time is an hour shorter or longer, so the
/// difference is rounded.
fn days_before(received: &glib::DateTime, now: &glib::DateTime) -> i64 {
    let day_start = |time: &glib::DateTime| {
        glib::DateTime::from_local(time.year(), time.month(), time.day_of_month(), 0, 0, 0.0)
            .expect("the start of a valid time's day")
    };
    let seconds = day_start(now).difference(&day_start(received)).as_seconds();
    (seconds as f64 / 86_400.0).round() as i64
}

/// The locale's time without seconds: the 12-hour form when the locale's
/// full time shows its AM/PM marker, else the 24-hour form
/// (specs/010-message-list/research.md §14).
fn locale_time_form(full_time: &str, am_pm: &str) -> &'static str {
    match !am_pm.is_empty() && full_time.contains(am_pm) {
        true => "%-I:%M %p",
        false => "%H:%M",
    }
}

/// The day and the month's name in the locale's order, told by the order
/// of the day 31 and the month 12 in the locale's short date of 31 December
/// (specs/010-message-list FR-004).
fn day_month_form(short_date: &str) -> &'static str {
    match (short_date.find("31"), short_date.find("12")) {
        (Some(day), Some(month)) if month < day => "%B %-d",
        _ => "%-d %B",
    }
}

fn formatted(time: &glib::DateTime, format: &str) -> String {
    time.format(format)
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
