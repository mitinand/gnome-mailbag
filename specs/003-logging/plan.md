# Implementation Plan: Logging

**Branch**: `claude/logging` | **Feature**: `003-logging`
**Created**: 2026-09-21 | **Spec**: [spec.md](spec.md)
**Status**: Approved by the maintainer 2026-09-21. Implement by portions with review pauses.

## Summary

With `--log-level=<level>` Mailbag writes what it does to the standard error
stream: steps with durations at info, single messages and decisions at debug,
one error line per failed operation. Without the option nothing of this
exists at run time.

Events are written with the `tracing` crate in all four crates. A span around
each load carries the account label and the operation identifier to every
line inside it. The library's one-line formatter lays lines out and escapes
string values; Mailbag's own writer thread sends them out, so a stream nobody
reads costs lines and never responsiveness. The IMAP library's protocol trace stays compiled out
([research](research.md)).

## Behavior

| Situation | Choice |
|---|---|
| Start without the option | No subscriber, no thread, no lines. GTK and GLib print as before |
| `--log-level=debug` | First line with versions, then the run's lines, on the standard error stream |
| Unknown level | Message naming the four levels, exit status 1, no window |
| Option given while Mailbag runs | Message that logging was not turned on, exit status 1; the running instance is not activated |
| Nobody reads the stream, or it cannot be written | Lines beyond a bounded queue are dropped; their number is written when writing resumes, without a reason |
| Quit | A last line, then a short bounded wait for the writer |
| A load reconnects after an unreadable structure | The extra connections appear at info, with a line that says why |
| The server's text repeats the sign-in name | Written at debug with `<login>` in its place, whatever the name's length |
| A part has a file name | The record says which parameter carries it, its shape and its extension; never the name |
| The account list is read | One line where the read completes, in `goa-adapter`; no operation identifier |
| An account identifier that Online Accounts did not generate | Labelled `account-N` within the record instead ([research §5](research.md#5-the-account-label)) |

## Review points

1. **The dependency.** `tracing` and `tracing-subscriber` are the one owner
   of the domain, the second with its registry, level filter and one-line
   formatter. The deciding need is context on every line of a load, which
   already crosses three crates and two threads, without threading parameters
   through `mailbag-content`; the comparison with `log`, GLib logging, a
   hand-written facade and a formatting layer of our own is in
   [research §1](research.md#1-one-owner-of-the-logging-domain). Mailbag's
   own code is the writer (queue, thread, lost-lines line), the time source,
   the option and the labels.
   2. **The account label** differs from the spec's wording in one case: the
   Online Accounts source shows that an administrator's template or another
   program can supply an arbitrary identifier. Such identifiers get a number
   within the record. FR-013 has been amended to say so. `goa-adapter` owns
   the test through a new `AccountId::generated_id()`, an addition to the
   shared 001 account contract approved on 2026-09-21; an arbitrary
   identifier's text never leaves the adapter.
3. **Changes to 002 documents** about server text, with their exact wording,
   are in [the record contract](contracts/record.md#changes-to-002-documents).
   `MessagePart` and `MimePart` do not change.
4. **Escaping rests on a rule.** The library escapes string fields, not the
   message text and not values passed with `%`. So messages are fixed text and
   external strings are plain fields; `scripts/check.sh` rejects `%` in event
   macros and a test feeds line breaks through
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
| Output | `mailbag::logging`: a writer for the formatter that feeds a bounded queue, and a writer thread |
| Time and versions | GLib date and OS information, GTK and libadwaita version functions, `/.flatpak-info` |
| Option | GApplication main option, `handle-local-options`, explicit registration to detect a running instance |
| Library trace | `log` stays at `max_level_off`; no bridge between `log` and `tracing` |
| Storage | None |
| Permissions | Unchanged |
| Validation | Capture buffer in tests, scripted IMAP server with marker strings, blocking writer, manual and installed-Flatpak runs ([quickstart](quickstart.md)) |

## Ownership and function map

`mailbag::logging` owns the level, the time source, the queue, the lost count,
load numbers and account labels. The library owns the layout. Each crate owns
the events of its own work. The load owns the load's error line; the account
observer owns the line about each read of the account list.

```text
main
  add_log_level_option          declare --log-level for --help and parsing
  handle_local_options
    parse_log_level             reject an unknown value, status 1
    refuse_when_already_running register; remote → message, status 1
    start_logging               install the subscriber, start the writer
      write_first_line          versions, build kind, level
  on shutdown: finish_logging   last line, close the queue, wait ≤ 1 s

mailbag::logging
  LocalTime                     the formatter's timer, from GLib
  QueueWriter                   the formatter's writer: try_send, or count the line as lost
  write_lines                   writer thread: lost-lines line first, then the line
  next_load_operation           "load-7"
  account_label                 AccountId::generated_id(), or account-N

mailbag::inbox_load
  start_load                    open the load span, info "load started"
  finish: one of                info finished / info cancelled / error failed,
                                plus warnings for unreadable content and a refused list

mailbag-imap                    info per step with duration; debug per message,
  server_text_for_log           part and command; writes server text itself,
                                sign-in name of any length replaced

mailbag-content                 debug per selection and decoded part
  value_shape                   list headers and file names; only when debug is on

goa-adapter                     one line per read of the account list, where it ends
mailbag::accounts, settings     appearing and excluded accounts, problems,
                                opening Settings
```

Which line each step writes is fixed by [log events](log-events.md); field
names and the rules for writing events by
[the record contract](contracts/record.md).

## Project Structure

```text
specs/003-logging/
  spec.md, log-events.md, plan.md, research.md, data-model.md, quickstart.md
  contracts/record.md           command line, line format, fields, rules, 002 changes
  checklists/requirements.md

crates/mailbag/src/
  logging.rs                    new: option, timer, queue writer, thread, labels, first line
  logging/tests.rs              new
  main.rs                       option and start/finish of logging
  inbox_load.rs                 load span, load lines, dispatcher for the worker
  accounts.rs, account_ui.rs, settings.rs, mail_ui.rs   their events
crates/mailbag-imap/src/        session.rs, transport.rs, reader.rs, part_tree.rs: events;
                                server_text_for_log
crates/mailbag-content/src/lib.rs   events; value_shape
crates/goa-adapter/src/         client.rs, imap_access.rs: events;
                                account_model.rs: AccountId::generated_id
Cargo.toml files, Cargo.lock, cargo-sources.json
scripts/check.sh                no tracing-log; log stays at max_level_off; no % in events
README.md                       "Reporting a problem" section
AGENTS.md                       one line: every feature records its log events
specs/001-goa-account-observation/   contracts/accounts.md, research.md: the ID as a label
specs/002-imap-integration/     contracts/imap-reading.md, data-model.md: server text
```

## Cost and implementation portions

Estimate: one new production module of about 150 lines; about 60 event call
sites across ten existing files, 250–400 lines with their duration
measurements; two small functions with logic of their own
(`server_text_for_log`, `value_shape`); one thread that exists only with the
option; three small types (level, timer, queue writer). Tests: about 20
focused cases and marker strings added to existing fixtures. Reassess before
exceeding about 1.5 times this estimate.

One intended feature PR: **Logging**. The maintainer creates commits and the
PR. Implement in this order:

| Portion | Reviewable result | Required checks and proposed commit |
|---|---|---|
| 1 — The record | Dependencies and regenerated sources; `logging.rs` with the option, both refusals, first and last line, timer, queue writer, thread and lost-lines line; the checks in `scripts/check.sh`. No events besides start and quit | Escaping, option and stalled-writer tests; `scripts/check.sh`; Flatpak compilation without build-network access; the native manual runs of [quickstart](quickstart.md). `feat(logging): record a run on request` |
| 2 — Accounts and application | Verify the account rows of log events against the code and correct them; events of account observation, problems, exclusion, discarded mail, opening Settings, opening a message; AGENTS.md rule | Private-bus tests assert lines for read, failure, recovery, appearing and excluded accounts; SC-006 for account lines. `feat(logging): log account observation` |
| 3 — Load and IMAP | Load span and the worker's dispatcher; the load's info, warning and error lines; IMAP step lines with durations, reconnection, part trees, certificate errors, server text through `server_text_for_log`; the 002 document changes | SC-004, SC-005, SC-006 on the scripted server; sign-in name replacement cases. `feat(logging): log the Inbox load` |
| 4 — Content, privacy and instructions | Selection and decoding lines, `value_shape` for headers and file names; marker strings in fixtures and the SC-002 and SC-003 checks over the whole path; README section; installed-Flatpak acceptance | SC-001–003, SC-007–009; all of [quickstart](quickstart.md). `feat(logging): log message content decisions and document reporting` |

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
| I — Necessary complexity | One dependency pair chosen against four alternatives, for a need that exists today: a load's lines come from three crates and two threads, and `mailbag-content` may not depend on GLib. The library's formatter is used instead of a layer of our own. The writer thread answers a stalled pipe, which occurs with `… \| less`. No file output, rotation, per-component levels, machine-readable format or in-memory history. The label fallback exists because Online Accounts accepts arbitrary identifiers |
| II — Clear language | The plan states behavior, review points and portions; the line format, fields and 002 wording are in the contract; the comparison of mechanisms is in research. Names say what they do: `server_text_for_log`, `value_shape`, `account_label` |
| III — Truthful failure | One error line per failed operation with the UI's step and cause; lost lines are stated in the record; nothing personal beyond the spec's debug limits, checked with markers |
| IV — One owner | `mailbag::logging` owns level and output, the library owns layout and escaping; `mailbag-imap` alone turns server text into a field; each feature's log events own the level and fields of its lines |
| V — Responsive, bounded work | Formatting happens where the event happens and only when the level is on; writing happens on its own thread; the queue is bounded; quitting waits a short bounded time |
| VI — Evidence | Automated checks per success criterion and manual runs are listed in [quickstart](quickstart.md). None has been run; the installed Flatpak is checked in portion 4 |

No constitutional exception is proposed.
