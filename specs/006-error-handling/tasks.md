# Tasks: Error Handling

**Feature**: `006-error-handling`
**Created**: 2026-09-25 · **Branch**: `claude/errors` · **Status**: Documents
approved 2026-09-25; portions 1–4 implemented, the simplify review applied
(option A) and acceptance done live 2026-09-25 (plan.md,
"Post-implementation"). Portion 5, where the wording lives, planned
2026-09-26 on `claude/failure-ownership`. Portion 6, failures as domain
values, planned 2026-09-26 on `claude/storage`, before 007's portions.

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table, the
function map and the portions, [research](research.md) owns the decisions
with alternatives, the [contract](contracts/failure-declaration.md) owns
the declaration's shape, [quickstart](quickstart.md) owns the manual
acceptance. Follow [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses):
implement one portion, run its checks, compare the size with the plan's
table, report and stop. The maintainer creates commits and PRs. Do not
start code before document approval. Nothing committed may mention where
an idea came from outside this repository.

Phases follow the plan's portions. Story labels trace tasks to US1 (a
failed load with nothing to show), US3 (an incomplete list stays visibly
incomplete), US4 (a person reports a problem with its technical details)
and US5 (one message's problem stays with that message). US2 (a failed
refresh while messages are on screen) is deferred with FR-013 and has no
tasks. Tests are part of every portion and live beside their modules; no
test pins wording except the privacy invariants and the general arms.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(errors): specify and plan error handling | Error handling |
| 1. `<login>` at the source | T002–T004 | fix(imap): replace the sign-in name where a failure is built | Error handling |
| 2. The declarations | T005–T007 | feat(providers): declare every failure once | Error handling |
| 3. Channels, forms and the failure dialog | T008–T016 | feat: show every failure through its channel and the failure dialog | Error handling |
| 4. Panic on the worker | T017–T019 | feat(providers): turn a panic on the mail worker into a failed load | Error handling |
| Polish | T020–T022 | (per review) | Error handling |
| 5. Where the wording lives | T023–T027 | refactor(errors): write failure wording in the application | Failure wording in the application |
| 6. Failures as domain values | T028–T032 | refactor(errors): hand failures to the application as domain values | Mail storage |

## Phase 1: documents and review

- [X] T001 STOP: present specs/006-error-handling/ (spec.md, plan.md, research.md, contracts/failure-declaration.md, quickstart.md, checklists/requirements.md, this tasks.md) together with the AGENTS.md changes ("UI wording" and the forms-only rule), and wait for explicit maintainer approval before any code change.

## Phase 2: `<login>` at the source (portion 1)

**Purpose:** the server's words carry `<login>` in place of the account's
sign-in name from the moment an IMAP failure is built, so every channel and
the record show the same text (spec FR-005; research §3). Serves US1 and
US4. Plan row: `<login>` at the source.
**Independent check:** the IMAP tests with the scripted server whose
refusal repeats the sign-in name.

- [X] T002 Amend specs/003-logging/spec.md FR-011: the sign-in name is replaced with `<login>` once, where the IMAP failure is built, in the server's reply, its alerts and the refusal of a short list; the record, the page and the failure dialog show that one text; delete "the reply itself travels on to the UI unchanged". Amend specs/003-logging/research.md §6 accordingly (no separate copy for the record; the one copy is the masked one). Add a line to the 003 spec's Assumptions or Clarifications naming 006 as the amending feature and the date.
- [X] T003 [US1] [US4] In crates/mailbag-imap/src/session.rs rename `server_text_for_log` to `replace_sign_in_name` (same body; the doc comment says it now serves the error and the record alike) and update its callers: the debug lines in `ServerNotices::keep` (alert), `offer_readable_names` and `identify_client` keep calling it on the raw text they log; in `ServerNotices::error` replace the name once in the reply that will enter `ImapError` (the step's reply or the BYE taken from `self.bye`) and in every alert moved out of `self.alerts`, and log `reply.text` as it then is. In crates/mailbag-imap/src/reader.rs, in `fetch_rows`, replace the name in the refusal of the `Ok(MessageList { refusal })` branch only; the failing branches pass their reply to `self.error`, which replaces it there, so no text is replaced twice (a sign-in name such as `in` would otherwise turn `<login>` into `<log<login>>`). Update the import and the call in crates/mailbag-imap/src/tests/record.rs (line 41 calls the function by name). Tests, beside the two-character sign-in name fixture in crates/mailbag-imap/src/tests/record.rs: an `ImapError` from a rejected sign-in holds `<login>` in `server_reply.text` and in each alert, never the name; a `MessageList.refusal` from a refused FETCH holds `<login>`; a sign-in name of `in` gives exactly one `<login>` per occurrence; the existing record checks still pass.
- [X] T004 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare the portion's size with plan.md's table (~6 lines, 2 tests); review constitution I/II; report what changed, the evidence and limitations, suggest the commit, and wait before portion 2.

## Phase 3: the declarations (portion 2)

**Purpose:** every failure a channel can show is declared once, in the
provider layer, as a `DeclaredFailure` (spec FR-001–FR-005, FR-009,
FR-012; contract). The window is not changed in this portion; its wording
and the declarations coexist for one commit. Serves US1, US3, US4, US5.
Plan rows: the declaration.
**Independent check:** the declaration tests in providers.

- [X] T005 [US1] [US3] [US4] [US5] Create crates/mailbag-providers/src/failure.rs with `DeclaredFailure`, `FailureAction` and `RemoteText` exactly as contracts/failure-declaration.md defines them (derive Debug, Clone, PartialEq, Eq), and export the three from crates/mailbag-providers/src/lib.rs. Move the naming of the record's error line into providers: `LoadFailure::cause_name(&self) -> String` (`Failed(SignIn)`, `TimedOut(Connect)`, `Refused`, `WorkerStopped`, `Timeout`… exactly as crates/mailbag/src/inbox.rs `log_load_failure` names them today), `status(&self) -> Option<u32>`, `server_code(&self) -> Option<&str>`; make `log_load_failure` call the three and delete its own match and `CauseName`. Implement `impl LoadFailure { pub fn declare(&self) -> DeclaredFailure }`, `impl IncompleteList { pub fn declare(&self) -> DeclaredFailure }` and `impl ReceivedContent { pub fn declare(&self) -> Option<DeclaredFailure> }` as the plan's function map lists them: `declare_access_failure` (one arm per variant, `Cancelled` included with an ordinary `Retry` declaration, never a panic), `declare_imap_failure` (with `failed_step_title`, `failed_step_explanation`, `waiting_step_explanation` and `credential_may_be_wrong` moved from crates/mailbag/src/window_ui.rs), `declare_graph_failure` (401 → a rejected sign-in with `OnlineAccounts`; any other status → the general "Request failed" with `Retry`; `ConnectionFailed`, `TimedOut`, `InvalidReply` → `Retry`), `declare_worker_stopped` (title "Refresh stopped", explanation "Loading this Inbox stopped because of an internal error.", advice to try again and report with the details, `Retry`, technical details `Failure: WorkerStopped`; no payload yet). Wording rules: titles of a few words, fit for one banner line (for example "Sign-in rejected", "Server unreachable", "Certificate not trusted", "No sign-in method", "Server not responding"); explanations in plain impersonal words with no code, status, protocol term or server text; advice only when the action alone does not say what to do; remote texts with the four fixed sources ("Alert from the mail server" first, then "Reply from the mail server", "Message from the mail service", "From the system"); technical details as `Failure: <cause_name>`, `Server code: …`, `Status: …`, `Service code: …`, `Rows received: …` when known. Content explanations move from crates/mailbag/src/mail_ui.rs (`explain_content`, `reader_body_text`), reworded impersonally; `TextNotReturned` gets `Retry` and no advice; the others no action. Do not delete the window's copies yet.
- [X] T006 [US1] [US3] [US4] [US5] Add crates/mailbag-providers/src/failure/tests.rs (or a `tests` module in failure.rs) with about 11 cases that assert action, advice presence, remote-text sources and technical-detail labels, never sentences: a timeout at sign-in → `Retry`, no advice, a `Failure:` line equal to `cause_name`; a rejected sign-in with `AUTHENTICATIONFAILED` and an alert → `OnlineAccounts`, advice present, remote texts in the order alert then reply, `Server code: AUTHENTICATIONFAILED`; `UNAVAILABLE` → `Retry`; an IMAP code the code does not tell apart (`LIMIT`) → the general step arm with `Retry` and the code in the details; a failed secure connection and `NoSignInMethod` → no action; Graph 401 → `OnlineAccounts`; Graph 500 → `Retry`, `Status: 500`, the service's message as "Message from the mail service"; `ConnectionFailed` with a reason → `Retry` and "From the system"; `AccessError::NoEncryption` → `OnlineAccounts`; `IncompleteList::ServerRefused` → `Retry` with the reply as a remote text, `MoreAvailable` → no action, no details; `ReceivedContent::Text` → `None`, `Encrypted` → no action, `TextNotReturned` → `Retry`; `log_load_failure`'s line still names `cause`, `status` and `code` as before (the existing inbox tests in crates/mailbag/src/inbox/tests.rs stay green). Privacy: an `ImapError` built by hand whose reply and alert already carry `<login>` (as portion 1 guarantees) declares remote texts with `<login>` and no explanation that contains the reply's text, a bare status number or the word "said".
- [X] T007 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare with plan.md's table (~190 lines, of which ~110 moved, 11 cases); confirm the window's behaviour is unchanged; review constitution I/II; report, suggest the commit, and wait before portion 3.

## Phase 4: channels, forms and the failure dialog (portion 3)

**Purpose:** the window shows every failure through its channel from the
declaration alone, with the widgets declared in the forms: the list's
status page, the banner, the reader's status page, the toast, and the
failure dialog behind every Details and banner button (spec FR-005–FR-011;
Assumptions, the four layout changes). Serves US1, US3, US4, US5. Plan
rows: the channels; the failure dialog.
**Independent check:** the window tests through the scripted loader and
the dialog tests; the quickstart's manual steps after portion 4.

- [X] T008 Amend specs/002-imap-integration/contracts/ui.md: mark the whole failure part as superseded by specs/006-error-handling with the date: the "Failure wording" table (the wording lives in the declarations), the toast for an incomplete list and the paragraph that accepts its disappearance (the banner replaces it), the ALERT rule (an alert is the first block of the failure dialog, not part of the explanation), the sentence that shows server text "with the step … beside any ALERT texts" (server text belongs to the dialog), the reader's explanation in place of the text (a status page in the body's place), and the paragraph that keeps a separate plain-text label because the description is markup (the escaped description is used). Amend specs/002-imap-integration/data-model.md (the `LoadFailure` row: server text is for the failure dialog and, with the sign-in name replaced, for the record) and contracts/imap-reading.md ("inert server text for the failure explanation only" → for the failure dialog). Amend specs/005-microsoft-graph-integration/research.md §5: the service's message may appear in the failure dialog as a remote text ("Message from the mail service"), never in the explanation. Date and name 006 in each.
- [X] T009 [P] [US1] [US3] [US4] [US5] Copy the layout changes into crates/mailbag/resources/ui/: in mailbag.ui add the `AdwBanner id="list_banner"` (revealed false, use-markup false) between `search_bar` and `list_stack`, and give `account_status` a `child` `GtkBox id="status_buttons"` (horizontal, spacing 12, halign center) with `GtkButton id="status_action"` (visible false, style classes `pill` and `suggested-action`) and `GtkButton id="status_details"` (label "Details", visible false, style class `pill`); in message-content.ui add, as a child of `message_body_content` after `body_slot`, an `AdwStatusPage id="content_status"` (icon-name `dialog-warning-symbolic`, hexpand, vexpand, visible false) whose child is `GtkButton id="content_action"` (halign center, visible false, `pill` and `suggested-action`); add failure-dialog.ui (`AdwDialog id="failure_dialog"`, content-width 480, an `AdwToolbarView` with a top `AdwHeaderBar` holding `GtkButton id="copy_button"` (icon `edit-copy-symbolic`, tooltip "Copy to Clipboard", and an `<accessibility>` `label` "Copy to Clipboard" for FR-011) at its end, and a `GtkScrolledWindow` (hscrollbar never, propagate-natural-height) around `GtkBox id="dialog_content"` (vertical, spacing 24, margins 24 sides and bottom, 6 top) with `GtkBox id="dialog_message"` (vertical, spacing 12, labels `dialog_explanation` and `dialog_advice`, wrap, xalign 0, use-markup false), `GtkBox id="dialog_blocks"` (vertical, spacing 24) and `GtkButton id="dialog_action"` (halign center, visible false, `pill`, `suggested-action`)); add failure-block.ui (`GtkBox id="block"`, vertical, spacing 6: `GtkLabel id="block_heading"` (xalign 0, style `heading`) and a `GtkBox` with style `card` holding `GtkLabel id="block_text"` (selectable, wrap, xalign 0, hexpand, margins 12, use-markup false)). Every form carries the SPDX header, `domain="mailbag"` and the `requires` lines of the existing forms; `translatable="yes"` on the fixed labels. No other widget changes.
- [X] T010 [P] [US1] [US3] [US4] Create crates/mailbag/src/failure_dialog.rs: `pub fn action_button(action: FailureAction) -> (&'static str, &'static str)` gives the label and the action name (`Retry` → "Retry", `app.refresh-inbox`; `OnlineAccounts` → "Online Accounts", `app.accounts`), the one place that turns an action into a button; `pub fn present(parent: &impl IsA<gtk::Widget>, failure: &DeclaredFailure)` builds `failure-dialog.ui` with `gtk::Builder::from_string(include_str!(…))`, sets the dialog's title from `failure.title`, fills `dialog_explanation` and `dialog_advice` (visible only when non-empty), appends one `failure-block.ui` per `remote_texts` entry (heading = source, text) and one with the heading "Technical details" when `details` is not empty, sets `dialog_action` from `action_button` (label, `set_action_name`, visible only with an action) and closes the dialog on its click, and binds `copy_button` to put `report_text(failure)` on `parent.clipboard()`; `pub fn report_text(failure: &DeclaredFailure) -> String` joins, with blank lines and skipping empties: the title, the explanation, the advice, each remote text as `<source>:\n<text>`, and `Technical details:\n<details>`. A `pub(crate) fn build(failure) -> FailureDialogWidgets` (the dialog and its labels, blocks box and action button) serves the tests; `present` calls it. No toast on copy: the spec asks for none. Long block text wraps as `mail_ui::show_inert_text` wraps.
- [X] T011 [US1] [US3] In crates/mailbag/src/main.rs register `retry-accounts` as an application action beside `accounts` and `refresh-inbox` (the action `AccountUi` creates today, added with `app.add_action`), so that every status page button is driven by an action name. In crates/mailbag/src/window_ui.rs make the window the single writer of the status page and the banner: fetch `status_action`, `status_details` and `list_banner` from the builder; delete `MailStatus`, `SIGN_IN_HINT`, `failure_status`, `online_accounts_status`, `imap_failure_status`, `graph_failure_status`, `credential_may_be_wrong`, `failed_step_title`, `failed_step_reason`, `waiting_step_reason`, `incomplete_list_notice`, the `mail_explanation` label and the toast in `finish_load`. In `render`: an account page (from `AccountUi::page_text` and a new `AccountUi::page_action() -> Option<PageAction>` with `RetryCheck` and `OnlineAccounts`) sets the mail icon, the title, the escaped description, `status_action` (label "Retry Check" with action name `app.retry-accounts`, or "Online Accounts" with `app.accounts`; the "Checking…" label and insensitivity while `AccountUi::retry_pending()` is true) and hides `status_details`; a failed load calls `show_failed_load(&failure.declare())`: icon `dialog-warning-symbolic`, title, description = `glib::markup_escape_text` of the explanation and the advice joined by a blank line, `status_action` from `failure_dialog::action_button` (visible only with an action), `status_details` visible; the states without a failure (nothing loaded, loading, empty Inbox, rows) set the mail icon, keep their titles and descriptions ("No mail loaded" keeps its sentence that points to Refresh Inbox in the main menu, now as the description) and hide both buttons; a received batch with `incomplete` calls `show_short_list(&incomplete.declare())`: `list_banner` title, button label "Details", revealed; every other state hides the banner. `status_details` and the banner's button open `failure_dialog::present` with the declaration of the current state.
- [X] T012 [US1] In crates/mailbag/src/account_ui.rs delete `create_status_buttons`, `StatusButtons`, `status_actions()`, the `retry` and `online_accounts` fields and `show_page_buttons`; hand the `retry-accounts` action to `main.rs` for registration (`connect_retry_check` keeps connecting the observer's refresh to it); add `page_action()` returning the action 001's pages need (`ReadFailed` and `MailUnavailable` → `RetryCheck`; `NoAccounts` and `NoEligibleAccounts` → `OnlineAccounts`; the rest none) and `retry_pending()` forwarding to `AccountList::retry_pending`. `show_toast` stays for hidden-account notices and the Settings failure. The row popover of 001 is still built in code and is the known exception to the forms rule until 001 is next changed (AGENTS.md).
- [X] T013 [P] [US5] In crates/mailbag/src/mail_ui.rs delete `reader_body_text` and `explain_content`; add `body_slot`, `content_status` and `content_action` (from message-content.ui) to `ReaderWidgets`; in `open_message`, `message.content.declare()`: `Some(failure)` hides `body_slot`, sets `content_status`'s title and escaped description (explanation and advice), `content_action` from `failure_dialog::action_button` (visible only with an action), and shows the status page; `None` shows the text in `reader_body` as today; `close_reader` hides the status page and shows `body_slot` again. `show_inert_text` and `inert_text` stay for the text.
- [X] T014 [P] [US1] In crates/mailbag/src/settings.rs reword `LaunchError::message` for `AccessDenied` impersonally: "Access to Settings was denied. Open Online Accounts from Settings."; the `Unavailable` sentence stays. No declaration: the toast shows this one sentence (contract).
- [X] T015 [US1] [US3] [US4] [US5] Tests, about 150 lines: the window tests live in crates/mailbag/src/mail_ui/tests.rs, where `ScriptedLoader` and `mail_ui_transitions` are; rewrite `mail_ui_transitions` (it reads the deleted `mail_explanation` label, old titles and server text in the explanation) and `a_message_without_text_explains_why_in_the_reader` (it calls the deleted `explain_content`); delete the 7 wording tests of crates/mailbag/src/window_ui/tests.rs and replace them with channel tests through the scripted loader: a failed load shows the status page with `dialog-warning-symbolic`, the declared title, a description that contains the declared explanation and advice, `status_action` with the label and action name of `action_button` (and hidden for a failure without an action), and `status_details`; the failure page is there again after switching to another account and back (US1 scenario 4); a short list shows the rows and the revealed banner with the declared title; the next complete load hides the banner and restores the mail icon; switching accounts and back shows the same banner and rows; an account page shows the mail icon, its action button with `app.retry-accounts` or `app.accounts` and no Details; `failure_dialog::build` from a declaration with an alert, a reply and details yields the labels, three blocks in order and the action button, and `report_text` holds them in order with `<login>` and none of the fixture's private markers; a content problem shows `content_status` with the declared title and hides `body_slot`, a text shows the body.
- [X] T016 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare with plan.md's table (~230 lines plus the forms, 9 cases); confirm that this feature's changes build no widget in code (the 001 row popover is the known exception) and that the four layout changes are the only form changes; review constitution I/II; report, suggest the commit, and wait before portion 4. The look on the running application is checked by the maintainer in T021.
- [X] T016a Layout change approved by the maintainer on 2026-09-25: a failed load gets its own page in `list_stack` of crates/mailbag/resources/ui/mailbag.ui, `AdwStatusPage id="failure_status"` (icon-name `dialog-warning-symbolic`) whose child `GtkBox id="failure_buttons"` (horizontal, spacing 12, halign center) holds `failure_action` (visible false, `pill`, `suggested-action`) and `failure_details` (label "Details", `pill`); `status_details` and the `status_buttons` box leave `account_status`, which keeps the mail icon and has `status_action` (halign center) as its child, for 001's pages, as `content_action` is the reader page's child. crates/mailbag/src/window_ui.rs shows the page `failed` for a failed load and sets no icon; `MAIL_ICON` goes. The window test checks the visible page instead of the icon name. Spec Assumptions and plan function map amended.
- [X] T016b Simplification accepted by the maintainer on 2026-09-25 (from T020, recorded here with the other form changes): `account_status` holds two buttons declared in mailbag.ui, `status_retry_check` ("Retry Check", `app.retry-accounts`) and `status_online_accounts` ("Online Accounts", `app.accounts`), in place of `status_action`; the window only shows one of them and sets the Retry Check progress; the banner's `button-label` "Details" is declared in the form.
- [X] T016c PR review fixes, 2026-09-25: a status page's description cuts every run too long to wrap by word (a 64 KiB character set name froze the reader for minutes); the failure dialog's action handler holds the dialog weakly (each closed dialog stayed in memory); a failed secure connection offers Retry (a cut handshake is the same failure as a refused certificate); the copied report bounds each part as the dialog bounds each block. The Graph service message is not masked: the spec's Assumptions accept that the service receives only a token.
- [X] T016d Names that changed after the tasks were written, recorded 2026-09-25: `action_button` became `show_action_button` (T020); `present` builds from the form without a `build` function or a widgets struct, and the dialog's widgets are checked in `mail_ui_transitions`; no `Rows received` line, since `IncompleteList` carries no count. In the final refactor (after the PR review): `guarded_load` became `load_catching_panics`, `show_content_failure` became `show_body_or_failure`, `render` hides the banner once and reveals it in the received arm, one test owns the technical-detail format, and the general arm for an unreadable service answer got its test.

## Phase 5: panic on the worker (portion 4)

**Purpose:** a panic on the mail worker becomes a failed load whose
technical details carry the panic's message and place, and the worker keeps
serving (spec FR-014, SC-006; research §4). Serves US1 and US4. Plan row:
panic on the worker.
**Independent check:** the worker tests with a load that panics on purpose.

- [X] T017 [US1] [US4] In crates/mailbag-providers/src/batch.rs change `LoadFailure::WorkerStopped` to `WorkerStopped(Option<String>)`, documented as the panic's message and place, `message at file:line`, or `None` when the worker vanished without one; `cause_name` stays `WorkerStopped`; in crates/mailbag-providers/src/worker.rs let `report_outcome` report `WorkerStopped(None)`, add `install_panic_hook()` guarded by a `std::sync::Once`: the hook formats `"{message} at {file}:{line}"` from `PanicHookInfo::payload_as_str().unwrap_or("panic")` and `location()`, stores it in a `thread_local! static LAST_PANIC: RefCell<Option<String>>`, then calls the previous hook; `run_worker` installs it before the loop; split `run_load` so that the load future is wrapped in `futures_util::FutureExt::catch_unwind` over `AssertUnwindSafe` in a `guarded_load` function whose `Err(_)` (payload unread) becomes `LoadResult::Failed(LoadFailure::WorkerStopped(LAST_PANIC.take()))`, with the cancellation `select` kept around it. Add a `#[cfg(test)] LoadKind::PanicsForTest` arm that panics with a fixed message. In crates/mailbag-providers/src/failure.rs let `declare_worker_stopped(Some(panic))` add the technical-details line `Panic: <panic>` after `Failure: WorkerStopped`.
- [X] T018 [US1] [US4] Tests, about 35 lines: in crates/mailbag-providers/src/tests.rs a `PanicsForTest` load through the worker ends as `Failed(WorkerStopped(Some(text)))` where `text` contains the fixed message and `worker.rs:`, and afterwards the worker's queue sender is still open (`!loads.is_closed()`, the check `MailWorker::queue` makes) and a scripted IMAP load on the same worker succeeds; in the failure tests a `WorkerStopped(Some("boom at x.rs:1"))` declares `Retry` and a `Panic:` line. Adapt `a_stopped_worker_ends_the_load_with_a_visible_failure` to `WorkerStopped(None)`.
- [X] T019 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare with plan.md's table (~35 lines, 2 cases) and the whole feature with the table (≈330 new, ≈200 moved, 3 types, ≈320 test lines); review constitution I/II over the whole feature; report, suggest the commit, and wait.

## Phase 6: polish

- [X] T020 Run the simplify review on the branch diff in a fresh session; bring scope-adding findings to the maintainer with the cost of each; apply what is accepted as one more commit if any.
- [X] T021 Run quickstart.md on the installed build with the maintainer (steps 1–3, 5–8; the wrong password and the accounts are the maintainer's); record in plan.md, under a "Post-implementation" section, what was verified live and what only by tests (steps 4 and 9 by tests).
- [X] T022 STOP: final report with the whole-feature size against plan.md's table, the PR description, the list of amended documents (002, 003, 005, AGENTS.md), and the state of every deferred item.

## Dependencies

- Portions run in order: 1 → 2 → 3 → 4 → polish. Each STOP task ends its
  portion; nothing of the next portion starts before the maintainer's
  instruction.
- Document amendments precede the code that needs them: T002 before T003;
  T008 before T009–T015.
- Within portion 3: T009 (forms) and T010 (dialog module) are independent
  of each other; T011 needs both; T012 follows T011 (the status page's
  single writer); T013 and T014 are independent of T011 and of each other;
  T015 last.
- US2 is deferred with FR-013 and has no task.

## Parallel opportunities

- T009 and T010 in portion 3; T013 and T014 beside T011/T012.
- Review pauses (T004, T007, T016, T019, T022) are never parallel with
  anything.

## Implementation strategy

Portion 1 is small and independent, a safe first commit. Portion 2 adds
the declarations without changing behaviour, so its review is about
wording and the general arms. Portion 3 is the visible change and the
layout change; it is reviewed on the running application as well as by
tests. Portion 4 is independent of the window and last. The minimum that
delivers value to the user is portions 1–3; portion 4 can wait for a later
commit without leaving anything half-built.

## Deferred, no tasks

The stale-mail banner over stored messages (spec US2, FR-013a, feature
007); failures of background operations (FR-013b, 016); a notice for a
panic on the main thread; the host and the sign-in method in the technical
lines; a separate declaration for status 429; Online Accounts'
EnsureCredentials; a crash file; a backtrace with the panic.

## Phase 7: where the wording lives (portion 5)

**Purpose:** the wording and the action of every failure are written in
the application, the provider layer keeps the typed value, the technical
details and the protocol facts (research §1; plan, "Correction
2026-09-26"). No user-visible change.
**Independent check:** the moved declaration tests, the new provider tests
and the window's GTK test, each unchanged in what it asserts.

- [X] T023 STOP: present the amended spec (FR-001 and FR-012 without the
  design rule "in the code of the feature that owns it", the Scope line and
  the "Repeating helps" assumption), contract, research §1, §4, §5, plan and
  this phase, and wait for the maintainer's approval before any code change.
- [X] T024 In crates/mailbag-providers/src/failure.rs keep `cause_name`,
  `status`, `server_code`; make `LoadFailure::technical_details` public;
  add `IncompleteList::technical_details` (the refusal's `Server code:`
  line); add `LoadFailure::credentials_rejected` (the rule of
  `credential_may_be_wrong` plus Graph status 401) and
  `LoadFailure::server_temporarily_unavailable` (IMAP `UNAVAILABLE`).
  Remove the declaration types, the headings, the advice constant and
  every `declare*` function, and the re-export of the three types from
  crates/mailbag-providers/src/lib.rs.
- [X] T025 Create crates/mailbag/src/failure_declarations.rs with
  `DeclaredFailure`, `FailureAction` (Retry documented as "runs the failed
  operation again; the window chooses it from the carrier"), `RemoteText`,
  the four headings and the sign-in advice, and
  `declare_load_failure`, `declare_short_list`, `declare_content` with the
  functions moved from providers unchanged in wording, using the provider
  facts instead of reading IMAP codes or the Graph status; register the
  module in crates/mailbag/src/main.rs; switch the calls in
  crates/mailbag/src/window_ui.rs (4), crates/mailbag/src/mail_ui.rs (1)
  and the imports of crates/mailbag/src/failure_dialog.rs; add to
  `show_action_button` that Retry's operation is Refresh Inbox because
  every carrier today is a load.
- [X] T026 Tests: move crates/mailbag-providers/src/failure/tests.rs into
  crates/mailbag/src/failure_declarations/tests.rs, keeping their
  assertions; keep in providers the technical-details test and add one for
  `credentials_rejected` (AUTHENTICATIONFAILED, no code, UNAVAILABLE, Graph
  401 and 500) and `server_temporarily_unavailable`; switch `.declare()`
  calls in crates/mailbag/src/mail_ui/tests.rs and
  crates/mailbag/src/failure_dialog/tests.rs.
- [X] T027 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  on its own (`cargo test -p mailbag <name> -- --ignored --exact`); compare
  the size with plan.md's correction table; run the simplify review on the
  branch diff in a fresh subagent and bring scope-adding findings to the
  maintainer; report and suggest the commit.

## Phase 8: failures as domain values (portion 6)

Plan: "Amendment 2026-09-26: failures as domain values"; the
[contract](contracts/failure-declaration.md) fixes the types and the
kinds. Every wording literal stays as it is; the technical `Failure:` line
and the record's `cause` name the kind (decided 2026-09-26), the other
technical lines and record fields stay as they are.

- [X] T028 Create crates/mailbag-domain (Cargo.toml without workspace
  dependencies, the workspace lints; add it to the workspace members) with
  the contract's `Failure`, `FailureKind` (each variant documented with the
  value it comes from), `ServerStep`, `RemoteText` and `RemoteSource`; move
  `ReceivedContent` with its `Debug` from
  crates/mailbag-providers/src/batch.rs, recreate `IncompleteList` in the
  contract's form (`ServerRefused { reply, code }`, which providers fill from
  `mailbag-imap`'s `ServerReply`), `IncompleteList::technical_details`
  from crates/mailbag-providers/src/failure.rs, `ContentExplanation` with
  `is_by_design` from crates/mailbag-content/src/lib.rs, and the panic hook
  and its slot from crates/mailbag-providers/src/worker.rs into
  `src/panic.rs` (`install_panic_hook`, `take_panic`); make
  mailbag-content depend on it and return its `ContentExplanation`; add to
  scripts/check.sh that mailbag-domain depends on no GTK, GLib or workspace
  crate.
- [X] T029 In crates/mailbag-providers make `LoadFailure` private to the
  crate; in failure.rs add `LoadFailure::into_failure(self) -> Failure` with
  `failure_kind()` (the contract's table, using `credentials_rejected` and
  `server_temporarily_unavailable`), `remote_texts()` (alerts then the
  reply; the service's message after a refusal or the platform's text) and
  `technical_details()` whose `Failure:` line names the kind (`Status:`,
  `Server code:`, `Service code:`, `Panic:` as today); in worker.rs report
  `LoadResult::Failed(Failure)`; add `LoadFailure::give_up(self, account)
  -> LoadResult`, which writes the load's error line through
  `log_load_failure(account, kind, status, code, alerts)` (moved from
  crates/mailbag/src/inbox.rs, the one function that writes a load's error
  line, `cause` naming the kind, its other fields unchanged) and returns
  `Failed(self.into_failure())`, and call it
  from both places that give a load up: the worker, and `start_transfer` in
  lib.rs for an Online Accounts failure, which ends the load on GTK's
  context before the worker is involved; switch imports of the moved types
  in lib.rs, imap_batch.rs and microsoft365.rs.
- [X] T030 In crates/mailbag/src/failure_declarations.rs replace
  `declare_load_failure` and its protocol arms with `declare_failure(&Failure)`,
  one arm per `FailureKind` with the same title, explanation, advice and
  action as today, and `remote_heading(RemoteSource)` for the four headings;
  let `declare_short_list` and `declare_content` take the domain types;
  switch crates/mailbag/src/inbox.rs, window_ui.rs, mail_ui.rs and
  failure_dialog.rs to `Failure`; remove `mailbag-imap` and `mailbag-graph`
  from crates/mailbag/Cargo.toml.
- [X] T031 Tests, about 60 new lines: in providers, the kind of every
  `AccessError`, of `Failed` and `TimedOut` at each step with the codes
  `AUTHENTICATIONFAILED`, none, `UNAVAILABLE` and another, of
  `NoSignInMethod`, `InboxChanged`, Graph 401, 500, a failed connection, a
  timeout and an invalid reply, and of a panic; one error line for a failed
  Online Accounts request and one for a failed transfer; the remote texts in
  order;
  the `Failure:` line naming the kind and the other technical lines as
  today. Rewrite
  crates/mailbag/src/failure_declarations/tests.rs over kinds, keeping its
  assertions; switch the fixtures of crates/mailbag/src/mail_ui/tests.rs,
  failure_dialog/tests.rs and inbox/tests.rs to `Failure`.
- [ ] T032 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  on its own; confirm with `git diff` that no wording literal of
  failure_declarations.rs changed (the `Failure:` line and `cause` change by
  decision); compare the size with the amendment's
  table; report and suggest the commit.
