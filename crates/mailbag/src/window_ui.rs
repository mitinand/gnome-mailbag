// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decides what the window shows: the account page Online Accounts needs, or
//! the selected account's stored mail with its latest refresh's outcome. It
//! is the only owner of `list_stack`. It reads the store on GIO's thread pool,
//! never on GTK's thread (specs/007-mail-storage FR-011).

#[cfg(test)]
mod tests;

use crate::account_ui::{AccountUi, PageAction, show_check_progress};
use crate::accounts::AccountPage;
use crate::failure_declarations::{DeclaredFailure, declare_failure, declare_short_list};
use crate::failure_dialog::{self, RetriedOperation, show_action_button, status_description};
use crate::inbox::{InboxController, RefreshOutcome};
use crate::mail_ui::MailUi;
use adw::{gio, glib, gtk, prelude::*};
use goa_adapter::AccountUpdate;
use mailbag_domain::{AccountId, Failure, Message, catch_panic};
use mailbag_providers::{LoadResult, LoadsInbox, MailProvider};
use mailbag_store::Store;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::Arc};

pub struct WindowUi {
    accounts: Rc<RefCell<AccountUi>>,
    inboxes: RefCell<InboxController>,
    store: Arc<Store>,
    shown_inbox: RefCell<ShownInbox>,
    mail: Rc<MailUi>,
    loader: Box<dyn LoadsInbox>,
    list_stack: gtk::Stack,
    /// The account page and the states of mail that are not failures.
    status: adw::StatusPage,
    /// The account page's buttons; at most one is shown.
    status_retry_check: gtk::Button,
    status_online_accounts: gtk::Button,
    /// A failure that left nothing to show, in the list's place.
    failure_status: adw::StatusPage,
    failure_action: gtk::Button,
    /// Opens the failure dialog for the failure page.
    failure_details: gtk::Button,
    /// Names the latest refresh's failure or short list over the stored rows.
    list_banner: adw::Banner,
    refresh_inbox: gio::SimpleAction,
    read_stored_inbox: gio::SimpleAction,
    /// The sidebar box that shows the spinner while a load runs.
    loading_spinner_box: gtk::Box,
}

/// The selected account's stored Inbox as the window last read it.
#[derive(Default)]
struct ShownInbox {
    account: Option<AccountId>,
    /// The number of the latest read. An older read's answer is dropped, so
    /// it never replaces what a newer read found (research §6).
    latest_read: u64,
    stored: StoredInbox,
}

#[derive(Default)]
enum StoredInbox {
    /// Not read: a refresh forgot a failed read, so its own outcome shows.
    #[default]
    NotRead,
    Reading,
    /// What the read found: `None` when no load of the account completed.
    Read(Option<Rc<[Message]>>),
    ReadFailed(Failure),
}

/// What the list area shows for the selected account's mail.
enum ShownMail {
    /// The stored rows, and the latest refresh's failure or short list.
    Messages {
        account_id: AccountId,
        messages: Rc<[Message]>,
        banner: Option<DeclaredFailure>,
    },
    /// A failure that left nothing to show, and the operation Retry repeats.
    Failed {
        failure: DeclaredFailure,
        retried: RetriedOperation,
    },
    /// A state that is not a failure.
    Status {
        title: &'static str,
        description: Option<&'static str>,
    },
    /// A read is running: the list's page without rows.
    Reading,
}

impl WindowUi {
    pub fn new(builder: &gtk::Builder, loader: Box<dyn LoadsInbox>, store: Arc<Store>) -> Rc<Self> {
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
            store,
            shown_inbox: RefCell::new(ShownInbox::default()),
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
            read_stored_inbox: gio::SimpleAction::new("read-stored-inbox", None),
            loading_spinner_box,
        });
        let refreshing = Rc::downgrade(&window);
        window.refresh_inbox.connect_activate(move |_, _| {
            if let Some(window) = refreshing.upgrade() {
                window.refresh_inbox();
            }
        });
        let reading = Rc::downgrade(&window);
        window.read_stored_inbox.connect_activate(move |_, _| {
            if let Some(window) = reading.upgrade() {
                window.read_shown_inbox();
                window.render();
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
                window.show_selected_account();
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

    /// Reads the shown account's stored Inbox again: the Retry of a read that
    /// failed, for the application to publish as `app.read-stored-inbox`.
    pub fn read_stored_inbox_action(&self) -> &gio::SimpleAction {
        &self.read_stored_inbox
    }

    /// Applies an account update: forgets the refresh outcomes of accounts
    /// Online Accounts no longer shows, cancels a load running for one, and
    /// on a complete answer deletes the stored mail of accounts gone or with
    /// Mail off. The load is cancelled first, so its write finds itself
    /// cancelled whichever takes the store's lock first (research §6).
    pub fn apply_account_update(&self, update: &AccountUpdate) {
        self.accounts.borrow_mut().apply_update(update);
        let accounts = self.accounts.borrow();
        self.inboxes
            .borrow_mut()
            .discard_excluded(|account_id| accounts.shows_account(account_id));
        drop(accounts);
        if update.last_check.is_complete() {
            self.delete_removed_accounts(update);
        }
        self.render();
    }

    /// Deletes, on GIO's thread pool, the stored mail of every account the
    /// complete answer does not list with Mail on (FR-008). An account whose
    /// Mail service is missing is not shown but keeps its mail. A failed
    /// deletion happens again at the next complete answer.
    fn delete_removed_accounts(&self, update: &AccountUpdate) {
        let accounts_with_mail: BTreeSet<AccountId> = update
            .accounts
            .iter()
            .filter(|(_, details)| details.mail_enabled)
            .map(|(account_id, _)| account_id.clone())
            .collect();
        let store = self.store.clone();
        glib::spawn_future_local(async move {
            let deleted = run_on_pool(move || store.keep_accounts(&accounts_with_mail)).await;
            match deleted {
                Ok(accounts) => {
                    for account_id in accounts {
                        tracing::info!(
                            account = account_id.as_str(),
                            "stored mail of a removed account deleted"
                        );
                    }
                }
                Err(failure) => tracing::error!(
                    cause = ?failure.kind,
                    "stored mail of removed accounts not deleted"
                ),
            }
        });
    }

    /// Quit: cancels a running load without waiting for its worker.
    pub fn cancel_loads(&self) {
        self.inboxes.borrow_mut().cancel_load();
    }

    /// Refresh Inbox: starts the only load; the stored rows stay meanwhile.
    fn refresh_inbox(self: &Rc<Self>) {
        let Some((account_id, provider)) = self.refreshable_account() else {
            return;
        };
        if self.inboxes.borrow().is_loading() {
            return;
        }
        // The list shows the refresh's outcome, the newest, rather than an
        // earlier read that failed.
        self.shown_inbox.borrow_mut().forget_read_failure();
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

    /// Shows the selected account's stored mail; selecting never loads. The
    /// Inbox already on screen is not read again, so selecting its account
    /// once more keeps the open message: every write to it is followed by a
    /// read.
    fn show_selected_account(self: &Rc<Self>) {
        let selected = self.accounts.borrow().selected_id().cloned();
        let on_screen =
            selected.is_some_and(|account_id| self.shown_inbox.borrow().holds(&account_id));
        if !on_screen {
            self.read_shown_inbox();
        }
        self.render();
    }

    /// Records how the load ended; a completed load of the shown account
    /// replaced its stored Inbox, which the window then reads.
    fn finish_load(self: &Rc<Self>, account_id: &AccountId, result: LoadResult) {
        let stored = matches!(result, LoadResult::Stored { .. });
        self.inboxes.borrow_mut().finish_load(account_id, result);
        if stored && self.accounts.borrow().selected_id() == Some(account_id) {
            self.read_shown_inbox();
        }
        self.render();
    }

    /// Reads the selected account's stored Inbox on GIO's thread pool and
    /// shows what the latest read found. A panic in the read ends it as a
    /// failure, and a failed read writes its error line.
    fn read_shown_inbox(self: &Rc<Self>) {
        let Some(account_id) = self.accounts.borrow().selected_id().cloned() else {
            return;
        };
        let read = self.shown_inbox.borrow_mut().start_read(&account_id);
        let store = self.store.clone();
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let read_account = account_id.clone();
            let answer = run_on_pool(move || store.read_inbox(&read_account)).await;
            if let Err(failure) = &answer {
                tracing::error!(
                    account = account_id.as_str(),
                    cause = ?failure.kind,
                    "stored Inbox read failed"
                );
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let latest = window.shown_inbox.borrow_mut().finish_read(read, answer);
            if latest {
                window.render();
            }
        });
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
        let shown_mail = self.shown_mail();
        match &shown_mail {
            ShownMail::Messages {
                account_id,
                messages,
                ..
            } => self.mail.show_inbox(account_id, messages),
            _ => self.mail.clear(),
        }
        let accounts = self.accounts.borrow();
        let selected = accounts.selected_id();
        self.mail
            .show_account(selected.and_then(|account_id| accounts.label_of(account_id)));
        // The list's pages and the banner have one writer: this function. Each
        // state shows one page and fills it whole, and only stored rows reveal
        // the banner, so no notice outlives its cause.
        self.list_banner.set_revealed(false);
        // The account page comes first; it covers the list and the reader
        // without touching the account's mail.
        if accounts.page() != AccountPage::SelectedAccount {
            self.show_account_page(&accounts);
        } else {
            match shown_mail {
                ShownMail::Messages { banner, .. } => {
                    self.list_stack.set_visible_child_name("messages");
                    if let Some(banner) = banner {
                        self.list_banner.set_title(banner.title);
                        self.list_banner.set_revealed(true);
                    }
                }
                ShownMail::Failed { failure, retried } => self.show_failure(&failure, retried),
                ShownMail::Status { title, description } => {
                    self.show_mail_status(title, description)
                }
                ShownMail::Reading => self.list_stack.set_visible_child_name("messages"),
            }
        }
        let inboxes = self.inboxes.borrow();
        self.loading_spinner_box.set_visible(inboxes.is_loading());
        self.refresh_inbox
            .set_enabled(!inboxes.is_loading() && accounts.selected_provider().is_some());
    }

    /// What the list shows for the selected account, the first that applies:
    /// stored rows; a load running; a failed read; a failed refresh; a read
    /// running; an empty stored Inbox; nothing stored
    /// (specs/007-mail-storage FR-005, FR-006, FR-013).
    fn shown_mail(&self) -> ShownMail {
        let no_mail_loaded = ShownMail::Status {
            title: "No mail loaded",
            description: Some(
                "Choose Refresh Inbox in the main menu to load this account's Inbox.",
            ),
        };
        let accounts = self.accounts.borrow();
        let Some(account_id) = accounts.selected_id() else {
            return no_mail_loaded;
        };
        let inboxes = self.inboxes.borrow();
        let outcome = inboxes.outcome_of(account_id);
        let shown = self.shown_inbox.borrow();
        let stored = match &shown.account {
            Some(shown_account) if shown_account == account_id => &shown.stored,
            _ => &StoredInbox::NotRead,
        };
        match (stored, outcome) {
            (StoredInbox::Read(Some(messages)), _) if !messages.is_empty() => ShownMail::Messages {
                account_id: account_id.clone(),
                messages: messages.clone(),
                banner: outcome.and_then(banner_of),
            },
            _ if inboxes.is_loading_account(account_id) => ShownMail::Status {
                title: "Loading Inbox",
                description: None,
            },
            (StoredInbox::ReadFailed(failure), _) => ShownMail::Failed {
                failure: declare_failure(failure),
                retried: RetriedOperation::ReadStoredInbox,
            },
            (_, Some(RefreshOutcome::Failed(failure))) => ShownMail::Failed {
                failure: declare_failure(failure),
                retried: RetriedOperation::RefreshInbox,
            },
            (StoredInbox::Reading, _) => ShownMail::Reading,
            (StoredInbox::Read(Some(_)), _) => ShownMail::Status {
                title: "Inbox is empty",
                description: None,
            },
            (StoredInbox::Read(None) | StoredInbox::NotRead, _) => no_mail_loaded,
        }
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

    /// A failure that left nothing to show: the failure page takes the list's
    /// place. Its Details button is always there: such a failure always has
    /// technical details.
    fn show_failure(&self, failure: &DeclaredFailure, retried: RetriedOperation) {
        self.list_stack.set_visible_child_name("failed");
        self.failure_status.set_title(failure.title);
        self.failure_status
            .set_description(Some(&status_description(failure)));
        show_action_button(&self.failure_action, failure.action, retried);
    }

    /// Opens the failure dialog for what the list area shows: the failure
    /// page, or the banner over the stored rows.
    fn open_failure_dialog(&self) {
        let (failure, retried) = match self.shown_mail() {
            ShownMail::Failed { failure, retried } => (failure, retried),
            ShownMail::Messages {
                banner: Some(failure),
                ..
            } => (failure, RetriedOperation::RefreshInbox),
            _ => return,
        };
        failure_dialog::present(&self.list_stack, &failure, retried);
    }

    /// Whether a read of the stored Inbox is running, for the graphical test
    /// to wait for.
    #[cfg(test)]
    pub fn reads_stored_inbox(&self) -> bool {
        matches!(self.shown_inbox.borrow().stored, StoredInbox::Reading)
    }
}

/// Runs store work on GIO's thread pool, off GTK's thread; a panic inside it
/// ends it as a failure of kind `Stopped` with the panic's message and place
/// (specs/007-mail-storage/research.md §9).
async fn run_on_pool<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    match gio::spawn_blocking(move || catch_panic(work)).await {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => Err(Failure::stopped(Some(panic))),
        // The work catches its own panics, so the pool's guard is not reached.
        Err(_) => Err(Failure::stopped(None)),
    }
}

/// The banner over the stored rows: the latest refresh's failure, or why its
/// list is short.
fn banner_of(outcome: &RefreshOutcome) -> Option<DeclaredFailure> {
    match outcome {
        RefreshOutcome::Failed(failure) => Some(declare_failure(failure)),
        RefreshOutcome::Stored(incomplete) => incomplete.as_ref().map(declare_short_list),
    }
}

impl ShownInbox {
    /// Whether this account's stored Inbox is on screen, or being read.
    fn holds(&self, account_id: &AccountId) -> bool {
        self.account.as_ref() == Some(account_id)
            && matches!(self.stored, StoredInbox::Read(_) | StoredInbox::Reading)
    }

    /// Starts a read of the account's stored Inbox and returns its number.
    fn start_read(&mut self, account_id: &AccountId) -> u64 {
        self.latest_read += 1;
        self.account = Some(account_id.clone());
        self.stored = StoredInbox::Reading;
        self.latest_read
    }

    /// Keeps what read `read` found, if no newer read started meanwhile.
    fn finish_read(&mut self, read: u64, answer: Result<Option<Vec<Message>>, Failure>) -> bool {
        if read != self.latest_read {
            return false;
        }
        self.stored = match answer {
            Ok(messages) => StoredInbox::Read(messages.map(Rc::from)),
            Err(failure) => StoredInbox::ReadFailed(failure),
        };
        true
    }

    fn forget_read_failure(&mut self) {
        if matches!(self.stored, StoredInbox::ReadFailed(_)) {
            self.stored = StoredInbox::NotRead;
        }
    }
}
