# Implementation Plan: GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation` · **Branch**: `codex/goa`
**Status**: PR 1 review fixes (portion 4c) implemented and checked; awaiting maintainer review. GTK and Settings remain pending.

Implement [the approved requirements](spec.md) in two sequential PRs. Preserve the
existing layout and use the existing GNOME runtime and build tooling.

## Responsibilities

| Component | Owns | Contract |
|---|---|---|
| account-model | Common account fields, last check result and validity checks | [Accounts](contracts/accounts.md) |
| goa-adapter | Stateless decoding, accepted source facts, D-Bus events, checks and delivery | [GOA](contracts/observation.md) |
| mailbag::accounts | Visible rows, selection, status and exclusion notices | [Account representation](contracts/accounts.md#application-representation) |
| mailbag::account_ui | Existing GTK row objects, focus, explanations and toasts | [UI](contracts/ui.md) |
| mailbag::settings | Both entry points for opening Online Accounts | [Settings](contracts/ui.md#settings-launch-protocol) |

Application wiring selects GOA. Account rules depend only on account-model.
The shared crate contains no transport, registry or generic source interface.
GOA runs on one dedicated GLib thread; GTK borrows immutable received snapshots.
The last observation result and pending work have separate meanings. All exact
protocol, identity, deadline and size decisions live in the linked contracts.

## Technical choices

Use Rust 2024/toolchain 1.95.0 and the locked gio/glib 0.22.9 family with existing
libadwaita/GTK. [Research](research.md) records the dependency trade-offs and the
reason for direct subscriptions. No additional runtime or channel dependency is
introduced. Account data and selection remain in memory.

The adapter compiles into Mailbag through path dependencies. The existing Meson,
Flatpak vendoring and runtime provide the remaining build components. PR 2 adds
only the two named permissions in the UI contract. The approved baseline is
`7a69c49`; it is already present in this branch.

## Delivery and validation

| PR | Deliverable | Portions |
|---|---|---|
| 1 | Account contract, GOA client, account rules and component/integration tests | 1–4c |
| 2 | Accounts in the approved UI, Settings, Flatpak and installed acceptance | 5–7 |

[Tasks](tasks.md) is the sole execution list and records commit/review boundaries.
Each portion includes its relevant tests and scripts/check.sh, followed by
maintainer review under [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses).
The maintainer creates commits and PRs.

Headless tests cover transport, application rules and their boundary. Graphical
checks cover row identity, focus and input; installed checks establish actual GOA
and Settings compatibility. The UI test target is applying a 30-account update
within 250 ms under a normally running event loop. See [quickstart](quickstart.md)
for commands and supported environment. Synthetic tests do not prove installed
compatibility.

## Constitution check

- **I–II:** Remove speculative identity reconstruction, duplicate state and public
  publication counters. Keep each decision in its owning contract; use domain names.
- **III–IV:** Preserve explicit safe errors. The decoder validates, the worker
  accepts source facts, AccountList owns row/selection policy and GTK presents it.
- **V–VI:** Keep bounded work off GTK, use one latest snapshot and verify failure,
  ordering, recovery and shutdown alongside success.

No exception is requested. Future mail/storage/onboarding policy stays in
[the specification](spec.md#assumptions-and-future-work).
