# Tasks: Logging

**Feature**: F03 / `003-logging`
**Created**: 2026-09-21 · **Branch**: `claude/logging` · **Status**: Implemented and committed; simplified 2026-09-22, when the spec was reduced to its principles.

[Spec](spec.md) owns the rules every line follows, [plan](plan.md) owns
boundaries and portions, [research](research.md) owns decisions, and
[quickstart](quickstart.md) owns commands and acceptance checks. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one agreed
portion, run its checks, report and stop. A checked handoff does not mean
approval. The maintainer creates commits and PRs. Do not start code before
document approval.

Phases follow the approved portions rather than one phase per story: the
record itself, account events, load events and content events each serve
several stories and ship as separate reviewable commits. Story labels trace
tasks to US1 (follow a run during development), US2 (produce a record for an
issue report) and US3 (trust what a record contains). Tests are part of every
portion under AGENTS.md and live beside their modules.

Nothing committed may mention where an idea came from outside this repository;
state reasons on their own merits.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(logging): specify and plan logging | Logging |
| 1. The record | T002–T012 | feat(logging): record a run on request | Logging |
| 2. Accounts and application | T013–T020 | feat(logging): log account observation | Logging |
| 3. Load and IMAP | T021–T032 | feat(logging): log the Inbox load | Logging |
| 4. Content, privacy and instructions | T033–T042 | feat(logging): log message content decisions and document reporting | Logging |

## Phase 1: documents and review

- [X] T001 STOP: present specs/003-logging/ (spec.md, log-events.md, plan.md, research.md, data-model.md, quickstart.md, contracts/record.md, checklists/requirements.md and this tasks.md) and wait for explicit maintainer approval before any code change. Approved 2026-09-21.

## Phase 2: foundation — the record (portion 1)

**Purpose:** `--log-level` produces a correct, escaped record with
its first and last line, and nothing exists at run time without it. No events
of accounts or mail yet. Serves US1.1, US2.3 and FR-001–003, FR-014–016.
**Independent check:** escaping, option, label and failing-writer tests, and
the native manual runs of quickstart.md, without touching account or mail code.

- [X] T002 Repeat the check recorded in specs/003-logging/research.md §1 with the releases of `tracing` and `tracing-subscriber` current at that time (probed on 2026-09-21: 0.1.44 and 0.3.23): `tracing` with `default-features = false, features = ["std"]`, `tracing-subscriber` with `default-features = false, features = ["std", "registry", "fmt"]`; neither pulls `tracing-log`, ANSI color crates or `log`; every new crate's license passes deny.toml and ships its license files; string fields are written escaped and each event is one `write` of one line. Record the pinned versions in research.md §1; record any departure there and stop for a decision instead of changing it silently.
- [X] T003 Declare `tracing` in crates/mailbag/Cargo.toml, crates/mailbag-imap/Cargo.toml, crates/mailbag-content/Cargo.toml and crates/goa-adapter/Cargo.toml through a `[workspace.dependencies]` entry in the root Cargo.toml, and `tracing-subscriber` in crates/mailbag/Cargo.toml only. Leave mailbag-imap's `log` dependency with `max_level_off` and `release_max_level_off` unchanged. Update Cargo.lock, regenerate cargo-sources.json with scripts/generate-cargo-sources.sh, and add notices for any new crate that ships none, following specs/002-imap-integration/contracts/packaging.md.
- [X] T004 [P] Extend scripts/check.sh: fail when `tracing-log` appears in `cargo tree --locked --package mailbag --edges normal`; fail when crates/mailbag-imap/Cargo.toml's `log` dependency lacks `max_level_off` or `release_max_level_off` (research.md §1); reject a field passed with the `%` sigil in an event or span macro through Clippy's `disallowed-methods` in a root clippy.toml, since the sigil expands to `tracing::field::display`; a `%` operator, comment or string is not a finding (research.md §9).
- [X] T005 [US1] Create crates/mailbag/src/logging.rs with `LogLevel` (error, warning, info, debug), `parse_log_level` returning the message of contracts/record.md for an unknown value, and the mapping to `tracing` levels (`warning` selects `WARN`). Add crates/mailbag/src/logging/tests.rs with the four levels and rejected values such as `debg`, an empty value and `trace`.
- [X] T006 [US3] Prove escaping in crates/mailbag/src/logging/tests.rs (research.md §9, contracts/record.md rule 3): with the library's one-line formatter writing into a buffer, log a folder name and a server sentence that contain line breaks, quotes and a NUL as plain string fields and assert one line per event with the characters escaped. If the pinned formatter does not escape a string field this way, stop and report instead of adding a formatter of our own.
- [X] T007 [US1] Add `LocalTime` to crates/mailbag/src/logging.rs: the formatter's timer, writing `glib::DateTime::now_local()` with milliseconds and the UTC offset (research.md §4). Configure the formatter in one function: one line per event, colors off, module path shown, span fields shown. Test a line inside a `load` span with a nested `message` span: the account and the UID appear on the line.
- [X] T008 [US2] Give the formatter its output in crates/mailbag/src/logging.rs (research.md §3): the standard error stream in the application, a buffer in tests, passed as a parameter. A write that fails is ignored. No queue, no writer thread and no count of lost lines. Test in crates/mailbag/src/logging/tests.rs with an output that returns an error on every write: events are still accepted and nothing panics (SC-008, FR-016).
- [X] T009 [US2] Add `start_logging(level, output)` and `finish_logging()` to crates/mailbag/src/logging.rs: write the first line of contracts/record.md "First line" to the output before installing the subscriber, so that no level filters it, beginning with the time from `LocalTime` and the fixed word `INFO` — version from `CARGO_PKG_VERSION`, `Flatpak build` with the `runtime` key of `/.flatpak-info` when that file exists, otherwise `native build` with GLib's OS pretty name, GTK and libadwaita versions from their runtime functions, and the level (research.md §4); then install the registry with the level filter and the formatter of T007 over the output of T008 as the global subscriber. `finish_logging` writes the quit line at info. Test the first line's fields with a buffer, that it appears at level `error`, and that without `start_logging` no subscriber is set (SC-001).
- [X] T010 [US2] Wire the option in crates/mailbag/src/main.rs (research.md §2): declare `--log-level` with `add_main_option` so `--help` lists the four levels; in `handle-local-options` return "continue" without the option; on an unknown value print the message to the standard error stream and return exit status 1; otherwise register the application and, when `is_remote()`, print the "already running" message of contracts/record.md and return 1 without activating the running instance; otherwise call `start_logging` with the standard error stream. Call `finish_logging` after the application's run returns. Without the option no subscriber may exist (FR-001).
- [X] T011 [US1] Add `next_load_operation()` and `account_label(account_id, provider)` to crates/mailbag/src/logging.rs (research.md §5, data-model.md): one process-wide counter giving `load-N`; the label is `account-N` followed by the provider type, with `N` given at an account's first appearance within the run and repeated for the same account afterwards. Test a first and a repeated appearance, that the same set of accounts met in identifier order gets the same numbers, and that no identifier's text, generated or arbitrary, appears in a label. `goa-adapter` is not changed. Never pass an `AccountId` to an event as a field (contracts/record.md rule 5). Portion 2 replaced the label by the Online Accounts identifier (research.md §5) and removed `account_label`; the load identifier was removed as well, because one load runs at a time (spec FR-013).
- [X] T012 STOP: run the portion's tests and ./scripts/check.sh, including a temporary `tracing-log` dependency that must fail and is reverted; run ./scripts/build-flatpak.sh with Cargo offline in the build sandbox; run the native manual checks of quickstart.md, including SC-001 on real stdout and stderr of a start without the option, then `info`, `debg`, a second start and a pipe; run git diff --check. For this portion expect only the first line and the quit line when logging is on; account and load event checks belong to their later portions. Review constitution I/II, report what changed, the evidence and limitations outside the repository, suggest the commit, and wait before portion 2.

## Phase 3: accounts and application (portion 2)

**Goal:** a record shows account observation, account problems, exclusion,
discarded mail, opening Settings and opening a message (US1.4, SC-006 for
account lines).
**Independent check:** the private GOA fixture and a capture buffer, without a
mail server.

- [X] T013 [US1] Before writing events, read crates/goa-adapter/src/client.rs, accounts.rs and account_model.rs and crates/mailbag/src/accounts.rs, account_ui.rs and settings.rs, and compare them with the "Application" and "Account observation" sections of specs/003-logging/log-events.md. Correct every row that disagrees with the code, especially the rows about an unavailable Mail service and recovery after a failed read (plan.md review point 6). The list follows the code; do not add behavior for a row.
- [X] T014 [US1] Log reads of the account list in crates/goa-adapter/src/client.rs where a read completes (`finish_read`), as log-events.md gives them: info for a completed read with the number of accounts; one error line for a failed read with the read and `cause` as the name of the `ErrorCause` value (`Unavailable`, `AccessDenied`, `Timeout`, `InvalidReply`), the value the UI explains, not the UI's wording; debug where a change signal arrives, with the kind of signal. No span for a read. A result published again while a Retry is running is not a new failure: write nothing for it. In crates/mailbag/src/accounts.rs or its caller log info for Retry Check when the user asks for it.
- [X] T015 [US1] Log account changes where crates/mailbag/src/accounts.rs applies an update: info for an account that appeared (label, provider type), info for an account that was removed, had Mail disabled or is unsupported (label, `reason`), warning for an account that needs attention or whose Mail service is unavailable (label, which), info when that problem is gone, info with label and `reason` for each account not shown, at every complete read (log-events.md, as corrected by T013). Never write the address, display name or icon.
- [X] T016 [US1] Log discarded mail in crates/mailbag/src/inbox.rs `discard_excluded`: info with the account identifier and the number of messages, only when mail of an excluded account is really discarded.
- [X] T017 [P] [US1] Log opening Online Accounts in crates/mailbag/src/settings.rs and its caller: info on success, one error line with `cause` as the name of the `LaunchError` value (`Unavailable`, `AccessDenied`, `Timeout`, `InvalidReply`), the value `LaunchError::message` explains; the message's wording is not logged or changed.
- [X] T018 [P] [US1] Log the opened message in crates/mailbag/src/mail_ui.rs at debug with `uid` and the account identifier, and nothing else about the message (US1.4).
- [X] T019 [US3] Add tests beside the modules above using the private GOA fixture and a capture buffer: a line for each row of T014–T016, exactly one ERROR for a failed read and none for a result published again during Retry, no WARN or ERROR for a normal read, and no marker address or display name of the fixture accounts anywhere in the buffer at debug (FR-013). The AGENTS.md line this task first added was removed with FR-017's list of events per feature.
- [X] T020 STOP: run the portion's tests and ./scripts/check.sh; run Mailbag natively at `info` and `debug`, add and remove an account in Online Accounts and compare the lines with log-events.md; run git diff --check. Review constitution I/II, report, suggest the commit, and wait before portion 3.

## Phase 4: load and IMAP (portion 3)

**Goal:** a record shows every step of an Inbox load, its
single error line, its warnings, reconnections, part trees and server text
with the sign-in name replaced (US1.2, US1.3, US1.5, US1.6, SC-004–006).
**Independent check:** the scripted IMAP server of `mailbag-imap` and a
capture buffer.

- [X] T021 [US1] In crates/mailbag/src/inbox.rs let `InboxController::begin_load` create a `load` span with `account` from the account's identifier (`AccountId::as_str`) only for an accepted load, and keep it in `RunningLoad` while logging is on (contracts/record.md "Context fields"). In window_ui.rs enter this span when calling the loader; the controller enters it itself when a result arrives. In inbox_load.rs capture the caller's span in `MailLoader::start_load`, enter it in the `request_imap_access` completion callback and hand a clone to the worker with `LoadRequest`; attach it to the load's future with `Instrument`. The controller uses the same span for its cancellation and result decisions. Make the worker use its starting thread's dispatcher (`get_default` before spawning, `with_default` in `run_worker`, research.md §8). Test propagation through the worker and the result. The Online Accounts callback is checked in the native run of T032: mailbag's tests cannot start a `GoaAdapter`, and opening one for them was not worth a test-support feature. Ensure the account context survives at every enabled level, including `error`.
- [X] T022 [US1] Write starts, accepted completion summaries and warnings in crates/mailbag/src/inbox.rs `InboxController`, inside the load's span: messages received and unsupported, unreadable-content count and refused-list rows/code as log-events.md specifies. In inbox_load.rs keep debug observations of disappearing UIDs where they are known; carry no count for the log and leave the application's load result unchanged (data-model.md). Follow contracts/record.md "Load outcomes and cancellation": `discard_excluded` and `cancel_load` record one cancellation with the known reason before dropping an active handle; `finish_load` records a discarded late batch or failure at the existing applicability decision and produces no completion, warning or error for it. A cancellation acknowledgement is silent. Write the cancellation where it happens and do not change the shutdown order for the record: closing the window already cancels the load before the quit line; the Quit action does not cancel it. Preserve the existing rules for applying results; no second load state machine.
- [X] T023 [US1] Write the load's single error line in crates/mailbag/src/inbox.rs only when `InboxController` accepts a `LoadFailure`: `step` and `cause`, `code` when present, the number of `alerts` when there are any, including Inbox changed and worker stopped. `step` and `cause` are the names of the existing failure values (`ImapStep`, `ImapFailure`, `ImapAccessError`, `LoadFailure::WorkerStopped`), for example `step=SignIn cause=TimedOut` or `cause=NoEncryption`, written as plain string fields from one small function in inbox.rs; never a value's `Debug` of anything that holds server text. crates/mailbag/src/window_ui.rs and its wording are not changed. `inbox_load`, `mailbag-imap` and `mailbag-content` write no load error line. A failed step adds no line of its own, and no line carries a duration (contracts/record.md "Durations").
- [X] T024 [US1] Log access to settings and password in crates/mailbag/src/inbox_load.rs, in the completion callback of `request_imap_access` inside the load's span: info with `encryption` on success; host and port are on the connection's debug line (T026); on failure write nothing and report the unchanged failure to the controller, which writes the load's error line. Cancellation writes no second line: the controller already recorded it (T022). crates/goa-adapter/src/imap_access.rs writes no line. Never the sign-in name or password.
- [X] T025 [US3] Add `server_text_for_log(sign_in_name, text)` to crates/mailbag-imap/src/session.rs (research.md §6): replace every occurrence of the sign-in name with `<login>`, ignoring ASCII case, whatever the name's length. Test a refusal that repeats the name, a different case, several occurrences, a name that does not occur, and a two-character name that also occurs inside ordinary words. No other function turns server text into a field (contracts/record.md rule 6).
- [X] T026 [US1] Log connection and sign-in in crates/mailbag-imap/src/transport.rs and session.rs: info "connected" for every real connection, debug with `host`, `port` and `address`; info "connection secured" with `encryption` and `tls`; debug with `tls_error`, GIO's text for the failure, and `certificate_errors`, the names of the GIO certificate flags, where the handshake fails, without any certificate field (research.md §7); info with `capabilities` as already received, no added command; info "signed in" with `method`. Each alert as it arrives: info that it arrived, debug its text through `server_text_for_log` (T029). A failed step returns its failure and writes no line of its own.
- [X] T027 [US1] Log Inbox reading in crates/mailbag-imap/src/reader.rs and session.rs: info "Inbox opened" with `messages`, debug with `folder`, `uid_validity`, `uid_next`; info "message list loaded" with `rows`, debug with the UID range; info "part structures loaded" with `messages`; info "reconnecting after a structure that could not be read", one line per reconnection and no counter for it; info "text loaded" with `messages` and `commands`; debug per command group with sections and `uids`, on success or failure; debug per message for text not returned and for a message that disappeared. A failed step writes no line of its own. The raw list header lines and received parts are never fields.
- [X] T028 [US1] Log part trees in crates/mailbag-imap/src/part_tree.rs before projection: at debug, one line per part inside a `message` span with `uid`, and `section`, `content_type`, `charset`, `format`, `delsp`, `disposition`, `transfer_encoding` and `size`; nothing about file names, whose rule belongs to `mailbag-content` (research.md §7). For an unreadable structure, reader.rs writes the UID and whether the server refused or parsing failed. Never log an attached envelope, part description, content identifier or other parameter values. `MessagePart`, `MimePart` and crate dependencies do not change.
- [X] T029 [US3] Write the debug lines with `server_text` and `alert` in crates/mailbag-imap/src/session.rs and reader.rs, each through `server_text_for_log`: where a failure is built (`ServerNotices::error`, which every failed step passes through, with the reply of a refused sign-in or failed command and a closing reply), where an alert arrives (`ServerNotices::keep`), and where a FETCH is refused (`InboxReader::fetch`, which the message list, structures and text share). The reply travels to the load and the UI unchanged; add no field or type that carries a sanitized copy across the crate boundary. The controller's warning and error lines in crates/mailbag/src/inbox.rs carry the response code only.
- [X] T030 [US3] Apply the wording of specs/003-logging/contracts/record.md "Changes to 002 documents" to specs/002-imap-integration/contracts/imap-reading.md and specs/002-imap-integration/data-model.md, and reword the doc comment on `ImapError`'s `Debug` implementation in crates/mailbag-imap/src/lib.rs; the implementation keeps leaving the text out.
- [X] T031 [US1] Add capture tests with the scripted server in crates/mailbag-imap/src/tests/, crates/mailbag/src/inbox_load/tests.rs and controller/window tests. Each failed load still applicable to its account gives one ERROR whose `step` and `cause` name the failure the UI explains; refused lists and unreadable content give one WARN each. Exercise exclusion and closing the window during access acquisition and transfer: one cancellation INFO with the right reason and load context, no WARN/ERROR, no second line on the worker's acknowledgement. A late batch or failure for an excluded account is discarded without completion/WARN/ERROR. Check the existing synchronous-result path. Inboxes of 1 and 100 ordinary messages have equal INFO counts. No line has a `duration_ms`. Load context persists during account updates (SC-004–006), and the sign-in name, password, host at info and raw headers never appear.
- [X] T032 STOP: run the portion's tests and ./scripts/check.sh; run Mailbag natively at `info` and at `debug` against a real Generic IMAP account and compare the lines with log-events.md, confirming by eye that the info record has no folder, host, UID or address; run git diff --check. Review constitution I/II, report, suggest the commit, and wait before portion 4.

## Phase 5: content, privacy and instructions (portion 4)

**Goal:** a record explains how each message's text was chosen and decoded;
the privacy limits are checked over the whole path; a user can follow the
README on the installed Flatpak (US2, US3, SC-001–003, SC-007, SC-008).
**Independent check:** MIME fixtures with marker strings, the scripted server
and the installed application.

- [X] T033 [US1] Log part selection in crates/mailbag-content/src/lib.rs `select_text_parts`: debug with the selected `sections`, or with the `explanation` when none is selected (no plain text, encrypted, S/MIME); and a debug line at each decision where the walk makes it: the `alternative` chosen, the last with plain text, and the root of a related set with `start_matched` saying whether its `start` named a part. Never write a content identifier. Where `visit` skips a text part that has a file name and no inline disposition, write debug with its section, never the name; no list is collected for it. The function has no UID; it comes from a `message` span with `uid` that crates/mailbag/src/inbox_load.rs opens, only when debug is enabled, around each of its three calls into `mailbag-content` for one message: `select_text_parts` in the selection loop, the decoding of received text in the `fetch_text` callback, and `decode_display_fields` where the rows are assembled (T034 and T035 rely on the same span).
- [X] T034 [US1] Log decoding in crates/mailbag-content/src/lib.rs `decode_text_part`: debug for a decoded part with `charset`, `transfer_encoding`, `flowed` and `characters_out`; debug for a part that could not be decoded with `cause`, the explanation the reader shows, which names an unknown character set or encoding. No section: parts are decoded in the order of the selected sections, and the part tree gives each part's character set and transfer encoding. Never the decoded or raw text.
- [X] T035 [US3] In crates/mailbag-content/src/lib.rs `decode_display_fields` write debug with `header` when a present header yielded no value or a value containing U+FFFD, claiming no cause; an absent header is normal and writes nothing. Nothing of the value. Test a header that yields no value, one with replacement characters, that an absent header writes no line and absence of header values from the record.
- [X] T036 [US3] Put marker strings into the scripted server scenarios of crates/mailbag-imap/src/test_server.rs (`FixtureMessage::with_private_markers`, `PRIVATE_MARKERS`); the MIME fixtures under tests/fixtures/mime/ keep the texts their tests expect: a password, a sign-in name, a folder name, a host name, a subject, an address, an attachment file name in Content-Type and one in Content-Disposition, body text, the subject and address of an attached `message/rfc822`, a part description, a content identifier, and a sign-in refusal that repeats a two-character sign-in name. Use synthetic values only.
- [X] T037 [US3] Add the privacy check over the whole path in crates/mailbag/src/inbox_load/tests.rs (SC-002): run loads at `info` and at `debug` into a capture buffer; at info none of the markers may appear; at debug the password, the sign-in name (the two-character name a refusal repeats is checked by mailbag-imap's record test), subject, address, file name, body text, the attached message's subject and address, the part description and the content identifier may not appear, while the folder and host may.
- [X] T038 [US1] Add the defect check in crates/mailbag/src/inbox_load/tests.rs (SC-003): for each defective fixture — unknown character set, unknown transfer encoding, undecodable part, unreadable structure, text not returned, list header with no value — assert that the debug record gives the folder and `uid`, the failing step and cause where the code knows one, the part tree where the description was read, and for the header case the header's name with no cause. For a text part with a file name assert the debug line that it was left out as a file and that no name appears (FR-011).
- [X] T039 [US2] Add a "Reporting a problem" section to README.md (FR-018): the exact command for the Flatpak build (`flatpak run io.github.mitinand.Mailbag --log-level=debug 2> mailbag.log`) and for a native build, that Mailbag must not be running already, what a debug record contains (Online Accounts identifiers of accounts, also at other levels, folder and label names, message identifiers, the server's host, server replies with the sign-in name replaced) and what it never contains (passwords, addresses, subjects, attachment file names, message text), that GTK's own warnings share the file, and the advice to read the file before attaching it.
- [X] T040 [US3] Check every line the code writes against the spec's rules (FR-004–013): each is covered by a test or seen in the acceptance run. log-events.md is a working file and is not brought in step with the code.
- [X] T041 [US2] Run the implementer's checks in specs/003-logging/quickstart.md: SC-001 on the started application's two streams, all native scenarios, and installed Flatpak output following the README, second start and unchanged permissions (SC-007). If no installed environment is available, leave the corresponding criterion and this task incomplete and report the limitation at T042. Record results and remaining gaps in the handoff message or an artifact outside the repository; quickstart.md remains the reusable procedure, not a validation report.
- [X] T042 STOP: run ./scripts/check.sh and git diff --check. Review constitution I/II over the whole feature, report what changed, the evidence per success criterion and the remaining limitations, suggest the commit and the PR description, and wait for the maintainer.

## Simplification after implementation, 2026-09-22

The maintainer had the finished feature simplified before its commit. The tasks
below are the record of what was implemented and are not rewritten; what the
simplification changed:

- one line per event: the info and debug pairs of `mailbag-imap` became single
  lines, and the connection's host and port moved to a debug line written
  before the attempt (T026, T027);
- the `load` span was removed: one load runs at a time, so the controller's own
  lines name the account and the lines inside a load do not (T021, T022);
- the load's error line names the failure value in one `cause` field, so the
  `step` and cause name tables of `inbox.rs` are gone (T022);
- the line about a list header that did not decode was dropped, and with it
  the `header` field (T035, T038); the lines about opening Settings were
  dropped as well (T017);
- the tests keep the guarantees and no longer pin the text of lines: the record
  tests of `goa-adapter` and of account observation were deleted, and those of
  `mailbag-imap`, the controller and the loader reduced (T031, T037, T038);
  SC-003 and SC-005 stopped being criteria, and SC-006 is deferred;
- `log-events.md` and `data-model.md` were deleted, spec.md's Clarifications
  moved their reasons into `research.md`, and `contracts/record.md` was reduced
  to what several crates must agree on;
- on the same day the spec was reduced to the principles every feature follows,
  its User Story 3 becoming requirements FR-009–012, and `contracts/record.md`
  was removed: its command line, its field names and its rules for writing
  events are now in the spec, and the full field table is only in the code.

The story and criterion labels in the phase goals below, such as US1.4 or
SC-003, name the spec as it stood when the tasks were written (2026-09-21).

## Dependencies

- T001 blocks everything.
- Portion 1 (T002–T012) blocks portions 2–4: they need the formatter, the labels
  and the capture buffer.
- Portion 3 follows the reviewed portion 2, reusing its account naming and
  extending the same `inbox.rs` controller that T016 instruments.
- Portion 4 adds the message spans in T033 and completes the marker check that
  portions 2 and 3 begin.
- Inside a portion, tasks marked [P] touch files no other open task of that
  portion touches.

## Parallel opportunities

- Portion 1: T004 beside T005–T011.
- Portion 2: T017 and T018 beside T014–T016.
- Portion 3: T025–T028 (`mailbag-imap`) beside T021–T024 (`mailbag`).
- Review pauses (T012, T020, T032, T042) are never parallel with anything.

## Implementation strategy

Portion 1 alone is already useful: a developer sees a correct first line,
GTK's warnings and the quit line, and every rule about output is proven before
any event exists. Each later portion adds lines for one area and proves its
own levels and privacy, so a review never has to judge format, output and
content at once. There is no smaller slice of a portion that leaves the
record truthful: a portion's lines and its tests land together.
