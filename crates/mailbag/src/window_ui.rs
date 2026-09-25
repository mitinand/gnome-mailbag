// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decides what the window shows: the account page Online Accounts needs, or
//! the selected account's mail. It is the only owner of `list_stack`.

#[cfg(test)]
mod tests;

use crate::account_ui::{AccountUi, PageAction, show_check_progress};
use crate::accounts::AccountPage;
use crate::failure_declarations::{DeclaredFailure, declare_load_failure, declare_short_list};
use crate::failure_dialog::{self, show_action_button, status_description};
use crate::inbox::{AccountInbox, InboxController};
use crate::mail_ui::MailUi;
use adw::{gio, gtk, prelude::*};
use goa_adapter::{AccountId, AccountUpdate};
use mailbag_providers::{LoadResult, LoadsInbox, MailProvider};
use std::{cell::RefCell, rc::Rc};

pub struct WindowUi {
    accounts: Rc<RefCell<AccountUi>>,
    inboxes: RefCell<InboxController>,
    mail: Rc<MailUi>,
    loader: Box<dyn LoadsInbox>,
    list_stack: gtk::Stack,
    /// The account page and the states of mail that are not failures.
    status: adw::StatusPage,
    /// The account page's buttons; at most one is shown.
    status_retry_check: gtk::Button,
    status_online_accounts: gtk::Button,
    /// A load that delivered no mail, in the list's place.
    failure_status: adw::StatusPage,
    failure_action: gtk::Button,
    /// Opens the failure dialog for the failed load.
    failure_details: gtk::Button,
    /// Says that the list on screen is short of messages.
    list_banner: adw::Banner,
    refresh_inbox: gio::SimpleAction,
    /// The sidebar box that shows the spinner while a load runs.
    loading_spinner_box: gtk::Box,
}

impl WindowUi {
    pub fn new(builder: &gtk::Builder, loader: Box<dyn LoadsInbox>) -> Rc<Self> {
        let accounts = AccountUi::new(builder);
        let loading_spinner_box: gtk::Box = builder
            .object("sync_button_list")
            .expect("mailbag.ui: sync_button_list");
        builder
            .object::<gtk::Stack>("sync_icon_list")
            .expect("mailbag.ui: sync_icon_list")
            .set_visible_child_name("active");
        let window = Rc::new(Self {
            accounts,
            inboxes: RefCell::new(InboxController::default()),
            mail: MailUi::new(builder),
            loader,
            list_stack: builder
                .object("list_stack")
                .expect("mailbag.ui: list_stack"),
            status: builder
                .object("account_status")
                .expect("mailbag.ui: account_status"),
            status_retry_check: builder
                .object("status_retry_check")
                .expect("mailbag.ui: status_retry_check"),
            status_online_accounts: builder
                .object("status_online_accounts")
                .expect("mailbag.ui: status_online_accounts"),
            failure_status: builder
                .object("failure_status")
                .expect("mailbag.ui: failure_status"),
            failure_action: builder
                .object("failure_action")
                .expect("mailbag.ui: failure_action"),
            failure_details: builder
                .object("failure_details")
                .expect("mailbag.ui: failure_details"),
            list_banner: builder
                .object("list_banner")
                .expect("mailbag.ui: list_banner"),
            refresh_inbox: gio::SimpleAction::new("refresh-inbox", None),
            loading_spinner_box,
        });
        let refreshing = Rc::downgrade(&window);
        window.refresh_inbox.connect_activate(move |_, _| {
            if let Some(window) = refreshing.upgrade() {
                window.refresh_inbox();
            }
        });
        let explaining = Rc::downgrade(&window);
        window.failure_details.connect_clicked(move |_| {
            if let Some(window) = explaining.upgrade() {
                window.open_failure_dialog();
            }
        });
        let explaining = Rc::downgrade(&window);
        window.list_banner.connect_button_clicked(move |_| {
            if let Some(window) = explaining.upgrade() {
                window.open_failure_dialog();
            }
        });
        let selecting = Rc::downgrade(&window);
        window.accounts.borrow().connect_selection_changed(move || {
            if let Some(window) = selecting.upgrade() {
                // Selecting an account shows its mail; it never loads.
                window.render();
            }
        });
        window.render();
        window
    }

    pub fn accounts(&self) -> &Rc<RefCell<AccountUi>> {
        &self.accounts
    }

    /// The Refresh Inbox action, for the application to publish under its
    /// menu item.
    pub fn refresh_action(&self) -> &gio::SimpleAction {
        &self.refresh_inbox
    }

    /// Applies an account update and discards the mail of accounts Online
    /// Accounts no longer shows.
    pub fn apply_account_update(&self, update: &AccountUpdate) {
        self.accounts.borrow_mut().apply_update(update);
        let accounts = self.accounts.borrow();
        self.inboxes
            .borrow_mut()
            .discard_excluded(|account_id| accounts.shows_account(account_id));
        drop(accounts);
        self.render();
    }

    /// Quit: cancels a running load without waiting for its worker.
    pub fn cancel_loads(&self) {
        self.inboxes.borrow_mut().cancel_load();
    }

    /// Refresh Inbox: clears the selected account's list and reader and
    /// starts the only load.
    fn refresh_inbox(self: &Rc<Self>) {
        let Some((account_id, provider)) = self.refreshable_account() else {
            return;
        };
        if self.inboxes.borrow().is_loading() {
            return;
        }
        let window = Rc::downgrade(self);
        let loaded_account = account_id.clone();
        // The result arrives later on this context, never inside start_load.
        let cancellation = self.loader.start_load(
            &account_id,
            provider,
            Box::new(move |result| {
                if let Some(window) = window.upgrade() {
                    window.finish_load(&loaded_account, result);
                }
            }),
        );
        self.inboxes
            .borrow_mut()
            .begin_load(&account_id, cancellation);
        self.render();
    }

    fn finish_load(&self, account_id: &AccountId, result: LoadResult) {
        self.inboxes.borrow_mut().finish_load(account_id, result);
        self.render();
    }

    /// The account Refresh Inbox would load, with the sequence it needs.
    fn refreshable_account(&self) -> Option<(AccountId, MailProvider)> {
        let accounts = self.accounts.borrow();
        Some((
            accounts.selected_id().cloned()?,
            accounts.selected_provider()?,
        ))
    }

    fn render(&self) {
        let accounts = self.accounts.borrow();
        let inboxes = self.inboxes.borrow();
        let selected = accounts.selected_id();
        let inbox = selected.and_then(|account_id| inboxes.inbox_of(account_id));
        match inbox {
            Some(AccountInbox::Received(batch)) => self.mail.show_batch(batch),
            _ => self.mail.clear(),
        }
        self.mail
            .show_account(selected.and_then(|account_id| accounts.label_of(account_id)));
        // The account page comes first; it covers the list and the reader
        // without touching the mail received for the account.
        let shows_account_page = accounts.page() != AccountPage::SelectedAccount;
        // The list's pages and the banner have one writer: this function. Each
        // state shows one page and fills it whole, and only a short list on
        // screen reveals the banner, so no failure outlives its cause.
        self.list_banner.set_revealed(false);
        match inbox {
            _ if shows_account_page => self.show_account_page(&accounts),
            None => self.show_mail_status(
                "No mail loaded",
                Some("Choose Refresh Inbox in the main menu to load this account's Inbox."),
            ),
            Some(AccountInbox::Loading) => self.show_mail_status("Loading Inbox", None),
            Some(AccountInbox::Failed(failure)) => {
                self.show_failed_load(&declare_load_failure(failure))
            }
            Some(AccountInbox::Received(batch)) => {
                if batch.messages.is_empty() {
                    self.show_mail_status("Inbox is empty", None);
                } else {
                    self.list_stack.set_visible_child_name("messages");
                }
                if let Some(incomplete) = &batch.incomplete {
                    self.show_short_list(&declare_short_list(incomplete));
                }
            }
        }
        self.loading_spinner_box.set_visible(inboxes.is_loading());
        self.refresh_inbox
            .set_enabled(!inboxes.is_loading() && accounts.selected_provider().is_some());
    }

    /// Online Accounts' page: its text and its one button.
    fn show_account_page(&self, accounts: &AccountUi) {
        let (title, description) = accounts.page_text();
        self.list_stack.set_visible_child_name("empty");
        self.status.set_title(title);
        self.status
            .set_description((!description.is_empty()).then_some(description));
        let action = accounts.page_action();
        self.status_retry_check
            .set_visible(action == Some(PageAction::RetryCheck));
        show_check_progress(&self.status_retry_check, accounts.retry_pending());
        self.status_online_accounts
            .set_visible(action == Some(PageAction::OnlineAccounts));
    }

    /// A state of the selected account's mail that is not a failure.
    fn show_mail_status(&self, title: &str, description: Option<&str>) {
        self.list_stack.set_visible_child_name("empty");
        self.status.set_title(title);
        self.status.set_description(description);
        self.status_retry_check.set_visible(false);
        self.status_online_accounts.set_visible(false);
    }

    /// A load that delivered no mail: the failure page takes the list's
    /// place. Its Details button is always there: a failed load always has
    /// technical details.
    fn show_failed_load(&self, failure: &DeclaredFailure) {
        self.list_stack.set_visible_child_name("failed");
        self.failure_status.set_title(failure.title);
        self.failure_status
            .set_description(Some(&status_description(failure)));
        show_action_button(&self.failure_action, failure.action);
    }

    /// The rows on screen are fewer than the Inbox offered.
    fn show_short_list(&self, failure: &DeclaredFailure) {
        self.list_banner.set_title(failure.title);
        self.list_banner.set_revealed(true);
    }

    /// Opens the failure dialog for what the list area shows: a failed load
    /// or a short list of the selected account.
    fn open_failure_dialog(&self) {
        let failure = {
            let accounts = self.accounts.borrow();
            let inboxes = self.inboxes.borrow();
            match accounts
                .selected_id()
                .and_then(|account_id| inboxes.inbox_of(account_id))
            {
                Some(AccountInbox::Failed(failure)) => Some(declare_load_failure(failure)),
                Some(AccountInbox::Received(batch)) => {
                    batch.incomplete.as_ref().map(declare_short_list)
                }
                Some(AccountInbox::Loading) | None => None,
            }
        };
        if let Some(failure) = failure {
            failure_dialog::present(&self.list_stack, &failure);
        }
    }
}
