// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decides what the window shows: the account page Online Accounts needs, or
//! the selected mailbox's stored mail with its account's latest refresh
//! outcome. It is the only owner of `list_stack`. It reads the store on GIO's
//! thread pool, never on GTK's thread (specs/007-mail-storage FR-011).

#[cfg(test)]
mod tests;

use crate::accounts::{AccountPage, Selection};
use crate::failure_declarations::{DeclaredFailure, declare_failure, declare_short_list};
use crate::failure_dialog::{self, RetriedOperation, show_action_button, status_description};
use crate::mail_ui::MailUi;
use crate::refreshes::{RefreshOutcome, Refreshes};
use crate::sidebar_ui::{PageAction, SidebarUi, show_check_progress};
use adw::{gio, glib, gtk, prelude::*};
use goa_adapter::AccountUpdate;
use mailbag_domain::{AccountId, Failure, FailureKind, Folder, FolderRef, Message, catch_panic};
use mailbag_providers::{LoadResult, LoadTarget, LoadsMail, MailProvider};
use mailbag_store::Store;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::Arc};

pub struct WindowUi {
    sidebar: Rc<RefCell<SidebarUi>>,
    refreshes: RefCell<Refreshes>,
    store: Arc<Store>,
    folder_lists: RefCell<FolderListsRead>,
    shown_mailbox: RefCell<ShownMailbox>,
    mail: Rc<MailUi>,
    loader: Box<dyn LoadsMail>,
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
    refresh_mailbox: gio::SimpleAction,
    refresh_account: gio::SimpleAction,
    read_stored_mail: gio::SimpleAction,
    /// The sidebar box that shows the spinner while a load runs.
    loading_spinner_box: gtk::Box,
}

/// The shown accounts' folder lists as the window last read them. The
/// sidebar holds what was read; a failed read leaves it as it was.
#[derive(Default)]
struct FolderListsRead {
    /// The number of the latest read. An older read's answer is dropped, so
    /// it never replaces what a newer read found.
    latest_read: u64,
    reading: bool,
    failure: Option<Failure>,
}

/// The selected mailbox's stored messages as the window last read them.
#[derive(Default)]
struct ShownMailbox {
    folder: Option<FolderRef>,
    /// The number of the latest read, as for the folder lists.
    latest_read: u64,
    stored: StoredMailbox,
}

#[derive(Default)]
enum StoredMailbox {
    /// Not read: a refresh forgot a failed read, so its own outcome shows.
    #[default]
    NotRead,
    Reading,
    /// What the read found: `None` when no load of the mailbox completed.
    Read(Option<Rc<[Message]>>),
    ReadFailed(Failure),
}

/// A notice over the stored rows, and the operation its Retry repeats.
type Banner = (DeclaredFailure, RetriedOperation);

/// What the list area shows for the selected mail.
enum ShownMail {
    /// The stored rows, and the latest refresh's failure or short list.
    Messages {
        account_id: AccountId,
        messages: Rc<[Message]>,
        banner: Option<Banner>,
    },
    /// A stored mailbox without messages, and why its list may be short.
    EmptyMailbox { banner: Option<Banner> },
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
    pub fn new(builder: &gtk::Builder, loader: Box<dyn LoadsMail>, store: Arc<Store>) -> Rc<Self> {
        let sidebar = SidebarUi::new(builder);
        let loading_spinner_box: gtk::Box = builder
            .object("sync_button_list")
            .expect("mailbag.ui: sync_button_list");
        builder
            .object::<gtk::Stack>("sync_icon_list")
            .expect("mailbag.ui: sync_icon_list")
            .set_visible_child_name("active");
        let window = Rc::new(Self {
            sidebar,
            refreshes: RefCell::new(Refreshes::default()),
            store,
            folder_lists: RefCell::new(FolderListsRead::default()),
            shown_mailbox: RefCell::new(ShownMailbox::default()),
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
            refresh_mailbox: gio::SimpleAction::new("refresh-mailbox", None),
            refresh_account: gio::SimpleAction::new("refresh-account", None),
            read_stored_mail: gio::SimpleAction::new("read-stored-mail", None),
            loading_spinner_box,
        });
        let refreshing = Rc::downgrade(&window);
        window.refresh_mailbox.connect_activate(move |_, _| {
            if let Some(window) = refreshing.upgrade() {
                window.refresh_mailbox();
            }
        });
        let refreshing = Rc::downgrade(&window);
        window.refresh_account.connect_activate(move |_, _| {
            if let Some(window) = refreshing.upgrade() {
                window.refresh_account();
            }
        });
        let reading = Rc::downgrade(&window);
        window.read_stored_mail.connect_activate(move |_, _| {
            if let Some(window) = reading.upgrade() {
                window.read_folder_lists();
                window.read_shown_mailbox();
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
        window.sidebar.borrow().connect_selection_changed(move || {
            if let Some(window) = selecting.upgrade() {
                window.show_selection();
            }
        });
        window.render();
        window
    }

    pub fn sidebar(&self) -> &Rc<RefCell<SidebarUi>> {
        &self.sidebar
    }

    /// Refresh Mailbox, for the application to publish under its menu item.
    pub fn refresh_mailbox_action(&self) -> &gio::SimpleAction {
        &self.refresh_mailbox
    }

    /// Refresh Account, for the application to publish under its menu item.
    pub fn refresh_account_action(&self) -> &gio::SimpleAction {
        &self.refresh_account
    }

    /// Reads the stored mail again, the folder lists and the shown mailbox:
    /// the Retry of a read that failed, for the application to publish as
    /// `app.read-stored-mail`.
    pub fn read_stored_mail_action(&self) -> &gio::SimpleAction {
        &self.read_stored_mail
    }

    /// Applies an account update: forgets the refresh outcomes of accounts
    /// Online Accounts no longer shows and cancels a load running for one;
    /// on a complete answer, deletes the stored mail of accounts gone or with
    /// Mail off and reads the folder lists of the shown accounts. The load is
    /// cancelled first, so its write finds itself cancelled whichever takes
    /// the store's lock first (specs/007-mail-storage/research.md §6).
    pub fn apply_account_update(self: &Rc<Self>, update: &AccountUpdate) {
        self.sidebar.borrow_mut().apply_update(update);
        let sidebar = self.sidebar.borrow();
        self.refreshes
            .borrow_mut()
            .discard_excluded(|account_id| sidebar.shows_account(account_id));
        self.shown_mailbox
            .borrow_mut()
            .forget_excluded(|account_id| sidebar.shows_account(account_id));
        drop(sidebar);
        if update.last_check.is_complete() {
            self.delete_removed_accounts(update);
            self.read_folder_lists();
        }
        self.render();
    }

    /// Deletes, on GIO's thread pool, the stored mail of every account the
    /// complete answer does not list with Mail on (007 FR-008). An account
    /// whose Mail service is missing is not shown but keeps its mail. A
    /// failed deletion happens again at the next complete answer.
    fn delete_removed_accounts(&self, update: &AccountUpdate) {
        let accounts_with_mail: BTreeSet<AccountId> = update
            .accounts
            .iter()
            .filter(|(_, details)| details.mail_enabled)
            .map(|(account_id, _)| account_id.clone())
            .collect();
        let store = self.store.clone();
        glib::spawn_future_local(async move {
            let deleted = run_on_pool(FailureKind::Stopped, move || {
                store.delete_other_accounts(&accounts_with_mail)
            })
            .await;
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
        self.refreshes.borrow_mut().cancel_load();
    }

    /// Refresh Mailbox: loads the selected mailbox's newest messages.
    fn refresh_mailbox(self: &Rc<Self>) {
        let Some((Selection::Mailbox(folder), provider)) = self.selection_with_provider() else {
            return;
        };
        self.start_load(
            folder.account.clone(),
            provider,
            LoadTarget::Mailbox(folder),
        );
    }

    /// Refresh Account: loads the folder list of the selected account or of
    /// the selected mailbox's account.
    fn refresh_account(self: &Rc<Self>) {
        let Some((selection, provider)) = self.selection_with_provider() else {
            return;
        };
        self.start_load(
            selection.account().clone(),
            provider,
            LoadTarget::FolderList,
        );
    }

    /// Starts the only load; what is stored stays on screen meanwhile.
    fn start_load(
        self: &Rc<Self>,
        account_id: AccountId,
        provider: MailProvider,
        target: LoadTarget,
    ) {
        if self.refreshes.borrow().is_loading() {
            return;
        }
        // The list shows the refresh's outcome, the newest, rather than an
        // earlier read that failed.
        self.shown_mailbox.borrow_mut().forget_read_failure();
        self.folder_lists.borrow_mut().failure = None;
        let window = Rc::downgrade(self);
        let loaded_account = account_id.clone();
        let loaded_target = target.clone();
        // The result arrives later on this context, never inside start_load.
        let cancellation = self.loader.start_load(
            &account_id,
            provider,
            target.clone(),
            Box::new(move |result| {
                if let Some(window) = window.upgrade() {
                    window.finish_load(&loaded_account, loaded_target, result);
                }
            }),
        );
        self.refreshes
            .borrow_mut()
            .begin_load(&account_id, target, cancellation);
        self.render();
    }

    /// Shows the selected mail; selecting never loads. The mailbox already on
    /// screen is not read again, so selecting it once more keeps the open
    /// message: every write to it while selected is followed by a read. The
    /// window forgets it once an account or nothing is selected, since a
    /// load may write it meanwhile, and when its account is hidden, whose
    /// mail may then be deleted.
    fn show_selection(self: &Rc<Self>) {
        match self.selected_mailbox() {
            Some(folder) if self.shown_mailbox.borrow().holds(&folder) => {}
            Some(_) => self.read_shown_mailbox(),
            None => self.shown_mailbox.borrow_mut().forget(),
        }
        self.render();
    }

    /// Records how the load ended. A completed folder list is read again
    /// with every other; it left the selected mailbox's rows as they were, so
    /// they are read only when the window does not hold them: the load's
    /// start forgot a failed read of them (007 FR-013). A completed load of
    /// the selected mailbox replaced its stored messages, which the window
    /// then reads.
    fn finish_load(
        self: &Rc<Self>,
        account_id: &AccountId,
        target: LoadTarget,
        result: LoadResult,
    ) {
        let stored = matches!(result, LoadResult::Stored { .. });
        self.refreshes.borrow_mut().finish_load(account_id, result);
        let selected = self.selected_mailbox();
        match target {
            LoadTarget::FolderList if stored => {
                self.read_folder_lists();
                let held = selected
                    .as_ref()
                    .is_some_and(|folder| self.shown_mailbox.borrow().holds(folder));
                if selected.is_some() && !held {
                    self.read_shown_mailbox();
                }
            }
            LoadTarget::Mailbox(folder) if stored && selected.as_ref() == Some(&folder) => {
                self.read_shown_mailbox()
            }
            _ => {}
        }
        self.render();
    }

    /// Reads every shown account's stored folder list on GIO's thread pool, in
    /// one read, and shows what the latest read found. A failed read leaves
    /// the sidebar as it was and shows the failure (specs/008-folders FR-008).
    fn read_folder_lists(self: &Rc<Self>) {
        let accounts = self.sidebar.borrow().shown_accounts();
        let read = self.folder_lists.borrow_mut().start_read();
        let store = self.store.clone();
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let answer = run_on_pool(FailureKind::StoredMailUnreadable, move || {
                accounts
                    .into_iter()
                    .map(|account| {
                        let folders = store.read_folders(&account)?;
                        Ok((account, folders))
                    })
                    .collect::<Result<Vec<_>, Failure>>()
            })
            .await;
            if let Err(failure) = &answer {
                tracing::error!(cause = ?failure.kind, "stored folder lists read failed");
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let latest = window.folder_lists.borrow_mut().finish_read(read, &answer);
            if !latest {
                return;
            }
            if let Ok(lists) = answer {
                window.show_folder_lists(lists);
            }
            window.render();
        });
    }

    /// Shows each account's folders; the sidebar clears a selection that its
    /// new folders no longer show (specs/008-folders FR-010), and the window
    /// then forgets that mailbox's rows.
    fn show_folder_lists(&self, lists: Vec<(AccountId, Vec<Folder>)>) {
        let mut sidebar = self.sidebar.borrow_mut();
        for (account, folders) in lists {
            if sidebar.show_folders(&account, folders) {
                self.shown_mailbox.borrow_mut().forget();
            }
        }
    }

    /// Reads the selected mailbox's stored messages on GIO's thread pool and
    /// shows what the latest read found. A panic in the read ends it as a
    /// failure, and a failed read writes its error line.
    fn read_shown_mailbox(self: &Rc<Self>) {
        let Some(folder) = self.selected_mailbox() else {
            return;
        };
        let read = self.shown_mailbox.borrow_mut().start_read(&folder);
        let store = self.store.clone();
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let account = folder.account.clone();
            // A panic in the read is the read's failure (006 FR-014).
            let answer = run_on_pool(FailureKind::StoredMailUnreadable, move || {
                store.read_mailbox(&folder)
            })
            .await;
            if let Err(failure) = &answer {
                tracing::error!(
                    account = account.as_str(),
                    cause = ?failure.kind,
                    "stored mailbox read failed"
                );
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let latest = window.shown_mailbox.borrow_mut().finish_read(read, answer);
            if latest {
                window.render();
            }
        });
    }

    fn selected_mailbox(&self) -> Option<FolderRef> {
        match self.sidebar.borrow().selection()? {
            Selection::Mailbox(folder) => Some(folder.clone()),
            Selection::Account(_) => None,
        }
    }

    /// The selection the refresh actions load, with the sequence it needs.
    fn selection_with_provider(&self) -> Option<(Selection, MailProvider)> {
        let sidebar = self.sidebar.borrow();
        Some((sidebar.selection()?.clone(), sidebar.selected_provider()?))
    }

    fn render(&self) {
        let shown_mail = self.shown_mail();
        match &shown_mail {
            ShownMail::Messages {
                account_id,
                messages,
                ..
            } => self.mail.show_rows(account_id, messages),
            _ => self.mail.clear(),
        }
        let sidebar = self.sidebar.borrow();
        let (mailbox_name, account_label) = match sidebar.selection() {
            Some(Selection::Mailbox(folder)) => (
                sidebar.folder_name(folder),
                sidebar.label_of(&folder.account),
            ),
            Some(Selection::Account(account)) => (None, sidebar.label_of(account)),
            None => (None, None),
        };
        self.mail
            .show_title(mailbox_name.as_deref(), account_label.as_deref());
        // The list's pages and the banner have one writer: this function. Each
        // state shows one page and fills it whole, and only stored rows reveal
        // the banner, so no notice outlives its cause.
        self.list_banner.set_revealed(false);
        // The account page comes first; it covers the list and the reader
        // without touching the account's mail.
        if sidebar.page() != AccountPage::AccountsShown {
            self.show_account_page(&sidebar);
        } else {
            match shown_mail {
                ShownMail::Messages { banner, .. } => {
                    self.list_stack.set_visible_child_name("messages");
                    self.show_banner(banner);
                }
                ShownMail::EmptyMailbox { banner } => {
                    self.show_mail_status("Mailbox is empty", None);
                    self.show_banner(banner);
                }
                ShownMail::Failed { failure, retried } => self.show_failure(&failure, retried),
                ShownMail::Status { title, description } => {
                    self.show_mail_status(title, description)
                }
                ShownMail::Reading => self.list_stack.set_visible_child_name("messages"),
            }
        }
        let refreshes = self.refreshes.borrow();
        self.loading_spinner_box.set_visible(refreshes.is_loading());
        let can_load = !refreshes.is_loading() && sidebar.selected_provider().is_some();
        self.refresh_mailbox
            .set_enabled(can_load && matches!(sidebar.selection(), Some(Selection::Mailbox(_))));
        self.refresh_account.set_enabled(can_load);
    }

    /// What the list shows, the first that applies: stored folder lists that
    /// cannot be read; nothing selected; for a mailbox, its stored rows, a
    /// read running, a load of it running, a failed read, a failed refresh,
    /// an empty stored mailbox or nothing stored (specs/007-mail-storage
    /// FR-005, FR-006, FR-013; specs/008-folders FR-008 to FR-010).
    fn shown_mail(&self) -> ShownMail {
        if let Some(failure) = &self.folder_lists.borrow().failure {
            return ShownMail::Failed {
                failure: declare_failure(failure, RetriedOperation::ReadStoredMail),
                retried: RetriedOperation::ReadStoredMail,
            };
        }
        let sidebar = self.sidebar.borrow();
        let Some(selection) = sidebar.selection() else {
            return ShownMail::Status {
                title: "Select a mailbox",
                description: None,
            };
        };
        let shown_folder = match selection {
            Selection::Mailbox(folder) => Some(folder),
            Selection::Account(_) => None,
        };
        let refreshes = self.refreshes.borrow();
        let account = selection.account();
        // A load's outcome and progress belong to what it loaded: a
        // mailbox's to that mailbox, a folder list's to the whole account
        // (specs/008-folders FR-010).
        let concerns_shown = |target: &LoadTarget| match target {
            LoadTarget::FolderList => true,
            LoadTarget::Mailbox(folder) => shown_folder == Some(folder),
        };
        let outcome = refreshes
            .outcome_of(account)
            .filter(|(target, _)| concerns_shown(target));
        let banner = outcome.and_then(|(target, outcome)| banner_of(outcome, retried_by(target)));
        let loading = refreshes
            .loading_target(account)
            .filter(|target| concerns_shown(target));
        let no_mail_loaded = ShownMail::Status {
            title: "No mail loaded",
            description: Some("Choose Refresh Account or Refresh Mailbox in the main menu."),
        };
        let shown = self.shown_mailbox.borrow();
        let stored = match shown_folder {
            Some(folder) if shown.folder.as_ref() == Some(folder) => &shown.stored,
            _ => &StoredMailbox::NotRead,
        };
        match (stored, outcome) {
            (StoredMailbox::Read(Some(messages)), _) if !messages.is_empty() => {
                ShownMail::Messages {
                    account_id: account.clone(),
                    messages: messages.clone(),
                    banner,
                }
            }
            (StoredMailbox::Reading, _) => ShownMail::Reading,
            _ if loading.is_some() => ShownMail::Status {
                title: match loading {
                    Some(LoadTarget::FolderList) => "Loading mailbox list",
                    _ => "Loading mailbox",
                },
                description: None,
            },
            (StoredMailbox::ReadFailed(failure), _) => ShownMail::Failed {
                failure: declare_failure(failure, RetriedOperation::ReadStoredMail),
                retried: RetriedOperation::ReadStoredMail,
            },
            (_, Some((target, RefreshOutcome::Failed(failure)))) => ShownMail::Failed {
                failure: declare_failure(failure, retried_by(target)),
                retried: retried_by(target),
            },
            (StoredMailbox::Read(Some(_)), _) => ShownMail::EmptyMailbox { banner },
            (StoredMailbox::Read(None) | StoredMailbox::NotRead, _) => no_mail_loaded,
        }
    }

    /// Online Accounts' page: its text and its one button.
    fn show_account_page(&self, sidebar: &SidebarUi) {
        let (title, description) = sidebar.page_text();
        self.list_stack.set_visible_child_name("empty");
        self.status.set_title(title);
        self.status
            .set_description((!description.is_empty()).then_some(description));
        let action = sidebar.page_action();
        self.status_retry_check
            .set_visible(action == Some(PageAction::RetryCheck));
        show_check_progress(&self.status_retry_check, sidebar.retry_pending());
        self.status_online_accounts
            .set_visible(action == Some(PageAction::OnlineAccounts));
    }

    /// The latest refresh's failure or short list over the stored rows.
    fn show_banner(&self, banner: Option<Banner>) {
        if let Some((banner, _)) = banner {
            self.list_banner.set_title(banner.title);
            self.list_banner.set_revealed(true);
        }
    }

    /// A state of the selected mail that is not a failure.
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
                banner: Some(banner),
                ..
            }
            | ShownMail::EmptyMailbox {
                banner: Some(banner),
            } => banner,
            _ => return,
        };
        failure_dialog::present(&self.list_stack, &failure, retried);
    }

    /// Whether a read of the stored mail is running, for the graphical test
    /// to wait for.
    #[cfg(test)]
    pub fn reads_stored_mail(&self) -> bool {
        self.folder_lists.borrow().reading
            || matches!(self.shown_mailbox.borrow().stored, StoredMailbox::Reading)
    }
}

/// Runs store work on GIO's thread pool, off GTK's thread; a panic inside it
/// ends it as a failure of the work's own kind, `panicked_as`, with the
/// panic's message and place (specs/007-mail-storage/research.md §9).
async fn run_on_pool<T: Send + 'static>(
    panicked_as: FailureKind,
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    match gio::spawn_blocking(move || catch_panic(work)).await {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => Err(Failure::from_panic(panicked_as, Some(panic))),
        // The work catches its own panics, so the pool's guard is not reached.
        Err(_) => Err(Failure::from_panic(panicked_as, None)),
    }
}

/// The action whose Retry repeats a load of `target`.
fn retried_by(target: &LoadTarget) -> RetriedOperation {
    match target {
        LoadTarget::FolderList => RetriedOperation::RefreshAccount,
        LoadTarget::Mailbox(_) => RetriedOperation::RefreshMailbox,
    }
}

/// The banner over the stored rows: the latest refresh's failure, or why its
/// list is short.
fn banner_of(outcome: &RefreshOutcome, retried: RetriedOperation) -> Option<Banner> {
    let banner = match outcome {
        RefreshOutcome::Failed(failure) => declare_failure(failure, retried),
        RefreshOutcome::Stored(incomplete) => declare_short_list(incomplete.as_ref()?),
    };
    Some((banner, retried))
}

impl FolderListsRead {
    /// Starts a read and returns its number.
    fn start_read(&mut self) -> u64 {
        self.latest_read += 1;
        self.reading = true;
        self.latest_read
    }

    /// Keeps how read `read` ended, if no newer read started meanwhile.
    fn finish_read<T>(&mut self, read: u64, answer: &Result<T, Failure>) -> bool {
        if read != self.latest_read {
            return false;
        }
        self.reading = false;
        self.failure = answer.as_ref().err().cloned();
        true
    }
}

impl ShownMailbox {
    /// Whether this mailbox's stored messages are on screen, or being read.
    fn holds(&self, folder: &FolderRef) -> bool {
        self.folder.as_ref() == Some(folder)
            && matches!(self.stored, StoredMailbox::Read(_) | StoredMailbox::Reading)
    }

    /// Starts a read of the mailbox's stored messages and returns its number.
    fn start_read(&mut self, folder: &FolderRef) -> u64 {
        self.latest_read += 1;
        self.folder = Some(folder.clone());
        self.stored = StoredMailbox::Reading;
        self.latest_read
    }

    /// Keeps what read `read` found, if no newer read started meanwhile.
    fn finish_read(&mut self, read: u64, answer: Result<Option<Vec<Message>>, Failure>) -> bool {
        if read != self.latest_read {
            return false;
        }
        self.stored = match answer {
            Ok(messages) => StoredMailbox::Read(messages.map(Rc::from)),
            Err(failure) => StoredMailbox::ReadFailed(failure),
        };
        true
    }

    /// Forgets what was read of an account Online Accounts no longer shows,
    /// whose stored mail may be deleted; a read still running for it is
    /// dropped when it answers.
    fn forget_excluded(&mut self, is_visible: impl Fn(&AccountId) -> bool) {
        if self
            .folder
            .as_ref()
            .is_some_and(|folder| !is_visible(&folder.account))
        {
            self.forget();
        }
    }

    /// Forgets what was read, so the next selection reads the store; a read
    /// still running is dropped when it answers.
    fn forget(&mut self) {
        self.latest_read += 1;
        self.folder = None;
        self.stored = StoredMailbox::NotRead;
    }

    fn forget_read_failure(&mut self) {
        if matches!(self.stored, StoredMailbox::ReadFailed(_)) {
            self.stored = StoredMailbox::NotRead;
        }
    }
}
