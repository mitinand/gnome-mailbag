# Error Handling: Research

Decisions that had alternatives, with what was checked. Facts are marked
checked (a source or an experiment), inferred, or unknown.

## 1. Where the declarations live

**Decision** (corrected 2026-09-26): the wording and the action are written
in `mailbag`, in `failure_declarations.rs`, one exhaustive function per
failure value (`declare_load_failure`, `declare_short_list`,
`declare_content`). The lower layer keeps what only it knows: the typed
value, the remote texts built with the sign-in name replaced, the technical
details (`technical_details()`), the record's values (`cause_name`,
`status`, `server_code`) and the facts read from a protocol's codes
(`LoadFailure::credentials_rejected`, `server_temporarily_unavailable`).
The Settings launch failure, a type of `mailbag`, keeps its one sentence.

**Why the first decision was wrong**: it put the declarations in
`mailbag-providers`, reading "the code of the feature that owns the
failure" as "the crate where the loads meet". No other owner could declare
there: the store (007) lies below providers, a synchronization engine will
lie above it and would make providers depend on it, and the window's own
failures (a stored Inbox that cannot be read) belong to the window. The
wording would also have to be translated in a lower layer. The rejected
alternative "in the window" was rejected for two reasons that do not hold:
a synchronization layer decides whether to try again from the typed
failure, not from wording or a button; and the window already depends on
the protocol crates and receives `LoadFailure` today. The phrase is gone from
the specification, which describes what the user sees; where the wording
is written is this decision.

**Checked** (`Cargo.toml` of every crate): `goa-adapter`, `mailbag-imap`,
`mailbag-graph` and `mailbag-content` depend on nothing in the workspace;
`mailbag-providers` depends on all four; `mailbag` on all five. Every layer
that runs an operation for the user is `mailbag-providers` or above it, so
its failure value reaches `mailbag` without a new dependency.

**Alternatives**:
- A new leaf crate for the declaration's type and wording, below every
  owner: every owner could declare, but every owner would write wording and
  need translation, and lower crates without the operation's context (the
  IMAP crate does not know that a password came from Online Accounts) would
  be invited to write advice they cannot know.
- The declarations in providers, the rule narrowed to "the layer that runs
  the operation declares": no new crate, but the wording stays in a lower
  layer and splits across providers, the window and later the engine, each
  translated.
- A shared failure type with a short list of kinds in a domain crate, which
  every layer converts its errors into: the application would stop matching
  protocol types, but to keep today's texts the list must tell apart about
  thirty cases, a copy of the existing enumerations with conversions; or it
  stays coarse and the texts change. Deferred to §5.

## 2. The failure dialog and the status pages

**Decision**: the dialog is an `AdwDialog` from a form (`failure-dialog.ui`)
with an `AdwToolbarView`, a header bar (title, close, copy button) and a
column: two paragraphs, one `failure-block.ui` per remote text and one for
the technical lines, the action button. Spacing: 12 between the
paragraphs, 24 between the paragraphs and the blocks, 24 between blocks,
24 to the action; in a block 6 between heading and card, 12 inside the
card; 24 from the sides. The status pages put the explanation and the
advice into `description`, escaped, and keep only the buttons as their
child. The banner follows Workbench's Banner demo and the status pages its
Status Page demo, as the dialog follows its Dialog demo.

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
slot and calls the previous hook; `load_catching_panics` wraps the load future in
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

**Checked** (2026-09-26, a prototype outside the repository with
`std::thread` and `gio::spawn_blocking`): the hook's slot holds the panic
only on the thread that panicked; the code that waits for the result on
another thread reads nothing there. A panic in work sent to GIO's thread
pool is therefore caught inside that work, where `catch_unwind` and the
slot give the message and the place, and the pool keeps serving. The code
that sends work to a thread catches its panics there; a
helper for work outside the mail worker comes with the first such work
(007).

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
- A shared domain crate with one failure type and a short list of kinds
  (§1): it is needed when failures of several lower crates must be told
  apart the same way, for example when a synchronization engine hands the
  application outcomes that do not depend on the provider, or when two
  lower crates need the same types (007's content outcome, shared by the
  store and providers, is the first candidate). The classification of
  failures goes into that crate then, never into one of its producers;
  the typed facts providers gives today
  (`credentials_rejected`, `server_temporarily_unavailable`) are where it
  starts.
- Server text as a type that can only be built with the sign-in name
  replaced, so that the compiler refuses unmasked text in a failure:
  today the name is replaced where IMAP failures are built, in one place
  plus the refusal of a short list, and tests check both (§3). Revisit at
  the next global refactor, or as soon as a second place builds IMAP
  failures from server replies, such as new commands of a synchronization
  engine.
