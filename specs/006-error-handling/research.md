# Error Handling: Research

Decisions that had alternatives, with what was checked. Facts are marked
checked (a source or an experiment), inferred, or unknown.

## 1. Where the declarations live

**Decision** (corrected twice on 2026-09-26): a lower layer hands the
application a failure in the domain's terms, never in a protocol's.
`mailbag-domain`, a crate below every layer, defines the failure every layer
speaks: `Failure { kind, remote_texts, details }`, where `kind` is a
`FailureKind` that says what went wrong in the application's terms (the
stable reason), `remote_texts` are the remote side's words with their
source, and `details` are the identifiers for a report. The layer that meets
a failure converts its own values into it: providers turn an `AccessError`,
an `ImapError` or a `GraphError` into a `FailureKind`, which is where a
protocol's codes are read (a sign-in rejected for its credentials, a server
temporarily unavailable); the store (007) turns SQLite's errors into its
kinds. The layer that gives an operation up also writes its error line to
the record (003 FR-004), since it holds the protocol values the line names.
The wording and the action are written in `mailbag`, in
`failure_declarations.rs`, one exhaustive `match` over `FailureKind`; the
window no longer sees a protocol type and does not depend on the protocol
crates. The content outcome (`ReceivedContent`) and the short list
(`IncompleteList`) are domain types for the same reason.

**Why the first decision was wrong**: it put the declarations in
`mailbag-providers`, reading "the code of the feature that owns the
failure" as "the crate where the loads meet". No other owner could declare
there: the store (007) lies below providers, a synchronization engine will
lie above it and would make providers depend on it, and the window's own
failures (a stored Inbox that cannot be read) belong to the window. The
wording would also have to be translated in a lower layer.

**Why the second decision was incomplete**: it moved the wording into
`mailbag` but let the protocol values travel up: `failure_declarations.rs`
matched `ImapFailure`, `ImapStep`, `GraphFailure` and `AccessError`, read the
provider's facts, and `mailbag` depended on `mailbag-graph` and
`mailbag-imap` only to word their errors. Every new layer that fails would
have added its own type to the window. The application's architecture puts
the shared definitions, outcome types included, into one domain crate that
every layer depends on, and lets only the application explain an outcome;
the store, the second layer whose failures reach the window, is where the
cost of converting is lowest.

**Checked** (`Cargo.toml` of every crate): `goa-adapter`, `mailbag-imap`,
`mailbag-graph` and `mailbag-content` depend on nothing in the workspace;
`mailbag-providers` depends on all four; `mailbag` on all five. A domain
crate with no workspace dependency can be reached by all of them.
`ContentExplanation` moves from `mailbag-content` into it, so that the
domain crate depends on nothing and `mailbag-content` returns the domain's
type.

**The kinds**: one per text the window shows today, so that no string
changes ([contract](contracts/failure-declaration.md)). The mail server's
steps are a domain enumeration (`ServerStep`) that repeats `ImapStep`: this
repetition is the price of a window that knows no protocol. The kind also
names the failure in the technical details (`Failure:`) and in the record's
`cause` (decided by the maintainer on 2026-09-26), so a report and the
record say it the same way the window chooses its words; the status and
the codes stay as separate lines and fields. `ServerUnavailable` carries its
step, so no kind names less than the protocol value it replaces.

**Alternatives**:
- The wording in `mailbag`, matching the protocol types (the second
  decision): no domain crate, but the window depends on every protocol and
  on every lower layer's error type.
- A new leaf crate for the declaration's type and wording, below every
  owner: every owner would write wording and need translation, and lower
  crates without the operation's context would be invited to write advice
  they cannot know.
- A coarse list of kinds (a few for all remote failures): fewer variants,
  but the texts that name the failed step would change.

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
(007). The hook and its slot live in `mailbag-domain` (corrected
2026-09-26): every layer that sends work to a thread needs them, and a
caught panic becomes a `Failure` of kind `Stopped` whose details carry it.

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
- A shared domain crate with one failure type: decided, see §1 (corrected
  2026-09-26).
- Server text as a type that can only be built with the sign-in name
  replaced, so that the compiler refuses unmasked text in a failure:
  today the name is replaced where IMAP failures are built, in one place
  plus the refusal of a short list, and tests check both (§3). Revisit at
  the next global refactor, or as soon as a second place builds IMAP
  failures from server replies, such as new commands of a synchronization
  engine.
