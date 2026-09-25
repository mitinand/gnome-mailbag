# Implementation Plan: Error Handling

**Branch**: `claude/errors` | **Feature**: `006-error-handling`
**Date**: 2026-09-24 | **Spec**: [spec.md](spec.md)
**Status**: Implemented on `claude/errors` and accepted live 2026-09-25;
challenged and analyzed on 2026-09-25, findings applied.
The specification's decisions are settled and are not reopened here.

## Size

The budget agreed at sizing on 2026-09-24, and this plan's estimate after
reading the code. Reassess with the maintainer before exceeding about 1.5
times an estimate; at every review pause the size so far is compared with
this table.

| Item | Budget | This plan (estimate; the measured size is under Post-implementation) |
|---|---|---|
| New modules and production lines | ≤ ~350 new, ~200 moved | `mailbag-providers/src/failure.rs` ~190 (of which ~110 are wording moved from the window); `mailbag/src/failure_dialog.rs` ~90; `mailbag-providers/src/worker.rs` +35; `mailbag-imap` +6; `mailbag/src/window_ui.rs` +70 −210; `mail_ui.rs` +30 −45; `account_ui.rs` +20 −40; `settings.rs` a reworded sentence. New ≈ 330, moved ≈ 200, removed ≈ 300 (old wording, widgets built in code). Four forms, ~200 lines of XML, copied from the prototype |
| Call sites or existing files touched | — | 12 Rust files: `mailbag-imap` `session.rs`, `reader.rs`; `mailbag-providers` `lib.rs`, `batch.rs`, `worker.rs`, new `failure.rs`; `mailbag` `window_ui.rs`, `mail_ui.rs`, `account_ui.rs`, `settings.rs`, `main.rs` (one action registered), `inbox.rs` (the record line calls the shared cause accessors), new `failure_dialog.rs`. Forms: `mailbag.ui`, `message-content.ui`, new `failure-dialog.ui`, `failure-block.ui` |
| New threads, timers, queues | 0 | 0 |
| New state, types, error types | 3 | 3: `DeclaredFailure`, `FailureAction`, `RemoteText`. The panic is one string, not a type (plan challenge, decision 2) |
| New fields in existing data | 0 | `LoadFailure::WorkerStopped` gains a payload, `Option<String>`: the panic's message and place; no field on batches, messages or accounts |
| Changes to other features' contracts or documents | 002 UI contract; 003 FR-011 and research §6; 005 research §5 | The same three, written before the portion that needs each; plus the new [failure declaration contract](contracts/failure-declaration.md). 001's status page buttons move from code into the form, behaviour unchanged |
| New dependencies | 0 | 0. `futures-util` is already a dependency with its default `std` feature, which has `catch_unwind` |
| Tests | ≤ ~350 lines | ≈ 320: providers declarations ~115 (11 cases); imap masking ~20 (2 cases); window channels and dialog ~150 (9 cases, replacing the 8 wording tests of `window_ui/tests.rs` and 5 of `mail_ui/tests.rs`); panic capture ~35 (2 cases) |

## Summary

Every failure the user can meet is declared once, in `mailbag-providers`,
as a `DeclaredFailure`: a short title, a plain explanation, advice or none,
one action or none, texts from the remote side with their source, and
technical details. The window reads the declaration and nothing else: a
failed load goes to the list's status page, a short list to the banner, a
message's content problem to a status page in the reader's body, a Settings
launch failure to a toast. Every Details and banner button opens the
failure dialog, built from a form, which shows the declaration in the
spec's order and copies it whole. The sign-in name is replaced with
`<login>` where the IMAP failure is built, so every channel and the record
show the same text. A panic on the mail worker becomes a failed load whose
technical details carry the panic's message and place.

## Minimal version

Everything below is built, in the four portions of the last section. Each
line names its cost.

| Step | What it does | Cost |
|---|---|---|
| `<login>` at the source | `ServerNotices::error` replaces the sign-in name in the reply text and in the alerts before they enter `ImapError`; `fetch_rows` does the same for the refusal that travels with a short list. `server_text_for_log` is the function used, renamed for its new role; the debug lines log the already-masked text. 003 FR-011 and research §6 amended | ~6 lines, 2 tests |
| The declaration | `failure.rs` in providers: `DeclaredFailure`, `FailureAction { Retry, OnlineAccounts }`, `RemoteText { source, text }`; `LoadFailure::declare`, `IncompleteList::declare`, `ReceivedContent::declare` (`None` for text). The wording of `window_ui.rs` (status titles, reasons, sign-in hint) and of `mail_ui.rs` (content explanations) moves here, rewritten impersonally; the "credential may be wrong" rule moves with it. `LoadFailure::cause_name`, `status` and `server_code` give the failure value, status and code exactly as the record's error line names them; they move out of `inbox.rs`, which now calls them, so the record and the declaration have one owner (constitution IV). [Contract](contracts/failure-declaration.md) | ~200 lines (~125 moved), 11 tests |
| The channels | `window_ui.rs`: a failed load shows the list's failure page, declared in `mailbag.ui` with the warning icon, its action button and its Details button, and sets its title, escaped description (explanation and advice) and action (label and action name); a short list reveals the banner with the title; `mail_ui.rs`: a content problem shows the reader's status page in the body's place; `settings.rs`: `LaunchError::message` stays the toast's one line, title and advice, reworded impersonally; no declaration, since the toast shows nothing more. `account_ui.rs` drives 001's page with the same form buttons instead of building them. 002 UI contract and 005 research §5 amended | ~140 lines (+ forms), 6 tests |
| The failure dialog | `failure_dialog.rs`: builds `failure-dialog.ui`, fills the paragraphs, appends one `failure-block.ui` per remote text and one for the technical details, binds the action button to the action's name and closes on it, binds the copy button to the report text (title, paragraphs, blocks, in order) on the clipboard | ~90 lines, 3 tests |
| Panic on the worker | `worker.rs`: a panic hook installed once when the worker thread starts stores one string, the panic's message and its place (`message at file:line`), in a thread-local slot; `load_catching_panics` wraps the load in `catch_unwind`, ignores the payload and turns a caught panic into `LoadFailure::WorkerStopped(Some(panic))`; the worker loop continues. `declare` puts the string into one technical-details line, `Panic: …`. The hook calls the previous hook, so the panic still reaches the error stream | ~35 lines, 2 tests |

Not built: a crash file or a report at the next start; a call to Online
Accounts' EnsureCredentials; the host and the sign-in method in the
technical details (each would need a field in `ImapError`); certificate
errors in the technical details (same); a separate declaration for status
429 and `Retry-After`; a backtrace with the panic; the stale-mail banner
(007); a third action.

## Function map

The entry points and their steps, as the code will read.

**`mailbag-providers/src/failure.rs`**

- `LoadFailure::declare(&self) -> DeclaredFailure`: one arm per source —
  `declare_access_failure(AccessError)`, `declare_imap_failure(&ImapError)`,
  `declare_graph_failure(&GraphError)`,
  `declare_worker_stopped()`; the `Panic:` line comes from `technical_details`.
- `declare_imap_failure`: title and explanation by step and outcome
  (`failed_step_title`, `failed_step_explanation`, `waiting_step_explanation`);
  action: `OnlineAccounts` when the sign-in was rejected with
  `AUTHENTICATIONFAILED` or without a code (the rule moved from the window),
  none for a missing sign-in method, `Retry` for everything else, a failed
  secure connection included: a refused certificate and a handshake cut
  short arrive as the same failure (corrected after the PR review,
  2026-09-25); advice with the `OnlineAccounts` action; remote texts:
  the alerts ("Alert from the mail server") then the reply ("Reply from the
  mail server"); technical details: `Failure`, `Server code`.
- `declare_graph_failure`: by kind and status: 401 is a rejected sign-in with
  `OnlineAccounts`, any other status the general "Request failed" with
  `Retry`; a connection failure, a timeout or an invalid reply with `Retry`;
  remote texts: the service's message ("Message from the mail service") or the
  platform's text ("From the system"); technical details: `Failure`, `Status`,
  `Service code`.
- `declare_access_failure`: one title, explanation and action per
  `AccessError` variant; `Cancelled` never reaches a channel (FR-010) but
  gets an ordinary declaration with `Retry`, never a panic.
- `LoadFailure::cause_name(&self) -> String`, `status(&self) -> Option<u32>`,
  `server_code(&self) -> Option<&str>`: the failure value (`Failed(SignIn)`,
  `Refused`, `WorkerStopped`, `Timeout`), the status and the code exactly as
  `inbox.rs::log_load_failure` names them today; that function calls them
  from now on, and `declare` builds `Failure:`, `Status:` and `Server
  code:`/`Service code:` from the same three.
- General arms, one test each: a service status the code does not tell
  apart (500), an IMAP server code other than the two distinguished
  (AUTHENTICATIONFAILED, UNAVAILABLE), a reply the code cannot read. Every
  `match` over a failure enumeration is exhaustive, so a variant without a
  declaration does not compile.
- `IncompleteList::declare`: `ServerRefused` with `Retry`, the reply as a
  remote text and the code as a technical-details line; `MoreAvailable` with no
  action and no lines.
- `ReceivedContent::declare(&self) -> Option<DeclaredFailure>`: `None` for
  text; one declaration per `ContentExplanation`, `StructureUnreadable` and
  `TextNotReturned` (the last with `Retry`).

**`mailbag-providers/src/worker.rs`**

- `run_worker`: `install_panic_hook()` once, then the loop as today.
- `load_catching_panics`: `catch_unwind(AssertUnwindSafe(load))`; on `Err`,
  `LoadFailure::WorkerStopped(LAST_PANIC.take())`.

**`mailbag/src/window_ui.rs`**

- `render`: as today, but a failed load calls `show_failed_load(&declared)`
  and a received batch with `incomplete` calls `show_short_list(&declared)`;
  every other state shows another page of the list and hides the banner, so
  neither outlives its cause (FR-008); the wording functions are gone.
- `show_failed_load`: the failure page (`failure_status`, its icon in the
  form) with title, description and action button; its Details button is
  always there: a failed load always has technical details.
- `show_short_list`: banner title and button, revealed.
- `open_failure_dialog(&declared)`: from the Details button and the banner.

**`mailbag/src/failure_dialog.rs`**

- `present(parent, &DeclaredFailure)`: builds the form, `show_paragraph` for
  the explanation and the advice, `append_blocks`, `show_action_button`,
  connects the close and the copy handlers, presents.
- `status_description(&DeclaredFailure) -> String`: the escaped explanation
  and advice for the window's and the reader's status pages.
- `report_text(&DeclaredFailure) -> String`: what the copy button copies.
- `show_action_button(&gtk::Button, Option<FailureAction>)`: gives a form's
  action button its label and action name, or hides it (`Retry` → "Retry", `app.refresh-inbox`;
  `OnlineAccounts` → "Online Accounts", `app.accounts`), the one place
  that turns an action into a button; the window, the reader and the
  dialog call it. `main.rs` registers `retry-accounts` as an application
  action too, so every status page button is driven by an action name and
  no button carries a click handler beside one.

**`mailbag/src/mail_ui.rs`**

- `open_message`: `content.declare()`, then `show_body_or_failure`: `Some`
  shows the reader's status page in the body's place, `None` the text.

**`mailbag/src/account_ui.rs`**

- `page_action`: which of the form's two buttons 001's pages need
  (`status_retry_check`, `status_online_accounts`, each with its action
  name in the form); the window shows that one, with the Retry Check
  progress state.

## Optional mechanisms

None is planned. Each would need the situation named beside it.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| The server's host and the sign-in method in the technical details | Issue reports from people with several IMAP accounts do not say which server failed, or which sign-in method the server refused | A field each in `ImapError` (budget line "provider error types unchanged"), ~10 lines each |
| A declaration for status 429 with a wait advice | Background synchronization (016) makes the service's request limit reachable; today one request per refresh | ~8 lines, 1 test |
| Certificate errors in the technical details | Reports of "Certificate not trusted" cannot be told apart: expired, wrong name, private authority | A field in `ImapError`, ~10 lines; the errors are already read for the debug line |
| A backtrace with the panic | Panic reports name a line that is not enough to find the cause | `Backtrace::force_capture` in the hook, ~10 lines; long text in the dialog; symbols in release builds are not checked |
| Asking Online Accounts to re-check credentials after a rejected sign-in | Users expect the account's warning icon at once, not after Online Accounts' own check | One D-Bus call in goa-adapter, ~30 lines |
| A crash file and a report at the next start | Main-thread panics turn out frequent and users cannot reach the error stream | ~50 lines, a file in the cache directory; out of scope by the spec |

## Portions and review pauses

One commit each, with its tests and `scripts/check.sh`; stop after each
for the maintainer's review and compare the size with the table above.

1. **`<login>` at the source.** `mailbag-imap` masks the reply, the alerts
   and the refusal where they enter the error; the debug lines use the
   masked text; 003 FR-011 and research §6 amended first. Tests: the error
   holds `<login>`; the record still does. Suggested commit: "Replace the
   sign-in name where an IMAP failure is built".
2. **The declaration.** `failure.rs`, the three `declare` methods, the
   contract; `WorkerStopped` is declared without a panic payload, which
   portion 4 adds; the window not yet changed (it keeps its wording until
   portion 3, so the two coexist for one commit, which the library crate's
   public items allow without warnings). Tests: action, advice,
   remote texts and technical details per failure; no private marker in any
   declaration built from the 003 fixtures. Suggested commit: "Declare
   every failure once in the provider layer".
3. **The channels, the forms and the dialog.** Forms copied from the
   prototype; `window_ui`, `mail_ui`, `account_ui`, `settings` read the
   declarations; `failure_dialog.rs`; the old wording and the code-built
   widgets removed; 002 UI contract and 005 research §5 amended first.
   Tests: channel per carrier, the dialog's parts and report text, the
   banner's life with the batch. Suggested commit: "Show every failure
   through its channel and the failure dialog".
4. **Panic on the worker.** Hook, `catch_unwind`, the panic string, its
   declaration. Tests: a panicking load ends as `WorkerStopped` with the
   message and place; the next load runs on the same worker. Suggested
   commit: "Turn a panic on the mail worker into a failed load".

After portion 4: the simplify review on the branch diff, then the
quickstart's manual checks on the installed build.

## Technical Context

**Language/Version**: Rust 1.95, edition 2024, as the workspace.
**Primary Dependencies**: gtk4 0.11, libadwaita 0.9 (`v1_8`), glib/gio
0.22, futures-util 0.3; no addition.
**Storage**: none; nothing about a failure is persisted.
**Testing**: `cargo test` with the scripted IMAP server, the scripted Graph
service and the Online Accounts test double; window tests through the
scripted loader; manual checks in [quickstart.md](quickstart.md).
**Target Platform**: GNOME desktop, native and Flatpak.
**Project Type**: desktop application.
**Constraints**: every widget declared in a form; no widget built in code;
no new thread; nothing written to disk; text from servers inert and masked.

## Constitution Check

- **I. Necessary complexity**: each mechanism names its situation: the
  declarations serve the failures the code already produces; the dialog
  serves issue reports, which the maintainer asked for; the panic capture
  serves a hostile message that panics the parser, which today loses the
  worker. The optional table holds what has no situation yet.
- **II. Clear language and names**: `DeclaredFailure`, `FailureAction`,
  `RemoteText`, `declare`, `report_text`; wording is
  impersonal and plain by FR-009 and AGENTS.md.
- **III. Explicit failures**: nothing is disguised: a failed load is a
  status page, a short list a banner, a content problem a status page, a
  panic a failed load; cancellation shows nothing. Texts are masked where
  built.
- **IV. One owner**: each failure's wording, action and lines have one
  owner, its declaration; the choice of channel has one owner, the window.
- **V. Responsive work**: no blocking work is added; the clipboard write
  and the dialog are main-thread widgets.
- **VI. Evidence**: tests per portion; the quickstart's manual checks for
  keyboard and screen reader (SC-005); the record checked by the existing
  fixtures (SC-003).

Gates pass; no violation to justify.

## Project Structure

### Documentation (this feature)

```text
specs/006-error-handling/
├── plan.md
├── research.md
├── quickstart.md
├── contracts/failure-declaration.md
└── tasks.md
```

No data model: nothing is persisted.

### Source Code

```text
crates/mailbag-imap/src/session.rs, reader.rs      masking at the source
crates/mailbag-providers/src/failure.rs            the declarations (new)
crates/mailbag-providers/src/batch.rs, lib.rs      WorkerStopped payload
crates/mailbag-providers/src/worker.rs             panic capture
crates/mailbag/src/failure_dialog.rs               the dialog (new)
crates/mailbag/src/window_ui.rs                    channels
crates/mailbag/src/mail_ui.rs                      reader status page
crates/mailbag/src/account_ui.rs                   form buttons
crates/mailbag/src/settings.rs                     toast sentence reworded
crates/mailbag/src/main.rs                         retry-accounts as an app action
crates/mailbag/resources/ui/mailbag.ui             banner, status children
crates/mailbag/resources/ui/message-content.ui     reader status page
crates/mailbag/resources/ui/failure-dialog.ui      new form
crates/mailbag/resources/ui/failure-block.ui       new form
```

The forms come from the prototype's `ui/` directory, which is not part of
the repository; only the files above are.

## Documents amended before implementing

| Document | Change | Portion |
|---|---|---|
| specs/003-logging/spec.md FR-011; research.md §6 | The sign-in name is replaced once, where the failure is built; the reply no longer travels to the UI unchanged; no separate copy for the record | 1 |
| specs/002-imap-integration/contracts/ui.md | The wording table and the toast rule for an incomplete list are superseded by 006; the reader's explanation in place of the text becomes a status page; the status page's explanation goes into the escaped description | 3 |
| specs/005-microsoft-graph-integration/research.md §5 | The service's message may appear in the failure dialog as a remote text | 3 |
| specs/002-imap-integration/data-model.md; contracts/imap-reading.md | Server text, with the sign-in name replaced where the failure is built, is for the failure dialog and the debug lines | 3 |
| AGENTS.md | "UI wording" (the impersonal voice, the Workbench demos) and the forms-only layout rule | before 1 |

## Post-implementation

Acceptance on 2026-09-25 with the installed Flatpak build of the branch,
by the maintainer, following [quickstart.md](quickstart.md).

| Step | How | Result |
|---|---|---|
| 1. Rejected sign-in | Live. Online Accounts cannot store a wrong password (it checks the sign-in when an account is added and offers no password change), so the account's app password was revoked at the provider | The failure page, the Online Accounts button and Details, as declared |
| 2. The failure dialog and copy | Live, on the rejected sign-in and on an unreachable server | Blocks in the spec's order; the copied text matches |
| 3. Unreachable server, Retry | Live, network off, then on | "Server unreachable" with Retry and Details; Retry loads the list |
| 4. Short list banner | Tests only (the scripted server and service) | — |
| 5. Content problem in the reader | Live | The reader's status page under the envelope; rows kept |
| 6. Keyboard | Live | As described |
| 7. Screen reader | Live, Orca | As described |
| 8. The record | Live, `--log-level=debug` | One error line per failed load (`cause=Failed(SignIn) code="AUTHENTICATIONFAILED"`, `cause=Failed(Connect)`); the server's reply at debug; no address, password or token in the file |
| 9. Panic on the worker | Tests only (a load that panics on purpose) | — |

Not verified live: `<login>` in a server's text, because the real
server's rejection did not repeat the sign-in name (the IMAP tests with
the scripted server cover it); the Retry Check progress state on the
account page's own button; the banner's button by keyboard and its text by
screen reader (SC-005), because the banner cannot be provoked live (step
4). The Settings toast has no test through the window: `settings/tests.rs`
checks that the launcher reports each error once, and the toast shows the
constant `LaunchError::message` (SC-001, amended 2026-09-25).

Size against the table above, measured on the branch after the review
fixes and the final refactor: production +945 −473 lines, net
+472, against ~350 new and ~200 moved (reassessed with the
maintainer after portion 2, who accepted about 420); `failure.rs` 463
lines against ~190, `failure_dialog.rs` 140 against ~90, `worker.rs` +71
−17 against +35; 13 Rust files, not 12; four new types, `PageAction`
added to the three; tests +567 −342 lines, the window channels in
one graphical test; forms +207.
