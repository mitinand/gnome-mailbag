# Tasks: GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation`
**Revised**: 2026-09-16 · **Status**: Account presentation cleanup ready for review; manual acceptance remains

This list replaces the previous completed implementation/mechanism tasks after the
new maintainer decisions. Portions A and B are implemented. Automated and graphical
checks have passed; the remaining manual and installed cases are unchecked below.
Component tests do not establish installed acceptance.

[Spec](spec.md) owns behavior, [plan](plan.md) owns boundaries, contracts own details,
and [quickstart](quickstart.md) owns commands and acceptance scenarios.
Follow [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one agreed
portion, run its checks, report and stop. A checked handoff does not mean approval.
The maintainer creates commits and PRs. Do not start code before document approval.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T069–070 | docs(goa): simplify F01 observation design | Current F01 PR |
| A. Account observation and UI | T071–081 | refactor(goa): observe full account lists in the main context | Current F01 PR |
| B. Settings | T082–085 | refactor(settings): simplify Online Accounts launch | Current F01 PR |
| C. Acceptance | T086–089 | docs(goa): track remaining manual acceptance | Current F01 PR |
| D. Test cleanup | T090–091 | test(goa): remove redundant coverage and simplify fixtures | Current F01 PR |
| E. Account presentation cleanup | T092–094 | refactor(accounts): centralize status and simplify selection | Current F01 PR |

## Phase 1: documents and review

- [X] T069 Revise specs/001-goa-account-observation/spec.md, plan.md, contracts/, research.md, data-model.md, quickstart.md and tasks.md for the accepted decisions and constitution 2.2.0.
- [X] T070 STOP: present the document diff and transport evidence; leave every code task below pending until explicit maintainer approval under AGENTS.md.

## Phase 2: shared foundation for portion A

Keep the existing workspace, build tooling and private-bus fixtures. No setup
framework or temporary compatibility interface is needed.

- [X] T071 Adapt tests/support/goa.rs and tests/support/bus.rs for synchronous full snapshots, property/interface triggers and service replacement; retain private-bus isolation, synthetic identities, outer deadlines and cleanup.

## Phase 3: US1 — see available accounts (P1, portion A)

**Goal:** Typed account records, correct initial/empty/problem states and unchanged
approved UI. **Independent check:** supply accepted and rejected full lists without
Settings or mail access; verify stable IDs, labels, selection and missing Mail.

- [X] T072 [US1] Keep required-field/optional-string parsing in crates/goa-adapter/src/accounts/tests.rs and whole-read rejection in crates/goa-adapter/src/client/tests.rs; remove standalone model and cross-component tests.
- [X] T073 [US1] Simplify crates/goa-adapter/src/account_model.rs and crates/goa-adapter/src/accounts.rs under contracts/accounts.md: remove unknown required fields, invalid_fields, unused metadata/domain/code and custom data-limit machinery.
- [X] T074 [US1] Update crates/mailbag/src/accounts.rs and crates/mailbag/src/accounts/tests.rs for one list error, retained rows, cold-start missing Mail with Retry, and typed provider/boolean fields; preserve label, icon and selection policy.

## Phase 4: US2 — trust account changes (P1, portion A)

**Goal:** Full reads follow GOA changes and preserve truthful UI through failures.
**Independent check:** a private GOA service drives the observer; AccountList unit tests separately
assert rows, selection, errors and notices.

- [X] T075 [US2] Adapt crates/goa-adapter/src/client/event_tests.rs and crates/goa-adapter/src/client/tests.rs for changes during reads, one follow-up after coalesced triggers, failure preservation, Retry, owner replacement and successful removal. Use producer-valid message sequences.
- [X] T076 [US2] Implement main-context asynchronous full reads and signal triggers in crates/goa-adapter/src/client.rs under contracts/observation.md, and remove crates/goa-adapter/src/client/worker_state.rs; retain only one active operation and refetch_needed for read scheduling.
- [X] T077 [US2] Update crates/goa-adapter/src/lib.rs and crates/mailbag/src/main.rs for GoaAdapter::start(on_update), manual refresh and cleanup. Remove GoaUpdates, worker/exchange files and SourceStopped; keep callbacks and widget updates in the main context.
- [X] T078 [US2] Update crates/mailbag/src/account_ui.rs, crates/mailbag/src/account_ui/tests.rs and crates/mailbag/src/accounts/notice_tests.rs for successful removal without a global error, manual-only “Checking…”, initial loading, last-row empty state and one ordinary toast per applied exclusion. Preserve approved geometry, icons and focus behavior.
- [X] T079 [US2] Remove mechanism-only tests under crates/goa-adapter/src/client/ and obsolete receiver doctests in crates/goa-adapter/src/lib.rs: counters, Mutex/Waker exchanges, independent-worker progress, polling, shutdown drain periods, data limits and partial-record rescue. Retain failure, timeout and cancellation behavior tests.
- [X] T080 [US2] Retain account_ui_transitions in crates/mailbag/src/account_ui/tests.rs for row reuse, hover without selection and focus after removal; remove graphical subprocess cases.
- [X] T081 [US2] STOP: run scripts/check.sh, git diff --check and the graphical cases in specs/001-goa-account-observation/quickstart.md. Review constitution I/II, report limits and hand over portion A; wait before portion B.

## Phase 5: US3 — open system account management (P2, portion B)

**Goal:** Both existing entry points share one launch; failure produces one ordinary
toast. **Independent check:** the private Settings fixture verifies parameters,
exact action parameters, repeated activation, one error notification and another attempt.

- [X] T082 [US3] Update crates/mailbag/src/settings/tests.rs for one pending launch, exact action parameters, one failure notification and another attempt. Preserve private-bus tests and exact action parameters.
- [X] T083 [US3] Simplify crates/mailbag/src/settings.rs under contracts/ui.md: use one pending flag, the D-Bus method timeout and a weak task reference; remove the overall deadline, stopped flag, stop/Drop and stored task handle.
- [X] T084 [US3] Update crates/mailbag/src/main.rs, crates/mailbag/src/account_ui.rs and crates/mailbag/src/account_ui/tests.rs to present launch failures only through AdwToastOverlay. Remove retained Settings status and its effect on account pages; keep both entry points.
- [X] T085 [US3] STOP: run scripts/check.sh, git diff --check and relevant graphical checks from specs/001-goa-account-observation/quickstart.md; hand over portion B and wait before portion C.

## Phase 6: combined acceptance (portion C)

- [X] T086 Run all automated and graphical cases in specs/001-goa-account-observation/quickstart.md on the revised implementation. Verify user-visible intermediate states, cancellation and scope; do not replace them with mechanism assertions.
- [X] T087 Run the manual input/display matrix in specs/001-goa-account-observation/quickstart.md: keyboard, touch, narrow layout, enlarged text, high contrast and focus. Record unavailable cases as unverified. Completion confirmed by the maintainer.
- [X] T088 Run installed Flatpak acceptance in specs/001-goa-account-observation/quickstart.md and verify io.github.mitinand.Mailbag.yml permissions, actual GOA discovery and both Settings entry points. Do not claim real-account acceptance when no suitable account exists. Completion confirmed by the maintainer, including installed account activation, Settings menu/button activation and cross-workspace presentation.
- [X] T089 STOP: run scripts/check.sh and git diff --check, review constitution I/II and hand over portion C. Keep unmet acceptance unchecked in specs/001-goa-account-observation/tasks.md and report it; do not declare F01 complete from synthetic tests alone.

## Phase 7: test cleanup (portion D)

- [X] T090 Remove duplicate F01 tests and unused fixture mechanisms. Keep distinct decoding, presentation, private-bus integration and visible GTK behavior checks under contracts/observation.md#verification; remove the graphical observation/closure subprocess and activation fixtures under quickstart.md.
- [X] T091 STOP: run scripts/check.sh, the graphical cases and git diff --check; review constitution I/II and report removed coverage, remaining checks and limitations. Leave T087/T088 open and wait for maintainer review.

## Phase 8: account presentation cleanup (portion E)

- [X] T092 Remove the unused availability classification and unused selection-command variants in crates/mailbag/src/accounts.rs. Let AccountPage own read-error priority and simplify crates/mailbag/src/account_ui.rs accordingly; preserve visible selection, problem indicators and Retry behavior.
- [X] T093 Fix account-empty guidance for unsupported providers with disabled Mail. First reproduce the misleading enable-Mail advice in the existing model/GTK tests, then classify provider support before Mail enablement under contracts/accounts.md.
- [X] T094 STOP: run scripts/check.sh, the graphical cases and git diff --check; review the final diff under constitution I/II and hand over portion E. Keep T087/T088 open and wait for maintainer review.

## Dependencies and parallel opportunities

Documents → explicit approval → portion A → review → portion B → review → portion C.
Within A, US1 data changes precede US2 transport/application integration. They ship
as one buildable portion because the observer's public data and consumers change
together. Settings is independently testable and remains a separate portion.

For US1, field decoding and AccountList assertions may be prepared independently
once the contract is fixed. For US2, transport and graphical test review may run
independently after wiring is available. For US3, private protocol checks and UI
toast checks are independent after integration. Shared-file edits remain sequential;
these opportunities never override review pauses or authorize additional agents.

The first useful increment is portion A. No UI redesign, credentials, mail sync,
background lifetime or new runtime is included in any portion.
