# Feature Specification: Logging

**Feature**: `003-logging`
**Created**: 2026-09-21
**Status**: Approved by the maintainer 2026-09-21
**Input**: When asked to, Mailbag records what it does, so that a developer can
follow a run and a user can reproduce a problem and attach the record to an
issue report.

Mailbag has no logging today. Failures reach the user only as UI text, and the
IMAP library's own protocol trace is compiled out because it contains
credentials and mail ([002 research](../002-imap-integration/research.md)). The
next features connect Gmail and Microsoft 365, whose problems (a refused
authorization, throttling, an expired session, HTTP errors) appear only on live
servers and cannot be diagnosed from UI text.

**Scope**: This is the complete logging specification for Mailbag, not a first
layer. Its rules must already hold for the target application: three providers,
one of them HTTP-based; ten to thirty accounts working at the same time;
background synchronization, local storage and durable user actions; running
without a window. Later features add their events under these rules and do not
redesign them.

Only the lines of what exists today are implemented now: account
observation, the Inbox load and text decoding. They are planned in
[log events](log-events.md), a working file of this feature's
implementation.

The lasting guarantees are: nothing is recorded unless the person asked for it
(FR-001); the privacy limits of each level (FR-009–012); one meaning of the
levels for every feature (FR-004–008); and that logging never fails an
operation and costs nothing when it is off (FR-016).

## Clarifications

### Session 2026-09-21

- Q: Environment variables, command-line options or both? → A: Command-line
  options only. They appear in `--help`, pass through `flatpak run` unchanged,
  and a second start can say that logging was not turned on (Edge Cases).
- Q: Is there an option that writes the record to a file? → A: No. The record
  goes to the standard error stream and the person redirects it. The host shell
  owns the file, so the Flatpak sandbox needs no permission. A record left on
  for days needs a file that stays bounded; no such run exists before
  synchronization and storage, so that option arrives later as an amendment to
  this spec.
- Q: What separates the levels? → A: Two questions, not privacy. How the
  operation ended separates error, warning and info. How closely the line looks
  separates info (an account and an operation) from debug (a message, a part,
  a decision). The privacy limits are laid over this and mostly follow from it.
- Q: May values of message headers (subject, addresses) appear at debug for a
  failing message? → A: No, at no level. Debug records which header it was.
  Folder and UID identify a message, and a line at debug names the message the
  user opens, so the person can match the screen to the record. There is no
  fifth level and no constitution amendment.
- Q: Is the server host name written at info? → A: No, at debug. A personal
  domain identifies its owner, and info must be publishable as it is.
- Q: Are durations recorded? → A: No. Every line already carries its time
  with milliseconds, taken when the event happens, and the lines of one
  operation follow each other, so the duration of a step or of a whole
  operation is the difference between two lines, for a failed step as well.
  Revised during implementation: a total on the final line only helped a
  record at level error, which has no first line; the person reporting a
  problem is asked for debug. Timing of decoding in fractions of a millisecond is a matter for a
  benchmark, not for the record. A duration field can be added to any line
  later; the format is not a contract.
- Q: Should Mailbag keep anything about errors while logging is off, such as
  an error file or a short record in memory written out when an error happens?
  → A: No. A person learns about an error from the UI, at once and with
  technical details; that is the error presentation feature, and it does not
  need the log. The log is not a notification. Errors that happen with nobody
  present and cannot be reproduced arrive with background synchronization, and
  a background record is decided then, as an amendment to this spec. FR-001
  stays as it is.
- Q: Does a line name its account by the GNOME Online Accounts identifier or
  by a number given within the record? → A: By the identifier, such as
  `account_1726920000_0`; lines that observe an account add its provider
  type. Revised during implementation: a number such as `account-7 imap` did
  not tell which of ten IMAP accounts a line meant, and the numbers shifted
  when an account was added or removed. The identifier stays the same, and
  the person who owns the record finds it in the Online Accounts
  configuration next to the account's provider and name. A generated
  identifier holds the account's creation time; one from an administrator's
  template or an application's request is written as it is, escaped. 001 is
  amended to allow it.
- Q: A server's status reply can repeat the sign-in name, which is never
  recorded. Is server text still written at debug? → A: Yes. It is often the
  whole diagnosis, and a user attaches the record, not the window. Before
  writing server text, Mailbag replaces every occurrence of the account's
  sign-in name with `<login>`. The rest of the sentence is written as sent,
  and the README says so.
- Q: When does debug record a header whose value did not decode, given that
  the decoder does not report a partial failure? → A: On what can be
  observed: the header line is present but no value came out, or the value
  contains replacement characters. The line names the header and claims no
  cause. Revised during implementation: a description of the raw value's
  encoding was dropped, because the message has to be examined by hand
  anyway.
- Q: Are attachment file names written at debug? → A: No. A debug line names
  each text part the choice of text parts left out as a file, by its section,
  which is all that the choice looks at. Nothing in Mailbag uses the name itself yet;
  its shape and extension arrive with the feature that handles attachments.
  Values of headers and of file names are thus never recorded, without
  exception.
- Q: Is the record written through a queue and a thread of its own, so that a
  stream nobody reads cannot block Mailbag? → A: No. Lines go straight to the
  standard error stream, as GLib's own warnings do in every GNOME
  application. A terminal and a redirection to a file do not block. A pipe
  whose reader has stopped does, and then Mailbag waits until it is read
  again; the person who set up that pipe sees it at once and can undo it.
  The maintainer accepted this reading of constitution V for output that
  exists only on request. A queue, a count of lost lines and a bounded wait
  at quit are not worth their cost for that one case.
- Q: Does the error line repeat the UI's wording of the step and cause? → A:
  It names the same failure values, such as `step=SignIn cause=TimedOut`. The
  UI's wording is rewritten by the error presentation feature; sharing it now
  would mean reworking UI code for the record.

## User Scenarios & Testing

### User Story 1 — Follow a run during development (Priority: P1)

The maintainer or an agent starts Mailbag from a terminal with the level option
set to debug, refreshes an Inbox and reads in the console what happened: which
steps ran, how long they took, which messages were handled unusually and why.

**Why this priority**: Without it, every defect that appears only on a live
server is diagnosed by guessing.
**Independent Test**: Load a test Inbox that contains ordinary messages and
messages with known defects (an unknown character set, a structure the server
cannot describe, text the server does not return). Read the record without
looking at the application window.

**Acceptance Scenarios**:

1. **Given** Mailbag is started without the level option, **when** it runs and
   loads mail, **then** it writes none of its own lines to either standard
   stream and creates no file. GTK and GLib warnings appear as in any GNOME
   application.
2. **Given** the level is info, **when** a load succeeds, **then** the record
   shows the load's steps with their times and message counts, and no
   line is about a single message: Inboxes of 1 and of 100 ordinary messages
   give the same number of info lines.
3. **Given** the level is debug, **when** a message's content could not be
   read, **then** the record has a warning on the load with the number of such
   messages, and a debug line for the message with its folder, UID, failing
   step and cause, and its part tree when the server's description of it could
   be read. These observations guide a synthetic test fixture; the record
   holds no content and does not promise that every defect can be rebuilt from
   it.
4. **Given** the level is debug, **when** the user opens a message, **then** a
   line names its UID, so the person can tell which lines belong to the message
   they see on screen.
5. **Given** a load fails, **then** the record has exactly one error line for
   it, naming the same step and cause the UI explains, and no error lines for
   steps that could not run.
6. **Given** a load is cancelled because its account was excluded or its
   window was closed, **then** the record has an info line and no warning or
   error.

### User Story 2 — Produce a record for an issue report (Priority: P1)

A user whose mail does not load, or whose message shows wrongly, is not a
developer. They follow a short instruction in the README: start Mailbag from a
terminal with the option at debug and the error stream redirected to a file,
reproduce the problem, read the file, attach it to an issue.

**Why this priority**: This is the only way a defect on another person's server
reaches the maintainer in a usable form.
**Independent Test**: Follow the README instruction on the installed Flatpak
build without adding any permission.

**Acceptance Scenarios**:

1. **Given** the installed Flatpak build, **when** the user runs the command
   from the README, **then** the named file appears on the host and contains
   Mailbag's first line (FR-015) and the run's lines. GTK or GLib warnings of
   that run are in the same file, unchanged, and may stand before Mailbag's
   first line.
2. **Given** the instruction, **then** it tells the person what a debug record
   contains (FR-011), so they can read the file before attaching it.
3. **Given** Mailbag is already running, **when** the user runs the command,
   **then** the terminal says that Mailbag is already running and logging was
   not turned on, and tells them to quit Mailbag first. An empty file is never
   the only answer.

### User Story 3 — Trust what a record contains (Priority: P1)

Anyone who turns logging on knows the limits of what the record can hold at the
level they chose, and Mailbag's lines at error, warning and info can be
published as they are. The limits cover Mailbag's own lines. A redirected
stream also holds whatever GTK and GLib printed, which Mailbag does not
control, so the README advises reading any file before sharing it.

**Why this priority**: These limits are permanent. Every later feature writes
its lines inside them.
**Independent Test**: Run loads against a test server whose fixtures contain
known markers: a password, a folder name, a host name, a subject, an address,
an attachment file name and body text. Search the record for the markers at
info and at debug.

**Acceptance Scenarios**:

1. **Given** the level is info, **then** Mailbag's lines contain none of the
   markers.
2. **Given** the level is debug, **then** Mailbag's lines contain no password,
   subject, address or body text, and no sign-in name even when the server's
   reply repeats it, however short that name is, and no attachment file name.
   They may contain the folder name and the host name.

### Edge Cases

| Situation | Required visible result | Basis |
|---|---|---|
| The level option is given while Mailbag is already running | The second start prints that logging was not turned on and that Mailbag must be quit first. The running instance is unchanged | Mailbag is a single-instance application: the second start only raises the existing window, so the person following the README would get an empty file |
| The level option has a value that is not a level, such as `debg` | Mailbag does not start. It prints the accepted levels | A typing mistake would otherwise give a run without a record, found only after reproducing the problem |

## Requirements

### Functional Requirements

**Turning logging on**

- **FR-001 — Off by default**: Without the level option Mailbag MUST NOT write
  any line of its own to the standard streams, to a file or to the system
  journal. Logging on by default is excluded from the target design as well.
  Mailbag MUST NOT suppress or redirect the warnings GTK and GLib print
  themselves.
- **FR-002 — One option**: The command-line option `--log-level=<level>`, called
  the level option in this spec, MUST turn logging on for that run and set its
  level: `error`, `warning`, `info` or `debug`. A level includes the levels
  before it. There is no environment variable, settings switch or
  in-application control. `--help` MUST describe the option. A value that is
  not a level MUST stop the start with a message naming the accepted levels.
  When Mailbag is already running, the second start MUST say that logging was
  not turned on (Edge Cases).
- **FR-003 — One stream**: The whole record MUST go to the standard error
  stream, where GTK and GLib warnings already go, so that one redirection
  captures both. Mailbag MUST NOT write the record to the standard output
  stream. Lines of one thread keep the order of their events. No order is
  promised between events that happen at the same moment on different threads,
  or relative to GTK and GLib warnings.

**Levels**

- **FR-004 — Error**: An operation did not complete and has no result: sign-in
  refused, the secure connection failed, a wait limit ran out, the Inbox load
  failed, the account list could not be read. Each failed operation MUST
  produce exactly one error line, written by whoever gives the operation up
  (the load for an Inbox load, the account observer for a read of the account
  list), naming the same step and cause the UI explains. Steps that could not
  run MUST NOT produce error lines. The step and cause are the failure values
  Mailbag already has, not one protocol's replies, so an HTTP provider fits
  without new rules.
- **FR-005 — Warning**: An operation completed, but its result is worse than
  normal or Mailbag took a workaround: the server refused to finish the message
  list, some messages have content that could not be read, an account needs
  attention in Online Accounts. A warning is something a reader should look at
  even if nobody complained. Content that this version does not support by
  design, such as HTML-only or encrypted mail, is not a warning; it is counted
  at info.
- **FR-006 — Info**: The normal course of work, told about an account and an
  operation: an account appeared or was excluded, a connection was secured,
  sign-in succeeded, the Inbox was opened with N messages, a load finished or
  was cancelled. Cancellation is not an error. No info line is about a single
  message or part. Every connection Mailbag makes is recorded. Reading info alone, a person can retell what
  Mailbag did.
- **FR-007 — Debug**: What happens inside an operation: a message, a part, a
  decision. For a message that fails or is handled unusually, debug MUST record
  its folder and UID, the failing step and its cause, and, when the server's
  description of its parts could be read, the part tree with the fields FR-011
  lists, and the decision that chose the text parts. These observations guide
  a synthetic test fixture. The record holds no content, so it does not
  promise that a defect can be rebuilt from it, and a description that could
  not be read is not written out. Debug's volume grows with the number of
  messages.
- **FR-008 — Times and durations**: Every line carries the time of its event
  with milliseconds (FR-014), so the duration of a step or of a whole
  operation is the difference between the times of two of its lines. No line
  carries a duration of its own.

A failure of one message inside an operation therefore produces two things: a
warning on the operation with a count and no identifiers, and a debug line for
the message. That is why the README asks a user for debug. Two checks catch a
wrong level: a line written at warning during normal work is not a warning; an
error line without a failure shown in the UI is either not an error or a UI
defect.

**Privacy**

Turning logging on is an explicit act, and the person accepts that the record
describes their mailbox. The limits are:

- **FR-009 — Never recorded**: At no level: passwords, tokens and other
  credentials, sign-in names, message bodies, attachment content, and the
  values of message headers, including subjects and addresses, also those of an
  attached message inside a part tree, and attachment file names. For a list
  header that is present but whose value did not come out of decoding, or
  came out with replacement characters, debug records which header it was;
  an absent header is recorded as absent.
- **FR-010 — Publishable levels**: Lines at error, warning and info MUST NOT
  contain folder or label names, message identifiers, file names, server host
  names or addresses, or the text of server replies, so Mailbag's lines at
  these levels can be published as they are. The server's response code, such as
  `AUTHENTICATIONFAILED`, and its capability list are allowed. The host name is
  the one place where privacy overrides FR-006: a line about connecting belongs
  to info, but the host is written only at debug.
- **FR-011 — Debug**: Debug MAY add: folder and label names, UIDs and other
  message identifiers, the server's host, port and address, the message
  structure, and server text. The structure is, for each part: its section
  number, content type, the parameters `charset`, `format` and `delsp`,
  disposition, transfer encoding and size. A text part left out as a file is
  named by its section; never by its file name. Other parameters, the part's free-text description and
  content identifiers are left out. Server text is the text of a
  status reply, meaning the completion of a command or the reply with which
  the server closes a connection, and of an alert. Before it is written, every
  occurrence of the account's sign-in name in it MUST be replaced with
  `<login>`, whatever the name's length. Debug MUST NOT contain any other
  server data, because replies that carry message data contain headers and
  bodies. The README instruction MUST state what a debug record contains.
- **FR-012 — Libraries**: A library's own log output MUST stay off unless this
  spec's owner shows that it stays within FR-009–011. The 002 decision to
  compile out the IMAP library's protocol trace is not reopened.

**What a line carries**

- **FR-013 — Account and operation**: Every line about an account MUST name it
  by its GNOME Online Accounts identifier, which stays the same across runs;
  never by the mail address or the account's display name. Lines that observe
  an account also give its provider type. Every line of an Inbox load MUST
  carry its account. Mailbag runs one load at a time, so the load's first line
  and its account tell its lines apart; an identifier of an operation arrives
  with background synchronization, when loads of several accounts run
  together. A line about a message MUST say which message.
- **FR-014 — Line format**: One event is one line that gives: the time with
  milliseconds and the offset from UTC, the level, where in Mailbag the line
  was written, the account identifier when there is one,
  then the text and its fields. Values that come from mail or from a server
  MUST be escaped so that they cannot break a line. The format is for people
  to read; this spec prescribes what a line contains, not its columns or
  punctuation. It is not a contract for programs, so later features may extend
  it. For illustration only:

  ```text
  2026-09-21T14:03:15.102+03:00  INFO load{account="account_1726920000_0"}: mailbag_imap::session: signed in method="PLAIN"
  2026-09-21T14:03:16.020+03:00  WARN load{account="account_1726920000_0"}: mailbag::inbox: some messages have content that could not be read messages=2 of=100
  ```

- **FR-015 — First line**: Whenever logging is on, at any level, Mailbag's
  first line MUST give: the application version, whether the build is native or
  Flatpak, the runtime or operating system version, the GTK and libadwaita
  versions in use, and the chosen level.

**Other guarantees**

- **FR-016 — Never in the way**: Logging MUST NOT make an operation fail: a
  line that cannot be written is dropped and work continues. Lines are written
  straight to the standard error stream, as GLib's own warnings are; when the
  person has piped the stream to a reader that stopped, Mailbag waits until
  it is read again (Clarifications). With logging off, the cost MUST be
  negligible: nothing is installed and no line is built.
- **FR-017 — Lines of every feature**: Every feature writes its lines under
  these rules, and review checks them in the code; no feature keeps a list of
  its lines. Logging MUST NOT add behavior for the sake of a line. A line that
  a person relies on, such as the error that names a refused authorization,
  is an ordinary requirement of the feature that owns it.
- **FR-018 — User instruction**: The README MUST contain the instruction of
  User Story 2, with the exact command for the Flatpak build and for a native
  build, what a debug record contains, and the advice to read the file before
  attaching it.

### Key Entities

- **Record**: Everything Mailbag wrote during one run with logging on. Its
  first line is FR-015's, and it belongs to the person who started the run.
- **Line**: One event: time, level, where in Mailbag it was written, account
  identifier, text and fields.
- **Level**: error, warning, info or debug, with the meaning FR-004–007 give it
  and the limits of FR-009–011.
- **Account identifier**: The GNOME Online Accounts identifier by which a
  record names an account.

## Success Criteria

### Measurable Outcomes

- **SC-001**: A run without the level option produces zero Mailbag lines on
  both standard streams: a test shows that no subscriber exists, and one
  start of Mailbag shows it on the real streams (FR-001).
- **SC-002**: With known markers in the test fixtures, an automated check finds
  at info none of: password, sign-in name, folder name, host name, subject,
  address, file name, body text; and at debug none of: password, sign-in
  name, subject, address, file name, body text, including those of an
  attached message and a sign-in name of two characters that the test server
  repeats in its refusal. The check reads Mailbag's lines (US3; FR-009–013).
- **SC-003**: For each defective test message (unknown character set, unknown
  transfer encoding, undecodable part, structure the server cannot describe,
  text the server does not return, list header whose value does not come out),
  the debug record alone gives folder and UID; the failing step and cause
  where the code knows one; the part tree where the description could be read;
  and for the header case the header's name, with no cause claimed (US1;
  FR-007, FR-009).
- **SC-004**: Every failed load that 002 can show produces exactly one error
  line whose step and cause are the failure the UI explains; every cancelled
  load produces zero warning and error lines; an incomplete message list and
  unreadable content each produce one warning (FR-004–006).
- **SC-005**: Inboxes of 1 and of 100 ordinary messages produce the same
  number of info lines (FR-006).
- **SC-006**: Every line about an account carries its identifier, every line
  of a load included, also while account observation reports changes during
  the load (FR-013).
- **SC-007**: Following the README on the installed Flatpak build gives a
  file with the first line and the run's lines, without adding a permission. Starting it a second time while
  Mailbag runs prints the explanation instead (US2; FR-002, FR-015, FR-018).
- **SC-008**: With the stream unwritable during a load at debug, the load
  finishes and the window stays usable; with the stream piped to a reader
  that reads, a load at debug finishes without a visible delay (FR-016).

## Assumptions

- The maintainer agreed the decisions in Clarifications on 2026-09-21. The
  choice of a logging mechanism, its coexistence with the compiled-out library
  trace, and how lines leave the working threads belong to the plan.
- Started by the desktop session instead of a terminal, a run's error stream
  normally lands in the system journal. Mailbag does nothing for this, and
  there is no way yet to pass the option to such a start; it arrives with
  background running.
- The UI tells the person about failed actions and persistent problems
  (constitution III). Nobody is expected to find out about a failure from the
  record; it serves diagnosis after the person already knows.
- The person who turns logging on owns the record. Mailbag never reads it back,
  uploads it or sends anything about it over the network.
- 001 is amended: the account adapter gives an identifier's text, and the
  record names accounts by it (FR-013). Addresses and display names stay out
  of diagnostics.
- Writing the text of server status replies at debug changes a 002 design
  rule, which kept server text for the UI only
  ([IMAP reading](../002-imap-integration/contracts/imap-reading.md),
  [data model](../002-imap-integration/data-model.md)). Those documents are
  amended with this feature. 002 FR-009, "no credentials or personal mail in
  diagnostics", stays as it is and is met by FR-009 here.
- Out of scope, besides what Clarifications exclude: saving a message as a
  fixture; a log viewer or a synchronization activity panel, so the approved
  UI layout does not change.
- The [constitution](../../.specify/memory/constitution.md) governs this
  feature. Implementation portions and review pauses will be in the plan.
