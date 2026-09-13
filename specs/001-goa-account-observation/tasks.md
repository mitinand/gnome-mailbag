# Tasks: GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation` · **Branch**: `codex/goa`
**Status**: Portions 1–5 implemented and checked; portion 5 awaiting maintainer review.

This is the execution list. [Spec](spec.md) owns behavior; [plan](plan.md) owns
component boundaries; the [GOA](contracts/observation.md),
[account](contracts/accounts.md) and [UI](contracts/ui.md) contracts own details.
[Quickstart](quickstart.md) owns validation commands and installed acceptance.

Follow [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one
agreed portion with its tests and required checks, then stop for explicit maintainer
review. A checked STOP records handoff, not permission to proceed. The maintainer
creates commits and PRs. Tests precede the behavior they verify. No task authorizes
spawning agents or crossing a review boundary.

| Portion | PR | Tasks | Suggested commit subject |
|---|---|---|---|
| 1. Design | 1 | T001–002 | docs: define GOA account observation |
| 2. Initial client | 1 | T003–010 | feat(goa): read accounts on a dedicated GLib thread |
| 3. Changes and recovery | 1 | T011–018 | feat(goa): follow changes and recover failed checks |
| 4. Account rules | 1 | T019–023 | feat(accounts): define availability and selection rules |
| 4b. Shared contract | 1 | T045–049 | refactor(accounts): separate account data from GOA |
| 4c. Review fixes | 1 | T050–057 | refactor(goa): simplify account observation and state handling |
| 5. Account UI | 2 | T024–033 | feat(ui): show GOA accounts and explain changes |
| 6. Settings | 2 | T034–039 | feat(settings): open Online Accounts from Mailbag |
| 7. Acceptance | 2 | T040–044 | test: verify GOA integration and account UI |

## Completed PR 1 foundation

- [X] T001 Check feature documents against the accepted scope and constitution.
- [X] T002 STOP: hand over design for PR 1 review under AGENTS.md.
- [X] T003 Verify the approved baseline 7a69c49 is present; run scripts/check.sh.
- [X] T004 Create the GOA workspace crate with the locked GIO/GLib dependencies.
- [X] T005 Add isolated D-Bus/GOA fixtures in tests/support with deadlines and cleanup.
- [X] T006 Add decoder field, identity and data-limit tests.
- [X] T007 Implement GOA account decoding and safe errors under contracts/observation.md.
- [X] T008 Test initial acquisition, read-only protocol, failures and independence from GTK.
- [X] T009 Implement the dedicated worker, acquisition, commands and latest-update receiver.
- [X] T010 STOP: run required checks and hand over portion 2 for PR 1 review.
- [X] T011 Test relevant events, ordering, process replacement, activation and stale replies.
- [X] T012 Test delivery/command races, bounded bursts, data limits and shutdown.
- [X] T013 Test periodic scheduling, silent hangs, recovery and command coalescing.
- [X] T014 Implement event handling and guarded account acquisition under the GOA contract.
- [X] T015 Implement event-driven latest-state delivery with no update history or polling.
- [X] T016 Implement the ten-second idle health check and recovery.
- [X] T017 Complete cancellation, teardown and unexpected worker-exit handling.
- [X] T018 STOP: run required checks and hand over portion 3 for PR 1 review.
- [X] T019 Test provider eligibility, display, statuses and selection in mailbag::accounts.
- [X] T020 Test applied exclusions, notice grouping and reappearance.
- [X] T021 Implement AccountList and its row/selection rules in crates/mailbag/src/accounts.rs.
- [X] T022 Implement AccountHiddenNotice generation under FR-017.
- [X] T023 STOP: run required checks and hand over portion 4 for PR 1 review.
- [X] T045 Update the accepted documents for the source-independent account boundary.
- [X] T046 Test shared validity and GOA translation without moving provider policy into the adapter.
- [X] T047 Define one normalized account format for GOA and application rules (now in goa-adapter::account_model after review).
- [X] T048 Preserve the single worker/receiver design and keep GOA protocol details out of account rules.
- [X] T049 STOP: run required checks and hand over portion 4b for PR 1 review.

## Portion 4c: approved review fixes (PR 1)

- [X] T050 Update contracts first: stateless identity validation, separate pending/result state and shared snapshots.
- [X] T051 Add regression tests for conflicting paths/IDs, manual pending state and irrelevant events.
- [X] T052 Separate decoding from worker reconciliation; remove old-path identity reconstruction.
- [X] T053 Unify data-limit validation and prepare property candidates without rollback.
- [X] T054 Remove the public publication counter and deep copy at receipt; update AccountList to borrow snapshots.
- [X] T055 Replace redundant notice assertions with adapter-to-AccountList tests for superseded/applied changes.
- [X] T056 Consolidate feature documents around their owners; align statuses and acceptance references.
- [X] T057 STOP: run scripts/check.sh and relevant integration/graphical checks, review principles I/II, and hand over all seven fixes for PR 1. Maintainer creates the commit/PR; wait before portion 5.

## Portion 5: accounts in the existing UI (PR 2)

- [X] T024 [US1] Add graphical account/status/input tests under contracts/ui.md.
- [X] T025 [US1] Bind stable account rows by AccountId using the approved forms and selection policy.
- [X] T026 [US1] Connect AccountPage to the existing status area; preserve the approved layout.
- [X] T027 [US1] Wire the agreed problem button, tooltip and accessible explanation.
- [X] T028 [US1] Wire GOA startup/receiver and GTK consumer lifetime; yield between updates.
- [X] T029 [US2] Test graphical changes, retained focus/selection, retry and notices.
- [X] T030 [US2] Apply AccountList results in place; handle disappearing rows/icons and popovers.
- [X] T031 [US2] Connect both retry entry points using the shared pending/result contract.
- [X] T032 [US2] Present notices through the bounded toast owner in contracts/ui.md#notices.
- [X] T033 [US2] STOP: run headless and graphical checks and hand over portion 5 for PR 2 review.

Portion 5 validation: `scripts/check.sh` passed (84 unit tests and two
compile-fail doctests); both graphical cases in quickstart.md passed separately.
The UI cases use synthetic snapshots and cover status, row identity, selection,
focus, popovers, retry, notices and collapsed navigation. Physical keyboard/touch,
enlarged text/high contrast, combined load/shutdown and installed-host acceptance
remain for portions 6–7. The Online Accounts button targets `app.accounts`; its
launcher is intentionally still T034–T037. No installed compatibility is claimed.

## Portion 6: Settings and packaging (PR 2)

- [ ] T034 [US3] Add private Settings fixtures using tests/support/bus.rs.
- [ ] T035 [US3] Test the Settings protocol, failures, coalescing and cancellation.
- [ ] T036 [US3] Implement the shared async Settings launcher under contracts/ui.md.
- [ ] T037 [US3] Wire/test both Settings entry points and persistent failure explanations.
- [ ] T038 [US3] Add the two named Flatpak permissions and verify path-crate packaging.
- [ ] T039 [US3] STOP: run required checks and hand over portion 6 for PR 2 review.

## Portion 7: combined acceptance (PR 2)

- [ ] T040 Run combined client/UI tests, the 30-account 250-ms target, yielding and shutdown under load.
- [ ] T041 Run the graphical/input acceptance matrix in quickstart.md; report unavailable cases.
- [ ] T042 Run installed Flatpak acceptance in quickstart.md on the supported GNOME environment.
- [ ] T043 Review scope/diagnostics, update README prerequisites and align test commands.
- [ ] T044 STOP: run required checks and hand over portion 7 for PR 2 review; leave unmet acceptance unchecked.

## Dependencies and acceptance

Complete portion 4c and PR 1 review before GTK work. Portion 5 builds US1 and US2
presentation on the shared rules; portion 6 supplies the Settings action and
packaging required by the empty-state button. Portion 7 establishes installed
acceptance. No earlier portion claims complete F01 or installed compatibility.

Each feature criterion SC-001–008 is mapped to its requirements in
[spec.md#acceptance](spec.md#acceptance). Tests accompany the responsible component;
combined tests verify boundaries, and T041–042 establish graphical/host evidence.
Do not duplicate that matrix here or treat skipped/zero tests as passing evidence.

## Phase 8: Convergence

These completion checks trace the remaining PR 2 work already scheduled in
portions 5–7. Complete the referenced tasks in their original order and mark each
corresponding convergence item when its acceptance evidence is available. Preserve
the review pauses at T033, T039 and T044, and complete PR 1 review before starting
portion 5.

- [X] T058 CRITICAL: Connect the existing GOA adapter and receiver to the application through T028; verify the path dependency in crates/mailbag/Cargo.toml, startup in crates/mailbag/src/main.rs, GTK yielding and receiver/shutdown lifetime per FR-001, FR-006, FR-012 and plan: GOA application wiring (missing).
- [X] T059 Bind AccountList to stable GTK account rows and ID-based selection through T025 and T030 in mailbag::account_ui; verify updates, retained focus and confirmed exclusion per FR-003, FR-004 and FR-006–010 (partial).
- [X] T060 Present AccountPage and account problems, and connect accessible status actions and Retry Check through T026, T027 and T031 in mailbag::account_ui per FR-005, FR-008, FR-009, FR-012, FR-014 and FR-015 (partial).
- [X] T061 Present AccountHiddenNotice through the bounded toast owner in T032; verify single/group wording and notifications for applied exclusions per FR-017 and SC-008 (partial).
- [ ] T062 Implement and test the shared Settings launcher and both entry points through T034–T037 in mailbag::settings, including safe failures, coalescing and cancellation per FR-011, SC-004 and plan: Settings launch (missing).
- [ ] T063 Complete T038 in io.github.mitinand.Mailbag.yml; add the two named D-Bus permissions and verify packaging of the application path dependencies per FR-016, SC-007 and plan: Flatpak integration (partial).
- [ ] T064 Complete the GTK behavioral/input coverage in T024 and T029 and combined acceptance in T040 and T041; verify focus, status transitions, retry, notices, yielding, shutdown and the 30-account/250-ms target per SC-001, SC-003, SC-005, SC-006, SC-008 and plan: graphical validation (partial).
- [ ] T065 Run supported installed-Flatpak acceptance and complete prerequisite/evidence documentation through T042 and T043; verify actual GOA discovery, both Settings entry points, input access and installed permissions per SC-004, SC-005, SC-007 and plan: installed acceptance (partial).

## Portion 5 review: consolidate the account contract

- [X] T066 Update the accepted boundary: normalized account data belongs to the public goa-adapter contract; no separate account-model crate.
- [X] T067 Move data, validation and their tests into goa-adapter::account_model; export public data types and keep validation helpers internal. Remove the workspace member and dependency entries, and update imports without changing behavior.
- [X] T068 Run scripts/check.sh and the graphical cases, then hand over the current portion for maintainer review under PR 2. No new account service interface is introduced.
