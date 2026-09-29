// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::message_item::MessageItem;
use super::*;
use crate::accounts::Selection;
use crate::failure_declarations::{declare_content, declare_failure, declare_short_list};
use crate::failure_dialog::RetriedOperation;
use crate::test_directory::TestDirectory;
use crate::window_ui::WindowUi;
use goa_adapter::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider, AccountUpdate,
    ErrorCause,
};
use mailbag_domain::{
    AccountId, ContentExplanation, Failure, FailureKind, Folder, FolderBatch, FolderRef,
    FolderRole, FolderState, IncompleteList, Message, RemoteSource, RemoteText, ServerStep,
};
use mailbag_providers::{
    CancelsLoadOnDrop, LoadEvent, LoadResult, LoadTarget, LoadsMail, MailProvider,
};
use mailbag_store::{Store, StoreWrite};
use std::{
    cell::Cell,
    sync::Arc,
    time::{Duration, Instant},
};

/// One load the window started, waiting for the result the test chooses.
struct StartedLoad {
    account_id: AccountId,
    provider: MailProvider,
    target: LoadTarget,
    on_event: Box<dyn FnMut(LoadEvent)>,
    /// Set when the window drops the load's step, which cancels it.
    cancelled: Rc<Cell<bool>>,
}

/// Reports the load results the test chooses, so the window is exercised
/// without Online Accounts and without a mail server. A completed load writes
/// its folder list or its messages into the window's store, as the mail
/// worker does.
struct ScriptedLoader {
    started_loads: RefCell<Vec<StartedLoad>>,
    cancelled_loads: Rc<Cell<usize>>,
    store: Arc<Store>,
}

/// A running load's step: dropping it counts a cancellation and marks the
/// load cancelled.
struct CountedStep {
    cancellations: Rc<Cell<usize>>,
    cancelled: Rc<Cell<bool>>,
}

impl CancelsLoadOnDrop for CountedStep {}

impl Drop for CountedStep {
    fn drop(&mut self) {
        self.cancellations.set(self.cancellations.get() + 1);
        self.cancelled.set(true);
    }
}

impl LoadsMail for ScriptedLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        target: LoadTarget,
        on_event: Box<dyn FnMut(LoadEvent)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        let cancelled = Rc::new(Cell::new(false));
        self.started_loads.borrow_mut().push(StartedLoad {
            account_id: account_id.clone(),
            provider,
            target,
            on_event,
            cancelled: cancelled.clone(),
        });
        Box::new(CountedStep {
            cancellations: self.cancelled_loads.clone(),
            cancelled,
        })
    }
}

/// The window owns its loader, while the test keeps a handle to the same one.
/// `LoadsMail` lives in another crate, so `Rc` itself cannot carry it.
struct SharedLoader(Rc<ScriptedLoader>);

impl LoadsMail for SharedLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        target: LoadTarget,
        on_event: Box<dyn FnMut(LoadEvent)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        self.0.start_load(account_id, provider, target, on_event)
    }
}

impl ScriptedLoader {
    fn new(store: Arc<Store>) -> Self {
        Self {
            started_loads: RefCell::default(),
            cancelled_loads: Rc::default(),
            store,
        }
    }

    fn running_loads(&self) -> usize {
        self.started_loads.borrow().len()
    }

    /// The account the running load was started for.
    fn loading_account(&self) -> Option<AccountId> {
        self.started_loads
            .borrow()
            .last()
            .map(|started| started.account_id.clone())
    }

    /// The load sequence the window chose for the running load.
    fn loading_provider(&self) -> Option<MailProvider> {
        self.started_loads
            .borrow()
            .last()
            .map(|started| started.provider)
    }

    /// What the running load loads.
    fn loading_target(&self) -> Option<LoadTarget> {
        self.started_loads
            .borrow()
            .last()
            .map(|started| started.target.clone())
    }

    fn take_running_load(&self) -> StartedLoad {
        self.started_loads
            .borrow_mut()
            .pop()
            .expect("a load is running")
    }

    /// Ends the running load the way the worker would.
    fn report(&self, result: LoadResult) {
        (self.take_running_load().on_event)(LoadEvent::Finished(result));
    }

    /// Stores a batch of the running mailbox load into `folder`, which may
    /// be another folder of its account, and reports it, as a cycle does;
    /// the load goes on.
    fn report_batch(&self, folder: &FolderRef, batch: &FolderBatch) {
        let mut loads = self.started_loads.borrow_mut();
        let started = loads.last_mut().expect("a load is running");
        let write = self
            .store
            .store_batch(folder, batch, || started.cancelled.get())
            .expect("the test store takes the batch");
        if write == StoreWrite::Stored {
            (started.on_event)(LoadEvent::BatchStored);
        }
    }

    /// Ends the running folder-list load as a completed one, as the worker
    /// does: a list with folders becomes the account's stored list, unless
    /// the load was cancelled before the store took it; an empty one stores
    /// nothing.
    fn report_folders(&self, folders: &[Folder]) {
        let started = self.take_running_load();
        assert_eq!(started.target, LoadTarget::FolderList);
        let mut report = started.on_event;
        if folders.is_empty() {
            report(LoadEvent::Finished(LoadResult::Stored { incomplete: None }));
            return;
        }
        let write = self
            .store
            .replace_folders(&started.account_id, folders, || started.cancelled.get())
            .expect("the test store takes the folder list");
        report(LoadEvent::Finished(stored_or_cancelled(write, None)));
    }

    /// Ends the running mailbox load as a completed one, as the worker does:
    /// its messages become the folder's stored ones, unless the load was
    /// cancelled before the store took them.
    fn report_stored(&self, messages: &[Message], incomplete: Option<IncompleteList>) {
        let started = self.take_running_load();
        let LoadTarget::Mailbox(folder) = &started.target else {
            panic!("a mailbox load is running");
        };
        let write =
            store_completed_cycle(&self.store, folder, messages, || started.cancelled.get())
                .expect("the test store takes the load");
        let mut report = started.on_event;
        report(LoadEvent::Finished(stored_or_cancelled(write, incomplete)));
    }
}

fn stored_or_cancelled(write: StoreWrite, incomplete: Option<IncompleteList>) -> LoadResult {
    match write {
        StoreWrite::Stored => LoadResult::Stored { incomplete },
        StoreWrite::LoadCancelled => LoadResult::Cancelled,
    }
}

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn accounts_update(accounts: &[(&str, AccountProvider, &str)]) -> AccountUpdate {
    let mut update = AccountUpdate {
        last_check: AccountCheckResult::Complete,
        ..Default::default()
    };
    for (name, provider, label) in accounts {
        update.accounts.insert(
            account(name),
            AccountDetails {
                provider: *provider,
                mail_enabled: true,
                needs_attention: false,
                mail_service_available: true,
                display_name: Some((*label).to_owned()),
                email_address: Some(format!("{label}@example.invalid")),
            },
        );
    }
    update
}

fn imap_and_google_accounts() -> AccountUpdate {
    accounts_update(&[
        ("synthetic-generic", AccountProvider::ImapSmtp, "Generic"),
        ("synthetic-google", AccountProvider::Google, "Google"),
    ])
}

fn folder(identity: &str, role: Option<FolderRole>) -> Folder {
    Folder {
        identity: identity.to_owned(),
        name: identity.to_owned(),
        parent: None,
        role,
        selectable: true,
    }
}

/// The Inbox and one folder of the user's.
fn inbox_and_projects() -> Vec<Folder> {
    vec![
        folder("INBOX", Some(FolderRole::Inbox)),
        folder("Projects", None),
    ]
}

fn folder_of(account: &AccountId, identity: &str) -> FolderRef {
    FolderRef {
        account: account.clone(),
        identity: identity.to_owned(),
    }
}

/// Stores the account's folders and `messages` in its Inbox, as loads of
/// an earlier run did.
fn store_mail(store: &Store, account: &AccountId, messages: &[Message]) {
    store
        .replace_folders(account, &inbox_and_projects(), || false)
        .expect("the test store takes the folder list");
    store_completed_cycle(store, &folder_of(account, "INBOX"), messages, || false)
        .expect("the test store takes the messages");
}

fn two_messages() -> Vec<Message> {
    vec![
        Message {
            identity: "uid:20".to_owned(),
            fields: DisplayFields {
                subject: Some("Second subject".to_owned()),
                from: Some("Second sender".to_owned()),
                to: Some("Recipient".to_owned()),
            },
            received_unix: Some(1_700_000_000),
            seen: false,
            content: ReceivedContent::Text("Second body".to_owned()),
        },
        Message {
            identity: "uid:10".to_owned(),
            fields: DisplayFields {
                subject: Some("First subject".to_owned()),
                from: Some("First sender".to_owned()),
                to: None,
            },
            received_unix: Some(1_699_000_000),
            seen: true,
            content: ReceivedContent::StructureUnreadable,
        },
    ]
}

/// One message the sender never wrapped and one of ordinary lines, both at
/// the 64 KiB display boundary, and one whose character set name is as long.
fn unwrapped_and_ordinary_messages() -> Vec<Message> {
    let bodies = [
        ("Never wrapped", ReceivedContent::Text("я".repeat(32_768))),
        (
            "Ordinary lines",
            ReceivedContent::Text("яяяяяяяя ".repeat(3_856)),
        ),
        (
            "Unknown character set",
            ReceivedContent::Explained(ContentExplanation::UnknownCharset("x".repeat(65_536))),
        ),
    ];
    (1..)
        .zip(bodies)
        .map(|(number, (subject, body))| Message {
            identity: format!("uid:{}", number * 10),
            fields: DisplayFields {
                subject: Some(subject.to_owned()),
                from: Some("Long sender".to_owned()),
                to: None,
            },
            // Newest first, as the list orders them.
            received_unix: Some(1_700_000_000 - number),
            seen: true,
            content: body,
        })
        .collect()
}

fn rejected_sign_in() -> Failure {
    Failure {
        kind: FailureKind::ServerRejectedSignIn,
        remote_texts: vec![
            RemoteText {
                source: RemoteSource::ServerAlert,
                text: "Mailbox quota is nearly full".to_owned(),
            },
            RemoteText {
                source: RemoteSource::ServerReply,
                text: "Invalid credentials".to_owned(),
            },
        ],
        details: "Failure: ServerRejectedSignIn\nServer code: AUTHENTICATIONFAILED".to_owned(),
    }
}

/// A failure without words of the server.
fn failure_of(kind: FailureKind) -> Failure {
    Failure {
        kind,
        remote_texts: Vec::new(),
        details: format!("Failure: {kind:?}"),
    }
}

/// A window over `store`, with its scripted loader and its widgets.
fn open_window(
    store: Arc<Store>,
) -> (adw::Window, Rc<WindowUi>, Rc<ScriptedLoader>, WindowWidgets) {
    let builder = gtk::Builder::from_string(include_str!("../../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").expect("window");
    let loader = Rc::new(ScriptedLoader::new(store.clone()));
    let ui = WindowUi::new(&builder, Box::new(SharedLoader(loader.clone())), store);
    window.present();
    (window, ui, loader, WindowWidgets { builder })
}

/// Runs the window's pending work and waits for its reads of the store,
/// which run on GIO's thread pool.
fn settle(ui: &WindowUi) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        dispatch_pending();
        if !ui.reads_stored_mail() {
            return;
        }
        assert!(Instant::now() < deadline, "the stored mail was not read");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The window's widgets, read the way the user sees them.
struct WindowWidgets {
    builder: gtk::Builder,
}

impl WindowWidgets {
    fn list_page(&self) -> String {
        self.stack("list_stack")
            .visible_child_name()
            .expect("list page")
            .to_string()
    }

    fn reader_page(&self) -> String {
        self.stack("reader_stack")
            .visible_child_name()
            .expect("reader page")
            .to_string()
    }

    fn stack(&self, name: &str) -> gtk::Stack {
        self.builder.object(name).expect("stack")
    }

    /// The account page, which also shows the states of mail that are not
    /// failures.
    fn status_title(&self) -> String {
        self.status_page("account_status").title().to_string()
    }

    fn status_description(&self) -> String {
        description_of(&self.status_page("account_status"))
    }

    fn failure_title(&self) -> String {
        self.status_page("failure_status").title().to_string()
    }

    fn failure_description(&self) -> String {
        description_of(&self.status_page("failure_status"))
    }

    fn status_page(&self, name: &str) -> adw::StatusPage {
        self.builder.object(name).expect("status page")
    }

    /// A button of a status page, as `(label, action name)` when shown.
    fn status_button(&self, name: &str) -> Option<(String, String)> {
        let button: gtk::Button = self.builder.object(name).expect("status button");
        button.is_visible().then(|| {
            (
                button.label().unwrap_or_default().to_string(),
                button.action_name().unwrap_or_default().to_string(),
            )
        })
    }

    fn banner(&self) -> adw::Banner {
        self.builder.object("list_banner").expect("list_banner")
    }

    /// The banner's title while it is revealed.
    fn banner_title(&self) -> Option<String> {
        let banner = self.banner();
        banner.is_revealed().then(|| banner.title().to_string())
    }

    /// The reader's status page, shown in the body's place.
    fn content_status(&self) -> Option<adw::StatusPage> {
        let slot: gtk::Box = self
            .builder
            .object("singleton_slot")
            .expect("singleton_slot");
        descendants::<adw::StatusPage>(&slot.upcast())
            .into_iter()
            .find(|status| status.is_visible())
    }

    fn shows_load_feedback(&self) -> bool {
        self.builder
            .object::<gtk::Box>("sync_button_list")
            .expect("sync_button_list")
            .is_visible()
    }

    fn messages(&self) -> gtk::ListView {
        self.builder.object("messages").expect("messages")
    }

    /// The list's row objects, which its shown rows are bound to.
    fn rows(&self) -> Vec<MessageItem> {
        let model = self.messages().model().expect("the list's model");
        (0..model.n_items())
            .map(|position| {
                model
                    .item(position)
                    .and_downcast::<MessageItem>()
                    .expect("a message item")
            })
            .collect()
    }

    /// Opens the row at `position`, as a click or Enter does.
    fn open_row(&self, position: u32) {
        self.messages().emit_by_name::<()>("activate", &[&position]);
    }

    /// The position of the selected row, which marks the open message.
    fn selected_row(&self) -> Option<u32> {
        let model = self.messages().model().expect("the list's model");
        let selected = model
            .downcast::<gtk::SingleSelection>()
            .expect("a single selection")
            .selected();
        (selected != gtk::INVALID_LIST_POSITION).then_some(selected)
    }

    fn reader_body(&self) -> String {
        descendants::<gtk::Label>(
            &self
                .builder
                .object::<gtk::Box>("singleton_slot")
                .expect("singleton_slot")
                .upcast(),
        )
        .into_iter()
        .map(|label| label.text().to_string())
        .collect::<Vec<_>>()
        .join("\n")
    }

    /// The reader's body label: the only wrapping, selectable label of the
    /// message form.
    fn reader_body_label(&self) -> gtk::Label {
        descendants::<gtk::Label>(
            &self
                .builder
                .object::<gtk::Box>("singleton_slot")
                .expect("singleton_slot")
                .upcast(),
        )
        .into_iter()
        .find(|label| label.wraps() && label.is_selectable())
        .expect("reader body label")
    }

    /// Lays the opened message out, as showing it does, and reports how long
    /// that took.
    fn lay_out_reader(&self) -> Duration {
        let started = Instant::now();
        dispatch_pending();
        let body = self.reader_body_label();
        body.measure(gtk::Orientation::Horizontal, -1);
        body.measure(gtk::Orientation::Vertical, 800);
        started.elapsed()
    }

    fn reader_subject(&self) -> String {
        self.builder
            .object::<gtk::Label>("reader_subject")
            .expect("reader_subject")
            .text()
            .to_string()
    }

    fn list_title(&self) -> (String, String) {
        let title: adw::WindowTitle = self.builder.object("list_title").expect("list_title");
        (title.title().to_string(), title.subtitle().to_string())
    }

    /// Selects the sidebar row of the account, or of one of its folders, as
    /// a click does.
    fn select(&self, ui: &WindowUi, account: &AccountId, folder: Option<&str>) {
        let position = ui
            .sidebar()
            .borrow()
            .position_of_row(account, folder)
            .expect("the row is shown");
        let tree = self
            .builder
            .object::<gtk::ListBox>("folder_tree")
            .expect("folder_tree");
        tree.select_row(tree.row_at_index(position as i32).as_ref());
    }
}

fn selection_of(ui: &WindowUi) -> Option<Selection> {
    ui.sidebar().borrow().selection().cloned()
}

fn description_of(status: &adw::StatusPage) -> String {
    status
        .description()
        .map(|description| description.to_string())
        .unwrap_or_default()
}

/// The texts of the list's row widgets built so far.
fn shown_labels(widgets: &WindowWidgets) -> Vec<String> {
    descendants::<gtk::Label>(&widgets.messages().upcast())
        .into_iter()
        .filter(|label| label.is_mapped())
        .map(|label| label.text().to_string())
        .collect()
}

/// What the row template shows of the item: sender, subject and date.
fn row_texts(item: &MessageItem) -> String {
    format!("{} {} {}", item.sender(), item.subject(), item.date_text())
}

/// Whether the row template shows the item's unread dot.
fn shows_unread_dot(item: &MessageItem) -> bool {
    item.unread()
}

/// Whether the failure dialog offers a shown button with this action.
fn dialog_offers(dialog: &adw::Dialog, action_name: &str) -> bool {
    descendants::<gtk::Button>(&dialog.clone().upcast())
        .iter()
        .any(|button| button.is_visible() && button.action_name().as_deref() == Some(action_name))
}

fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    if let Some(widget) = widget.downcast_ref::<T>() {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(descendants::<T>(&current));
        child = current.next_sibling();
    }
    found
}

fn dispatch_pending() {
    let context = glib::MainContext::default();
    for _ in 0..100 {
        if !context.pending() {
            break;
        }
        context.iteration(false);
    }
}

#[test]
#[ignore = "requires a graphical GTK session"]
fn mail_ui_transitions() {
    adw::init().expect("GTK display");
    let directory = TestDirectory::new();
    let store_path = directory.store_path();
    let (window, ui, loader, widgets) = open_window(Arc::new(Store::at(store_path.clone())));
    let refresh_mailbox = ui.refresh_mailbox_action().clone();
    let refresh_account = ui.refresh_account_action().clone();
    settle(&ui);

    // Without a selection the list asks for a mailbox and nothing refreshes.
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "Select a mailbox");
    assert_eq!(widgets.status_button("status_retry_check"), None);
    assert_eq!(widgets.status_button("status_online_accounts"), None);
    assert_eq!(widgets.list_title(), ("Mailbag".to_owned(), String::new()));
    assert!(!refresh_account.is_enabled());
    assert!(!refresh_mailbox.is_enabled());

    // An account without a folder list shows that nothing is stored, as an
    // unloaded folder does, and selecting it loads nothing (FR-009).
    let (generic, google) = (account("synthetic-generic"), account("synthetic-google"));
    widgets.select(&ui, &generic, None);
    settle(&ui);
    assert_eq!(loader.running_loads(), 0);
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert!(
        widgets.status_description().contains("Refresh Account"),
        "{}",
        widgets.status_description()
    );
    assert_eq!(widgets.list_title(), ("Generic".to_owned(), String::new()));
    assert!(refresh_account.is_enabled());
    assert!(!refresh_mailbox.is_enabled());
    assert!(!widgets.shows_load_feedback());

    // Refresh Account on a Google account asks for its folder list with the
    // Gmail sequence, not the Generic IMAP one.
    widgets.select(&ui, &google, None);
    settle(&ui);
    refresh_account.activate(None);
    settle(&ui);
    assert_eq!(loader.loading_provider(), Some(MailProvider::Gmail));
    assert_eq!(loader.loading_target(), Some(LoadTarget::FolderList));
    assert_eq!(widgets.status_title(), "Loading mailbox list");

    // With nothing stored, a failed load takes the list's place with its
    // declaration; the server's words stay in the failure dialog.
    let rejected = declare_failure(&rejected_sign_in(), RetriedOperation::RefreshAccount);
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert_eq!(widgets.failure_title(), rejected.title);
    // The page reads its description as markup, so the text arrives escaped.
    let description = widgets.failure_description();
    let advice = rejected.advice.expect("sign-in advice");
    assert!(advice.contains("Refresh Account"), "{advice}");
    for paragraph in [rejected.explanation.as_str(), advice] {
        let escaped = glib::markup_escape_text(paragraph);
        assert!(description.contains(escaped.as_str()), "{description}");
    }
    assert!(
        !description.contains("Invalid credentials"),
        "{description}"
    );
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Online Accounts".to_owned(), "app.accounts".to_owned()))
    );
    assert!(widgets.status_button("failure_details").is_some());
    assert!(refresh_account.is_enabled());
    click(&widgets, "failure_details");
    // The dialog shows the paragraphs, one block per remote text and the
    // technical details, in the spec's order, and the action.
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert_eq!(dialog.title(), rejected.title);
    let labels = descendants::<gtk::Label>(&dialog.clone().upcast());
    let shown_texts: Vec<String> = labels
        .iter()
        .filter(|label| label.is_visible())
        .map(|label| label.text().to_string())
        .collect();
    assert!(
        shown_texts.contains(&rejected.explanation),
        "{shown_texts:?}"
    );
    assert!(shown_texts.contains(&advice.to_owned()), "{shown_texts:?}");
    let headings: Vec<String> = labels
        .iter()
        .filter(|label| label.has_css_class("heading"))
        .map(|label| label.text().to_string())
        .collect();
    assert_eq!(
        headings,
        [
            "Alert from the mail server",
            "Reply from the mail server",
            "Technical details"
        ]
    );
    assert!(dialog_offers(&dialog, "app.accounts"));
    // A closed dialog is released with its widgets; the accessibility layer
    // lets go of it a few milliseconds after the window does.
    let closed_dialog = dialog.downgrade();
    dialog.force_close();
    drop((dialog, labels));
    let deadline = Instant::now() + Duration::from_secs(2);
    while closed_dialog.upgrade().is_some() && Instant::now() < deadline {
        dispatch_pending();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(closed_dialog.upgrade().is_none());

    // A failure nothing the user does can change offers no action, only
    // Details.
    refresh_account.activate(None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Loading mailbox list");
    loader.report(LoadResult::Failed(failure_of(FailureKind::NoSignInMethod)));
    settle(&ui);
    assert_eq!(widgets.status_button("failure_action"), None);
    assert!(widgets.status_button("failure_details").is_some());

    // A folder list that did not arrive is retried with Refresh Account.
    refresh_account.activate(None);
    settle(&ui);
    loader.report(LoadResult::Failed(failure_of(
        FailureKind::ServerNotResponding(ServerStep::ListFolders),
    )));
    settle(&ui);
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Retry".to_owned(), "app.refresh-account".to_owned()))
    );

    // A refresh loads once, with the spinner and without a second attempt.
    widgets.select(&ui, &generic, None);
    settle(&ui);
    refresh_account.activate(None);
    refresh_account.activate(None);
    settle(&ui);
    assert_eq!(loader.running_loads(), 1);
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    assert_eq!(loader.loading_provider(), Some(MailProvider::GenericImap));
    assert_eq!(widgets.status_title(), "Loading mailbox list");
    assert_eq!(widgets.list_page(), "empty");
    assert!(widgets.shows_load_feedback());
    assert!(!refresh_account.is_enabled());

    // The stored folder list makes the account a heading, which is no
    // longer selected and cannot be (FR-009, FR-010).
    loader.report_folders(&inbox_and_projects());
    settle(&ui);
    assert_eq!(selection_of(&ui), None);
    assert_eq!(widgets.status_title(), "Select a mailbox");
    assert!(!widgets.shows_load_feedback());
    assert!(!refresh_account.is_enabled());
    widgets.select(&ui, &generic, None);
    settle(&ui);
    assert_eq!(selection_of(&ui), None);

    // A folder never loaded shows that nothing is stored.
    let inbox = folder_of(&generic, "INBOX");
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(loader.running_loads(), 0);
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert_eq!(
        widgets.list_title(),
        ("Inbox".to_owned(), "Generic".to_owned())
    );
    assert!(refresh_mailbox.is_enabled());
    assert!(refresh_account.is_enabled());

    // Refresh Mailbox loads the selected folder, with the spinner.
    refresh_mailbox.activate(None);
    settle(&ui);
    assert_eq!(
        loader.loading_target(),
        Some(LoadTarget::Mailbox(inbox.clone()))
    );
    assert_eq!(widgets.status_title(), "Loading mailbox");
    assert!(widgets.shows_load_feedback());
    assert!(!refresh_mailbox.is_enabled());
    assert!(!refresh_account.is_enabled());

    // A completed load's stored mailbox fills the list, newest first.
    loader.report_stored(&two_messages(), None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert!(!widgets.shows_load_feedback());
    assert!(refresh_mailbox.is_enabled());
    let rows = widgets.rows();
    assert_eq!(rows.len(), 2);
    assert!(
        row_texts(&rows[0]).contains("Second sender"),
        "{}",
        row_texts(&rows[0])
    );
    assert!(row_texts(&rows[0]).contains("Second subject"));
    assert!(shows_unread_dot(&rows[0]));
    assert!(!shows_unread_dot(&rows[1]));

    // Opening a message shows stored content and sends no request. A
    // message without text shows why in the body's place; its row stays.
    widgets.open_row(1);
    settle(&ui);
    assert_eq!(widgets.reader_page(), "message");
    let content_failure =
        declare_content(&ReceivedContent::StructureUnreadable).expect("no text to show");
    let content_status = widgets.content_status().expect("the reader's status page");
    assert_eq!(content_status.title(), content_failure.title);
    assert!(!widgets.reader_body_label().is_visible());
    assert_eq!(widgets.reader_subject(), "First subject");
    assert!(widgets.reader_body().contains("First sender"));
    assert_eq!(loader.running_loads(), 0);
    widgets.open_row(0);
    settle(&ui);
    assert!(widgets.content_status().is_none());
    assert!(widgets.reader_body_label().is_visible());
    assert_eq!(widgets.reader_body_label().text(), "Second body");

    // An account update that leaves this account alone keeps the rows and
    // the open message.
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.reader_page(), "message");
    // So does selecting the folder on screen again.
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.reader_page(), "message");

    // During a refresh the stored rows stay with the spinner (US2).
    refresh_mailbox.activate(None);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.list_page(), "messages");
    assert!(widgets.shows_load_feedback());

    // A list the server refused to finish replaces the rows and keeps the
    // open message, still listed, selected; the banner stays while that list
    // is on screen, and another folder does not show it.
    let refusal = IncompleteList::ServerRefused {
        reply: "Some messages could not be FETCHed".to_owned(),
        code: None,
    };
    loader.report_stored(&two_messages()[..1], Some(refusal.clone()));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.reader_page(), "message");
    assert_eq!(widgets.selected_row(), Some(0));
    let short_list_title = Some(declare_short_list(&refusal).title.to_owned());
    assert_eq!(widgets.banner_title(), short_list_title);
    widgets.select(&ui, &generic, Some("Projects"));
    settle(&ui);
    assert_eq!(widgets.banner_title(), None);
    assert_eq!(widgets.status_title(), "No mail loaded");
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.banner_title(), short_list_title);
    assert_eq!(widgets.rows().len(), 1);
    widgets.banner().emit_by_name::<()>("button-clicked", &[]);
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert_eq!(dialog.title(), declare_short_list(&refusal).title);
    dialog.force_close();

    // A message the sender never wrapped opens without freezing the window,
    // and ordinary text keeps word wrapping.
    refresh_mailbox.activate(None);
    settle(&ui);
    loader.report_stored(&unwrapped_and_ordinary_messages(), None);
    settle(&ui);
    // The next complete load leaves no banner behind.
    assert_eq!(widgets.banner_title(), None);
    widgets.open_row(0);
    // The content comes from the store with its wrapping chosen; were word
    // wrapping chosen, this wait would lay it out for minutes and time out.
    settle(&ui);
    assert_eq!(
        widgets.reader_body_label().wrap_mode(),
        gtk::pango::WrapMode::Char
    );
    let unwrapped = widgets.lay_out_reader();
    assert!(
        unwrapped < Duration::from_secs(5),
        "opening took {unwrapped:?}"
    );
    widgets.open_row(1);
    settle(&ui);
    assert_eq!(
        widgets.reader_body_label().wrap_mode(),
        gtk::pango::WrapMode::WordChar
    );
    let ordinary = widgets.lay_out_reader();
    assert!(
        ordinary < Duration::from_secs(5),
        "opening took {ordinary:?}"
    );
    // A name from the message in the reader's status page is laid out as
    // fast: that page wraps its description by word.
    widgets.open_row(2);
    let started = Instant::now();
    settle(&ui);
    let content_status = widgets.content_status().expect("the reader's status page");
    content_status.measure(gtk::Orientation::Horizontal, -1);
    content_status.measure(gtk::Orientation::Vertical, 800);
    let named = started.elapsed();
    assert!(named < Duration::from_secs(5), "opening took {named:?}");

    // A failed refresh keeps the stored rows and the open message under the
    // banner that names the failure; its button opens the failure dialog
    // (US3).
    refresh_mailbox.activate(None);
    settle(&ui);
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    let rejected = declare_failure(&rejected_sign_in(), RetriedOperation::RefreshMailbox);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 3);
    assert_eq!(widgets.reader_page(), "message");
    assert_eq!(widgets.banner_title(), Some(rejected.title.to_owned()));
    widgets.banner().emit_by_name::<()>("button-clicked", &[]);
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert_eq!(dialog.title(), rejected.title);
    dialog.force_close();
    // The banner is there again after another account and back, with the same
    // rows (US3).
    widgets.select(&ui, &google, None);
    settle(&ui);
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 3);
    assert_eq!(widgets.banner_title(), Some(rejected.title.to_owned()));

    // A refresh of that mailbox hides the banner while it runs; the rows stay.
    // It keeps loading for its own mailbox when the user selects another
    // account.
    refresh_mailbox.activate(None);
    settle(&ui);
    assert_eq!(widgets.banner_title(), None);
    assert_eq!(widgets.rows().len(), 3);
    widgets.select(&ui, &google, None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert!(widgets.shows_load_feedback());
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    loader.report_stored(&two_messages(), None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert!(widgets.rows().is_empty());
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.banner_title(), None);

    // An account failure covers the mail without discarding it.
    let mut failed_check = imap_and_google_accounts();
    failed_check.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.apply_account_update(&failed_check);
    settle(&ui);
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "Unable to get accounts");
    assert_eq!(
        widgets.status_button("status_retry_check"),
        Some(("Retry Check".to_owned(), "app.retry-accounts".to_owned()))
    );
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 2);

    // An empty mailbox is said so only after a completed load stored it.
    widgets.select(&ui, &generic, Some("Projects"));
    settle(&ui);
    assert_eq!(widgets.status_title(), "No mail loaded");
    refresh_mailbox.activate(None);
    settle(&ui);
    loader.report_stored(&[], None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Mailbox is empty");
    assert_eq!(widgets.banner_title(), None);
    // A list the server refused to finish leaves the notice over it.
    refresh_mailbox.activate(None);
    settle(&ui);
    let refused = IncompleteList::ServerRefused {
        reply: "Listing not available now".to_owned(),
        code: None,
    };
    loader.report_stored(&[], Some(refused.clone()));
    settle(&ui);
    assert_eq!(widgets.status_title(), "Mailbox is empty");
    assert_eq!(
        widgets.banner_title(),
        Some(declare_short_list(&refused).title.to_owned())
    );

    // A new window over the same store shows the same folders, rows and
    // content and starts no load; an account never loaded has no mail (US1,
    // FR-006).
    let (restarted_window, restarted, restarted_loader, restarted_widgets) =
        open_window(Arc::new(Store::at(store_path)));
    restarted.apply_account_update(&accounts_update(&[
        ("synthetic-generic", AccountProvider::ImapSmtp, "Generic"),
        ("synthetic-google", AccountProvider::Google, "Google"),
        (
            "synthetic-microsoft365",
            AccountProvider::Microsoft365,
            "Microsoft",
        ),
    ]));
    settle(&restarted);
    restarted_widgets.select(&restarted, &generic, Some("INBOX"));
    settle(&restarted);
    let restored_rows = restarted_widgets.rows();
    assert_eq!(restored_rows.len(), 2);
    // No refresh ended in this run, so no banner: a short list is not stored
    // (US2).
    assert_eq!(restarted_widgets.banner_title(), None);
    assert!(row_texts(&restored_rows[0]).contains("Second subject"));
    assert!(shows_unread_dot(&restored_rows[0]));
    restarted_widgets.open_row(0);
    settle(&restarted);
    assert_eq!(restarted_widgets.reader_body_label().text(), "Second body");
    restarted_widgets.open_row(1);
    settle(&restarted);
    let restored_status = restarted_widgets
        .content_status()
        .expect("the reader's status page");
    assert_eq!(restored_status.title(), content_failure.title);
    restarted_widgets.select(&restarted, &generic, Some("Projects"));
    settle(&restarted);
    assert_eq!(restarted_widgets.status_title(), "Mailbox is empty");
    restarted_widgets.select(&restarted, &account("synthetic-microsoft365"), None);
    settle(&restarted);
    assert_eq!(restarted_widgets.status_title(), "No mail loaded");
    assert_eq!(restarted_loader.running_loads(), 0);
    restarted_window.destroy();

    // A confirmed exclusion cancels the account's load and hides its mail.
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    refresh_mailbox.activate(None);
    settle(&ui);
    let cancelled_before_exclusion = loader.cancelled_loads.get();
    let mut without_generic = imap_and_google_accounts();
    without_generic.accounts.remove(&generic);
    ui.apply_account_update(&without_generic);
    settle(&ui);
    assert_eq!(loader.cancelled_loads.get(), cancelled_before_exclusion + 1);
    assert!(widgets.rows().is_empty());
    assert_eq!(widgets.list_title(), ("Mailbag".to_owned(), String::new()));

    // A result of the cancelled load changes nothing on screen.
    loader.report(LoadResult::Stored { incomplete: None });
    settle(&ui);
    assert!(widgets.rows().is_empty());
    assert!(!widgets.shows_load_feedback());

    // Without mail accounts the page points to Online Accounts.
    ui.apply_account_update(&AccountUpdate {
        last_check: AccountCheckResult::Complete,
        ..Default::default()
    });
    settle(&ui);
    assert_eq!(
        widgets.status_button("status_online_accounts"),
        Some(("Online Accounts".to_owned(), "app.accounts".to_owned()))
    );
    window.destroy();
}

/// Each folder keeps its own rows and the outcome of its own load; a
/// folder list's outcome belongs to the whole account, and a new list clears
/// a selection it no longer shows (US1, US5, US7).
#[test]
#[ignore = "requires a graphical GTK session"]
fn mailbox_navigation() {
    adw::init().expect("GTK display");
    let store = Arc::new(Store::in_memory());
    let generic = account("synthetic-generic");
    let inbox = folder_of(&generic, "INBOX");
    store_mail(&store, &generic, &two_messages());
    let (window, ui, loader, widgets) = open_window(store.clone());
    let refresh_mailbox = ui.refresh_mailbox_action().clone();
    let refresh_account = ui.refresh_account_action().clone();
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);

    // Refresh Mailbox stores and shows one folder's rows; another folder's
    // rows stay as they were.
    widgets.select(&ui, &generic, Some("Projects"));
    settle(&ui);
    refresh_mailbox.activate(None);
    settle(&ui);
    let mut report = two_messages().remove(0);
    report.identity = "uid:30".to_owned();
    report.fields.subject = Some("Report subject".to_owned());
    loader.report_stored(&[report], None);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 1);
    assert!(row_texts(&widgets.rows()[0]).contains("Report subject"));
    assert_eq!(
        widgets.list_title(),
        ("Projects".to_owned(), "Generic".to_owned())
    );
    widgets.select(&ui, &generic, Some("INBOX"));
    // While the store is read, the rows of the mailbox left are gone.
    assert!(widgets.rows().is_empty());
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert!(row_texts(&widgets.rows()[0]).contains("Second subject"));

    // A folder list without any folder changes nothing shown (FR-001).
    refresh_account.activate(None);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert!(widgets.shows_load_feedback());
    loader.report_folders(&[]);
    settle(&ui);
    assert_eq!(selection_of(&ui), Some(Selection::Mailbox(inbox.clone())));
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.banner_title(), None);
    let sidebar = ui.sidebar().clone();
    assert!(
        sidebar
            .borrow()
            .position_of_row(&generic, Some("Projects"))
            .is_some()
    );

    // A failed folder list leaves the shown rows under a banner whose Retry
    // repeats Refresh Account; the outcome belongs to every folder of the
    // account (FR-010).
    refresh_account.activate(None);
    settle(&ui);
    let not_listed = failure_of(FailureKind::ServerNotResponding(ServerStep::ListFolders));
    loader.report(LoadResult::Failed(not_listed.clone()));
    settle(&ui);
    let not_listed_title = declare_failure(&not_listed, RetriedOperation::RefreshAccount).title;
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.banner_title().as_deref(), Some(not_listed_title));
    widgets.banner().emit_by_name::<()>("button-clicked", &[]);
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert!(dialog_offers(&dialog, "app.refresh-account"));
    dialog.force_close();
    widgets.select(&ui, &generic, Some("Projects"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.banner_title().as_deref(), Some(not_listed_title));

    // A mailbox that did not open leaves its rows under a banner whose Retry
    // repeats Refresh Mailbox; that outcome belongs to this folder only.
    refresh_mailbox.activate(None);
    settle(&ui);
    let not_opened = failure_of(FailureKind::ServerStepFailed(ServerStep::OpenMailbox));
    loader.report(LoadResult::Failed(not_opened.clone()));
    settle(&ui);
    let not_opened_title = declare_failure(&not_opened, RetriedOperation::RefreshMailbox).title;
    assert_eq!(not_opened_title, "Mailbox not opened");
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.banner_title().as_deref(), Some(not_opened_title));
    widgets.banner().emit_by_name::<()>("button-clicked", &[]);
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert!(dialog_offers(&dialog, "app.refresh-mailbox"));
    dialog.force_close();
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.banner_title(), None);

    // A folder gone from a new list is no longer selected, and its stored
    // rows are gone with it (FR-001, FR-010).
    refresh_account.activate(None);
    settle(&ui);
    loader.report_folders(&[folder("Projects", None)]);
    settle(&ui);
    assert_eq!(selection_of(&ui), None);
    assert_eq!(widgets.status_title(), "Select a mailbox");
    assert!(widgets.rows().is_empty());
    assert!(
        sidebar
            .borrow()
            .position_of_row(&generic, Some("INBOX"))
            .is_none()
    );
    assert_eq!(
        store.read_folder_rows(&inbox).expect("the store reads"),
        None
    );

    // Two Gmail labels hold the same messages. A load of one label that
    // changes a message shows the change in the other label on screen, and
    // either load keeps the open message and its selected row (FR-004; 009
    // FR-013).
    let google = account("synthetic-google");
    widgets.select(&ui, &google, None);
    settle(&ui);
    refresh_account.activate(None);
    settle(&ui);
    loader.report_folders(&inbox_and_projects());
    settle(&ui);
    for label in ["INBOX", "Projects"] {
        store_completed_cycle(&store, &folder_of(&google, label), &two_messages(), || {
            false
        })
        .expect("the test store takes the messages");
    }
    let mut read_elsewhere = two_messages();
    read_elsewhere[0].seen = true;
    // The first load marks a message read, which the Inbox on screen shows;
    // the second changes nothing.
    for load_changes in ["a read", "nothing"] {
        widgets.select(&ui, &google, Some("Projects"));
        settle(&ui);
        refresh_mailbox.activate(None);
        settle(&ui);
        widgets.select(&ui, &google, Some("INBOX"));
        settle(&ui);
        widgets.open_row(1);
        settle(&ui);
        loader.report_stored(&read_elsewhere, None);
        settle(&ui);
        assert!(!shows_unread_dot(&widgets.rows()[0]), "{load_changes}");
        assert_eq!(widgets.reader_page(), "message", "{load_changes}");
        assert_eq!(widgets.selected_row(), Some(1), "{load_changes}");
    }
    // A load of the Inbox that removes the open message closes the reader.
    refresh_mailbox.activate(None);
    settle(&ui);
    loader.report_stored(&read_elsewhere[..1], None);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.reader_page(), "unselected");
    assert_eq!(widgets.selected_row(), None);

    // A folder of 100 000 messages is listed whole, and the list scrolls to
    // its end, without stalling the window (009 SC-007). The times are for
    // the reader of the output; the machine decides them.
    let many: Vec<Message> = (0..100_000)
        .map(|number| Message {
            identity: format!("gmail:{number}"),
            fields: DisplayFields {
                subject: Some(format!("Subject {number}")),
                from: Some(format!("Sender {number}")),
                to: None,
            },
            received_unix: Some(1_700_000_000 - number),
            seen: number % 2 == 0,
            content: ReceivedContent::TextNotReturned,
        })
        .collect();
    store_completed_cycle(&store, &folder_of(&google, "Projects"), &many, || false)
        .expect("the test store takes the messages");
    let started = Instant::now();
    widgets.select(&ui, &google, Some("Projects"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 100_000);
    println!("100 000 rows read and listed in {:?}", started.elapsed());
    // The shown rows are the template's, bound to their items.
    wait_until(|| shown_labels(&widgets).contains(&"Sender 1".to_owned()));
    let started = Instant::now();
    widgets
        .messages()
        .activate_action("list.scroll-to-item", Some(&99_999_u32.to_variant()))
        .expect("the list scrolls to an item");
    wait_until(|| shown_labels(&widgets).contains(&"Sender 99999".to_owned()));
    println!("scrolled to the last row in {:?}", started.elapsed());
    window.destroy();
}

/// A cycle's batches reach the window as they are stored: the list grows
/// and keeps the open message, a batch of another label of the account
/// changes a shared message on screen, and the previous refresh's banner
/// stays revealed while the rows are read again (009 FR-013, research §7).
#[test]
#[ignore = "requires a graphical GTK session"]
fn batches_update_the_shown_folder() {
    adw::init().expect("GTK display");
    let store = Arc::new(Store::in_memory());
    let (window, ui, loader, widgets) = open_window(store.clone());
    let refresh_mailbox = ui.refresh_mailbox_action().clone();
    let google = account("synthetic-google");
    let (inbox, projects) = (folder_of(&google, "INBOX"), folder_of(&google, "Projects"));
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    widgets.select(&ui, &google, None);
    settle(&ui);
    ui.refresh_account_action().activate(None);
    settle(&ui);
    loader.report_folders(&inbox_and_projects());
    settle(&ui);
    widgets.select(&ui, &google, Some("INBOX"));
    settle(&ui);

    // A first fill: the newest batch is listed before the load ends.
    refresh_mailbox.activate(None);
    settle(&ui);
    let arrivals = |messages: &[Message]| FolderBatch {
        arrived: messages.to_vec(),
        ..FolderBatch::default()
    };
    let [newer, older] = two_messages().try_into().expect("two messages");
    loader.report_batch(&inbox, &arrivals(std::slice::from_ref(&newer)));
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 1);
    assert!(widgets.shows_load_feedback());
    widgets.open_row(0);
    settle(&ui);
    loader.report_batch(&inbox, &arrivals(std::slice::from_ref(&older)));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.reader_page(), "message");
    assert_eq!(widgets.selected_row(), Some(0));
    assert_eq!(widgets.reader_body_label().text(), "Second body");
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    let rejected = declare_failure(&rejected_sign_in(), RetriedOperation::RefreshMailbox);
    assert_eq!(widgets.banner_title(), Some(rejected.title.to_owned()));

    // A cycle of another label relates the newer message there, read. The
    // Inbox on screen shows it read at once, and its banner, still the
    // latest outcome for it, never hides while the rows are read again.
    widgets.select(&ui, &google, Some("Projects"));
    settle(&ui);
    refresh_mailbox.activate(None);
    settle(&ui);
    widgets.select(&ui, &google, Some("INBOX"));
    settle(&ui);
    widgets.open_row(0);
    settle(&ui);
    assert!(widgets.banner().is_revealed());
    let related = FolderBatch {
        known_arrived: vec![(newer.identity.clone(), true)],
        ..FolderBatch::default()
    };
    loader.report_batch(&projects, &related);
    // Anything that redraws the window during the read, such as an Online
    // Accounts update, keeps the rows and the banner.
    ui.apply_account_update(&imap_and_google_accounts());
    assert!(widgets.banner().is_revealed());
    assert_eq!(widgets.rows().len(), 2);
    while ui.reads_stored_mail() {
        dispatch_pending();
        assert!(widgets.banner().is_revealed());
    }
    assert!(!shows_unread_dot(&widgets.rows()[0]));
    assert_eq!(widgets.reader_page(), "message");
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    assert!(!shows_unread_dot(&widgets.rows()[0]));
    assert_eq!(widgets.rows().len(), 2);
    window.destroy();
}

/// Stored mail that cannot be read shows the failure page, whose Retry reads
/// the folder lists and the shown mailbox again; a refresh then shows its
/// own outcome (007 FR-013, 008 FR-008).
#[test]
#[ignore = "requires a graphical GTK session"]
fn a_store_that_cannot_be_read() {
    adw::init().expect("GTK display");
    let directory = TestDirectory::new();
    // A file stands where the store's directory would be created.
    let blocking_file = directory.0.join("mailbag");
    std::fs::write(&blocking_file, "not a directory").unwrap();
    let store_path = directory.store_path();
    let (window, ui, loader, widgets) = open_window(Arc::new(Store::at(store_path.clone())));
    let generic = account("synthetic-generic");
    let read_again = Some(("Retry".to_owned(), "app.read-stored-mail".to_owned()));

    // Folder lists that cannot be read take the list's place, even before
    // anything is selected.
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert_eq!(widgets.status_button("failure_action"), read_again);
    click(&widgets, "failure_details");
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert!(dialog_offers(&dialog, "app.read-stored-mail"));
    dialog.force_close();

    // A refresh shows its own outcome instead of the failed read.
    widgets.select(&ui, &generic, None);
    settle(&ui);
    assert_eq!(widgets.status_button("failure_action"), read_again);
    ui.refresh_account_action().activate(None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Loading mailbox list");
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    let rejected = declare_failure(&rejected_sign_in(), RetriedOperation::RefreshAccount);
    assert_eq!(widgets.failure_title(), rejected.title);
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Online Accounts".to_owned(), "app.accounts".to_owned()))
    );

    // A mailbox the sidebar still shows cannot be read either. This store
    // never opened, so the folders are given to the sidebar directly.
    ui.sidebar()
        .borrow_mut()
        .show_folders(&generic, inbox_and_projects());
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.status_button("failure_action"), read_again);

    // Once the store can be opened, Retry reads the folder lists and the
    // shown mailbox: its rows come.
    std::fs::remove_file(&blocking_file).unwrap();
    store_mail(&Store::at(store_path), &generic, &two_messages());
    ui.read_stored_mail_action().activate(None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 2);
    window.destroy();

    // A completed Refresh Account reads a mailbox whose read failed again,
    // so it is not shown as never loaded: here a second store becomes usable
    // while the load runs.
    let later = TestDirectory::new();
    let blocking_file = later.0.join("mailbag");
    std::fs::write(&blocking_file, "not a directory").unwrap();
    let (window, ui, loader, widgets) = open_window(Arc::new(Store::at(later.store_path())));
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    ui.sidebar()
        .borrow_mut()
        .show_folders(&generic, inbox_and_projects());
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.status_button("failure_action"), read_again);
    ui.refresh_account_action().activate(None);
    settle(&ui);
    std::fs::remove_file(&blocking_file).unwrap();
    store_mail(&Store::at(later.store_path()), &generic, &two_messages());
    loader.report_folders(&inbox_and_projects());
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    window.destroy();
}

/// A complete Online Accounts answer that no longer lists an account, or
/// lists it with Mail off, deletes that account's stored mail; nothing else
/// deletes it, and a load that ends after the exclusion stores nothing (US4,
/// FR-008).
#[test]
#[ignore = "requires a graphical GTK session"]
fn stored_mail_leaves_with_its_account() {
    adw::init().expect("GTK display");
    let store = Arc::new(Store::in_memory());
    let (generic, google) = (account("synthetic-generic"), account("synthetic-google"));
    for account_id in [&generic, &google] {
        store_mail(&store, account_id, &two_messages());
    }
    let has_stored_mail = |account_id: &AccountId| {
        !store
            .read_folders(account_id)
            .expect("the store reads")
            .is_empty()
    };
    let only_generic =
        || accounts_update(&[("synthetic-generic", AccountProvider::ImapSmtp, "Generic")]);
    let with_generic = |change: fn(&mut AccountDetails)| {
        let mut update = only_generic();
        change(
            update
                .accounts
                .get_mut(&generic)
                .expect("the generic account"),
        );
        update
    };

    // An account missing from the first complete answer after a start loses
    // its mail.
    let (window, ui, _loader, widgets) = open_window(store.clone());
    ui.apply_account_update(&only_generic());
    wait_until(|| !has_stored_mail(&google));
    assert!(has_stored_mail(&generic));

    // A failed read, an answer not yet checked and a missing Mail service
    // delete nothing.
    let mut failed_read = accounts_update(&[]);
    failed_read.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Unavailable));
    ui.apply_account_update(&failed_read);
    ui.apply_account_update(&AccountUpdate::default());
    ui.apply_account_update(&with_generic(|details| {
        details.mail_service_available = false
    }));
    let_deletions_run();
    assert!(has_stored_mail(&generic));

    // Mail turned off deletes the account's mail, and the window forgets what
    // it read: with Mail on again the account has no folders and no mail.
    ui.apply_account_update(&only_generic());
    settle(&ui);
    widgets.select(&ui, &generic, Some("INBOX"));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    let mail_off = with_generic(|details| details.mail_enabled = false);
    ui.apply_account_update(&mail_off);
    wait_until(|| !has_stored_mail(&generic));
    ui.apply_account_update(&only_generic());
    settle(&ui);
    widgets.select(&ui, &generic, None);
    settle(&ui);
    assert!(widgets.rows().is_empty());
    assert_eq!(widgets.status_title(), "No mail loaded");
    window.destroy();
}

/// Runs the window's pending work until `condition` holds; the store's work
/// runs on GIO's thread pool.
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "the store's work did not finish");
        dispatch_pending();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Gives the deletions sent to GIO's thread pool time to run, where the test
/// checks that they deleted nothing.
fn let_deletions_run() {
    let until = Instant::now() + Duration::from_millis(100);
    while Instant::now() < until {
        dispatch_pending();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn click(widgets: &WindowWidgets, button: &str) {
    widgets
        .builder
        .object::<gtk::Button>(button)
        .expect("button")
        .emit_clicked();
    dispatch_pending();
}

#[test]
fn long_text_is_cut_at_a_character_boundary_without_an_explanation() {
    // Multi-byte characters must not be cut in half at the boundary.
    let long_text = "я".repeat(40_000);
    let shown = inert_text(&long_text);
    assert!(shown.len() <= DISPLAY_LIMIT_BYTES, "{}", shown.len());
    assert!(shown.len() > DISPLAY_LIMIT_BYTES - 4, "{}", shown.len());
    assert!(long_text.starts_with(&shown));
    // Nothing marks the cut.
    assert!(!shown.contains('\u{FFFD}') && !shown.contains('…'));
    for length in [65_535, 65_536, 65_537] {
        let text = "a".repeat(length);
        assert_eq!(inert_text(&text).len(), length.min(DISPLAY_LIMIT_BYTES));
    }
}

#[test]
fn a_nul_byte_never_reaches_a_gtk_label() {
    assert_eq!(inert_text("before\0after"), "before\u{FFFD}after");
}

#[test]
fn a_description_keeps_its_words_and_cuts_only_a_run_too_long_to_wrap_by_word() {
    let sentence = format!("Unknown character set: {}", "x".repeat(65_536));
    let described = cut_unbroken_runs(&sentence);
    assert!(described.starts_with("Unknown character set: x"));
    assert_eq!(longest_unbroken_run(&described), LONGEST_WORD_WRAPPED_RUN);
}

#[test]
fn text_without_a_place_to_break_a_line_is_recognized() {
    assert_eq!(longest_unbroken_run("Обычный текст в несколько слов."), 9);
    assert_eq!(longest_unbroken_run("line one\nline two"), 4);
    assert_eq!(longest_unbroken_run(""), 0);
    assert!(longest_unbroken_run(&"я".repeat(32_768)) > LONGEST_WORD_WRAPPED_RUN);
    // A long URL stays inside the word-wrapped range.
    let link = format!(
        "See https://example.invalid/{} for details",
        "path/".repeat(10)
    );
    assert!(
        longest_unbroken_run(&link) <= LONGEST_WORD_WRAPPED_RUN,
        "{link}"
    );
}

fn listed_row(identity: &str, seen: bool) -> MessageListRow {
    MessageListRow {
        identity: identity.to_owned(),
        fields: DisplayFields {
            subject: Some(format!("Subject of {identity}")),
            from: None,
            to: None,
        },
        received_unix: None,
        seen,
    }
}

/// Each change of a list's items as `(position, removed, added)`.
type ItemChanges = Rc<RefCell<Vec<(u32, u32, u32)>>>;

/// A list shown from `identities`, all unread, and every later change of
/// its items.
fn listed_items(identities: &[&str]) -> (gio::ListStore, ItemChanges) {
    let items = gio::ListStore::new::<MessageItem>();
    let rows: Vec<MessageListRow> = identities
        .iter()
        .map(|identity| listed_row(identity, false))
        .collect();
    update_list_by_difference(&items, &rows);
    let changes = Rc::new(RefCell::new(Vec::new()));
    let recorded = changes.clone();
    items.connect_items_changed(move |_, position, removed, added| {
        recorded.borrow_mut().push((position, removed, added));
    });
    (items, changes)
}

fn item_list(items: &gio::ListStore) -> Vec<MessageItem> {
    (0..items.n_items())
        .map(|position| items.item(position).and_downcast().expect("a message item"))
        .collect()
}

fn identities(items: &gio::ListStore) -> Vec<String> {
    item_list(items)
        .iter()
        .map(|item| item.listed().identity.clone())
        .collect()
}

#[test]
fn rows_arriving_at_the_end_are_appended_without_touching_the_others() {
    let (items, changes) = listed_items(&["a", "b"]);
    let before = item_list(&items);
    let rows = ["a", "b", "c", "d"].map(|identity| listed_row(identity, false));
    update_list_by_difference(&items, &rows);
    assert_eq!(identities(&items), ["a", "b", "c", "d"]);
    assert_eq!(*changes.borrow(), [(2, 0, 2)]);
    assert_eq!(item_list(&items)[..2], before[..]);
}

#[test]
fn a_row_arriving_at_the_top_is_inserted_there() {
    let (items, changes) = listed_items(&["b", "c"]);
    let rows = ["a", "b", "c"].map(|identity| listed_row(identity, false));
    update_list_by_difference(&items, &rows);
    assert_eq!(identities(&items), ["a", "b", "c"]);
    assert_eq!(*changes.borrow(), [(0, 0, 1)]);
}

#[test]
fn a_row_removed_in_the_middle_leaves_its_neighbours_in_place() {
    let (items, changes) = listed_items(&["a", "b", "c"]);
    let before = item_list(&items);
    let rows = ["a", "c"].map(|identity| listed_row(identity, false));
    update_list_by_difference(&items, &rows);
    assert_eq!(identities(&items), ["a", "c"]);
    assert_eq!(*changes.borrow(), [(1, 1, 0)]);
    assert_eq!(item_list(&items), [before[0].clone(), before[2].clone()]);
}

#[test]
fn a_changed_read_state_changes_its_item_in_place() {
    let (items, changes) = listed_items(&["a", "b"]);
    let before = item_list(&items);
    let rows = [listed_row("a", false), listed_row("b", true)];
    update_list_by_difference(&items, &rows);
    assert!(changes.borrow().is_empty());
    assert_eq!(item_list(&items), before);
    assert!(before[0].unread());
    assert!(!before[1].unread());
    assert_eq!(before[1].read_state_text(), "Read");
}

#[test]
fn a_message_listed_again_between_changes_keeps_its_item() {
    let (items, _) = listed_items(&["a", "b", "c"]);
    let kept = item_list(&items)[1].clone();
    // "a" and "c" leave, "b" stays between them, "d" arrives.
    let rows = ["b", "d"].map(|identity| listed_row(identity, false));
    update_list_by_difference(&items, &rows);
    assert_eq!(identities(&items), ["b", "d"]);
    assert_eq!(item_list(&items)[0], kept);
}

#[test]
fn a_message_whose_fields_changed_gets_a_new_item() {
    let (items, _) = listed_items(&["a"]);
    let before = item_list(&items);
    let mut renamed = listed_row("a", false);
    renamed.fields.subject = Some("Edited".to_owned());
    update_list_by_difference(&items, &[renamed]);
    assert_ne!(item_list(&items), before);
    assert_eq!(item_list(&items)[0].subject(), "Edited");
}

#[test]
fn the_same_rows_change_nothing() {
    let (items, changes) = listed_items(&["a", "b", "c"]);
    let before = item_list(&items);
    let rows = ["a", "b", "c"].map(|identity| listed_row(identity, false));
    update_list_by_difference(&items, &rows);
    assert!(changes.borrow().is_empty());
    assert_eq!(item_list(&items), before);
}

/// Stores `messages` as the folder's whole content, as a completed cycle
/// leaves it: stored messages not among them leave.
fn store_completed_cycle(
    store: &Store,
    folder: &FolderRef,
    messages: &[Message],
    load_cancelled: impl FnOnce() -> bool,
) -> Result<StoreWrite, Failure> {
    let removed = store
        .read_folder_rows(folder)?
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.identity)
        .filter(|identity| !messages.iter().any(|message| message.identity == *identity))
        .collect();
    let batch = FolderBatch {
        removed,
        arrived: messages.to_vec(),
        state: Some(FolderState {
            server_position: None,
            fill_place: None,
            synchronized: true,
        }),
        ..FolderBatch::default()
    };
    store.store_batch(folder, &batch, load_cancelled)
}
