# Tasks: Logging

**Feature**: F03 / `003-logging`
**Created**: 2026-09-21 · **Branch**: `claude/logging` · **Status**: Documents approved 2026-09-21; no code yet. Portion 1 is next.

[Spec](spec.md) owns behavior, [log events](log-events.md) own which line each
step writes, [plan](plan.md) owns boundaries and portions,
[research](research.md) owns decisions, [the record contract](contracts/record.md)
owns the line format, field names and rules for writing events, and
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
| 1. The record | T002–T013 | feat(logging): record a run on request | Logging |
| 2. Accounts and application | T014–T021 | feat(logging): log account observation | Logging |
| 3. Load and IMAP | T022–T033 | feat(logging): log the Inbox load | Logging |
| 4. Content, privacy and instructions | T034–T043 | feat(logging): log message content decisions and document reporting | Logging |

## Phase 1: documents and review

- [X] T001 STOP: present specs/003-logging/ (spec.md, log-events.md, plan.md, research.md, data-model.md, quickstart.md, contracts/record.md, checklists/requirements.md and this tasks.md) together with the amended sentence in specs/001-goa-account-observation/research.md, and wait for explicit maintainer approval before any code change. Approved 2026-09-21.

## Phase 2: foundation — the record (portion 1)

**Purpose:** `--log-level` produces a correct, bounded, escaped record with
its first and last line, and nothing exists at run time without it. No events
of accounts or mail yet. Serves US1.1, US2.3, US3.4 and FR-001–003, FR-014–016.
**Independent check:** escaping, option and stalled-writer tests, and the native
manual runs of quickstart.md, without touching account or mail code.

- [ ] T002 Repeat the check recorded in specs/003-logging/research.md §1 with the releases of `tracing` and `tracing-subscriber` current at that time (probed on 2026-09-21: 0.1.44 and 0.3.23): `tracing` with `default-features = false, features = ["std"]`, `tracing-subscriber` with `default-features = false, features = ["std", "registry", "fmt"]`; neither pulls `tracing-log`, ANSI color crates or `log`; every new crate's license passes deny.toml and ships its license files; string fields are written escaped and each event is one `write` of one line. Record the pinned versions in research.md §1; record any departure there and stop for a decision instead of changing it silently.
- [ ] T003 Declare `tracing` in crates/mailbag/Cargo.toml, crates/mailbag-imap/Cargo.toml, crates/mailbag-content/Cargo.toml and crates/goa-adapter/Cargo.toml through a `[workspace.dependencies]` entry in the root Cargo.toml, and `tracing-subscriber` in crates/mailbag/Cargo.toml only. Leave mailbag-imap's `log` dependency with `max_level_off` and `release_max_level_off` unchanged. Update Cargo.lock, regenerate cargo-sources.json with scripts/generate-cargo-sources.sh, and add notices for any new crate that ships none, following specs/002-imap-integration/contracts/packaging.md.
- [ ] T004 [P] Extend scripts/check.sh: fail when `tracing-log` appears in `cargo tree --locked --package mailbag --edges normal`; fail when crates/mailbag-imap/Cargo.toml's `log` dependency lacks `max_level_off` or `release_max_level_off` (research.md §1); fail when an event macro (`error!`, `warn!`, `info!`, `debug!`, `trace!`, `event!`, `span!` and the `*_span!` macros) in crates/*/src contains a `%` sigil (research.md §9).
- [ ] T005 [US1] Create crates/mailbag/src/logging.rs with `LogLevel` (error, warning, info, debug), `parse_log_level` returning the message of contracts/record.md for an unknown value, and the mapping to `tracing` levels (`warning` selects `WARN`). Add crates/mailbag/src/logging/tests.rs with the four levels and rejected values such as `debg`, an empty value and `trace`.
- [ ] T006 [US3] Prove escaping in crates/mailbag/src/logging/tests.rs (research.md §9, contracts/record.md rule 3): with the library's one-line formatter writing into a buffer, log a folder name and a server sentence that contain line breaks, quotes and a NUL as plain string fields and assert one line per event with the characters escaped. If the pinned formatter does not escape a string field this way, stop and report instead of adding a formatter of our own.
- [ ] T007 [US1] Add `LocalTime` to crates/mailbag/src/logging.rs: the formatter's timer, writing `glib::DateTime::now_local()` with milliseconds and the UTC offset (research.md §4). Configure the formatter in one function: one line per event, colors off, module path shown, span fields shown. Test a line inside a `load` span with a nested `message` span: the account label, the operation identifier, folder and UID all appear on the line.
- [ ] T008 [US2] Add `QueueWriter` and the writer thread to crates/mailbag/src/logging.rs (research.md §3, data-model.md): the formatter's writer puts each finished line into a bounded queue with `try_send` (1,024 lines, an internal constant) and counts a line that does not fit; a thread named `mailbag-log` writes lines to the output it was given, writes `N log lines were lost` before the next line when the count is not zero, counts a failed write as a lost line and continues, and ends when the queue closes. The output is a parameter (`Box<dyn Write + Send>`), so tests pass a buffer. No separate state type for writing and losing.
- [ ] T009 [US2] Test the writer in crates/mailbag/src/logging/tests.rs with an output that blocks until released: push more than 1,024 lines, confirm `push` never blocks, release the output and find the lost-lines line with the exact number; test an output that returns an error on every write and confirm nothing panics and the count grows (SC-008, FR-016).
- [ ] T010 [US2] Add `start_logging(level, output)` and `finish_logging()` to crates/mailbag/src/logging.rs: install the registry with the level filter and the formatter of T007 over the writer of T008 as the global subscriber, and put the first line of contracts/record.md "Service lines" straight into the queue, so that no level filters it — version from `CARGO_PKG_VERSION`, `Flatpak build` with the `runtime` key of `/.flatpak-info` when that file exists, otherwise `native build` with GLib's OS pretty name, GTK and libadwaita versions from their runtime functions, and the level (research.md §4). `finish_logging` writes the quit line at info, closes the queue and waits a short bounded time (one second, an internal constant) for the writer, without a timer thread. Test the first line's fields with a buffer, that it appears at level `error`, and that without `start_logging` no subscriber is set (SC-001).
- [ ] T011 [US2] Wire the option in crates/mailbag/src/main.rs (research.md §2): declare `--log-level` with `add_main_option` so `--help` lists the four levels; in `handle-local-options` return "continue" without the option; on an unknown value print the message to the standard error stream and return exit status 1; otherwise register the application and, when `is_remote()`, print the "already running" message of contracts/record.md and return 1 without activating the running instance; otherwise call `start_logging` with the standard error stream. Call `finish_logging` on shutdown. Without the option no subscriber and no thread may exist (FR-001).
- [ ] T012 [US1] Add `AccountId::generated_id(&self) -> Option<&str>` to crates/goa-adapter/src/account_model.rs, as specs/001-goa-account-observation/contracts/accounts.md defines it: the identifier's text when it is `account_` followed by digits, `_` and digits, `None` for any other form; keep `as_str` private to the crate. Test generated identifiers, an arbitrary one, and near misses such as `account_1_`, `account__1` and `Account_1_0`. Add `next_load_operation()` and `account_label(account_id, provider)` to crates/mailbag/src/logging.rs (research.md §5, data-model.md): one process-wide counter giving `load-N`; the label is `generated_id()` followed by the provider type, or `account-N` numbered by first appearance within the run when there is none. Test generated, arbitrary and repeated identifiers, and that an arbitrary identifier's text never appears in the label. Never pass an `AccountId` to an event as a field (contracts/record.md rule 5).
- [ ] T013 STOP: run the portion's tests and ./scripts/check.sh, including a temporary `tracing-log` dependency that must fail and is reverted; run ./scripts/build-flatpak.sh with Cargo offline in the build sandbox; run the native manual checks of quickstart.md for no option, `info`, `debg`, a second start and `2>&1 | less`; run git diff --check. Review constitution I/II, report what changed, the evidence and limitations, suggest the commit, and wait before portion 2.

## Phase 3: accounts and application (portion 2)

**Goal:** a record shows account observation, account problems, exclusion,
discarded mail, opening Settings and opening a message (US1.4, SC-006 for
account lines, FR-017's rule in AGENTS.md).
**Independent check:** the private GOA fixture and a capture buffer, without a
mail server.

- [ ] T014 [US1] Before writing events, read crates/goa-adapter/src/client.rs, accounts.rs and account_model.rs and crates/mailbag/src/accounts.rs, account_ui.rs and settings.rs, and compare them with the "Application" and "Account observation" sections of specs/003-logging/log-events.md. Correct every row that disagrees with the code, especially the rows about an unavailable Mail service and recovery after a failed read (plan.md review point 5). The list follows the code; do not add behavior for a row.
- [ ] T015 [US1] Log reads of the account list in crates/goa-adapter/src/client.rs where a read completes (`finish_read`), as log-events.md gives them: info for a completed read with the number of accounts by provider type and `duration_ms`; one error line for a failed read with the read and its cause in the UI's words; info when a successful read follows a failed one; debug where a change signal arrives, with the kind of signal. No span and no operation identifier for a read (contracts/record.md). A result published again while a Retry is running is not a new failure: write nothing for it. In crates/mailbag/src/accounts.rs or its caller log info for Retry Check when the user asks for it.
- [ ] T016 [US1] Log account changes where crates/mailbag/src/accounts.rs applies an update: info for an account that appeared (label, provider type), info for an account that was removed, had Mail disabled or is unsupported (label, `reason`), warning for an account that needs attention or whose Mail service is unavailable (label, which), info when that problem is gone, info for the number of accounts hidden because their Mail service is unavailable. Never write the address, display name or icon.
- [ ] T017 [US1] Log discarded mail in crates/mailbag/src/inbox.rs `discard_excluded`: info with the account label and the number of messages, only when mail of an excluded account is really discarded.
- [ ] T018 [P] [US1] Log opening Online Accounts in crates/mailbag/src/settings.rs and its caller: info with `duration_ms` on success, one error line with the cause (`unavailable`, `access denied`, `timeout`, `invalid reply`) matching `LaunchError::message`.
- [ ] T019 [P] [US1] Log the opened message in crates/mailbag/src/mail_ui.rs at debug with `folder` and `uid` and the account label, and nothing else about the message (US1.4).
- [ ] T020 [US3] Add tests beside the modules above using the private GOA fixture and a capture buffer: a line for each row of T015–T017, exactly one ERROR for a failed read and none for a result published again during Retry, no WARN or ERROR for a normal read, and no marker address or display name of the fixture accounts anywhere in the buffer at debug. Add one line to AGENTS.md under Documentation: every feature records its log events — logged or not, level and fields — in its spec, under specs/003-logging/spec.md FR-017.
- [ ] T021 STOP: run the portion's tests and ./scripts/check.sh; run Mailbag natively at `info` and `debug`, add and remove an account in Online Accounts and compare the lines with log-events.md; run git diff --check. Review constitution I/II, report, suggest the commit, and wait before portion 3.

## Phase 4: load and IMAP (portion 3)

**Goal:** a record shows every step of an Inbox load with durations, its
single error line, its warnings, reconnections, part trees and server text
with the sign-in name replaced (US1.2, US1.3, US1.5, US1.6, SC-004–006).
**Independent check:** the scripted IMAP server of `mailbag-imap` and a
capture buffer.

- [ ] T022 [US1] In crates/mailbag/src/inbox_load.rs open a `load` span in `start_load` with `account` from `account_label` and `operation` from `next_load_operation`, covering the Online Accounts request and the transfer. Make the mail worker use the dispatcher of the thread that started it (`tracing::dispatcher::get_default` captured before `thread::Builder::spawn`, `with_default` inside `run_worker`), and attach the span to the load's future with `Instrument` (research.md §8). Test that lines written on the worker thread reach a test's buffer and carry the span's fields.
- [ ] T023 [US1] Write the load's own lines in crates/mailbag/src/inbox_load.rs as the "Inbox load" section of log-events.md gives them: info when a load starts; info when it finishes with messages received, messages with unsupported content (no plain text, encrypted, S/MIME), messages that disappeared and the total `duration_ms`; debug with the UIDs that disappeared; warning with the count of messages whose content could not be read (unknown character set, unknown transfer encoding, undecodable part, unreadable structure, text not returned); warning with rows received and `code` when the server refused to finish the list; info when cancelled with the reason; info when a late result for an excluded account is dropped.
- [ ] T024 [US1] Write the load's single error line in crates/mailbag/src/inbox_load.rs for every `LoadFailure`: `step` and `cause` in the same words the UI's explanation uses, `wait_limit_s` for a timeout, `code` when the server gave one; "Inbox changed" and "worker stopped" as causes. No other code writes an error line for a load (contracts/record.md rule 1). Share the step and cause wording with crates/mailbag/src/mail_ui.rs through one function instead of two copies (constitution IV).
- [ ] T025 [P] [US1] Log access to settings and password in crates/goa-adapter/src/imap_access.rs: info with `duration_ms` and `encryption` when both were received, debug with `host` and `port`; failures and cancellation are returned, not logged. Never the sign-in name or password.
- [ ] T026 [US3] Add `server_text_for_log(sign_in_name, text)` to crates/mailbag-imap/src/session.rs (research.md §6): replace every occurrence of the non-empty sign-in name with `<login>`, ignoring ASCII case, whatever the name's length. Test a refusal that repeats the name, a different case, several occurrences, a name that does not occur, a two-character name that also occurs inside ordinary words, and an empty name. No other function turns server text into a field (contracts/record.md rule 6).
- [ ] T027 [US1] Log connection and sign-in in crates/mailbag-imap/src/transport.rs and session.rs: info "connected" with `duration_ms` for every real connection, debug with `host`, `port` and `address`; info "connection secured" with `encryption`, `tls` and `duration_ms`; debug `certificate_errors` with the names of the GIO certificate flags where validation fails, without any certificate field (research.md §7); info with `capabilities` as already received, no added command; info "signed in" with `method` and `duration_ms`; info with the number of alerts and debug with each alert through `server_text_for_log`; debug "connection closed".
- [ ] T028 [US1] Log Inbox reading in crates/mailbag-imap/src/reader.rs: info "Inbox opened" with `messages`, debug with `folder`, `uid_validity`, `uid_next`; info "message list loaded" with `rows` and `duration_ms`, debug with the UID range; info "part structures loaded" with `messages` and `duration_ms`; info "reconnecting after a structure that could not be read" with the attempt number in `reconnect`; info "text loaded" with `messages`, `commands`, `bytes` and `duration_ms`; debug per command group with the sections, `uids`, `bytes` and `duration_ms`; debug per message for text not returned and for a message that disappeared. The raw list header lines and received parts are never fields.
- [ ] T029 [US1] Log part trees in crates/mailbag-imap/src/part_tree.rs where the server's description is projected: at debug, one line per part with `folder`, `uid`, `section`, `content_type`, `charset`, `format`, `delsp`, `disposition`, `transfer_encoding`, `size` and `file_name_params` (which of Content-Type's `name`, `name*`, `name*0*` and Content-Disposition's `filename`, `filename*`, `filename*0*` are present, compared without regard to letter case), read from the description before it becomes `MessagePart`; the parser hands over both parameter lists (research.md §7). Never write a file name's value, an attached message's envelope, a part's description, a content identifier or other parameters. For a structure that could not be read, one debug line in reader.rs with `folder`, `uid` and whether the server refused or the reply could not be parsed. `MessagePart` and `MimePart` do not change.
- [ ] T030 [US3] Write the debug lines with `server_text` in crates/mailbag-imap/src/session.rs and reader.rs, where the sign-in name and the reply are both at hand: for a refused sign-in, a refused or failed command, a refused message list and a closing reply, each through `server_text_for_log`. The reply travels to the load and the UI unchanged; add no field or type that carries a sanitized copy across the crate boundary. The load's warning and error lines in crates/mailbag/src/inbox_load.rs carry the response code only.
- [ ] T031 [US3] Apply the wording of specs/003-logging/contracts/record.md "Changes to 002 documents" to specs/002-imap-integration/contracts/imap-reading.md and specs/002-imap-integration/data-model.md, and reword the doc comment on `ImapError`'s `Debug` implementation in crates/mailbag-imap/src/lib.rs; the implementation keeps leaving the text out.
- [ ] T032 [US1] Add tests on the scripted server in crates/mailbag-imap/src/tests/ and crates/mailbag/src/inbox_load/tests.rs with a capture buffer: every failing load of the 002 scenarios gives exactly one ERROR whose step and cause equal the UI's explanation (SC-004); cancelled loads give no WARN or ERROR; a refused list and unreadable content give one WARN each; Inboxes of 1 and 100 ordinary messages give the same number of INFO lines, and one unreadable structure adds only connection lines (SC-005); every line inside a load carries the load's label and identifier while an account update arrives during the load (SC-006); the sign-in name, password, host at info, and raw header lines of the scenarios never appear.
- [ ] T033 STOP: run the portion's tests and ./scripts/check.sh; run Mailbag natively at `info` and at `debug` against a real Generic IMAP account and compare the lines with log-events.md, confirming by eye that the info record has no folder, host, UID or address; run git diff --check. Review constitution I/II, report, suggest the commit, and wait before portion 4.

## Phase 5: content, privacy and instructions (portion 4)

**Goal:** a record explains how each message's text was chosen and decoded;
the privacy limits are checked over the whole path; a user can follow the
README on the installed Flatpak (US2, US3, SC-001–003, SC-007–009).
**Independent check:** MIME fixtures with marker strings, the scripted server
and the installed application.

- [ ] T034 [US1] Log part selection in crates/mailbag-content/src/lib.rs `select_text_parts`: debug with the selected sections and `rule` — single part, the last alternative that has plain text (the code keeps the last such branch), or the root of a related set with `start_matched` saying whether its `start` named a part — and debug for no selection with which explanation (no plain text, encrypted, S/MIME). Never write a content identifier. The function has no folder or UID; they come from a `message` span the caller opens per message in crates/mailbag/src/inbox_load.rs with `folder` and `uid`, only when debug is enabled.
- [ ] T035 [US1] Log decoding in crates/mailbag-content/src/lib.rs `decode_text_part`: debug for a decoded part with `section`, `charset`, `transfer_encoding`, whether `format=flowed` was applied, bytes in, characters out and `duration_ms`; debug for a part that could not be decoded with the declared name and the failing stage (unknown character set, unknown transfer encoding, undecodable entity). Never the decoded or raw text.
- [ ] T036 [US3] Add `value_shape(raw_value)` to crates/mailbag-content/src/lib.rs (research.md §7): from raw bytes give the character sets and encodings (B or Q) of encoded words, an RFC 2231 form with its character set, the length in bytes and whether 8-bit bytes are present, and nothing of the value itself. Use it twice, only when debug is enabled (contracts/record.md rule 4): in `decode_display_fields`, a debug line with `header` and `shape` when the header line is present and no value came out or the value contains U+FFFD, claiming no cause, and a debug line for an absent header; and for each part whose Content-Type parameters carry a file name, a debug line with `section`, `file_name_shape` and `extension` — the text after the last dot of the raw value when it is at most eight ASCII letters or digits. For a file name the value has passed the IMAP parser, which replaces bytes that are not UTF-8 with U+FFFD, so the shape reports replacement characters there and 8-bit bytes only for headers (research.md §7). Test with a B-encoded word in an unknown character set, a Q-encoded word, raw 8-bit bytes in a header, replacement characters in a file name, an RFC 2231 value and a continuation, an absent header, a name with and without a visible extension, and assert that no byte of a value other than the extension appears.
- [ ] T037 [US3] Put marker strings into the fixtures under tests/fixtures/mime/ and the scripted server scenarios of crates/mailbag-imap/src/test_server.rs: a password, a sign-in name, a folder name, a host name, a subject, an address, an attachment file name in each parameter form, body text, the subject and address of an attached `message/rfc822`, a part description, a content identifier, and a sign-in refusal that repeats a two-character sign-in name. Use synthetic values only.
- [ ] T038 [US3] Add the privacy check over the whole path in crates/mailbag/src/inbox_load/tests.rs (SC-002): run loads at `info` and at `debug` into a capture buffer; at info none of the markers may appear; at debug the password, both sign-in names, subject, address, file name, body text, the attached message's subject and address, the part description and the content identifier may not appear, while the folder and host may.
- [ ] T039 [US1] Add the defect check in crates/mailbag/src/inbox_load/tests.rs (SC-003): for each defective fixture — unknown character set, unknown transfer encoding, undecodable part, unreadable structure, text not returned, list header with no value — assert that the debug lines give `folder` and `uid`, the failing step and cause where the code knows one, the part tree where the description was read, and for the header case the header's name and shape with no cause.
- [ ] T040 [US2] Add a "Reporting a problem" section to README.md (FR-018): the exact command for the Flatpak build (`flatpak run io.github.mitinand.Mailbag --log-level=debug 2> mailbag.log`) and for a native build, that Mailbag must not be running already, what a debug record contains (folder and label names, message identifiers, the server's host, server replies with the sign-in name replaced) and what it never contains (passwords, addresses, subjects, attachment file names, message text), that GTK's own warnings share the file, and the advice to read the file before attaching it.
- [ ] T041 [US3] Walk specs/003-logging/log-events.md row by row against the code and the tests: every logged row has a line in the code and is covered by a test or listed for the acceptance run; every "not logged" row has no line; correct the list where the code differs (SC-009).
- [ ] T042 [US2] Run all of specs/003-logging/quickstart.md, including the installed Flatpak section as a person who has not seen this directory (SC-007): the file appears on the host, its first Mailbag line says `Flatpak build` and names the runtime, and `flatpak info --show-permissions io.github.mitinand.Mailbag` is unchanged from 002. Record in quickstart.md what was run and what remains unverified (constitution VI).
- [ ] T043 STOP: run ./scripts/check.sh and git diff --check. Review constitution I/II over the whole feature, report what changed, the evidence per success criterion and the remaining limitations, suggest the commit and the PR description, and wait for the maintainer.

## Dependencies

- T001 blocks everything.
- Portion 1 (T002–T013) blocks portions 2–4: they need the layer, the labels
  and the capture buffer.
- Portion 2 and portion 3 touch different files and could be built in either
  order; the plan keeps 2 before 3 so the account label is proven before loads
  use it.
- Portion 4 needs portion 3's per-message span and completes the marker check
  that portions 2 and 3 begin.
- Inside a portion, tasks marked [P] touch files no other open task of that
  portion touches.

## Parallel opportunities

- Portion 1: T004 beside T005–T012.
- Portion 2: T018 and T019 beside T015–T017.
- Portion 3: T025 beside T026–T029.
- Review pauses (T013, T021, T033, T043) are never parallel with anything.

## Implementation strategy

Portion 1 alone is already useful: a developer sees a correct first line,
GTK's warnings and the quit line, and every rule about output is proven before
any event exists. Each later portion adds lines for one area and proves its
own levels and privacy, so a review never has to judge format, output and
content at once. There is no smaller slice of a portion that leaves the
record truthful: a portion's lines and its tests land together.
