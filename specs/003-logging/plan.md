# Implementation Plan: Logging

**Branch**: `claude/logging` | **Feature**: `003-logging`
**Created**: 2026-09-21 | **Spec**: [spec.md](spec.md)
**Status**: Approved by the maintainer 2026-09-21. Implement by portions with review pauses.

## Summary

With `--log-level=<level>` Mailbag writes what it does to the standard error
stream: steps with their times at info, single messages and decisions at debug,
one error line per failed operation. Without the option nothing of this
exists at run time.

Events are written with the `tracing` crate in all four crates. A span around
each load carries the account identifier to every
line inside it. The library's one-line formatter lays lines out and escapes
string values and writes them straight to the standard error stream, as
GLib's own warnings are written. The IMAP library's protocol trace stays
compiled out ([research](research.md)).

## Behavior

| Situation | Choice |
|---|---|
| Start without the option | No subscriber, no lines. GTK and GLib print as before |
| `--log-level=debug` | First line with versions, then the run's lines, on the standard error stream |
| Unknown level | Message naming the four levels, exit status 1, no window |
| Option given while Mailbag runs | Message that logging was not turned on, exit status 1; the running instance is not activated |
| The stream cannot be written | The line is dropped; no operation fails |
| The stream is piped to a reader that stopped | Mailbag waits at its next line until the stream is read again; accepted for output that exists only on request ([research §3](research.md#3-writing-to-the-standard-error-stream)) |
| Quit | A last line. Closing the window cancels a running load, whose line comes first; the Quit action ends Mailbag without cancelling it, so the record shows the load's start and then the last line. The shutdown order is not changed for the record |
| A load reconnects after an unreadable structure | The extra connections appear at info, with a line that says why |
| The server's text repeats the sign-in name | Written at debug with `<login>` in its place, whatever the name's length |
| A text part has a file name | The choice of text parts names it as left out as a file; never the name. Its shape and extension arrive with attachments |
| The account list is read | One line where the read completes, in `goa-adapter` |
| An account is named | By its Online Accounts identifier, which stays the same across runs and can be found in the Online Accounts configuration; lines that observe an account add its provider type ([research §5](research.md#5-naming-an-account)) |
| A load fails | One error line naming the failure values the UI explains, such as `step=SignIn cause=TimedOut`; the UI's wording is not shared or moved |

## Review points

1. **The dependency.** `tracing` and `tracing-subscriber` are the one owner
   of the domain, the second with its registry, level filter and one-line
   formatter. The deciding need is context on every line of a load, which
   already crosses three crates and two threads, without threading parameters
   through `mailbag-content`; the comparison with `log`, GLib logging, a
   hand-written facade and a formatting layer of our own is in
   [research §1](research.md#1-one-owner-of-the-logging-domain). Mailbag's
   own code is the time source, the option, the first line and the labels.
2. **Output is synchronous.** Lines go straight to the standard error
   stream. A pipe whose reader stopped makes Mailbag wait; the maintainer
   accepted this under constitution V on 2026-09-21 for output that exists
   only on request. The rejected queue and writer thread are described in
   [research §3](research.md#3-writing-to-the-standard-error-stream).
3. **Changes to 002 documents** about server text, with their exact wording,
   are in [the record contract](contracts/record.md#changes-to-002-documents).
   `MessagePart`, `MimePart`, the application's load result and the 001
   account contract do not change.
4. **Escaping rests on a rule.** The library escapes string fields, not the
   message text and not values passed with `%`. So messages are fixed text and
   external strings are plain fields; Clippy, run by `scripts/check.sh`,
   rejects `%` in event macros and a test feeds line breaks through
   ([research §9](research.md#9-values-that-reach-a-line)).
5. **One accommodation for tests in production code**: the mail worker uses
   the dispatcher of the thread that started it
   ([research §8](research.md#8-testing-a-record)). Three lines; no effect in
   the application.
6. **Not verified by reading code**: the rows of
   [log events](log-events.md) about an unavailable Mail service were checked
   against `accounts.rs` only. Portion 2 starts by checking them against
   `goa-adapter` and corrects the list where it disagrees with the code.

## Technical Context

| Area | Design |
|---|---|
| Language/platform | Rust 2024, toolchain 1.95, GLib/GIO 0.22.9, libadwaita 0.9.2, GNOME runtime 50; unchanged |
| Events | `tracing` in all four crates, default features off except `std` |
| Subscriber | `tracing-subscriber` with `std`, `registry` and `fmt`: span registry, level filter, one-line formatter without colors |
| Output | The formatter writes to the standard error stream; tests give it a buffer |
| Time and versions | GLib date and OS information, GTK and libadwaita version functions, `/.flatpak-info` |
| Option | GApplication main option, `handle-local-options`, explicit registration to detect a running instance |
| Library trace | `log` stays at `max_level_off`; no bridge between `log` and `tracing` |
| Storage | None |
| Permissions | Unchanged |
| Validation | Capture buffer in tests, scripted IMAP server with marker strings, a failing writer, manual and installed-Flatpak runs ([quickstart](quickstart.md)) |

## Ownership and function map

`mailbag::logging` owns the level, the time source and the names of provider
types. The library owns the layout. Each crate owns
the events of its own work. `InboxController` owns load outcomes, cancellation
and discarded results; the account observer owns each account-read outcome.
The [record contract](contracts/record.md#load-outcomes-and-cancellation)
defines these boundaries and the order at shutdown.

```text
main
  add_log_level_option          declare --log-level for --help and parsing
  handle_local_options
    parse_log_level             reject an unknown value, status 1
    refuse_when_already_running register; remote → message, status 1
    start_logging               first line, then install the subscriber
  after run returns             write the quit line

mailbag::logging
  LocalTime                     the formatter's timer, from GLib
  provider_type                 "imap", "google", "microsoft365", "other"

mailbag::inbox
  begin_load                    keep the load span, info "load started"
  discard_excluded / cancel_load info cancelled with the known reason, where the cancellation happens
  finish_load / show_result     log the accepted outcome or the discarded late result
  error line                    step and cause as the names of the failure values

mailbag::window_ui
  refresh_inbox / finish_load   enter the controller's load span for loader calls and results

mailbag::inbox_load
  start_load                    capture the caller's load span; pass it to the worker
  on settings received          info, inside span.in_scope (a GIO callback)

mailbag-imap                    info per step; debug per message,
  server_text_for_log           part and command; writes server text itself,
                                sign-in name of any length replaced

mailbag-content                 debug per selection, decoded part and header
                                that did not decode

goa-adapter                     one line per read of the account list, where it ends
mailbag::accounts, settings     appearing and excluded accounts, problems,
                                opening Settings
```

Which line each step writes is planned in the working file
[log events](log-events.md); field
names and the rules for writing events by
[the record contract](contracts/record.md).

## Project Structure

```text
specs/003-logging/
  spec.md, log-events.md, plan.md, research.md, data-model.md, quickstart.md
  contracts/record.md           command line, line format, fields, rules, 002 changes
  checklists/requirements.md

crates/mailbag/src/
  logging.rs                    new: option, timer, labels, first line
  logging/tests.rs              new
  main.rs                       option, start/finish of logging
  inbox.rs                      load span, cancellation and result lines
  window_ui.rs                  span scopes
  inbox_load.rs                 settings step, span propagation, dispatcher for the worker
  accounts.rs, account_ui.rs, settings.rs, mail_ui.rs   their events
crates/mailbag-imap/src/        session.rs, transport.rs, reader.rs, part_tree.rs: events;
                                server_text_for_log
crates/mailbag-content/src/lib.rs   events
crates/goa-adapter/src/         client.rs: one line per read of the account list
Cargo.toml files, Cargo.lock, cargo-sources.json
scripts/check.sh                no tracing-log; log stays at max_level_off
clippy.toml                     no % in events
README.md                       "Reporting a problem" section
specs/002-imap-integration/     contracts/imap-reading.md, data-model.md: server text
```

## Cost and implementation portions

Estimate: one new production module of about 60 lines; about 65 event call
sites across fifteen existing files, 250–350 lines; no duration fields; one
small function with logic of its own (`server_text_for_log`); no new thread;
two small types (level, timer). Tests: about
20 focused cases and marker strings added to existing fixtures. Reassess
before exceeding about 1.5 times this estimate.

`RunningLoad` gains the span already described above. The
application's load result, the protocol and MIME part models, the UI's
failure wording and the 001 account contract stay unchanged.

One intended feature PR: **Logging**. The maintainer creates commits and the
PR. Implement in this order:

| Portion | Reviewable result | Required checks and proposed commit |
|---|---|---|
| 1 — The record | Dependencies and regenerated sources; `logging.rs` with the option, both refusals, first and last line and timer; the checks in `scripts/check.sh`. No events besides start and quit | Escaping, option and failing-writer tests; `scripts/check.sh`; Flatpak compilation without build-network access; the native manual runs of [quickstart](quickstart.md). `feat(logging): record a run on request` |
| 2 — Accounts and application | Verify the account rows of log events against the code and correct them; events of account observation, problems, exclusion, discarded mail, opening Settings, opening a message | Private-bus tests assert lines for read, failure, recovery, appearing and excluded accounts; SC-006 for account lines. `feat(logging): log account observation` |
| 3 — Load and IMAP | Controller-owned load span and outcomes, cancellation lines where cancellation happens, the worker's dispatcher; IMAP step lines, reconnection, part trees, certificate errors and server text; the 002 document changes | SC-004–006 on the scripted server; both cancellation reasons, discarded late results and sign-in name replacement. `feat(logging): log the Inbox load` |
| 4 — Content, privacy and instructions | Selection and decoding lines, headers that did not decode; completed marker fixtures and SC-002/003 over the whole path; README section; installed-Flatpak acceptance | SC-001–003, SC-007, SC-008; all of [quickstart](quickstart.md), with results outside the repository and unperformed checks left unverified. `feat(logging): log message content decisions and document reporting` |

The privacy check of portion 4 also covers the lines of portions 2 and 3, so
each of those portions includes the marker check for its own lines; portion 4
completes the fixtures and makes it one check over the whole path.

After **each** portion, run its checks and `scripts/check.sh`, report what
changed, the evidence, limitations, a suggested commit and the intended PR,
then **stop for review**. Start the next portion only on explicit instruction.
The tasks document must keep these boundaries.

## Constitution Check

Before design: the spec asks for nothing at run time unless the person asks,
and for no file, viewer or background record. No gate is violated.

After design:

| Principle | Assessment |
|---|---|
| I — Necessary complexity | One dependency pair chosen against four alternatives, for a need that exists today: a load's lines come from three crates and two threads, and `mailbag-content` may not depend on GLib. The library's formatter is used instead of a layer of our own. No queue or writer thread for the one self-inflicted case of an unread pipe; no file output, rotation, per-component levels, machine-readable format or in-memory history; no shape of file names before anything uses them; no reworking of UI wording for the record; an account is named by the identifier Online Accounts already gives it |
| II — Clear language | The plan states behavior, review points and portions; the line format, fields and 002 wording are in the contract; the comparison of mechanisms is in research. Names say what they do: `server_text_for_log`, `provider_type` |
| III — Truthful failure | One error line per failed operation naming the failure the UI explains; nothing personal beyond the spec's debug limits, checked with markers |
| IV — One owner | `mailbag::logging` owns level and output, the library owns layout and escaping; `mailbag-imap` alone turns server text into a field; each crate writes the lines of its own work under the spec's rules |
| V — Responsive, bounded work | Formatting happens only when the level is on. Lines are written synchronously to the standard error stream, as GLib writes its warnings; a terminal and a file do not block, and the wait on an unread pipe was accepted by the maintainer on 2026-09-21 for output that exists only on request. Most lines of a load come from the mail worker's thread |
| VI — Evidence | Automated checks per success criterion and manual runs are listed in [quickstart](quickstart.md). None has been run; the installed Flatpak is checked in portion 4 |

No constitutional exception is proposed.
