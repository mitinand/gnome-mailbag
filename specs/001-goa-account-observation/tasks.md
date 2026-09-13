# Tasks: GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation` · **Branch**: `codex/goa`

**Input**: [spec.md](spec.md), [plan.md](plan.md), [research.md](research.md),
[data-model.md](data-model.md), [GOA contract](contracts/observation.md),
[UI contract](contracts/ui.md), [quickstart.md](quickstart.md).

**Status**: Portion 3 (T011–T018) is complete; awaiting maintainer review. F01 is not yet complete.
Tests are required by the specification. Add the relevant tests before the behavior,
verify that they expose the missing behavior, then make them pass within the same
portion. Do not hand over a portion with deliberately failing tests.

## Reading and execution rules

Paths are relative to the repository root. `[US1]`–`[US3]` identify user stories;
`[P]` identifies work in different files that can overlap after its prerequisites.
Parallel work never crosses a review stop and does not authorize spawning agents.

The shared client and account rules serve both US1 and US2 and must ship in PR 1.
They therefore appear once in the foundation. The story phases connect those rules
to user-visible behavior in PR 2; they do not implement a second account policy.

Follow [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): the maintainer
creates commits and PRs. At each STOP, run the stated checks, report changes,
evidence and limitations, suggest a commit message and identify the intended PR.
Wait for review and an explicit instruction before starting the next portion.
A checked STOP means that handoff occurred, not that permission to continue exists.

### Portions and review stops

| Portion | PR | Tasks | Suggested commit subject | Stop |
|---|---|---|---|---|
| 1. Reviewed requirements and design | 1 | T001–T002 | `docs: define GOA account observation` | T002 |
| 2. Initial GOA client | 1 | T003–T010 | `feat(goa): read accounts on a dedicated GLib thread` | T010 |
| 3. Changes, recovery and periodic checks | 1 | T011–T018 | `feat(goa): follow account changes and recover failed checks` | T018 |
| 4. Account display and selection rules | 1 | T019–T023 | `feat(accounts): define availability and selection rules` | T023 |
| 5. Accounts in the approved UI | 2 | T024–T033 | `feat(ui): show GOA accounts and explain changes` | T033 |
| 6. Settings and Flatpak access | 2 | T034–T039 | `feat(settings): open Online Accounts from Mailbag` | T039 |
| 7. Integration acceptance and fixes | 2 | T040–T044 | `test: verify GOA integration and account UI` | T044 |

Seven portions remain an estimate for review. Evidence alone does not require an
empty commit. Fix review feedback within its portion; do not silently add scope.

## Phase 1: Setup — reviewed scope (portion 1, PR 1)

**Purpose**: Hand over a consistent, executable specification before source changes.

- [X] T001 Check `specs/001-goa-account-observation/spec.md`, `plan.md`, `tasks.md`, supporting contracts and `checklists/requirements.md` against the accepted decisions and constitution; preserve the agreed scope, two PRs and review stops, correcting only actual inconsistencies.
- [X] T002 STOP for portion 1: run `scripts/check.sh`, validate task IDs and document links in `specs/001-goa-account-observation/tasks.md`, and hand the documents to the maintainer for PR 1 review under `AGENTS.md`; report that existing shell checks do not test F01, then wait.

## Phase 2: Foundation — shared client and account rules (PR 1)

**Purpose**: Complete the shared logic and its headless tests before connecting GTK.
The application must remain buildable and launchable throughout PR 1.

### Initial client (portion 2)

- [X] T003 Verify that the implementation checkout includes approved commit `7a69c494dbb97c10fe1baf8db88a5296d271278e` and its `Cargo.toml`, `crates/mailbag/src/main.rs` and UI resources; if absent, have the maintainer incorporate that baseline without losing local documents or unrelated changes. Do not create a merge commit yourself. Run `scripts/check.sh` before adapting source paths.
- [X] T004 Add the workspace member in `Cargo.toml` and create `crates/goa-adapter/Cargo.toml` and `crates/goa-adapter/src/lib.rs`; use the existing gio/glib 0.22.9 family and Rust toolchain, updating `Cargo.lock` only as required. Add no GOA binding, Tokio or native libgoa dependency.
- [X] T005 Create private D-Bus fixture helpers in `tests/support/bus.rs` and `tests/support/goa.rs`; provide synthetic accounts, controlled replies/signals and call recording, disable host service activation, enforce outer deadlines and clean up the private daemon. Include helpers only in tests, without a production account-injection option or another crate.
- [X] T006 [P] Add field-validation tests in `crates/goa-adapter/src/accounts/tests.rs` for missing/wrongly typed required fields, duplicate/missing IDs, optional display fallback, empty address, oversized values and separation of MailDisabled, Mail presence and AttentionNeeded; distinguish account-local errors from a list that cannot prove absence.
- [X] T007 Implement typed account/list/error data and validation in `crates/goa-adapter/src/accounts.rs` using `data-model.md` and `contracts/observation.md`; retain safe operation/domain/code/cause, never log IDs, addresses, raw replies or unchecked remote text, and keep provider eligibility in Mailbag.
- [X] T008 [P] Add initial-client contract tests in `crates/goa-adapter/src/client/tests.rs`: exact GetManagedObjects request/reply, healthy empty versus unavailable, invalid replies, startup deadline, changes during initial acquisition, and receiving data without iterating the GTK/default context. Assert that no credential or account-mutation methods are called.
- [X] T009 Implement initial connection/check and the client handle in `crates/goa-adapter/src/client.rs` and `crates/goa-adapter/src/lib.rs`: goa-adapter owns one persistent worker thread and private GLib context, installs subscriptions before accepting initial data, and exposes start/next_account_update/refresh_accounts/stop with plain Rust values. Include basic cancellation and last-handle cleanup; no thread per request or timer tick. If account data changes while the initial list request is pending, reject the outdated reply and repeat the check within the original attempt deadline. Process-restart recovery remains in T014.
- [X] T010 STOP for portion 2: run `scripts/check.sh` and the initial tests under `crates/goa-adapter/`, confirm the existing app still launches, report implemented behavior and remaining recovery work for PR 1 under `AGENTS.md`, then wait for review.

### Account changes and failures (portion 3)

- [X] T011 [P] Add event/recovery tests in `crates/goa-adapter/src/client/event_tests.rs` for every signal/property in `contracts/observation.md`: startup overlap, both Mail/interface orders, invalidation, process loss/replacement, no false removals on owner loss, stalled activation/account acquisition, failed recovery, fresh activation, and late replies/callbacks from an obsolete client even with the same GOA process.
- [X] T012 [P] Add concurrency/limit tests in `crates/goa-adapter/src/client/concurrency_tests.rs`: updates and commands racing with wait registration, status-only delivery, an idle consumer, 10,000 changes with a paused consumer, contract data limits and recovery, stop during connection/check/idle failure/load, last-handle drop, and unexpected worker exit. Assert one pending update, prompt stop and no growing callback queue.
- [X] T013 [P] Add periodic-check tests in `crates/goa-adapter/src/client/health_tests.rs`: one check per ten-second idle tick, five-second deadline, skipped busy ticks, manual-check reuse, continued periodic checks after failure without extra automatic attempts, same-process hang/recovery, missed event followed by later working signals, no catch-up burst after suspend, and no checks after stop. Use controlled timing with outer deadlines.
- [X] T014 Handle NameOwnerChanged, InterfacesAdded, InterfacesRemoved and PropertiesChanged through GIO in `crates/goa-adapter/src/client.rs`, using direct subscriptions on the private worker. Verify current full lists before confirming absence; reject old process/client/request/data-version results; bound activation, acquisition and rescheduled checks by the existing deadline. Do not add whole-session-bus recovery.
- [X] T015 Finish event-driven delivery and retry/stop commands in `crates/goa-adapter/src/client.rs` and `crates/goa-adapter/src/lib.rs`: keep one latest update, register/wake waiting tasks without a lost notification, prioritize stop, publish status changes independently of account versions and report actual data-limit failures. Use the existing GLib tasks and standard Rust wakeups, with no frequent command/UI polling or event history.
- [X] T016 Add the single repeating ten-second timer in `crates/goa-adapter/src/client.rs` on the existing worker; reuse the guarded full-list check, skip busy ticks, preserve confirmed availability while a routine check is pending, and continue periodic recovery after failure. Apply later property events without restoring older cached fields.
- [X] T017 Finish cancellation and teardown in `crates/goa-adapter/src/client.rs` and `crates/goa-adapter/src/lib.rs`: invalidate requests, remove timers/subscriptions, release callbacks and shared bus references, wake waiting consumers and prioritize stop under load. Meet the one-second fixture shutdown target without joining the worker from GTK or closing a shared bus connection.
- [X] T018 STOP for portion 3: run `scripts/check.sh` and all tests in `crates/goa-adapter/`; report event, timing, restart, resource and shutdown results for PR 1 under `AGENTS.md`, including any unverified limits, then wait for review.

### Shared account policy (portion 4)

These tests cover the shared parts of US1 and US2 before their GTK presentation.

- [ ] T019 [P] Add availability/selection tests in `crates/mailbag/src/accounts/tests.rs`: exact imap_smtp/google/ms_graph support, unsupported consumer Microsoft/provider keys, Microsoft 365 without IMAP, 30 accounts and duplicate labels, all status cases, isolated account errors, GOA loss/recovery, explicit disable amid other errors, no cold-start history, retained selection and no automatic reselection.
- [ ] T020 [P] Add change/notice tests in `crates/mailbag/src/accounts/notice_tests.rs`: changes superseded before UI application leave rows/selection intact; an applied exclusion clears selection; single labels and mixed-cause group counts; unselected exclusions; no duplicate/false/cross-run notices; reappearance followed by a new exclusion; and hiding/readding the final account without Welcome or synchronization.
- [ ] T021 Add the goa-adapter path dependency to `crates/mailbag/Cargo.toml` and implement shared account policy in `crates/mailbag/src/accounts.rs`, registering the module in `crates/mailbag/src/main.rs`. Own provider eligibility, VisibleAccount, selected ID and status decisions here; keep rows during uncertainty and distinguish explicit disablement, missing Mail and confirmed absence. Keep all state in memory.
- [ ] T022 Implement latest-state comparison and AccountHiddenNotice generation in `crates/mailbag/src/accounts.rs`: compare against rows actually applied by the UI, produce a single label or mixed-cause count only when rows are hidden, and discard labels when no longer needed. Do not add switch history, removal acknowledgements, persistence or cache deletion.
- [ ] T023 STOP for portion 4 and PR 1: run `scripts/check.sh`, `cargo test --locked -p goa-adapter` and `cargo test --locked -p mailbag accounts::`; verify nonzero behavioral coverage and a launchable app. Hand over results under `AGENTS.md` and wait for maintainer review before any PR 2 work.

## Phase 3: US1 — see available GNOME mail accounts (P1, portion 5, PR 2)

**Goal**: Display real account identities and truthful status in the existing UI.

**Independent test**: With private GOA fixtures, verify the US1 account matrix,
selection, duplicate names and problem explanations without a mail backend or real
Settings service. Button placement is checked here; launching Settings is US3.
The US1 installed-discovery evidence is collected in T042.

### Tests

- [ ] T024 [P] [US1] Add UI contract tests in `crates/mailbag/src/account_ui/tests.rs` for the US1 matrix: stable rows, selection by ID, distinct labels, pending/empty/excluded/attention/error status, no fake counts or mail, safe plain text/icons, and hover/click/Enter/Space problem explanations; keep graphical cases separately runnable from the headless gate.

### Implementation

- [ ] T025 [US1] Implement the flat account model and row binding in `crates/mailbag/src/account_ui.rs` using the existing folder_tree and folder-row.ui; reconcile by GOA ID with GtkSingleSelection autoselect disabled and unselection allowed, and translate selection to the accounts module.
- [ ] T026 [US1] Connect the existing status page in `crates/mailbag/resources/ui/mailbag.ui` and `crates/mailbag/src/account_ui.rs` to the shared status decisions; show cause-specific empty guidance and its Online Accounts button, with a neutral reader and unavailable mail actions. Keep dimensions, pane proportions, spacing, menu positions and breakpoints unchanged.
- [ ] T027 [US1] Replace the count suffix with a focusable problem button in `crates/mailbag/resources/ui/folder-row.ui` and wire its tooltip/explanation in `crates/mailbag/src/account_ui.rs`; use English plain text and accessible names, support mouse/touch/Enter/Space, and prevent icon activation from selecting another account.
- [ ] T028 [US1] Wire client startup and UI consumption in `crates/mailbag/src/main.rs` and `crates/mailbag/src/account_ui.rs`; keep GTK objects on GTK's thread, apply at most one update per dispatch and yield, cancel the consumer on window/app teardown with weak references, and show the list page initially/on account activation while closing an overlaid folder sidebar.

**Checkpoint**: US1 presentation is independently testable. Continue to US2 only
within this same authorized portion; portion 5 is handed over at T033.

## Phase 4: US2 — trust account state as GNOME changes (P1, portion 5, PR 2)

**Goal**: Preserve usable navigation during failures, allow retry and explain actual
account hiding. Reuse the PR 1 account policy rather than duplicating it in widgets.

**Independent test**: Drive private GOA changes, outages and recovery in the window;
verify rows, selection, indicators, retries and toasts without Settings or mail access.

### Tests

- [ ] T029 [P] [US2] Add graphical change tests in `crates/mailbag/src/account_ui/change_tests.rs` for retained rows/selection during individual/global failures, confirmed recovery/removal, no replay of superseded switches, last-account transitions, retry from both locations, no background loading flashes and one mixed-cause exclusion toast. Test focus and open explanations when rows/icons disappear or are recycled.

### Implementation

- [ ] T030 [US2] Apply account-policy changes in place in `crates/mailbag/src/account_ui.rs`; retain rows and selection during GOA/account errors, clear only resolved indicators, clear selection only on applied exclusion, and move focus to the surviving row or list/status action without selecting a neighbor. Close or reanchor explanations whose account disappears.
- [ ] T031 [US2] Connect Retry Check in `crates/mailbag/src/account_ui.rs` and the existing status area in `crates/mailbag/resources/ui/mailbag.ui`; reuse pending checks, show pending state at all retry entry points, preserve unresolved errors, and keep routine ten-second checks visually quiet. Do not claim failed credentials or stopped mail access.
- [ ] T032 [US2] Present AccountHiddenNotice through the existing toast overlay in `crates/mailbag/src/account_ui.rs`; name one account or count a mixed-cause group using the agreed combined wording, retain at most one active and one pending combined notice, discard unneeded labels, and keep persistent problems outside transient toasts.
- [ ] T033 [US2] STOP for portion 5: run `scripts/check.sh` plus US1/US2 graphical cases in `crates/mailbag/src/account_ui/`; report row, focus, retry, toast and narrow-layout results for PR 2 under `AGENTS.md`. Clearly mark unavailable display/touch checks and pending Settings integration, then wait for review.

## Phase 5: US3 — open system account management (P2, portion 6, PR 2)

**Goal**: Open Online Accounts from both existing entry points and explain failures.

**Independent test**: Invoke the launcher against private Settings success/failure
fixtures without GOA or a mail account; then test both UI entry points. Actual host
panel presentation is a separate installed check in T042.

### Tests

- [ ] T034 [US3] Add a private Settings service in `tests/support/settings.rs` using the existing isolated bus helper; record action arguments and support absent/denied/error/malformed/hanging replies without launching or interfering with host Settings.
- [ ] T035 [P] [US3] Add launcher contract tests in `crates/mailbag/src/settings/tests.rs` for the exact nested action body in `contracts/ui.md`, reply type, shared five-second bus-acquisition/call deadline, all failure classes, repeated activations, retry after completion and cancellation/late replies after quit.

### Implementation

- [ ] T036 [US3] Implement the asynchronous org.gtk.Actions Activate launcher in `crates/mailbag/src/settings.rs`; target org.gnome.Settings at /org/gnome/Settings with launch-panel/online-accounts, share one pending attempt, validate replies and preserve safe error causes. Use no shell/host-spawn fallback, fabricated activation token or success toast.
- [ ] T037 [US3] Register app.accounts in `crates/mailbag/src/main.rs` and connect both menu and status-button activation in `crates/mailbag/src/account_ui.rs` to the shared launcher; show failures through the bounded toast owner and retain the last launch error in the status explanation so account notices cannot hide it. Test both entry points and keyboard access in `crates/mailbag/src/account_ui/settings_tests.rs`.
- [ ] T038 [P] [US3] Add only --talk-name=org.gnome.OnlineAccounts and --talk-name=org.gnome.Settings to `io.github.mitinand.Mailbag.yml`; inspect `meson.build` and `scripts/build-flatpak.sh` to verify the path crate compiles into Mailbag and locked Rust dependencies are vendored. Keep native GIO/GLib in runtime 50, with host GOA/Settings and no libgoa, extra daemon or network permission.
- [ ] T039 [US3] STOP for portion 6: run `scripts/check.sh` and `cargo test --locked -p mailbag settings::`, verify both entry-point tests in `crates/mailbag/src/account_ui/settings_tests.rs`, and report source-manifest/build evidence for PR 2 under `AGENTS.md`; distinguish action acknowledgement from actual panel presentation, then wait for review.

## Phase 6: Polish and cross-cutting acceptance (portion 7, PR 2)

**Purpose**: Supply the remaining integration evidence and fix observed defects,
without expanding scope or repeating already adequate checks without a reason.

- [ ] T040 Complete combined client-to-UI tests in `crates/mailbag/src/account_ui/integration_tests.rs` using `tests/support/goa.rs`: cover the acceptance matrix in `specs/001-goa-account-observation/quickstart.md`, 30-account updates within the 250-ms UI target after acceptance, continuous updates yielding to unrelated GTK work, shutdown under load, and Settings failures during exclusion bursts. Fix evidenced integration defects in the responsible module and rerun affected tests.
- [ ] T041 Run the graphical/input matrix from `specs/001-goa-account-observation/quickstart.md` against `crates/mailbag/resources/ui/mailbag.ui` and `folder-row.ui`: keyboard/focus, hover/click/touch/Enter/Space, both actions, 360-width and 720sp/1100sp behavior, enlarged text/high contrast and approved geometry. Fix demonstrated regressions and report unavailable input paths as unverified.
- [ ] T042 Build/install with `scripts/build-flatpak.sh --install` and perform the installed checks in `specs/001-goa-account-observation/quickstart.md` on Fedora 44/GNOME 50/Wayland/x86_64/runtime 50: discover the existing Generic IMAP account, inspect installed permissions, open actual Settings from both entry points with Settings closed/open/another workspace, and check launch-only permission denials. Use a disposable maintainer-provided empty-account setup for the host status button; never remove personal accounts to reach it. Report absent environment evidence as pending.
- [ ] T043 Review changed code and `io.github.mitinand.Mailbag.yml` for unnecessary abstractions, unclear names, unsafe diagnostics and scope creep; update `README.md` with actual test prerequisites (including dbus-daemon), commands and integration permissions, and align `specs/001-goa-account-observation/quickstart.md` with implemented test entry points. Confirm no credentials, mail access/storage/deletion, Welcome flow, extra runtime or whole-bus recovery was introduced.
- [ ] T044 STOP for portion 7 and PR 2: run `scripts/check.sh` and verify actual behavioral test counts; hand over the evidence required by `specs/001-goa-account-observation/quickstart.md` under `AGENTS.md`, separating synthetic, graphical and installed results. Prepare an English PR-ready summary in the handoff, with no diary file, commit or PR creation; leave unmet acceptance tasks unchecked and wait for maintainer review.

## Dependencies and execution order

```text
Scope review (T001–T002)
  -> approved workspace and initial client (T003–T010)
  -> account changes, recovery and periodic checks (T011–T018)
  -> shared account rules (T019–T023) -> PR 1 review
  -> US1 presentation (T024–T028)
  -> US2 interaction and notices (T029–T033) -> portion 5 review
  -> US3 Settings and packaging (T034–T039) -> portion 6 review
  -> combined/installed acceptance (T040–T044) -> PR 2 review
```

Every arrow across a STOP also requires explicit maintainer instruction. Foundation
blocks all story integration. US2 presentation depends on US1's list; US3's launcher
can be tested without GOA, but its UI wiring depends on the existing status controls.
The complete US1 empty-state button behavior needs US3; installing any story also
needs T038 packaging. These are real dependencies, not separate deployable releases.

Within a portion, tests precede the behavior they verify. T006 and T008 can be
written together after T005; T007 supplies fields consumed by T009. T011–T013 can
be written together after T010, followed by T014–T017. T019–T020 can be written
together, followed by the shared policy implementation. Run validation after edits
settle, not concurrently with changes to its inputs.

### Execution order and parallel opportunities by story

| Story | Execution order / parallel work | Prerequisite / limit |
|---|---|---|
| US1 | Write T024 tests and confirm they fail before implementing T025 account model/row binding | PR 1 reviewed; keep T025–T028 source edits sequential |
| US2 | Write T029 tests and confirm they fail before implementing T030 row/focus handling | T028 complete; T031–T032 follow source edits sequentially |
| US3 | T035 launcher tests and T038 manifest/build inspection | T034 complete and portion 6 authorized; no shared edited files; T036 follows the tests |

These are optional scheduling opportunities, not a reason to add dependencies,
create more agents or bypass maintainer review.

## Coverage map

| Requirement / scenario | Shared logic and tests | User-visible / installed checks |
|---|---|---|
| FR-001–FR-004; US1 account/provider/identity matrix | T006–T009, T019, T021 | T024–T025, T042 |
| FR-005; truthful status, no Welcome or fake mail | T019–T021 | T024, T026–T028, T031, T041 |
| FR-006–FR-009; changes, partial failures, recovery | T006–T017, T019–T022 | T029–T031, T040 |
| FR-010; selection and actual versus superseded exclusion | T019–T022 | T025, T029–T030, T040 |
| FR-011; Settings success/failure | T034–T036 | T037, T039, T042 |
| FR-012; retry, ten-second checks, responsiveness and stop | T008–T017, T019, T021 | T028–T031, T035–T037, T040 |
| FR-013; no credentials, mail or persistence; safe diagnostics | T005–T009, T021–T022 | T038, T042–T044 |
| FR-014–FR-015; approved UI and accessible actions | T024, T029 | T025–T033, T037, T040–T041 |
| FR-016; actual sandbox integration | T038 | T042–T044 |
| FR-017; combined notices without event history | T020, T022 | T029, T032, T040–T041 |
| SC-001–SC-003; account matrix, lifecycle and explanations | T006–T022 | T024–T033, T040, T042 |
| SC-004–SC-005; actual Settings and input access | T034–T037 | T041–T042 |
| SC-006–SC-008; responsiveness, scope and notices | T011–T022 | T028–T044 |

## Implementation strategy

The first demonstrable UI increment is US1 over the fully tested PR 1 foundation.
It shows existing accounts and explanations without pretending to read mail. It is
an internal demo, not completed F01: US2 recovery interactions, US3 Settings and
installed acceptance remain required. Finish the agreed portions in order and
retain their review stops rather than making a separate MVP PR.

Use `cargo test --locked -p goa-adapter`, `cargo test --locked -p mailbag accounts::`
and `cargo test --locked -p mailbag settings::` for the relevant headless cases.
Use the separate graphical command and installed sequence in quickstart. Skipped
or zero tests are not evidence of the behavior. Public handoffs contain synthetic
identities only and identify the tested build/environment without private data.
