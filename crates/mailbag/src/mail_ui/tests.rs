// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::window_ui::WindowUi;
use goa_adapter::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountId, AccountProvider,
    AccountUpdate, ErrorCause,
};
use mailbag_imap::{ImapError, ImapFailure, ImapStep, ServerReply};
use mailbag_providers::{
    CancelsLoadOnDrop, IncompleteList, LoadFailure, LoadResult, LoadsInbox, MailProvider,
    MessageIdentity,
};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

/// One load the window started, waiting for the result the test chooses.
struct StartedLoad {
    account_id: AccountId,
    provider: MailProvider,
    report: Box<dyn FnOnce(LoadResult)>,
}

/// Reports the load results the test chooses, so the window is exercised
/// without Online Accounts and without a mail server.
#[derive(Default)]
struct ScriptedLoader {
    started_loads: RefCell<Vec<StartedLoad>>,
    cancelled_loads: Rc<Cell<usize>>,
}

struct CountedStep(Rc<Cell<usize>>);

impl CancelsLoadOnDrop for CountedStep {}

impl Drop for CountedStep {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

impl LoadsInbox for ScriptedLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        self.started_loads.borrow_mut().push(StartedLoad {
            account_id: account_id.clone(),
            provider,
            report,
        });
        Box::new(CountedStep(self.cancelled_loads.clone()))
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
}

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn imap_and_google_accounts() -> AccountUpdate {
    let mut update = AccountUpdate {
        last_check: AccountCheckResult::Complete,
        ..Default::default()
    };
    for (name, provider, label) in [
        ("synthetic-generic", AccountProvider::ImapSmtp, "Generic"),
        ("synthetic-google", AccountProvider::Google, "Google"),
    ] {
        update.accounts.insert(
            account(name),
            AccountDetails {
                provider,
                mail_enabled: true,
                needs_attention: false,
                mail_service_available: true,
                display_name: Some(label.to_owned()),
                email_address: Some(format!("{label}@example.invalid")),
            },
        );
    }
    update
}

fn batch_with_two_messages(account_id: &AccountId) -> ReceivedBatch {
    ReceivedBatch {
        account_id: account_id.clone(),
        uid_validity: Some(7),
        incomplete: None,
        messages: vec![
            ReceivedMessage {
                identity: MessageIdentity::ImapUid(20),
                fields: DisplayFields {
                    subject: Some("Second subject".to_owned()),
                    from: Some("Second sender".to_owned()),
                    to: Some("Recipient".to_owned()),
                },
                internal_date: Some(1_700_000_000),
                seen: false,
                content: ReceivedContent::Text("Second body".to_owned()),
                gmail: None,
            },
            ReceivedMessage {
                identity: MessageIdentity::ImapUid(10),
                fields: DisplayFields {
                    subject: Some("First subject".to_owned()),
                    from: Some("First sender".to_owned()),
                    to: None,
                },
                internal_date: Some(1_699_000_000),
                seen: true,
                content: ReceivedContent::Explained(ContentExplanation::UnreadableStructure),
                gmail: None,
            },
        ],
    }
}

/// One message the sender never wrapped and one of ordinary lines, both at
/// the 64 KiB display boundary.
fn unwrapped_and_ordinary_batch(account_id: &AccountId) -> ReceivedBatch {
    let bodies = [
        ("Never wrapped", "я".repeat(32_768)),
        ("Ordinary lines", "яяяяяяяя ".repeat(3_856)),
    ];
    ReceivedBatch {
        account_id: account_id.clone(),
        uid_validity: Some(7),
        incomplete: None,
        messages: (1..)
            .zip(bodies)
            .map(|(number, (subject, body))| ReceivedMessage {
                identity: MessageIdentity::ImapUid(number * 10),
                fields: DisplayFields {
                    subject: Some(subject.to_owned()),
                    from: Some("Long sender".to_owned()),
                    to: None,
                },
                internal_date: Some(1_700_000_000),
                seen: true,
                content: ReceivedContent::Text(body),
                gmail: None,
            })
            .collect(),
    }
}

fn rejected_sign_in() -> LoadFailure {
    LoadFailure::Imap(ImapError {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: Some("AUTHENTICATIONFAILED".to_owned()),
            text: "Invalid credentials".to_owned(),
        }),
        alerts: vec!["Mailbox quota is nearly full".to_owned()],
    })
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

    fn status_title(&self) -> String {
        self.builder
            .object::<adw::StatusPage>("account_status")
            .expect("account_status")
            .title()
            .to_string()
    }

    /// The window adds its plain-text mail explanation above the account
    /// buttons of the status page.
    fn mail_explanation(&self) -> Option<String> {
        let actions = self
            .builder
            .object::<adw::StatusPage>("account_status")
            .expect("account_status")
            .child()
            .expect("status page actions");
        let label = actions
            .first_child()
            .expect("mail explanation")
            .downcast::<gtk::Label>()
            .expect("mail explanation label");
        label.is_visible().then(|| label.text().to_string())
    }

    /// Toasts the window has shown: everything the toast overlay holds
    /// besides the window content.
    fn toast_texts(&self) -> Vec<String> {
        let overlay: adw::ToastOverlay = self.builder.object("toasts").expect("toasts");
        let content = overlay.child();
        let mut texts = Vec::new();
        let mut child = overlay.first_child();
        while let Some(current) = child {
            if Some(&current) != content.as_ref() {
                texts.extend(
                    descendants::<gtk::Label>(&current)
                        .into_iter()
                        .map(|label| label.text().to_string()),
                );
            }
            child = current.next_sibling();
        }
        texts
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
    let builder = gtk::Builder::from_string(include_str!("../../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").expect("window");
    let loader = Rc::new(ScriptedLoader::default());
    let ui = WindowUi::new(&builder, Box::new(SharedLoader(loader.clone())));
    let widgets = WindowWidgets {
        builder: builder.clone(),
    };
    let refresh = ui.refresh_action().clone();
    window.present();
    dispatch_pending();

    // Without a selection the account page decides what the list area shows.
    ui.apply_account_update(&imap_and_google_accounts());
    dispatch_pending();
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "Select an account");
    assert_eq!(widgets.mail_explanation(), None);
    assert_eq!(widgets.list_title(), ("Mailbag".to_owned(), String::new()));
    assert!(!refresh.is_enabled());

    // Selecting shows that nothing has been loaded, and loads nothing.
    let generic = account("synthetic-generic");
    widgets.select_account(0);
    dispatch_pending();
    assert_eq!(loader.running_loads(), 0);
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert!(
        widgets
            .mail_explanation()
            .is_some_and(|text| text.contains("Refresh Inbox")),
        "{:?}",
        widgets.mail_explanation()
    );
    assert_eq!(
        widgets.list_title(),
        ("Inbox".to_owned(), "Generic".to_owned())
    );
    assert!(refresh.is_enabled());
    assert!(!widgets.shows_load_feedback());

    // A Google account is loadable too, and gets the same hint.
    widgets.select_account(1);
    dispatch_pending();
    assert!(refresh.is_enabled());
    assert_eq!(widgets.status_title(), "No mail loaded");
    assert!(
        widgets
            .mail_explanation()
            .is_some_and(|text| text.contains("Refresh Inbox"))
    );
    // Refreshing it asks for the Gmail sequence, not the Generic IMAP one.
    refresh.activate(None);
    dispatch_pending();
    assert_eq!(loader.loading_provider(), Some(MailProvider::Gmail));
    loader.report(LoadResult::Failed(rejected_sign_in()));
    dispatch_pending();
    widgets.select_account(0);

    // A refresh loads once, with the spinner and without a second attempt.
    refresh.activate(None);
    refresh.activate(None);
    dispatch_pending();
    assert_eq!(loader.running_loads(), 1);
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    assert_eq!(loader.loading_provider(), Some(MailProvider::GenericImap));
    assert_eq!(widgets.status_title(), "Loading Inbox");
    assert_eq!(widgets.list_page(), "empty");
    assert!(widgets.shows_load_feedback());
    assert!(!refresh.is_enabled());

    // A received batch fills the list, newest first.
    loader.report(LoadResult::Received(batch_with_two_messages(&generic)));
    dispatch_pending();
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

    // Opening a message shows received content and sends no request.
    rows[1].emit_by_name::<()>("activate", &[]);
    dispatch_pending();
    assert_eq!(widgets.reader_page(), "message");
    assert!(
        widgets.reader_body().contains("could not be read"),
        "{}",
        widgets.reader_body()
    );
    assert_eq!(widgets.reader_subject(), "First subject");
    assert!(widgets.reader_body().contains("First sender"));
    assert_eq!(loader.running_loads(), 0);

    // An account update that leaves this account's mail alone keeps the rows
    // and the open message.
    ui.apply_account_update(&imap_and_google_accounts());
    dispatch_pending();
    assert_eq!(widgets.rows().len(), 2);
    assert_eq!(widgets.reader_page(), "message");

    // A list the server refused to finish keeps its rows and says why once.
    refresh.activate(None);
    dispatch_pending();
    let mut short_batch = batch_with_two_messages(&generic);
    short_batch.messages.pop();
    short_batch.incomplete = Some(IncompleteList::ServerRefused(ServerReply {
        code: None,
        text: "Some messages could not be FETCHed".to_owned(),
    }));
    loader.report(LoadResult::Received(short_batch));
    dispatch_pending();
    assert_eq!(widgets.rows().len(), 1);
    assert_eq!(widgets.list_page(), "messages");
    let notice = widgets
        .toast_texts()
        .into_iter()
        .find(|text| text.contains("could not be loaded"))
        .expect("a toast naming the refusal");
    assert!(notice.contains("Generic"), "{notice}");
    assert!(
        notice.contains("Some messages could not be FETCHed"),
        "{notice}"
    );

    // A message the sender never wrapped opens without freezing the window,
    // and ordinary text keeps word wrapping.
    refresh.activate(None);
    dispatch_pending();
    loader.report(LoadResult::Received(unwrapped_and_ordinary_batch(&generic)));
    dispatch_pending();
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

    // A refresh clears the list and the reader before loading again.
    refresh.activate(None);
    dispatch_pending();
    assert!(widgets.rows().is_empty());
    assert_eq!(widgets.reader_page(), "unselected");

    // A failed load explains its step, with the server's own text.
    loader.report(LoadResult::Failed(rejected_sign_in()));
    dispatch_pending();
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "The mail server rejected sign-in");
    let explanation = widgets.mail_explanation().expect("failure explanation");
    assert!(explanation.contains("Invalid credentials"), "{explanation}");
    assert!(explanation.contains("Online Accounts"), "{explanation}");
    assert!(
        explanation.contains("quota is nearly full"),
        "{explanation}"
    );
    assert!(refresh.is_enabled());

    // A repeated refresh that switches accounts keeps loading for its own one.
    refresh.activate(None);
    dispatch_pending();
    widgets.select_account(1);
    dispatch_pending();
    assert_eq!(widgets.status_title(), "The mail server rejected sign-in");
    assert!(widgets.shows_load_feedback());
    // The load continues for the account it was started for.
    assert_eq!(loader.loading_account().as_ref(), Some(&generic));
    loader.report(LoadResult::Received(batch_with_two_messages(&generic)));
    dispatch_pending();
    assert_eq!(widgets.list_page(), "empty");
    assert!(widgets.rows().is_empty());
    widgets.select_account(0);
    dispatch_pending();
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 2);

    // An account failure covers the mail without discarding it.
    let mut failed_check = imap_and_google_accounts();
    failed_check.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.apply_account_update(&failed_check);
    dispatch_pending();
    assert_eq!(widgets.list_page(), "empty");
    assert_eq!(widgets.status_title(), "Unable to get accounts");
    assert_eq!(widgets.mail_explanation(), None);
    ui.apply_account_update(&imap_and_google_accounts());
    dispatch_pending();
    assert_eq!(widgets.list_page(), "messages");
    assert_eq!(widgets.rows().len(), 2);

    // A confirmed exclusion discards that account's mail and cancels its load.
    refresh.activate(None);
    dispatch_pending();
    let cancelled_before_exclusion = loader.cancelled_loads.get();
    let mut without_generic = imap_and_google_accounts();
    without_generic.accounts.remove(&generic);
    ui.apply_account_update(&without_generic);
    dispatch_pending();
    assert_eq!(loader.cancelled_loads.get(), cancelled_before_exclusion + 1);
    assert!(widgets.rows().is_empty());
    assert_eq!(widgets.list_title(), ("Mailbag".to_owned(), String::new()));

    // The cancelled load ends without restoring the discarded mail.
    loader.report(LoadResult::Received(batch_with_two_messages(&generic)));
    dispatch_pending();
    assert!(widgets.rows().is_empty());
    assert!(!widgets.shows_load_feedback());
    window.destroy();
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
fn a_message_without_text_explains_why_in_the_reader() {
    let charset = explain_content(&ContentExplanation::UnknownCharset("x-weird".to_owned()));
    assert!(charset.contains("x-weird"), "{charset}");
    let html_only = explain_content(&ContentExplanation::NoPlainText { has_html: true });
    assert!(html_only.contains("HTML"), "{html_only}");
    let not_returned = explain_content(&ContentExplanation::TextNotReturned);
    assert!(not_returned.contains("Refresh Inbox"), "{not_returned}");
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
