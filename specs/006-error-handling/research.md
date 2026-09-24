# Error Handling: Research

Decisions that had alternatives, with what was checked. Facts are marked
checked (a source or an experiment), inferred, or unknown.

## 1. Where the declarations live

**Decision**: in `mailbag-providers`, next to the load sequences, as
`declare` methods on `LoadFailure`, `IncompleteList` and `ReceivedContent`.
The Settings launch failure, a type of the `mailbag` crate, keeps its one
sentence there: the toast shows nothing more.

**Checked** (`Cargo.toml` of every crate): `goa-adapter`, `mailbag-imap`,
`mailbag-graph` and `mailbag-content` depend on nothing in the workspace;
`mailbag-providers` depends on all four; `mailbag` on all five. A type
declared in providers is therefore invisible to the protocol crates, and
"declared in the code of the feature that owns it" (spec FR-001) means the
providers crate for every failure that comes through a load: the loads are
where 002, 004 and 005 already meet.

**Alternatives**: in the window, where the wording is today: keeps the
protocol types in front of the widgets and the wording out of reach of a
later scheduling layer (007), which will need the action to decide whether
to try again; a new crate below the protocol crates for the declaration
type: nothing else would live in it yet.

## 2. The failure dialog and the status pages

**Decision**: the dialog is an `AdwDialog` from a form (`failure-dialog.ui`)
with an `AdwToolbarView`, a header bar (title, close, copy button) and a
column: two paragraphs, one `failure-block.ui` per remote text and one for
the technical lines, the action button. Spacing: 12 between the
paragraphs, 24 between the paragraphs and the blocks, 24 between blocks,
24 to the action; in a block 6 between heading and card, 12 inside the
card; 24 from the sides. The status pages put the explanation and the
advice into `description`, escaped, and keep only the buttons as their
child.

**Checked**: libadwaita's stylesheet, extracted from the installed 1.9
library with `gresource`: preferences pages use 24 between groups and 6
between a group's title and its content; alert dialogs 24 between parts,
10 between heading and body; status pages 36 outside and 12 between title
and description, and 36 to a child widget. Workbench's Dialog demo is an
`AdwDialog` with a toolbar view and a header bar. `AdwStatusPage` parses
`description` as markup (libadwaita documentation, and 002's contract);
`glib::markup_escape_text` makes the text safe. The clipboard is
`gdk::Clipboard::set_text` on the window's clipboard.

**Alternatives**: `AdwAlertDialog`: centers its heading and body, which
wraps two paragraphs badly, and needs a Close response; rejected on the
prototype. `AdwPreferencesPage` for the spacing: rejected, a settings
widget. A banner in the reader for content problems: rejected, nothing
technical stands behind such a problem and the body becomes a web view in
011; the status page is GNOME's form for content that cannot be shown.

## 3. Replacing the sign-in name at the source

**Decision**: `ServerNotices::error` in `mailbag-imap` replaces the name in
the reply text and in every alert before they enter `ImapError`;
`InboxReader::fetch_rows` does the same for the refusal that travels with
a short list. The record's debug lines then log the text as it is.

**Checked** (`session.rs`, `reader.rs`): the name is known at all three
places where a failure is built (`notices.error(&account.login, …)`) and
where the refusal is returned; nowhere else. The window knows the account's
label and address, not the IMAP user name (`ImapUserName`, read in
goa-adapter for the load only). 003 FR-011 says the reply travels on
unchanged; that sentence and research §6 are amended.

**Alternatives**: a second, masked copy in `ImapError` for the dialog only
(a field in a provider error type, against the budget); the name carried to
the window (state per account, against the budget). Both were the spec
challenge's decision 1.

## 4. Catching a panic on the mail worker

**Decision**: a panic hook installed once when the worker thread starts
stores one string, the panic's message and its place, in a thread-local
slot and calls the previous hook; `run_load` wraps the load future in
`futures_util::FutureExt::catch_unwind` (with `AssertUnwindSafe`), leaves
the payload unread (the hook already has the message) and turns `Err` into
`LoadFailure::WorkerStopped(Some(panic))`. The worker loop goes on; no
state survives a load, so nothing is left poisoned. A panic inside a
library may carry part of the text it was handling: `str` slicing off a
char boundary prints up to 256 characters of the string (checked,
`core/src/str/mod.rs`); the dialog shows the text as received, and FR-014
says so.

**Checked**: Rust 1.81 release notes: a panic that unwinds out of an
`extern "C"` function aborts the process; glib-rs 0.22.9 signal trampolines
are `unsafe extern "C" fn` that call the closure directly (`object.rs`), so
a panic in a GTK callback ends the application and no notice is possible.
`std::panic::set_hook` runs on the panicking thread before unwinding and
gives the message and the location (`PanicHookInfo`). `futures-util` is a
dependency of providers with its default features, which include `std` and
`catch_unwind`. Today a panic on the worker ends the thread and the window
learns `WorkerStopped` without a reason (`worker.rs`, `report_outcome`).

**Inferred**: symbols in a release build are not enough for a useful
backtrace without debug information; a backtrace is therefore optional and
not planned.

**Alternatives**: joining the thread to read the panic payload (the thread
is long-lived); a fresh thread per load (a thread per load for the sake of
a rare bug); nothing, as today (the report then says only "stopped").

## 5. Rejected and deferred

- Online Accounts' EnsureCredentials after a rejected sign-in: Online
  Accounts checks on its own at start, on network changes and periodically
  (checked, 3.58.1 daemon source); the status page with its Online Accounts
  button covers the case.
- A crash file with a report at the next start: out of scope by the spec;
  the panic's text reaches the error stream and the journal.
- The stale-mail banner over stored messages: waits for 007 (spec US2,
  FR-013); until then a refresh starts from an empty list.
- The application version in the technical lines: the About dialog shows it.
