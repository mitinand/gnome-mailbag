# Feature Specification: Logging

**Feature**: `003-logging`
**Created**: 2026-09-21
**Status**: Approved 2026-09-21; reduced to its principles 2026-09-22.
**Input**: When asked to, Mailbag records what it does, so that a developer can
follow a run and a user can reproduce a problem and attach the record to an
issue report. Without a record, failures reach the user only as UI text, and the
IMAP library's own protocol trace is compiled out because it contains
credentials and mail ([002 research](../002-imap-integration/research.md)); the
problems of Gmail and Microsoft 365 — a refused authorization, throttling, an
expired session, HTTP errors — appear only on live servers.

**Scope**: This is the complete logging specification for Mailbag, not a first
layer. Its rules must already hold for the target application: three providers,
one of them HTTP-based; ten to thirty accounts working at the same time;
background synchronization, local storage and durable user actions; running
without a window. They are the principles every feature follows when it writes
its lines. This specification names no place where a line is written and keeps
no list of events or of fields: a write site belongs to the code of the feature
that owns it and is reviewed there (FR-017). The reasons, with the alternatives
that were rejected, are in [research](research.md).

## User Scenarios & Testing

### User Story 1 — Follow a run during development (Priority: P1)

The maintainer or an agent starts Mailbag from a terminal with the level option
at debug, uses it, and reads in the console what the application did and why:
which operations ran and how they ended, and which message was handled
unusually. Without this, a defect that appears only on a live server is
diagnosed by guessing. **Independent Test**: use a test server whose mail
contains known defects and read the record without looking at the window.

### User Story 2 — Produce a record for an issue report (Priority: P1)

A user whose mail does not load, or whose message shows wrongly, is not a
developer. They follow a short instruction in the README: quit Mailbag, start it
from a terminal with the option at debug and the error stream redirected to a
file, reproduce the problem, read the file, attach it to an issue. This is the
only way a defect on another person's server reaches the maintainer in a usable
form. **Independent Test**: follow the README instruction on the installed
Flatpak build without adding any permission.

### Edge Cases

Both are refused starts; FR-002 gives their exact wording and exit status.

| Situation | Why it must be refused |
|---|---|
| The level option while Mailbag is already running | A single-instance start only raises the existing window, so the person following the README would get an empty file and no explanation |
| A value that is not a level, such as `debg` | A typing mistake would otherwise give a run without a record, found only after reproducing the problem |

## Requirements

### Functional Requirements

**Turning logging on**

- **FR-001 — Off by default**: Without the level option Mailbag MUST NOT write
  any line of its own to the standard streams, to a file or to the system
  journal, and MUST install nothing and build no line. Logging on by default is
  excluded from the target design as well. Mailbag MUST NOT suppress or
  redirect the warnings GTK and GLib print themselves.
- **FR-002 — One option**: The command-line option `--log-level=<level>`, called
  the level option in this spec, turns logging on for one run and sets its
  level. There is no environment variable, settings switch or in-application
  control, and a running Mailbag's logging never changes.

  | Start | Result | Exit status |
  |---|---|---|
  | No `--log-level` | No record | as today |
  | `--log-level=error\|warning\|info\|debug` | Logging on for this run at that level; a level includes the levels before it | as today |
  | `--log-level=` anything else | `Unknown log level "<value>". Use error, warning, info or debug.` on the standard error stream; no window | 1 |
  | `--log-level=…` while Mailbag runs | `Mailbag is already running, so logging was not turned on. Quit Mailbag and start it again with this option.`; the running instance is not activated | 1 |

  `--help` MUST list the option with its four levels.
- **FR-003 — One stream**: The whole record MUST go to the standard error
  stream, where GTK and GLib warnings already go, so that one redirection
  captures both; never to the standard output stream. Lines are written
  synchronously, the way GLib writes its own warnings: no queue, no writer
  thread, no count of lost lines, no file option, no rotation. Lines of one
  thread keep the order of their events; no order is promised between threads or
  against GTK and GLib warnings.

**Levels**

- **FR-004 — Error**: An operation did not complete and has no result. Each
  failed operation MUST produce exactly one error line, written by whoever gives
  the operation up, naming in one field, `cause`, the same failure value the UI
  explains, such as `cause=TimedOut(SignIn)`. A component that reports its
  failure upwards writes no error line of its own, and a step that could not run
  writes none. An operation, for this rule, is work with an account, a mail
  server or stored mail: a launch of Settings that fails writes no line, because
  the UI explains it and it touches none of them. The cause is a failure value
  Mailbag already has, not one protocol's replies, so an HTTP provider fits
  without new rules.
- **FR-005 — Warning**: An operation completed, but its result is worse than
  normal or Mailbag took a workaround, and a reader should look at it even if
  nobody complained. Content this version does not support by design, such as
  HTML-only or encrypted mail, is not a warning; it is counted at info.
- **FR-006 — Info**: The normal course of work, told about an account and an
  operation. Cancellation is not an error and belongs here. No info line is
  about a single message or part, so the number of info lines of an operation
  MUST NOT grow with the number of messages. Reading info alone, a person can
  retell what Mailbag did.
- **FR-007 — Debug**: What happens inside an operation: a message, a part, a
  decision, a server. Its volume grows with the number of messages. The record
  holds no content, so it does not promise that a defect can be rebuilt from
  it.
- **FR-008 — Times, not durations**: Every line MUST carry the time of its event
  with milliseconds and the offset from UTC. No line carries a duration: a
  duration is the difference between the times of two lines, for a failed step
  as well.

Two checks catch a wrong level: a line written at warning during normal work is
not a warning, and an error line without a failure shown in the UI is either
not an error or a UI defect.

**Privacy** — turning logging on is an explicit act, and the person accepts that
the record describes their mailbox. These limits hold for Mailbag's own lines.

- **FR-009 — Never recorded**: At no level: passwords, tokens and other
  credentials, sign-in names, message bodies and attachment content, attachment
  file names, and the values of message headers, subjects and addresses
  included, also those of a message attached inside another. A header whose
  value did not come out of decoding is not recorded either, not even by name.
- **FR-010 — Details of a message or a server belong to debug**: Folder and
  label names, message identifiers, a server's host and port, the structure of a
  message and the text of a server's replies MUST NOT appear at error, warning
  or info, which speak about an account and an operation. A
  server's response code, such as `AUTHENTICATIONFAILED`, and its capability
  list are allowed at every level: they describe the server software, not the
  mailbox.
- **FR-011 — Server text at debug**: Before the text of a status reply or of an
  alert becomes a field, every occurrence of the account's sign-in name in it
  MUST be replaced with `<login>`, whatever the name's length; the reply itself
  travels on to the UI unchanged. Debug may also carry the TLS library's text
  for a failed handshake, which holds no server data. No other server data may
  be recorded: replies that carry message data carry headers and bodies.
- **FR-012 — Libraries**: A library's own log output MUST stay off unless this
  spec's owner shows that it stays within FR-009–011, and Mailbag MUST NOT
  bridge its own lines into a library's log system. The 002 decision to compile
  out the IMAP library's protocol trace is not reopened. `scripts/check.sh`
  fails when either guarantee is lost.

**What a line carries**

- **FR-013 — Account and message**: Every line about an account MUST name it by
  its GNOME Online Accounts identifier, by the identifier's text and never by a
  debug rendering of an identifier type; never by the mail address or the
  account's display name. Lines that observe an account also give its provider
  type. Because Mailbag runs one operation of a kind at a time, the lines of an
  operation's owner name the account and the lines written inside the operation
  do not repeat it. A line about a message MUST say which message, also in code
  that does not know which message it works on. **Deferred** until background
  synchronization, when operations of several accounts run at the same time: an
  account, or an identifier of the operation, on every line of that operation.
- **FR-014 — One line per event**: One event is one line, written once, at one
  level; a detail either has an event of its own that is useful by itself or is
  not written. A line gives its time, its level, where in Mailbag it was
  written, the message it is about when there is one, then fixed message text
  and its fields. Everything that comes from mail, a server or the system MUST
  be a field and MUST be escaped so that it cannot break a line; it MUST NOT be
  part of the message text and MUST NOT be passed in a form that skips escaping,
  which Clippy rejects in `scripts/check.sh`
  ([research §9](research.md#9-values-that-reach-a-line)). The layout is for
  people to read, not a contract for programs, and tests assert fields and
  markers rather than columns. Four field names cross crates and MUST mean the
  same everywhere: `account`, `cause`, `code` and `uid`; every other name lives
  in the code that writes it.
- **FR-015 — First line**: Whenever logging is on, at any level, Mailbag's first
  line MUST give: the application version, whether the build is native or
  Flatpak, the runtime or operating system version, the GTK and libadwaita
  versions in use, and the chosen level.

**Other guarantees**

- **FR-016 — Never in the way**: Logging MUST NOT make an operation fail: a line
  that cannot be written is dropped and work continues. When the person has
  piped the stream to a reader that stopped, Mailbag waits until it is read
  again. A value that costs work to build MUST be built only when its level is
  enabled.
- **FR-017 — Lines of every feature**: Every feature writes its lines under
  these rules; its write sites live in its own code and review checks them
  there. No specification keeps a list of events or of fields. Logging MUST NOT
  add behavior, state or a data field for the sake of a line. A line a person
  relies on, such as the error that names a refused authorization, is an
  ordinary requirement of the feature that owns it.
- **FR-018 — User instruction**: The README MUST tell a person how to produce a
  record: the exact command for the Flatpak build and for a native build, that
  Mailbag must be quit first, what a debug record contains and never contains,
  that GTK's and GLib's own warnings share the file, and the advice to read the
  file before attaching it.

## Success Criteria

### Measurable Outcomes

- **SC-001**: A run without the level option produces zero Mailbag lines on both
  standard streams: a test shows that no subscriber exists, and one start of
  Mailbag shows it on the real streams (FR-001).
- **SC-002**: With known markers in the test fixtures, an automated check over a
  whole load finds none of them at info, and at debug none except the folder name
  and the host: no password, sign-in name, subject, address, file name or body
  text, those of an attached message included, and no sign-in name of two
  characters that the test server repeats in its refusal (FR-009–011).
- **SC-004**: Every failed operation produces exactly one error line whose cause
  is the failure value the UI explains; every cancelled operation produces zero
  warning and error lines; a result worse than normal produces one warning
  (FR-004–006).
- **SC-006** (deferred with FR-013): every line of an operation carries the
  account it belongs to, also while account changes arrive during it.
- **SC-007**: Following the README on the installed Flatpak build gives a file
  with the first line and the run's lines, without adding a permission. Starting
  it a second time while Mailbag runs prints the explanation instead (FR-002,
  FR-015, FR-018).
- **SC-008**: With the stream unwritable during a load at debug, the load
  finishes and the window stays usable; with the stream piped to a reader that
  reads, a load at debug finishes without a visible delay (FR-016).

## Assumptions

- Started by the desktop session instead of a terminal, a run's error stream
  normally lands in the system journal. Mailbag does nothing for this; passing
  the option to such a start arrives with background running.
- The UI tells the person about failed actions and persistent problems
  (constitution III). Nobody is expected to find out about a failure from the
  record; it serves diagnosis after the person already knows.
- The person who turns logging on owns the record. Mailbag never reads it back,
  uploads it or sends anything about it over the network.
- 001 is amended: the account adapter gives an identifier's text, and the record
  names accounts by it (FR-013). Addresses and display names stay out of
  diagnostics.
- 002 is amended: server status text, which it kept for the UI only, reaches
  debug with the sign-in name replaced
  ([IMAP reading](../002-imap-integration/contracts/imap-reading.md),
  [data model](../002-imap-integration/data-model.md)). Its FR-009, "no
  credentials or personal mail in diagnostics", is met by FR-009 here.
- Out of scope, besides what
  [research §10](research.md#10-what-the-record-deliberately-does-not-do)
  excludes: saving a message as a fixture; a log viewer or a synchronization
  activity panel, so the approved UI layout does not change.
- The [constitution](../../.specify/memory/constitution.md) governs this feature.
