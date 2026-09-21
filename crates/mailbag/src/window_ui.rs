// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decides what the window shows: the account page Online Accounts needs, or
//! the selected account's mail. It is the only owner of `list_stack`.

#[cfg(test)]
mod tests;

use crate::account_ui::AccountUi;
use crate::accounts::AccountPage;
use crate::inbox::{AccountInbox, InboxController, LoadFailure, LoadResult, ServerFailure};
use crate::inbox_load::LoadsInbox;
use crate::mail_ui::{MailUi, inert_text, show_inert_text};
use adw::{gio, gtk, prelude::*};
use goa_adapter::{AccountId, AccountProvider, AccountUpdate, ImapAccessError};
use mailbag_imap::{ImapFailure, ImapStep, ServerReply};
use std::{cell::RefCell, rc::Rc};

pub struct WindowUi {
    accounts: Rc<RefCell<AccountUi>>,
    inboxes: RefCell<InboxController>,
    mail: Rc<MailUi>,
    loader: Box<dyn LoadsInbox>,
    list_stack: gtk::Stack,
    status: adw::StatusPage,
    /// Mail explanations, including text a server sent, as plain text.
    mail_explanation: gtk::Label,
    refresh_inbox: gio::SimpleAction,
    /// The sidebar box that shows the spinner while a load runs.
    loading_spinner_box: gtk::Box,
}

/// What the status page says about the selected account's mail.
struct MailStatus {
    title: String,
    explanation: String,
}

impl WindowUi {
    pub fn new(builder: &gtk::Builder, loader: Box<dyn LoadsInbox>) -> Rc<Self> {
        let accounts = AccountUi::new(builder);
        let mail_explanation = gtk::Label::builder()
            .wrap(true)
            .max_width_chars(40)
            .justify(gtk::Justification::Center)
            .use_markup(false)
            .visible(false)
            .build();
        accounts
            .borrow()
            .status_actions()
            .prepend(&mail_explanation);
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
            mail_explanation,
            refresh_inbox: gio::SimpleAction::new("refresh-inbox", None),
            loading_spinner_box,
        });
        let refreshing = Rc::downgrade(&window);
        window.refresh_inbox.connect_activate(move |_, _| {
            if let Some(window) = refreshing.upgrade() {
                window.refresh_inbox();
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
        let Some(account_id) = self.refreshable_account() else {
            return;
        };
        if !self.inboxes.borrow_mut().begin_load(&account_id) {
            return;
        }
        // The cleared list and the spinner appear before the load starts.
        self.render();
        let window = Rc::downgrade(self);
        let loaded_account = account_id.clone();
        let cancellation = self.loader.start_load(
            &account_id,
            Box::new(move |result| {
                if let Some(window) = window.upgrade() {
                    window.finish_load(&loaded_account, result);
                }
            }),
        );
        self.inboxes
            .borrow_mut()
            .hold_cancellation(&account_id, cancellation);
    }

    fn finish_load(&self, account_id: &AccountId, result: LoadResult) {
        // A short list explains nothing by itself, so the refusal that caused
        // it is said once, as the load ends.
        if let LoadResult::Received(batch) = &result
            && let Some(refusal) = &batch.list_refusal
        {
            let accounts = self.accounts.borrow();
            let notice = incomplete_list_notice(accounts.label_of(account_id), refusal);
            accounts.show_toast(&notice);
        }
        self.inboxes.borrow_mut().finish_load(account_id, result);
        self.render();
    }

    /// The account Refresh Inbox would load: a selected Generic IMAP account.
    fn refreshable_account(&self) -> Option<AccountId> {
        let accounts = self.accounts.borrow();
        match accounts.selected_provider() {
            Some(AccountProvider::ImapSmtp) => accounts.selected_id().cloned(),
            _ => None,
        }
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
        let mail_status = match inbox {
            _ if shows_account_page => None,
            None => Some(nothing_loaded_status(accounts.selected_provider())),
            Some(AccountInbox::Loading) => Some(MailStatus::titled("Loading Inbox")),
            Some(AccountInbox::Failed(failure)) => Some(failure_status(failure)),
            Some(AccountInbox::Received(batch)) => batch
                .messages
                .is_empty()
                .then(|| MailStatus::titled("Inbox is empty")),
        };
        let shows_rows = !shows_account_page
            && matches!(inbox, Some(AccountInbox::Received(batch)) if !batch.messages.is_empty());
        self.list_stack.set_visible_child_name(match shows_rows {
            true => "messages",
            false => "empty",
        });
        if let Some(status) = &mail_status {
            // The account page left its own title and description empty.
            self.status.set_title(&status.title);
            self.status.set_description(None);
        }
        let explanation = mail_status
            .map(|status| status.explanation)
            .unwrap_or_default();
        show_inert_text(&self.mail_explanation, &explanation);
        self.mail_explanation.set_visible(!explanation.is_empty());
        self.loading_spinner_box.set_visible(inboxes.is_loading());
        self.refresh_inbox.set_enabled(
            !inboxes.is_loading()
                && accounts.selected_provider() == Some(AccountProvider::ImapSmtp),
        );
    }
}

impl MailStatus {
    /// A state that needs no explanation below its title.
    fn titled(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            explanation: String::new(),
        }
    }

    fn explained(title: &str, explanation: &str) -> Self {
        Self {
            title: title.to_owned(),
            explanation: explanation.to_owned(),
        }
    }
}

/// The server refused to finish the message list, so messages are missing from
/// the batch that is now on screen.
fn incomplete_list_notice(account: Option<String>, refusal: &ServerReply) -> String {
    let where_from = match account {
        Some(label) => format!("in {label}"),
        None => "in this account".to_owned(),
    };
    format!(
        "Some messages {where_from} could not be loaded. The mail server said: {}",
        inert_text(&refusal.text)
    )
}

/// Nothing has been loaded for this account in this run, which never means an
/// empty Inbox.
fn nothing_loaded_status(provider: Option<AccountProvider>) -> MailStatus {
    MailStatus::explained(
        "No mail loaded",
        match provider {
            Some(AccountProvider::ImapSmtp) => {
                "Choose Refresh Inbox in the main menu to load this account's Inbox."
            }
            _ => "Mailbag cannot load mail for this account yet.",
        },
    )
}

/// Names the step that failed and what it reported.
fn failure_status(failure: &LoadFailure) -> MailStatus {
    match failure {
        LoadFailure::OnlineAccounts(error) => online_accounts_status(*error),
        LoadFailure::Server(failure) => server_status(failure),
        LoadFailure::WorkerStopped => MailStatus::explained(
            "Mail could not be loaded",
            "Mailbag stopped loading this Inbox. Try Refresh Inbox again.",
        ),
    }
}

fn online_accounts_status(error: ImapAccessError) -> MailStatus {
    match error {
        ImapAccessError::Settings => MailStatus::explained(
            "Mail settings unavailable",
            "Unable to get this account's IMAP settings from Online Accounts.",
        ),
        ImapAccessError::NoEncryption => MailStatus::explained(
            "No encryption configured",
            "This account has no encryption configured. Choose SSL or STARTTLS for it in Online \
             Accounts. No password was requested and no connection was made.",
        ),
        ImapAccessError::Password => MailStatus::explained(
            "Password unavailable",
            "Unable to get this account's password from Online Accounts. No server sign-in was \
             attempted.",
        ),
        ImapAccessError::Timeout => MailStatus::explained(
            "Online Accounts did not respond",
            "Online Accounts did not respond in time. Try Refresh Inbox again.",
        ),
        // A cancelled request ends the load without a failure to explain.
        ImapAccessError::Cancelled => MailStatus::explained(
            "Mail could not be loaded",
            "Loading this Inbox stopped. Try Refresh Inbox again.",
        ),
    }
}

fn server_status(failure: &ServerFailure) -> MailStatus {
    let (title, reason) = match failure.failure {
        ImapFailure::Failed(step) => (failed_step_title(step), failed_step_reason(step)),
        ImapFailure::TimedOut(step) => (
            "The mail server stopped responding",
            waiting_step_reason(step),
        ),
        ImapFailure::NoSignInMethod => (
            "No supported sign-in method",
            "The mail server offers no sign-in method Mailbag supports.",
        ),
        ImapFailure::InboxChanged => (
            "The Inbox changed while loading",
            "The messages Mailbag was loading are no longer in this Inbox. Try Refresh Inbox \
             again.",
        ),
    };
    let mut explanation = vec![reason.to_owned()];
    if let Some(reply) = &failure.server_reply {
        explanation.push(format!("The mail server said: {}", inert_text(&reply.text)));
    }
    if password_may_be_wrong(failure) {
        explanation.push("You can change this account's password in Online Accounts.".to_owned());
    }
    explanation.extend(
        failure
            .alerts
            .iter()
            .map(|alert| format!("Alert from the mail server: {}", inert_text(alert))),
    );
    MailStatus {
        title: title.to_owned(),
        explanation: explanation.join("\n"),
    }
}

/// A rejected sign-in points to the password only when the server blamed the
/// credentials or gave no code; another code, such as a temporary
/// UNAVAILABLE, says nothing about the password.
fn password_may_be_wrong(failure: &ServerFailure) -> bool {
    if failure.failure != ImapFailure::Failed(ImapStep::SignIn) {
        return false;
    }
    match failure
        .server_reply
        .as_ref()
        .and_then(|reply| reply.code.as_deref())
    {
        None => true,
        Some(code) => code.eq_ignore_ascii_case("AUTHENTICATIONFAILED"),
    }
}

fn failed_step_title(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "Unable to reach the mail server",
        ImapStep::SecureConnection => "Secure connection failed",
        ImapStep::SignIn => "The mail server rejected sign-in",
        ImapStep::OpenInbox => "Unable to open the Inbox",
        ImapStep::FetchMessages => "Unable to get the message list",
        ImapStep::FetchText => "Unable to get the message text",
    }
}

fn failed_step_reason(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "Mailbag could not reach this account's mail server.",
        ImapStep::SecureConnection => {
            "Mailbag could not establish a verified encrypted connection, so it sent no password."
        }
        ImapStep::SignIn => "The mail server did not accept this account's sign-in.",
        ImapStep::OpenInbox => "The mail server did not open the Inbox.",
        ImapStep::FetchMessages => "The mail server did not return this Inbox's messages.",
        ImapStep::FetchText => "The mail server did not return the text of these messages.",
    }
}

fn waiting_step_reason(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "The mail server did not answer the connection.",
        ImapStep::SecureConnection => "The mail server stopped during the encrypted handshake.",
        ImapStep::SignIn => "The mail server stopped responding during sign-in.",
        ImapStep::OpenInbox => "The mail server stopped responding while opening the Inbox.",
        ImapStep::FetchMessages => {
            "The mail server stopped responding while sending this Inbox's messages."
        }
        ImapStep::FetchText => "The mail server stopped responding while sending the message text.",
    }
}
