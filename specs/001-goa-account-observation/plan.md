# Implementation Plan: GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation`
**Revised**: 2026-09-15 · **Status**: Approved design; implemented, manual acceptance remains

Apply [the approved decisions](spec.md) to the existing F01 implementation. Keep
the current account UI, icons, build tooling and two named sandbox permissions.
The maintainer creates commits and PRs.

## Responsibilities

| Component | Owns | Contract |
|---|---|---|
| goa-adapter::account_model | Typed account data and safe read errors | [Accounts](contracts/accounts.md) |
| goa-adapter | Full-list reads, subscriptions, one active read and one follow-up flag | [GOA](contracts/observation.md) |
| mailbag::accounts | Visible rows, selection, status and exclusion notices | [Account representation](contracts/accounts.md#application-representation) |
| mailbag::account_ui | Stable GTK widgets, focus, explanations and standard toasts | [UI](contracts/ui.md) |
| mailbag::settings | One pending Settings launch and launch-error reporting | [Settings](contracts/ui.md#settings-launch-protocol) |
| Application wiring | Observer lifetime and applying updates in the main context | [Delivery](contracts/observation.md#delivery-and-shutdown) |

Account rules consume ordinary Rust data exported by goa-adapter. There is no
additional model crate or generic source abstraction. The adapter retains the
last accepted full list; AccountList retains the rows actually displayed.

## Technical choices

Use Rust 2024/toolchain 1.95.0, locked gio/glib 0.22.9 and existing GTK/libadwaita.
The transport was checked against GOA 3.58.1 and native GLib 2.88.3.
Installed Flatpak validation must also record the selected runtime version.

Run asynchronous GOA calls and short update handlers in the application's main
GLib context. Networking waits remain asynchronous; blocking I/O and substantial
processing stay off GTK under constitution V. Observation no longer has to
progress while the main context is not dispatched.

Startup, Retry and GOA changes request a full list. One active read and one
`refetch_needed` flag coalesce overlapping triggers. Call the well-known GOA name
with normal activation and the standard GIO method timeout. A successful typed
reply replaces the list; any read/decoding failure preserves it and records one
error. [The transport contract](contracts/observation.md) owns the exact rules.

Remove the dedicated GOA worker, Mutex/Waker exchange, receiver, SourceStopped,
shutdown drain period, health timer, change counter, owner/path caches and direct
application of signal properties. No ObjectManager client or additional runtime
is introduced. [Research](research.md) records why full reads are sufficient and
why missing Mail remains different from disabled Mail.

Settings uses the same standard toast queue as account notices. Both entry points
share one pending launch under [the Settings contract](contracts/ui.md#settings-launch-protocol);
the UI keeps no separate retained Settings error.

## Delivery and validation

| Portion | Deliverable | Intended PR |
|---|---|---|
| Documents | Revised spec, plan, contracts, research, data model, tasks and quickstart | Current F01 PR |
| A | Typed accounts, full-list observer, application wiring and account UI behavior | Current F01 PR |
| B | Settings launch and toast simplification | Current F01 PR |
| C | Combined graphical and installed acceptance | Current F01 PR |

[Tasks](tasks.md) is the sole remaining execution list. Portion A keeps the
transport and its consumers buildable together and avoids a temporary compatibility
layer. Each portion includes relevant tests and `scripts/check.sh`, followed by
the review pause required by [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses).
No code portion begins before the revised documents are approved.

Keep private-bus GOA and Settings fixtures and user-visible transition tests.
Remove tests that only enforce retired mechanisms. Test asynchronous completion,
failure preservation, follow-up reads and GOA cancellation while dispatching the main
context. Graphical checks cover row identity, hover without selection and focus after removal; installed
checks establish actual GOA/Settings behavior. Fixture counts are not product
limits, and no 30-account/250-ms acceptance target remains. Commands and the
remaining manual matrix are in [quickstart](quickstart.md).

## Constitution check

- **I–II:** Requirements have current user outcomes and supported producer behavior.
  Full reads replace custom reconciliation; use direct names and keep detailed
  protocol decisions in their contract. No speculative recovery requirement remains.
- **III–IV:** Reject a bad read as one error, retain the previous list and separate
  source data, account display policy and widgets. Missing Mail is a valid state.
- **V–VI:** Use asynchronous I/O and short main-context callbacks. Validate actual
  transitions and cleanup; synthetic probes do not establish installed acceptance.

This applies constitution 2.2.0 without an exception. Credential runtimes and mail
synchronization remain outside F01.
