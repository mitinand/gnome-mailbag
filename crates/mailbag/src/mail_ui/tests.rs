// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::failure_declarations::{declare_content, declare_failure, declare_short_list};
use crate::test_directory::TestDirectory;
use crate::window_ui::WindowUi;
use goa_adapter::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider, AccountUpdate,
    ErrorCause,
};
use mailbag_domain::{
    AccountId, ContentExplanation, Failure, FailureKind, IncompleteList, RemoteSource, RemoteText,
};
use mailbag_providers::{CancelsLoadOnDrop, LoadResult, LoadsInbox, MailProvider};
use mailbag_store::{InboxWrite, Store};
use std::{
    cell::Cell,
    sync::Arc,
    time::{Duration, Instant},
};

/// One load the window started, waiting for the result the test chooses.
struct StartedLoad {
    account_id: AccountId,
    provider: MailProvider,
    report: Box<dyn FnOnce(LoadResult)>,
    /// Set when the window drops the load's step, which cancels it.
    cancelled: Rc<Cell<bool>>,
}

/// Reports the load results the test chooses, so the window is exercised
/// without Online Accounts and without a mail server. A completed load writes
/// its messages into the window's store, as the mail worker does.
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

impl LoadsInbox for ScriptedLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        let cancelled = Rc::new(Cell::new(false));
        self.started_loads.borrow_mut().push(StartedLoad {
            account_id: account_id.clone(),
            provider,
            report,
            cancelled: cancelled.clone(),
        });
        Box::new(CountedStep {
            cancellations: self.cancelled_loads.clone(),
            cancelled,
        })
    }
}

/// The window owns its loader, while the test keeps a handle to the same one.
/// `LoadsInbox` now lives in another crate, so `Rc` itself cannot carry it.
struct SharedLoader(Rc<ScriptedLoader>);

impl LoadsInbox for SharedLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        self.0.start_load(account_id, provider, report)
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

    /// Ends the running load the way the worker would.
    fn report(&self, result: LoadResult) {
        let started = self
            .started_loads
            .borrow_mut()
            .pop()
            .expect("a load is running");
        (started.report)(result);
    }

    /// Ends the running load as a completed one, as the worker does: its
    /// messages become the account's stored Inbox, unless the load was
    /// cancelled before the store took them.
    fn report_stored(&self, messages: &[Message], incomplete: Option<IncompleteList>) {
        let started = self
            .started_loads
            .borrow_mut()
            .pop()
            .expect("a load is running");
        let write = self
            .store
            .replace_inbox(&started.account_id, messages, || started.cancelled.get())
            .expect("the test store takes the load");
        (started.report)(match write {
            InboxWrite::Stored => LoadResult::Stored { incomplete },
            InboxWrite::LoadCancelled => LoadResult::Cancelled,
        });
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
            received_unix: Some(1_700_000_000),
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

/// Runs the window's pending work and waits for its read of the store,
/// which runs on GIO's thread pool.
fn settle(ui: &WindowUi) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        dispatch_pending();
        if !ui.reads_stored_inbox() {
            return;
        }
        assert!(Instant::now() < deadline, "the stored Inbox was not read");
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

    fn rows(&self) -> Vec<gtk::ListBoxRow> {
        let messages: gtk::ListBox = self.builder.object("messages").expect("messages");
        let mut rows = Vec::new();
        while let Some(row) = messages.row_at_index(rows.len() as i32) {
            rows.push(row);
        }
        rows
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

    fn select_account(&self, position: u32) {
        self.builder
            .object::<gtk::ListView>("folder_tree")
            .expect("folder_tree")
            .emit_by_name::<()>("activate", &[&position]);
    }
}

fn description_of(status: &adw::StatusPage) -> String {
    status
        .description()
        .map(|description| description.to_string())
        .unwrap_or_default()
}

fn row_texts(row: &gtk::ListBoxRow) -> String {
    descendants::<gtk::Label>(&row.clone().upcast())
        .into_iter()
        .map(|label| label.text().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn shows_unread_dot(row: &gtk::ListBoxRow) -> bool {
    descendants::<gtk::Image>(&row.clone().upcast())
        .into_iter()
        .filter(|image| image.icon_name().as_deref() == Some("media-record-symbolic"))
        .any(|dot| dot.is_visible())
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
    let refresh = ui.refresh_action().clone();
    settle(&ui);

    // Without a selection the account page decides what the list area shows.
    ui.apply_account_update(&imap_and_google_accounts());
    settle(&ui);
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "Select an account");
    assert_eq!(widgets.status_button("status_retry_check"), None);
    assert_eq!(widgets.status_button("status_online_accounts"), None);
    assert_eq!(widgets.list_title(), ("Mailbag".to_owned(), String::new()));
    assert!(!refresh.is_enabled());

    // Selecting shows that nothing is stored, and loads nothing.
    let generic = account("synthetic-generic");
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(loader.running_loads(), 0);
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert!(
        widgets.status_description().contains("Refresh Inbox"),
        "{}",
        widgets.status_description()
    );
    assert_eq!(
        widgets.list_title(),
        ("Inbox".to_owned(), "Generic".to_owned())
    );
    assert!(refresh.is_enabled());
    assert!(!widgets.shows_load_feedback());

    // A Google account is loadable too, and gets the same hint.
    widgets.select_account(1);
    settle(&ui);
    assert!(refresh.is_enabled());
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert!(widgets.status_description().contains("Refresh Inbox"));
    // Refreshing it asks for the Gmail sequence, not the Generic IMAP one.
    refresh.activate(None);
    settle(&ui);
    assert_eq!(loader.loading_provider(), Some(MailProvider::Gmail));

    // With nothing stored, a failed load takes the list's place with its
    // declaration; the server's words stay in the failure dialog.
    let rejected = declare_failure(&rejected_sign_in());
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert_eq!(widgets.failure_title(), rejected.title);
    // The page reads its description as markup, so the text arrives escaped.
    let description = widgets.failure_description();
    for paragraph in [
        rejected.explanation.as_str(),
        rejected.advice.expect("sign-in advice"),
    ] {
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
    assert!(refresh.is_enabled());
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
    let advice = rejected.advice.expect("sign-in advice").to_owned();
    assert!(shown_texts.contains(&advice), "{shown_texts:?}");
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
    assert!(
        descendants::<gtk::Button>(&dialog.clone().upcast())
            .iter()
            .any(|button| button.is_visible()
                && button.label().as_deref() == Some("Online Accounts"))
    );
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
    refresh.activate(None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Loading Inbox");
    loader.report(LoadResult::Failed(Failure {
        kind: FailureKind::NoSignInMethod,
        remote_texts: Vec::new(),
        details: "Failure: NoSignInMethod".to_owned(),
    }));
    settle(&ui);
    assert_eq!(widgets.status_button("failure_action"), None);
    assert!(widgets.status_button("failure_details").is_some());

    // A refresh loads once, with the spinner and without a second attempt.
    widgets.select_account(0);
    settle(&ui);
    refresh.activate(None);
    refresh.activate(None);
    settle(&ui);
    assert_eq!(loader.running_loads(), 1);
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    assert_eq!(loader.loading_provider(), Some(MailProvider::GenericImap));
    assert_eq!(widgets.status_title(), "Loading Inbox");
    assert_eq!(widgets.list_page(), "empty");
    assert!(widgets.shows_load_feedback());
    assert!(!refresh.is_enabled());

    // A completed load's stored Inbox fills the list, newest first.
    loader.report_stored(&two_messages(), None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "messages");
    assert!(!widgets.shows_load_feedback());
    assert!(refresh.is_enabled());
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
    rows[1].emit_by_name::<()>("activate", &[]);
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
    rows[0].emit_by_name::<()>("activate", &[]);
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
    // So does selecting the account on screen again.
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.reader_page(), "message");

    // During a refresh the stored rows stay with the spinner (US2).
    refresh.activate(None);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.list_page(), "messages");
    assert!(widgets.shows_load_feedback());

    // A list the server refused to finish replaces the rows and closes the
    // reader; the banner stays while that list is on screen.
    let refusal = IncompleteList::ServerRefused {
        reply: "Some messages could not be FETCHed".to_owned(),
        code: None,
    };
    loader.report_stored(&two_messages()[..1], Some(refusal.clone()));
    settle(&ui);
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.reader_page(), "unselected");
    let short_list_title = Some(declare_short_list(&refusal).title.to_owned());
    assert_eq!(widgets.banner_title(), short_list_title);
    widgets.select_account(1);
    settle(&ui);
    assert_eq!(widgets.banner_title(), None);
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(widgets.banner_title(), short_list_title);
    assert_eq!(widgets.rows().len(), 1);
    widgets.banner().emit_by_name::<()>("button-clicked", &[]);
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert_eq!(dialog.title(), declare_short_list(&refusal).title);
    dialog.force_close();

    // A message the sender never wrapped opens without freezing the window,
    // and ordinary text keeps word wrapping.
    refresh.activate(None);
    settle(&ui);
    loader.report_stored(&unwrapped_and_ordinary_messages(), None);
    settle(&ui);
    // The next complete load leaves no banner behind.
    assert_eq!(widgets.banner_title(), None);
    let long_rows = widgets.rows();
    long_rows[0].emit_by_name::<()>("activate", &[]);
    // The wrapping is checked before the layout runs, because word wrapping
    // would take minutes here instead of failing.
    assert_eq!(
        widgets.reader_body_label().wrap_mode(),
        gtk::pango::WrapMode::Char
    );
    let unwrapped = widgets.lay_out_reader();
    assert!(
        unwrapped < Duration::from_secs(5),
        "opening took {unwrapped:?}"
    );
    long_rows[1].emit_by_name::<()>("activate", &[]);
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
    long_rows[2].emit_by_name::<()>("activate", &[]);
    let started = Instant::now();
    dispatch_pending();
    let content_status = widgets.content_status().expect("the reader's status page");
    content_status.measure(gtk::Orientation::Horizontal, -1);
    content_status.measure(gtk::Orientation::Vertical, 800);
    let named = started.elapsed();
    assert!(named < Duration::from_secs(5), "opening took {named:?}");

    // A failed refresh keeps the stored rows and the open message under the
    // banner that names the failure; its button opens the failure dialog
    // (US3).
    refresh.activate(None);
    settle(&ui);
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
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
    widgets.select_account(1);
    settle(&ui);
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 3);
    assert_eq!(widgets.banner_title(), Some(rejected.title.to_owned()));

    // A repeated refresh that switches accounts keeps loading for its own one.
    refresh.activate(None);
    settle(&ui);
    widgets.select_account(1);
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert!(widgets.shows_load_feedback());
    // The load continues for the account it was started for.
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    loader.report_stored(&two_messages(), None);
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert!(widgets.rows().is_empty());
    widgets.select_account(0);
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

    // An empty Inbox is said so only after a completed load stored it.
    widgets.select_account(1);
    settle(&ui);
    refresh.activate(None);
    settle(&ui);
    loader.report_stored(&[], None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Inbox is empty");
    assert_eq!(widgets.banner_title(), None);
    // A service that offered more than it sent leaves the notice over it.
    refresh.activate(None);
    settle(&ui);
    loader.report_stored(&[], Some(IncompleteList::MoreAvailable));
    settle(&ui);
    assert_eq!(widgets.status_title(), "Inbox is empty");
    assert_eq!(
        widgets.banner_title(),
        Some(
            declare_short_list(&IncompleteList::MoreAvailable)
                .title
                .to_owned()
        )
    );

    // A new window over the same store shows the same rows and content and
    // starts no load; an account never loaded has no mail (US1, FR-006).
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
    restarted_widgets.select_account(0);
    settle(&restarted);
    let restored_rows = restarted_widgets.rows();
    assert_eq!(restored_rows.len(), 2);
    // No refresh ended in this run, so no banner: a short list is not stored
    // (US2).
    assert_eq!(restarted_widgets.banner_title(), None);
    assert!(row_texts(&restored_rows[0]).contains("Second subject"));
    assert!(shows_unread_dot(&restored_rows[0]));
    restored_rows[0].emit_by_name::<()>("activate", &[]);
    settle(&restarted);
    assert_eq!(restarted_widgets.reader_body_label().text(), "Second body");
    restored_rows[1].emit_by_name::<()>("activate", &[]);
    settle(&restarted);
    let restored_status = restarted_widgets
        .content_status()
        .expect("the reader's status page");
    assert_eq!(restored_status.title(), content_failure.title);
    restarted_widgets.select_account(1);
    settle(&restarted);
    assert_eq!(restarted_widgets.status_title(), "Inbox is empty");
    restarted_widgets.select_account(2);
    settle(&restarted);
    assert_eq!(restarted_widgets.status_title(), "No mail loaded");
    assert_eq!(restarted_loader.running_loads(), 0);
    restarted_window.destroy();

    // A confirmed exclusion cancels the account's load and hides its mail.
    widgets.select_account(0);
    settle(&ui);
    refresh.activate(None);
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

/// A stored Inbox that cannot be read shows the failure page, whose Retry
/// reads it again; a refresh then shows its own outcome (US5, FR-013).
#[test]
#[ignore = "requires a graphical GTK session"]
fn a_store_that_cannot_be_read() {
    adw::init().expect("GTK display");
    let directory = TestDirectory::new();
    // A file stands where the store's directory would be created.
    let blocking_file = directory.0.join("mailbag");
    std::fs::write(&blocking_file, "not a directory").unwrap();
    let (window, ui, loader, widgets) =
        open_window(Arc::new(Store::at(blocking_file.join("mail.sqlite"))));
    ui.apply_account_update(&imap_and_google_accounts());
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(widgets.list_page(), "failed");
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Retry".to_owned(), "app.read-stored-inbox".to_owned()))
    );
    click(&widgets, "failure_details");
    let dialog = window.visible_dialog().expect("the failure dialog");
    assert!(
        descendants::<gtk::Button>(&dialog.clone().upcast())
            .iter()
            .any(|button| button.is_visible()
                && button.action_name().as_deref() == Some("app.read-stored-inbox"))
    );
    dialog.force_close();

    // A refresh shows its own outcome instead of the failed read.
    ui.refresh_action().activate(None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "Loading Inbox");
    loader.report(LoadResult::Failed(rejected_sign_in()));
    settle(&ui);
    assert_eq!(
        widgets.failure_title(),
        declare_failure(&rejected_sign_in()).title
    );
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Online Accounts".to_owned(), "app.accounts".to_owned()))
    );

    // Retry reads the stored Inbox again: once the store can be opened, the
    // account shows that nothing is stored.
    widgets.select_account(1);
    settle(&ui);
    assert_eq!(
        widgets.status_button("failure_action"),
        Some(("Retry".to_owned(), "app.read-stored-inbox".to_owned()))
    );
    std::fs::remove_file(&blocking_file).unwrap();
    ui.read_stored_inbox_action().activate(None);
    settle(&ui);
    assert_eq!(widgets.status_title(), "No mail loaded");
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
        store
            .replace_inbox(account_id, &two_messages(), || false)
            .unwrap();
    }
    let has_stored_inbox = |account_id: &AccountId| {
        store
            .read_inbox(account_id)
            .expect("the store reads")
            .is_some()
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
    wait_until(|| !has_stored_inbox(&google));
    assert!(has_stored_inbox(&generic));

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
    assert!(has_stored_inbox(&generic));

    // Mail turned off deletes the account's mail, and the window forgets what
    // it read: with Mail on again the account has none.
    widgets.select_account(0);
    settle(&ui);
    assert_eq!(widgets.rows().len(), 2);
    let mail_off = with_generic(|details| details.mail_enabled = false);
    ui.apply_account_update(&mail_off);
    wait_until(|| !has_stored_inbox(&generic));
    ui.apply_account_update(&only_generic());
    widgets.select_account(0);
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
