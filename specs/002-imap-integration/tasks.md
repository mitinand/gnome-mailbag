# Tasks: IMAP Integration

**Feature**: F02 / `002-imap-integration`
**Created**: 2026-09-18 · **Status**: Documents approved 2026-09-19; portion 1 implemented 2026-09-19 and awaiting review

[Spec](spec.md) owns behavior, [plan](plan.md) owns boundaries and portions,
[research](research.md) owns decisions and evidence, contracts own details, and
[quickstart](quickstart.md) owns commands and acceptance scenarios. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one agreed
portion, run its checks, report and stop. A checked handoff does not mean
approval. The maintainer creates commits and PRs. Do not start code before
document approval.

Phases follow the approved portions rather than one phase per story: the
protocol, content and UI layers each serve several stories and ship as separate
reviewable commits. Story labels trace tasks to US1 (load recent mail), US2 (read
received text) and US3 (refresh and explain a failed load). Tests are part of
every portion under AGENTS.md and live beside their modules.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(imap): plan IMAP integration | IMAP integration |
| 1. Packaging and dependency policy | T002–T012 | build: prepare Flatpak sources and IMAP dependencies | IMAP integration |
| 2. GOA access | T013–T018 | feat(goa): provide IMAP access for the selected account | IMAP integration |
| 3. Secure connection and acquisition | T019–T029 | feat(imap): receive Inbox data over GIO | IMAP integration |
| 4. Content and load sequence | T030–T036 | feat(content): select and decode received plain text | IMAP integration |
| 5. Visible integration | T037–T045 | feat: show and refresh received Inbox mail | IMAP integration |
| Acceptance | T046–T048 | docs(imap): record IMAP integration acceptance | IMAP integration |

## Phase 1: documents and review

- [X] T001 STOP: present the 2026-09-19 revision of specs/002-imap-integration/ (spec.md, plan.md, research.md §9–§10, contracts/, data-model.md, quickstart.md, checklists/requirements.md and this tasks.md) and wait for explicit maintainer approval before any code change.

## Phase 2: setup — packaging and dependency policy (portion 1)

**Purpose:** the forks, generated sources and legal notices build offline in
Flatpak before mail code exists. Details: [packaging contract](contracts/packaging.md).

- [X] T002 Verify the baseline in specs/002-imap-integration/research.md §2 before editing: async-imap =0.11.3 with runtime-futures, both fork revisions, mail-parser 0.11.9 with full_encoding, glib/gio 0.22.9 and log with both levels off. Record any proposed departure in research.md and stop for a decision instead of changing it silently.
- [X] T003 Create skeleton crates crates/mailbag-imap/ and crates/mailbag-content/ (Cargo.toml and src/lib.rs with a crate doc comment and SPDX header, `publish = false`, workspace lints). Declare async-imap (default features off, runtime-futures), log (max_level_off, release_max_level_off), glib and gio in mailbag-imap, and mail-parser (full_encoding) in mailbag-content, following research.md §2 and §9.
- [X] T004 Update the root Cargo.toml with both workspace members and the two `[patch.crates-io]` fork entries at their full revisions; make crates/mailbag/Cargo.toml depend on both skeletons by path so `cargo build --package mailbag` compiles the forks. Update Cargo.lock and confirm with `cargo tree` that runtime-tokio, runtime-async-std and alternate TLS stacks are absent.
- [X] T005 [P] Add scripts/setup-cargo-generator.sh that installs flatpak-cargo-generator from flatpak-builder-tools at the full commit for `de2225a` with aiohttp and tomlkit; record that revision in scripts/tool-versions.env and call the script from scripts/setup.sh.
- [X] T006 Add scripts/generate-cargo-sources.sh with normal generation and `--check` under contracts/packaging.md “Generation and drift checks”: normalize `cargo/config` to `cargo/config.toml`, write deterministic output, and in `--check` mode compare a temporary result with the tracked file and fail without rewriting it. Generate cargo-sources.json.
- [X] T007 Update io.github.mitinand.Mailbag.yml to use cargo-sources.json instead of the vendor directory and .flatpak-builder/cargo-config.toml, set `CARGO_HOME=/run/build/mailbag/cargo`, keep `CARGO_NET_OFFLINE`, add meson.options and third-party-notices/ to the sources and pass `-Dcargo_vendor_dir=/run/build/mailbag/cargo/vendor`. Remove cargo vendor and the temporary config from scripts/build-flatpak.sh, the `/vendor/` entry from .gitignore and the generated vendor/ directory.
- [X] T008 Add meson.options with a `cargo_vendor_dir` string option and make meson.build read crate notices from it instead of `vendor/`, never setting CARGO_HOME. Treat files in a crate's `LICENSES/` directory as notices in addition to the existing name patterns. Install notices and Cargo.toml under `share/licenses/io.github.mitinand.Mailbag/<crate-version>/` using relative paths only, and keep failing on a crate without notices outside the map in T009.
- [X] T009 Add third-party-notices/stop-token/ (standard MIT and Apache-2.0 texts, authors from the resolved Cargo.toml) with ORIGIN.md stating the crate version, license expression and text sources. Install it through an exception map in meson.build keyed only by this package name. hashify needs no exception: its package supplies the texts in `LICENSES/` (research.md §8, revised 2026-09-19).
- [X] T010 Update deny.toml to allow BSD-3-Clause and set `[sources].allow-git` to exactly the two fork URLs. Update scripts/check.sh to run `scripts/generate-cargo-sources.sh --check`, the license and source-policy gates, and a `cargo tree` check of normal dependencies against the crate rules in research.md §9.
- [X] T011 Update .github/workflows/check.yml to run scripts/setup-cargo-generator.sh before the canonical checks, taking versions only from scripts/tool-versions.env; keep the Flatpak job building from cargo-sources.json.
- [X] T012 STOP: run ./scripts/check.sh, including a stale cargo-sources.json and a temporary forbidden edge (gio in crates/mailbag-content/Cargo.toml) that must both fail and are reverted; run ./scripts/build-flatpak.sh with Cargo offline in the build sandbox; inspect the installed license tree under contracts/packaging.md “Portion 1 acceptance”; run git diff --check. Review constitution I/II, report and wait before portion 2.

## Phase 3: GOA access (portion 2)

**Goal:** the selected account's IMAP settings and password reach the mail
worker; settings without encryption are refused before any password request.
**Independent check:** the private GOA fixture drives `request_imap_access`
without IMAP code or UI. Details: [GOA access contract](contracts/goa-access.md).

- [ ] T013 [US1] STOP before code: obtain explicit maintainer approval of the shared interface in specs/002-imap-integration/contracts/goa-access.md.
- [ ] T014 [US1] Extend tests/support/goa.rs with the Mail interface properties (ImapHost, ImapUserName, ImapUseSsl, ImapUseTls, ImapAcceptSslErrors) and PasswordBased.GetPassword for `imap-password`, using synthetic credentials only.
- [ ] T015 [US1] Add tests in crates/goa-adapter/src/imap_access/tests.rs for the returned object path, host with an explicit port, SSL and STARTTLS selection, AttentionNeeded and a prior observation failure not blocking access, Settings versus Password failures, absent account versus missing Mail interface, cancellation and stop, and no observer refresh or exclusion side effects.
- [ ] T016 [US3] Add tests in crates/goa-adapter/src/imap_access/tests.rs showing that false/false settings return a Settings failure with the no-encryption cause, GetPassword is never called and ImapAcceptSslErrors changes nothing (spec US3-6).
- [ ] T017 [US1] Implement `GoaAdapter::request_imap_access` in crates/goa-adapter/src/imap_access.rs and expose it through crates/goa-adapter/src/lib.rs: ImapAccess, ImapEncryption, ImapAccessStep, ImapAccessError and ImapAccessRequest under contracts/goa-access.md, asynchronous D-Bus calls on the adapter's context, no Debug, Display or Clone of secrets.
- [ ] T018 [US1] STOP: run scripts/check.sh, git diff --check and the F01 regression tests; review constitution I/II, report and wait before portion 3.

## Phase 4: secure connection and acquisition (portion 3)

**Goal:** `mailbag-imap` receives rows, part structures and requested sections
over a verified TLS session without changing server state. **Independent check:**
the scripted Rust/GIO server exercises the crate directly, without GOA or UI.
Details: [acquisition contract](contracts/imap-reading.md).

- [ ] T019 [P] [US3] Add tools/make-certs.sh that generates disposable CA, localhost, unknown-CA, wrong-host and expired certificates under target/test-certs with OpenSSL and never installs trust; add openssl to the prerequisites in README.md and .github/workflows/check.yml.
- [ ] T020 [US1] Add the scripted Rust/GIO IMAP server in crates/mailbag-imap/src/test_server.rs, compiled only for tests and the `test-support` feature, with the prototype behaviors needed by 002: implicit TLS, STARTTLS, PREAUTH, ALERT, UTF-8 greeting, stall, huge literal, deep structure and section requests. Bind dynamic loopback ports, generate missing certificates through tools/make-certs.sh, record only command names, UIDs, section identifiers and credential-transmission counts, and add the ignored `serve_fixture` entry point from quickstart.md.
- [ ] T021 [P] [US3] Add secure-session tests in crates/mailbag-imap/src/tests/: both TLS modes, missing or rejected STARTTLS, injected pre-TLS bytes, PREAUTH before TLS, unknown CA, wrong host and expiry with zero password transmissions, PLAIN with non-ASCII credentials, ASCII LOGIN fallback, LOGINDISABLED, no second method after rejection, capabilities re-read after sign-in, UTF-8 response text and ALERT text retained for a failing attempt.
- [ ] T022 [P] [US1] Add acquisition tests in crates/mailbag-imap/src/tests/: EXAMINE only; rows for 0, 1, 100 and 101 messages in descending UID order; structures requested by the returned UIDs; a disappearing UID; no mutation commands or flag changes.
- [ ] T023 [P] [US3] Add structure-isolation and error-origin tests in crates/mailbag-imap/src/tests/: one, several and all unreadable structures keep their rows with an unreadable-structure result; a fresh session follows each parse failure; UIDVALIDITY change on reconnect stops the attempt; transport errors, timeout, incomplete literal and the library response ceiling never enter isolation; a deep BODYSTRUCTURE leaves the process running.
- [ ] T024 [P] [US2] Add section-retrieval tests in crates/mailbag-imap/src/tests/: one command per complete request shape, root HEADER and leaf .MIME at section 1 never share a request, correlation by UID and section in any order, a missing section or NIL is a load failure, BODY.PEEK only.
- [ ] T025 [P] [US3] Add timeout and cancellation tests in crates/mailbag-imap/src/tests/: stalled versus slowly progressing input with an injected short socket timeout, cancellation during a pending read closes the connection, and no session is reused.
- [ ] T026 [US3] Implement crates/mailbag-imap/src/transport.rs under contracts/imap-reading.md “Execution, waiting and cancellation” and “Secure session”: SocketClient with a 30-second timeout, address parsing with default ports, TlsClientConnection with the default database and no accept-certificate handler, STARTTLS with discarded plaintext buffers, the ThreadGuard futures-io bridge and a private wrapper marking bridge-originated errors. Add only the futures I/O dependency the bridge needs and regenerate cargo-sources.json.
- [ ] T027 [P] [US2] Implement crates/mailbag-imap/src/part_tree.rs: project the parsed BODYSTRUCTURE into a typed tree with section paths, types, parameters, dispositions and transfer encodings, keeping the single-part root numbering.
- [ ] T028 [US1] Implement sessions and acquisition in crates/mailbag-imap/src/lib.rs: sign-in selection, capability refresh, EXAMINE, the row command, `UID FETCH <uids> (UID BODYSTRUCTURE)`, structure isolation with fresh sessions, grouped section retrieval for caller-supplied request shapes, ALERT retention and safe step/cause errors that never format library errors. The crate has no notion of a provider (research.md §9).
- [ ] T029 [US1] STOP: run `cargo test --locked -p mailbag-imap`, scripts/check.sh and git diff --check; confirm the dependency rules pass and cargo-sources.json is current. Review constitution I/II, report and wait before portion 4.

## Phase 5: content and load sequence (portion 4)

**Goal:** every row gets decoded display fields and complete text or an
explanation, and `mailbag` assembles complete batches on the worker.
**Independent check:** MIME samples test `mailbag-content` alone; load-sequence
tests drive the scripted server through the `test-support` feature without GTK
widgets.

- [ ] T030 [P] [US2] Add synthetic MIME samples to tests/fixtures/mime/ covering UTF-8, Windows-1251, KOI8-R, an Asian encoding, base64 and quoted-printable, invalid bytes, unknown charset and transfer encoding, encoded Subject/From/To, several mixed plain parts, a later plain part without disposition, a name without inline, signed, encrypted, related-first-child, HTML-only and nested message/rfc822. Use no real mail.
- [ ] T031 [US2] Add tests in crates/mailbag-content/src/tests.rs for selection and decoding under contracts/imap-reading.md “Selecting text sections” and “Decoding and publication”: returned part paths, replacement characters for invalid bytes, and explanations for unknown charset or encoding, encryption, S/MIME, HTML-only and unreadable structure.
- [ ] T032 [US2] Implement crates/mailbag-content/src/lib.rs: the MIME part description, text-part selection returning part paths, entity decoding through mail-parser with replacement characters, header display-field decoding and message-specific content explanations. Never call `body_text()` or `body_html()`; use no GIO, glib or protocol types.
- [ ] T033 [US1] Implement the received-data roles in crates/mailbag/src/inbox.rs under data-model.md “Data and ownership”: ReceivedBatch, ReceivedMessage, ReceivedContent and LoadFailure, with no raw MIME retained after decoding.
- [ ] T034 [US1] Add load-sequence tests in crates/mailbag/src/inbox_load/tests.rs with a dev-dependency on mailbag-imap's `test-support` feature: complete batches for 0, 1, 100 and 101 messages, rows kept for unreadable structures, text fetched only for selected parts (SC-002 payload absence), no publication after a network interruption, and connection closure before a cancelled load completes.
- [ ] T035 [US1] Implement crates/mailbag/src/inbox_load.rs: the selected-account worker thread with its own GLib MainContext, conversion from the mailbag-imap part tree to the mailbag-content description, the load sequence in plan.md “Ownership and function map” and completion through runtime-independent channels. No widget access (research.md §9).
- [ ] T036 [US2] STOP: run `cargo test --locked -p mailbag-content`, `cargo test --locked -p mailbag inbox`, scripts/check.sh and git diff --check; review constitution I/II, report and wait before portion 5.

## Phase 6: visible integration (portion 5)

**Goal:** the approved UI shows, opens and refreshes the received batch with
truthful loading and failure states. **Independent check:** unit tests for the
controller plus the graphical `mail_ui_transitions` test. Details:
[UI contract](contracts/ui.md).

- [ ] T037 [US3] Implement InboxController in crates/mailbag/src/inbox.rs with tests in crates/mailbag/src/inbox/tests.rs for data-model.md “Operation transitions”, keeping one AccountInbox per account: selection never loads, refresh clears and then loads, a result is stored only for its own account while switching during a load, a failure leaves the list empty, confirmed exclusion discards mail and cancels its load, and quit; one load at a time and Refresh unavailable while it runs.
- [ ] T038 [US1] Add WindowUi in crates/mailbag/src/window_ui.rs as the only owner of list_stack under contracts/ui.md “One page decision”, including the not-loaded state with Refresh Inbox unavailable for Google and Microsoft 365; stop crates/mailbag/src/account_ui.rs from setting list_stack directly.
- [ ] T039 [US1] Implement row binding in crates/mailbag/src/mail_ui.rs: a GListStore bound to the existing GtkListBox with message-row.ui, sender, subject and INTERNALDATE, the unread dot with an accessible “Unread”/“Read” description, and list_title/list_page titles under contracts/ui.md “List and reader binding”.
- [ ] T040 [US2] Implement local opening in crates/mailbag/src/mail_ui.rs: instantiate message-content.ui and envelope.ui once, fill reader fields, show at most the first 65,536 UTF-8 bytes of inert plain text without an explanation, replace NUL, keep attachment and location hidden and mail-changing controls insensitive. Opening sends no network request.
- [ ] T041 [US3] Add `app.refresh-inbox` immediately after Synchronization Status in crates/mailbag/resources/ui/mailbag.ui and wire it in crates/mailbag/src/main.rs as the only way to load: it clears the selected account's list and reader and starts a load; enabled for the selected Generic IMAP account while Idle and disabled while a load runs; show sync_button_list with the existing spinner only while a load runs; show a failed load on the account's status page, worded under contracts/ui.md “Failure wording”, without a toast.
- [ ] T042 [US1] Connect selection, account updates and GOA access to loads in crates/mailbag/src/main.rs and crates/mailbag/src/window_ui.rs: request access on the main context, hand ImapAccess to the worker, and cancel on confirmed exclusion and quit without joining a thread on GTK's context; switching accounts does not cancel a load.
- [ ] T043 [US1] Add `--share=network` to finish-args in io.github.mitinand.Mailbag.yml as the only permission added to the F01 FR-016 baseline.
- [ ] T044 [US3] Add the ignored graphical test mail_ui_transitions in crates/mailbag/src/mail_ui/tests.rs for page priority, selection without loading, refresh clearing and loading, a failed load and a repeated refresh, switching during a load, disabled busy Refresh, spinner visibility and a row with an unreadable structure.
- [ ] T045 [US1] STOP: run scripts/check.sh, git diff --check and `cargo test --locked -p mailbag mail_ui_transitions -- --ignored --test-threads=1`; review constitution I/II, report and hand over portion 5 before acceptance.

## Phase 7: acceptance

These checks need the maintainer's session, a disposable Generic IMAP account
and a test CA the maintainer installs. Record unavailable cases as unverified.

- [ ] T046 Run installed acceptance from specs/002-imap-integration/quickstart.md “Installed-app fixture and host trust” and “Visible integration and final acceptance” for SC-001–SC-007: both TLS modes, certificate failures with zero passwords, false/false refusal, STARTTLS downgrade attempts, display clipping at 64 KiB, reopening without requests, restart without restored mail and installed permissions. Record GOA, GLib, GnuTLS and Flatpak versions.
- [ ] T047 Run the SC-006 checks and a load of the maintainer's real mailbox in the installed app: unchanged F01 accessibility, Refresh Inbox and rows by keyboard, spoken read/unread state, navigation and quitting during a stall. Completion requires the maintainer's confirmation.
- [ ] T048 STOP: run scripts/check.sh and git diff --check, keep unmet acceptance unchecked in specs/002-imap-integration/tasks.md and report it; do not declare 002 complete from synthetic tests alone.

## Dependencies and parallel opportunities

Documents → approval → portion 1 → review → shared-interface approval →
portion 2 → review → portion 3 → review → portion 4 → review → portion 5 →
review → acceptance. Each portion builds on the previous one: packaging makes
the forks available, GOA access supplies credentials, `mailbag-imap` and
`mailbag-content` feed the load sequence, and the UI consumes complete batches.

Stories cut across the portions. US1 is testable end to end without UI after
portion 4 (0/1/100/101-message fixture Inboxes produce 0/1/100/100 rows) and
visibly after portion 5. US2 content is testable after portion 4 and local
opening after portion 5. US3 security and failure behavior is testable in
portions 2 and 3; refresh and retry in the UI after portion 5.

Within portion 3, the test tasks T021–T025 touch separate files and can be
written in parallel once the server in T020 exists; T027 is independent of
T026. Within portion 4, samples (T030) and content tests can be prepared while
`mailbag-content` is implemented. Shared-file edits stay sequential, and these
opportunities never override review pauses or authorize additional agents.

## Implementation strategy

The first user-visible increment is portion 5; portions 1–4 are verified by
their own tests and deliver no UI change. Stop after every portion for review.
If a portion grows beyond one reviewable change or the plan's estimate by about
1.5 times, split it and bring the change back before continuing. No IDLE,
polling, storage, attachment fetching, HTML reading or other provider is part
of any portion.
