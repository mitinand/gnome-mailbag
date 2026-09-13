# Research: GNOME Mail Accounts

Date: 2026-09-12. Amended 2026-09-13 with maintainer approval after initial-client
testing. UI and installed-Flatpak acceptance remain pending.

## Approved account-contract refactoring

A dependency-free `account-source` crate now defines the one account data contract.
The immediate benefit is separating Mailbag account rules from GOA terminology and
implementation. GOA translation stays in the adapter; provider support stays in
Mailbag. A universal trait, source registry and new delivery mechanism add no needed
behavior and are excluded. Safe source diagnostics are preserved as data, not used
as application decision codes. See [the account contract](contracts/accounts.md).

## 1. Existing project and dependency choice

**Decision:** Build on approved UI/workspace commit
`7a69c494dbb97c10fe1baf8db88a5296d271278e`, available locally as origin/main.
The feature branch is codex/goa; research used the earlier shell at `7083b8a`.
T003 in [tasks.md](tasks.md) verifies that the implementation checkout includes
the approved baseline before source changes.

Use its Rust 1.95.0 toolchain and existing library family: libadwaita 0.9.2,
GTK bindings 0.11.4, GIO/GLib 0.22.9. Add goa-adapter as a local crate and direct
GIO/GLib dependencies using those same locked versions.

**Rationale:** GIO already provides asynchronous D-Bus calls, object proxies and
change signals. Our code still needs to validate GOA fields and decide when a list
can prove removal. Reusing GIO avoids introducing another runtime or binding stack.

**Alternatives considered:** A Rust GOA binding/libgoa could reduce property-access
boilerplate. It would also add a binding/native-library dependency to maintain,
license-check and supply in the target environment. Application-specific error,
selection and display rules would remain. For F01's small read-only surface, direct
GIO use has the lower expected total cost. Reassess when credential access is
actually designed; fewer dependencies alone is not a reason to write a larger client.

A custom D-Bus protocol implementation is not selected: GIO handles the protocol.
Tokio would add a runtime without an F01 consumer.

Evidence: Cargo.lock, Cargo.toml, rust-toolchain.toml, scripts/check.sh, meson.build
and the manifest inspected at the approved commit. Its checks already cover the
workspace; its manifest already includes all of crates/.

## 2. Which GOA fields matter

**Decision:** Read Account and Mail fields only. MailDisabled, Mail-interface
presence and AttentionNeeded are independent. The GOA adapter translates exact
provider keys imap_smtp, google and ms_graph into the shared provider enum.
Mailbag decides which enum values it supports. Microsoft 365 does not need IMAP.

**Rationale:** GOA adds/removes Mail asynchronously after its setting changes.
AttentionNeeded requests human attention; it does not establish a specific mail
login failure. EmailAddress may be empty. GOA Id identifies an account; presentation
strings do not. Invalid required fields must not silently become false or empty.

**Alternatives considered:** Inferring a provider from the email domain, treating
missing Mail as disabled, or requiring IMAP for every provider would produce wrong
account states. Endpoints and credential methods have no F01 consumer.

Sources: [GOA Account interface](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Account.html),
[GOA Mail interface](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Mail.html),
[provider source](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/tree/master/src/goabackend).

## 3. Proving that an account was removed

**Decision (amended with maintainer approval, 2026-09-13):** Use asynchronous
GIO D-Bus calls and direct signal subscriptions on the dedicated GLib worker.
Do not construct GIO ObjectManager proxies. GIO still handles the wire protocol;
the adapter validates account data and handles the four signals in
[the contract](contracts/observation.md#events-to-subscribe-to).

Verify the full account list on startup, recovery, removal, invalidated required
fields, Retry Check and the ten-second health check. Accept replies only from the
current GOA process and current request, with no intervening account changes.
An interface-removal signal alone cannot prove an account was deleted.

**Evidence and trade-off:** With GIO 2.88.3 and Rust gio 0.22.9, the isolated
startup-race test received no ObjectManager property callback while only the
private context was dispatched. Temporarily dispatching the global default context
made that test pass. ObjectManager's asynchronous initializer runs its synchronous
initializer in a GTask thread; its internal subscriptions therefore use the default
context. That fails the requirement to observe changes independently of GTK.

Direct subscriptions remove proxy construction, proxy caches and reconstruction
readiness rules. The adapter must explicitly validate and apply signal fields;
the required account validation, request guards and event tests remain. This is
less machinery than adding a helper executor or another thread to preserve proxies.
The client API, dependencies, permissions, one-worker design and five-second
attempt deadline are unchanged.

Resolve the current unique GOA owner after installing subscriptions. Normal service
activation is allowed when no owner exists. Failed activation or list acquisition
is unavailable state, never an empty list. Retry and owner replacement use bounded
attempts and reject obsolete callbacks. Whole-session-bus recovery remains outside F01.

Sources: [ObjectManager initialization and subscriptions](https://github.com/GNOME/glib/blob/main/gio/gdbusobjectmanagerclient.c),
[default asynchronous initialization](https://github.com/GNOME/glib/blob/main/gio/gasyncinitable.c),
[GIO signal subscriptions](https://docs.gtk.org/gio/method.DBusConnection.signal_subscribe.html),
[async D-Bus calls](https://docs.gtk.org/gio/method.DBusConnection.call.html).

## 4. Latest account state, without replaying switches

**Decision:** Keep one current account list and check status for the UI. Replace a
pending update with a newer accepted state. Produce a notice only when applying
that state actually hides a previously visible row.

**Rationale:** If the user disables Mail and corrects it before the UI updates,
there is no benefit in deliberately hiding the account or clearing selection.
Current state is enough to drive F01. Failed checks keep the last accepted facts;
they do not turn an empty response or missing service into proof of removal.

**Alternatives considered:** Keeping every intermediate exclusion required extra
account tracking and processing rules solely to reproduce a transient UI change.
The maintainer rejected that behavior. A full event queue would also accumulate
work while GTK is busy. Neither mechanism is part of the revised design.

Actual loss of service data still requires a fresh successful check. Combining
valid updates is not itself a loss of data needed by this feature.

## 5. Limits and shutdown

**Decision:** One check at a time, five seconds per attempt. Recover through GOA
events, manual Retry Check and a ten-second periodic check when idle. Fast automatic
retries are omitted: they only shorten recovery latency while adding timers and
budget-reset rules to behavior already covered by those checks. Skip busy ticks rather than accumulating requests. The UI waits for new
account data and the worker waits for commands, using tasks on their existing GLib
contexts. Publishing data or a command wakes the waiting task. Repeated changes
replace the pending state; they do not add callbacks. Stop requests remain separate
from retry requests and GTK never waits for the worker to finish.

**Rationale:** Limits prevent endless waiting and accumulated work. A dedicated
context lets GOA progress without GTK. See the GOA contract for exact data limits,
request deadlines and shutdown tests; these are engineering budgets to validate.
A small standard Rust Future/Waker interface waits on the existing shared state;
GLib already runs and wakes these tasks. This adds no channel/runtime dependency.
The GOA contract describes the lock/wakeup rules needed to avoid missed updates.

The ten-second request checks the actual account list, not just whether the bus
daemon is alive. Events remain the immediate update path. This extra check detects
a hung GOA process without a name change and repairs current data after a missed
event. It does not prove subscriptions are working; test later events separately.
A healthy background check does not put rows into loading/unconfirmed state.
A silent failure can take up to the next ten-second tick plus the five-second
request deadline to detect under a normally running event loop. This does not
provide recovery from loss of the entire session bus.

**Alternatives considered:** Events alone cannot detect a silent hung process.
A bus-daemon ping would not verify that GOA answers account requests.
Fixed 25/50-ms command/UI timers wake the application
when nothing changed and are unnecessary. A new channel library could provide
notifications but would still need the latest-value state and command merging;
use the existing GLib task support and standard Rust wakeup for this small interface.
Unbounded rapid retries, event queues and blocking GTK joins are also rejected.
The intentional ten-second health timer is separate from command/UI notification
and is cancelled at shutdown.

Sources: the locked glib 0.22.9 source, main_context_futures.rs (spawn_local and its
cross-thread task wakeup); [GLib task API](https://docs.rs/glib/latest/glib/struct.MainContext.html#method.spawn_local),
[standard Rust Waker](https://doc.rust-lang.org/std/task/struct.Waker.html).
The claim is that these APIs support this design, not that the future F01 wakeup
tests have already passed.

## 6. Settings and what is shipped

**Decision:** Use the asynchronous org.gtk.Actions Activate method for Settings'
launch-panel action. Supply online-accounts as its panel argument. Both UI entry
points share one cancellable request and deadline. Exact types are in the UI contract.

The client code is compiled into Mailbag. Cargo supplies its Rust dependencies;
the GNOME runtime supplies native GIO/GLib. GOA and Settings remain host services.
No libgoa, bundled daemon or separate adapter process is included. Add only the two
named D-Bus permissions, without network or host-command access.

**Rationale:** Async call completion exposes service errors and timeouts. A void
remote-action helper cannot provide that failure result. A successful reply means
the command was accepted; actual panel presentation still needs an installed test.
Do not fabricate activation tokens. If Wayland presentation needs valid activation
metadata, resolve that during integration before declaring the panel test passed.

**Alternatives considered:** Starting gnome-control-center as a shell command from
the sandbox would require a different execution path and inappropriate host access.
GOA documents direct D-Bus use as well as use of its client library.

Sources: [Settings action implementation](https://raw.githubusercontent.com/GNOME/gnome-control-center/main/shell/cc-application.c),
[GApplication protocol](https://wiki.gnome.org/Projects/GLib/GApplication/DBusAPI),
[GOA integration options](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/overview.html),
[Flatpak dependencies](https://docs.flatpak.org/en/latest/dependencies.html),
[Flatpak permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).

## 7. Existing UI

**Decision:** Reuse the account list, row form, status page and toast overlay.
Show the existing list page at narrow widths; otherwise the approved template's
show-content=true initially hides it behind the reader. Keep geometry unchanged.

**Rationale:** The status area can explain missing accounts and open Settings
without a separate Welcome flow. Stable account rows preserve selection and focus.
A real problem button provides keyboard/touch access that a tooltip alone cannot.

**Alternatives considered:** Rebuilding the whole list for each update disrupts
focus. A separate Welcome or invented sync indicator expands F01 beyond its scope.

Sources: approved mailbag.ui, folder-row.ui and main.rs;
[GNOME placeholder pages](https://developer.gnome.org/hig/patterns/feedback/placeholders.html),
[GNOME toasts](https://developer.gnome.org/hig/patterns/feedback/toasts.html).

## Remaining implementation evidence

Design choices are resolved. Actual account discovery, Settings presentation,
input accessibility and timing/resource tests remain implementation acceptance.
No cache deletion policy is implied by account hiding in F01.
