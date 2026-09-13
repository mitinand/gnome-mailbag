# Implementation Plan: GNOME Mail Accounts

**Branch**: `codex/goa` | **Date**: 2026-09-12 | **Spec**: [spec.md](spec.md)

**Feature**: F01 / `001-goa-account-observation`

**Status**: GOA client and account display/selection rules implemented through portion 4.
The approved account-contract refactoring (portion 4b) is complete and ready for PR 1 review.
GTK integration and subsequent portions remain pending. Historical product documents are references; the clarified feature
specification and the latest accepted review decisions take precedence.

## Summary

Mailbag will list supported mail accounts from GNOME Online Accounts, explain
problems and open the system account settings. Account changes will appear without
restarting the application. The existing layout stays in place.

The UI uses the latest confirmed account state. If Mail is switched off and back
on before the UI updates, the account stays visible and selected, with no toast.
If an update actually hides the account, its selection clears and a toast explains
why. We do not keep a history of intermediate switches.

F01 only hides accounts. It does not store or delete mail, acquire credentials,
connect to a mail server or show a Welcome/synchronization screen.

Deliver this feature in **two sequential PRs, about eight substantive commits**.

## Technical Context

| Item | Choice |
|---|---|
| Language | Rust 2024; repository toolchain 1.95.0 |
| Existing UI libraries | libadwaita 0.9.2, GTK bindings 0.11.4 |
| GOA access | Existing GIO/GLib family, version 0.22.9 from Cargo.lock |
| Application structure | mailbag, goa-adapter and a dependency-free account-source data crate |
| Storage | Account state and selection in memory for the current run only |
| Supported test environment | Fedora 44, GNOME 50, Wayland, x86_64, GNOME runtime 50 |
| Scale fixture | 30 accounts, including identical display names |
| Checks | Rust tests with fake services, graphical tests, installed Flatpak checks |

GOA calls run on a dedicated GLib thread so they continue while GTK is busy.
New data wakes the UI to read the newest account list; there is no frequent command/UI polling or queue of every update.
A separate health check asks GOA for its account list every ten seconds when no
check is already pending. Each check has a five-second deadline. Failed checks wait
for a GOA event, the next periodic check or a manual Retry Check. Detailed limits and shutdown
rules are in [the GOA contract](contracts/observation.md).

The UI target is to display an accepted update within 250 ms with the 30-account
fixture and a normally running graphical event loop. This is a test target, not a
claim that GOA itself always responds that quickly.

## Constitution Check

Reviewed against constitution **2.1.0** before finalizing this revised design.
These are design checks; implementation acceptance is still pending.

| Principle | How the design meets it |
|---|---|
| I. Necessary complexity only | Keep the newest account state; no history of switches. Compare direct GIO use with a GOA client dependency in research. |
| II. Clear language and concrete names | Start with user behavior; use account/action names; keep D-Bus details in the contract. |
| III. Explicit failures and truthful state | Distinguish failed checks from empty accounts and real removal; preserve error causes; no mail/authentication claims. |
| IV. One owner per business rule | The accounts module decides which rows to show; widgets only present its result. |
| V. Responsive, bounded work | Dedicated GOA thread, one current update, finite request deadlines, one periodic check at a time and no GTK wait on shutdown. |
| VI. Evidence before completion | Test error cases with fake services; check actual Flatpak discovery and Settings separately. |
| Language and governance | Repository text stays English; no unrelated architecture or implementation changes. |

No constitution exception is needed.

## Project Structure

### Feature documents

```text
specs/001-goa-account-observation/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── tasks.md
├── contracts/
│   ├── accounts.md
│   ├── observation.md
│   └── ui.md
└── checklists/requirements.md
```

Each supporting document has one purpose: research explains choices, the data
model names the state, contracts define exact behavior at component boundaries,
and quickstart describes how to test it. [tasks.md](tasks.md) defines execution
order and the maintainer review stops.

### Planned source files

```text
crates/
├── account-source/           # Shared account data and validity checks
├── goa-adapter/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs            # Client handle
│       ├── accounts.rs       # Account fields and validation
│       └── client.rs         # D-Bus calls, changes, retries and shutdown
└── mailbag/
    ├── Cargo.toml
    ├── src/
    │   ├── main.rs           # Application wiring
    │   ├── accounts.rs       # Which accounts to show and select
    │   ├── account_ui.rs     # Rows, explanations and toasts
    │   └── settings.rs       # Open Online Accounts
    └── resources/ui/
        ├── mailbag.ui
        └── folder-row.ui
tests/support/               # Fake services, used only by tests
```

Unit tests live beside the behavior they test. Shared fake-service helpers do not
need another crate or a production account-injection feature.

## Phase 0: Research Decisions

[research.md](research.md) records the dependency comparison, GIO behavior and
primary sources. The important decisions are:

- Use GIO directly; do not add a GOA binding or Tokio for F01.
- Treat MailDisabled, Mail-interface presence and AttentionNeeded separately.
- After a GOA restart, verify the account list before deciding anything was removed.
- Keep only the latest accepted account state for the UI; do not replay brief changes.
- Open Settings through its asynchronous D-Bus action with visible failure handling.

## Phase 1: Application Design

### Responsibilities

`account-source` owns the shared data and validity checks described in
[the account contract](contracts/accounts.md). It contains no transport, GTK/GIO,
commands, threads or recovery. Both other crates depend on it. The application
startup code selects the GOA adapter; account rules import only `account-source`.
There is one `AccountUpdate` format, including inside the adapter. No parallel GOA
DTO set, universal trait, source registry or delivery mechanism is introduced.


`goa-adapter` gets GOA account fields, follows changes and reports failures. It
knows nothing about selected rows or toast messages. Its client uses one background
thread and sends ordinary Rust data to Mailbag.

`mailbag::accounts` decides whether each account can be shown, preserves known rows
during temporary problems and compares the previous visible list with the latest
state. It produces selection changes and notices when rows are actually hidden.

`mailbag::account_ui` updates existing GTK rows by account ID. Display-name changes
do not replace an account or move selection. `mailbag::settings` handles both ways
of opening system settings: the existing menu item and the account-empty button.

### Account events and the ten-second check

Subscribe before accepting the initial account list, then keep listening throughout
the run. Normal changes update Mailbag through events without waiting for a timer.

| D-Bus signal | What Mailbag learns |
|---|---|
| `org.freedesktop.DBus.NameOwnerChanged`, filtered to `org.gnome.OnlineAccounts` | GOA appeared, disappeared or was replaced; recheck after recovery |
| `org.freedesktop.DBus.ObjectManager.InterfacesAdded` | An account or its Mail interface appeared; check whether it can be shown |
| `org.freedesktop.DBus.ObjectManager.InterfacesRemoved` | An account or interface disappeared; verify removal before hiding a row |
| `org.freedesktop.DBus.Properties.PropertiesChanged` | Account or Mail fields changed, including MailDisabled, AttentionNeeded and display information |

Use direct GIO signal subscriptions on the private worker, with no ObjectManager
proxies or duplicate handlers. [Research](research.md#3-proving-that-an-account-was-removed)
records the approved change and its independent-context test evidence.
MailDisabled=true and a missing Mail interface retain their different meanings.
The [GOA contract](contracts/observation.md#events-to-subscribe-to) lists exact paths,
properties, callbacks and restart ordering.

Every ten seconds, if idle, use the existing GetManagedObjects request to check that
GOA still answers and to refresh account data. This detects a process that remains
present but stops answering, and repairs differences even if an event was missed.
Skip a tick while another check is pending; do not queue extra checks.
A routine check leaves confirmed rows and selection unchanged while pending, with
no loading flash or success toast. Errors follow the same failure rules below.

### Failure and recovery

A failed check does not mean there are no accounts. Keep known rows and selection
with problem indicators; a cold start has no earlier-run rows to restore.

If one account has bad data but the list itself is trustworthy, keep the other
accounts working. A missing Mail interface does not prove that Mail was disabled.
Apply an explicit disabled value without waiting for unrelated accounts to recover.

Mailbag must ignore replies from the previous GOA process. Check which
process and request a reply belongs to before accepting it. Waiting for recovery
also has a deadline. [The GOA contract](contracts/observation.md) contains the exact
request checks and the tests for them.

### UI

Use the existing account list, row form, status area and toast overlay. Show no
fictional folders, message counts or synchronization. Empty account states have
an Online Accounts button. Failed GOA checks have Retry Check.

A problem icon opens the same explanation by mouse, touch or keyboard; hover also
shows a tooltip. Keep selection and focus when data changes. A hidden selected
account leaves no selection; another account is not chosen automatically.

At narrow widths, show the existing list page so the GOA explanation is visible.
Keep approved dimensions, spacing, breakpoints and menu placement. See
[the UI contract](contracts/ui.md) for the state table and input behavior.

### What ships in Flatpak

| Part | How it is supplied |
|---|---|
| goa-adapter | Our Rust code, compiled into the Mailbag executable |
| Rust gio/glib dependencies | Existing Cargo.lock versions, included by the current dependency-vendoring step |
| Native GIO/GLib libraries | GNOME runtime 50 |
| GOA daemon and GNOME Settings | Existing services on the user's GNOME desktop |

There is no separate adapter executable, bundled GOA daemon or libgoa dependency.
The approved manifest already includes `crates/`, and Meson builds package mailbag
with its path dependencies. Add only the two named permissions from
[the UI contract](contracts/ui.md); no network or broad host access is needed.

### Work kept outside F01

Hiding an account and deleting stored mail are separate functions. F01 implements
hiding only. A later storage feature must decide retention/deletion rules and safe
handling of interrupted operations; this plan does not prescribe automatic cache
wiping when Mail is switched off.

Recovering the entire desktop session bus is outside F01; the GOA restart tests keep that bus running.

The accepted future rule about continuing mail work with usable credentials during
a GOA-only outage remains in the specification. It introduces no mail access here.
Welcome and initial synchronization are also future work.

## PRs and Commits

First incorporate the approved UI/workspace commit `7a69c49` into the implementation
checkout, preserving specifications and unrelated work. The planning checkout was
based on `7083b8a`; T003 verifies baseline ancestry before source implementation.

| PR | Result | Commits |
|---|---|---:|
| 1. GOA accounts and state handling | Reviewed documents, working client, restart/error handling, account rules and tests | About 5 |
| 2. Accounts in the existing UI | Rows and explanations, Settings/Flatpak integration, full acceptance and fixes | About 3 |

Follow [AGENTS.md: Commits, PRs and review pauses](../../AGENTS.md#commits-prs-and-review-pauses)
for every agreed portion below, and carry those boundaries into task generation.

Suggested commit content:

1. PR 1: specification and reviewed design.
2. PR 1: account client and initial tests.
3. PR 1: restart, errors, retries, latest-state delivery and shutdown tests.
4. PR 1: account display/selection rules and tests.
5. PR 2: UI rows, explanations, retry and account-hidden notices.
6. PR 2: Settings action, failure tests and Flatpak permissions.
7. PR 2: remaining integration tests and necessary fixes.

Tests accompany behavior. Eight commits is an estimate; manual evidence alone does
not require an empty commit or a separate diary. Each PR runs `scripts/check.sh`.

## Acceptance

| What must work | Main evidence | PR |
|---|---|---|
| Provider/account classification, distinct IDs, 30 accounts | Synthetic account tests | 1 |
| Empty versus unavailable GOA; one account error isolated | Client and account-rule tests | 1 |
| Each subscribed event, startup changes, restart and late replies | Fake GOA service tests; events update before the next health check | 1 |
| Silent GOA hang, missed event and later recovery without restart | Ten-second check, timeout, no parallel checks and continued periodic recovery | 1 |
| Brief changes superseded before UI update | Row remains selected; no false toast | 1 and 2 |
| A row actually hidden, then later restored | One toast; selection stays cleared | 1 and 2 |
| Retry, load and shutdown without GTK waiting | Independent-thread and resource-limit tests | 1 |
| Stable rows/focus, problem actions and narrow window | Graphical tests and manual checks | 2 |
| Settings failure and actual panel opening | Fake Settings tests plus installed Flatpak | 2 |
| Existing Generic IMAP account | Installed Flatpak on the agreed GNOME environment | 2 |
| No credentials, mail persistence, fake mail or extra permissions | Test fixture call checks and integration inspection | Both |

[quickstart.md](quickstart.md) gives commands and expected results. Synthetic tests
cannot establish installed-host compatibility. Real account removal is not required;
any real Mail toggle or GOA restart is supervised by the maintainer.

## Complexity Tracking

No exceptions. One client crate isolates D-Bus details. The latest-state exchange
keeps one pending account update and replaces it when newer data is accepted.
Packaging reuses the existing workspace, build scripts and runtime.
