# GOA Client Contract

`goa-adapter` translates GOA replies and signals into the shared `AccountUpdate`
from `account-source`, as defined in [the account contract](accounts.md).
It supplies these updates to `mailbag::accounts`. The application
alone decides which rows to show and when to notify the user. This is an internal
Rust interface, not a new network API.

## Client operations

- `GoaAdapter::start()` promptly returns `(GoaAdapter, GoaUpdates)` and starts one worker.
  The client is a cloneable command handle; the updates receiver cannot be cloned.
  Connection failure is reported as account-check failure, not an application crash.
- `GoaUpdates::next_account_update(&mut self)` asynchronously waits for and takes the newest pending
  account list/status. There is one UI consumer; waiting does not block GTK or
  repeatedly check a flag. Closing the client wakes the consumer and ends the wait.
- `refresh_accounts()` requests or reuses one current check. It does not obtain
  credentials or wait for GTK.
- `stop()` is idempotent. Dropping the last handle also requests stop. GTK never
  joins the worker; tests may wait for completion with a deadline.
  Dropping the updates receiver also requests stop. Dropping the last command
  handle requests stop even if the receiver still exists.

The mutable receiver borrow prevents simultaneous waits on one receiver. Command
handles expose no update-reading method. `AccountDetails::invalid_fields()`
belongs to `account-source` and computes unknown required fields from the current values; no second stored list
needs to be kept consistent. Mailbag uses this method for field explanations.

Only plain Rust account/error values cross this interface. No GTK/GIO objects,
Variants, proxies, endpoints, passwords or tokens cross it. Preserve the operation,
error class, underlying domain/code and safe cause. Do not log raw D-Bus replies,
account IDs, addresses or unchecked error text. Expected cancellation is not an error.

## Translation into the shared account contract

- ProviderType: exact `imap_smtp`, `google`, `ms_graph` map to ImapSmtp, Google,
  Microsoft365; other valid text maps to Other. Missing/invalid text maps to None.
- MailDisabled: invert a valid boolean into mail_enabled; invalid/missing is None.
- AttentionNeeded: copy a valid boolean into needs_attention; invalid/missing is None.
- Mail interface presence: mail_service_available, independent of mail_enabled.
- PresentationIdentity: validated display_name; optional fields retain their fallback rules.
- GIO errors: map to common categories and preserve whitelisted diagnostic domain,
  numeric code and static operation. Never copy remote error messages.

These translations do not decide provider support or row visibility.

## D-Bus interface

| Item | Value |
|---|---|
| Bus/name | Session / org.gnome.OnlineAccounts |
| Root | /org/gnome/OnlineAccounts |
| Full account check | org.freedesktop.DBus.ObjectManager.GetManagedObjects |
| Request/reply types | () / (a{oa{sa{sv}}}) |
| Account properties | Id, ProviderType, ProviderName, ProviderIcon, PresentationIdentity, MailDisabled, AttentionNeeded |
| Mail properties | Interface presence and EmailAddress |

Allow normal bus activation. Inspect account objects under the expected root;
unrelated manager objects are not accounts. Malformed account objects cannot simply
be omitted from an otherwise 'complete' list. Do not call credential methods, change
properties, remove accounts or connect to a mail server.

## Events to subscribe to

Install the subscriptions on the private GLib context before accepting the initial
account list. Keep them active while Mailbag runs, including after retry failures.
Use direct GIO D-Bus subscriptions, installed on that context, with one handler
per signal and no ObjectManager proxies. GIO handles transport and match rules.

| D-Bus signal | Source / GIO handler | What Mailbag does |
|---|---|---|
| `org.freedesktop.DBus.NameOwnerChanged` (`sss`) | Bus daemon at `/org/freedesktop/DBus`, filtered to `org.gnome.OnlineAccounts`; direct GIO NameOwnerChanged callback | Detect GOA disappearance, appearance or replacement. Mark known rows unconfirmed on loss, reject old requests and check the new process. |
| `org.freedesktop.DBus.ObjectManager.InterfacesAdded` (`oa{sa{sv}}`) | GOA root `/org/gnome/OnlineAccounts`; direct GIO InterfacesAdded callback | Check new account data or restored Mail interface. Add/show only an eligible account; do not require an application restart. |
| `org.freedesktop.DBus.ObjectManager.InterfacesRemoved` (`oas`) | GOA root; direct GIO InterfacesRemoved callback | Recheck the full list before confirming account absence. Missing Mail alone makes a known account temporarily unavailable; it does not imply MailDisabled=true. |
| `org.freedesktop.DBus.Properties.PropertiesChanged` (`sa{sv}as`) | Account object path, for Account or Mail; direct GIO PropertiesChanged callback | Apply changed account fields; explicitly invalidated required fields become unknown and trigger a check. |

Install subscriptions before resolving the current unique GOA owner and requesting
its list. Validate sender, object path and signal body before applying account facts.
Owner loss invalidates membership; it never synthesizes account removals. There is
no proxy reconstruction or readiness notification. All setup and acquisition work
shares the existing attempt deadline.

Handle these property changes explicitly:

| Property | Result |
|---|---|
| Account.MailDisabled | Update the enabled/disabled fact immediately; a newer enabled value can supersede it before the UI updates. |
| Account.AttentionNeeded | Update the problem indicator and explanation; do not infer a specific mail authentication failure. |
| Account.ProviderType | Recheck provider eligibility. |
| Account.Id | Validate identity; never silently rekey a visible row as if this were a label change. Recheck ambiguous identity. |
| Account.ProviderName, ProviderIcon, PresentationIdentity | Update presentation in place using the display-validation rules. |
| Mail.EmailAddress | Update the displayed address; an empty address is allowed. |

The final `PropertiesChanged` argument lists invalidated properties: their values
are no longer supplied by the signal. Treat them according to the required/optional
field rules; do not silently retain an invalidated required value as verified.
Coalesce duplicate GIO notifications about the same change into one pending check.
Signals from an obsolete client/process are ignored. Disconnect the old subscriptions
when replacing a client or shutting down.

Sources: [D-Bus standard interfaces](https://dbus.freedesktop.org/doc/dbus-specification.html),
[GIO signal subscriptions](https://docs.gtk.org/gio/method.DBusConnection.signal_subscribe.html).

## Worker and request checks

The worker acquires its own GLib context and sets it as thread-default before
creating GIO objects. The same context dispatches calls, signals, health checks and
cancellation without GTK being iterated.

The NameOwnerChanged subscription identifies GOA process changes. Resolve its
current unique name through the bus daemon before acquiring account data. When no
owner exists, allow normal activation and resolve again. A connected session bus
without an answering GOA process is still unavailable.

Verify the entire account list on startup, recovery, removal, invalidated required
fields, manual refresh and the ten-second health check. Send the request to the current unique process name.
Keep these internal request checks:

| Check | Prevents |
|---|---|
| Current unique GOA owner and account change number | Reply from a previous GOA process being accepted |
| Worker-owned subscription state, captured weakly by callbacks | Callbacks from a replaced client affecting a newer client, even for the same process |
| Account change number captured at request start | A reply overwriting account changes received while the request was pending |
| One owned request future and its attempt deadline | Superseded, cancelled or timed-out results being accepted |

The account change number advances on relevant service signals. Published UI updates
have a separate update number so pending/error transitions also reach the UI; those
publications must not invalidate their own request.

If account changes arrive during a full check, discard that reply and schedule one
new check within the remaining attempt deadline. Under sustained changes, fail at the deadline and wait for a later check; do not
reset the deadline indefinitely.

The worker awaits one request future at a time. Dropping that future cancels the
request; its later completion has no path to publish accounts. A new client owns
separate subscription state, even when GOA's unique owner is unchanged. These
ownership checks avoid additional counters for requests and client instances.

Subscription setup, owner resolution, activation and account acquisition share the
attempt deadline. Manual retry after an unavailable start permits fresh activation.
Replace obsolete subscription state on a new client instance and reject its queued
callbacks. Process appearance can start recovery without a proxy-readiness event.

These recovery guarantees cover the GOA process while the desktop session bus is
running. F01 does not promise that the application survives or reconnects after the
whole session bus is lost. Do not add bus reconnection or change the shared GIO
connection's exit-on-close policy for this feature. A GOA-only restart must still
preserve known rows and recover as specified.

## Accepting account data

Check list completeness separately from each account's fields. A valid complete
list can confirm absence even if one identifiable account has another bad field.
Ambiguous identity or malformed membership cannot confirm absence.

An object-removed signal schedules a full check; only a successful current list
can confirm account removal. A valid explicit MailDisabled value updates that
account immediately in the client's current state without waiting for other accounts.
A newer valid enabled state can supersede it before the UI consumes the update.

An invalidated required property becomes unknown and triggers a recheck. Missing
Mail with MailDisabled=false is a mail-service problem, not disablement. A failed
check never replaces account data with an empty successful list. Keep accepted facts
and report the failure; apply the row-retention rules in [the data model](../data-model.md).

## Ten-second GOA health check

Events handle normal account changes immediately. In addition, schedule one health
check every ten seconds on the worker context while Mailbag runs. Use the same
`GetManagedObjects` request, validation and five-second attempt deadline; a bus-daemon
ping alone would not prove that GOA can supply account data. Do not read only cached
account fields for this check.

If startup, a user check or recovery is already running,
skip that tick. Do not queue a missed tick or issue a parallel request. After suspend,
perform at most one due check, with no catch-up burst. Ordinary property signals do
not postpone this timer indefinitely.

On a healthy account list, a background check leaves availability and selection
unchanged while pending; no loading-screen flash or success toast. If it finds changed
accounts, apply the latest checked state using the normal display/notice rules.
If a returned list corrects a missed event, later property events apply only their
changed fields; older account fields must not overwrite fresher checked values.
A complete-list reply must not be mistaken for proof that every event subscription
is working, so missing-event fixtures test ongoing signal handling separately.

An error/timeout immediately applies the agreed unavailable/incomplete state. Keep
known rows and selection; never report empty accounts or removal from a failed check.
Failure does not schedule fast automatic retries. Ten-second checks continue, one
attempt per idle tick. Manual Retry Check and GOA events can request an earlier
check and share the one-active-check rule. A successful full check restores only
verified accounts.

These checks diagnose failure to obtain GOA data while the application and session
bus remain running. They do not add recovery of the entire desktop D-Bus bus or
promise that an application terminated by bus loss can show an error.

## Deliver updates without frequent polling

Store one latest account list/check status behind a short lock. Build and validate
data outside the lock; use the lock only to replace/take it or set retry/stop flags.
The slot retains its last accepted snapshot after delivery, with a separate
pending flag, so unexpected worker exit can report failure without losing known
accounts. Copy account data outside the lock; only the snapshot reference crosses
it. No widget work or D-Bus call runs under that lock. Replacing a pending update with
newer valid data does not require remembering what it replaced.

Run one waiting consumer task on the GTK context and one waiting command task on
the worker's private context using existing GLib task support. The shared state
stores at most one standard Rust Waker for each consumer. Checking for data and
registering a waiting task happen under the same lock. A producer writes the newest
state, takes the registered waker, releases the lock, then wakes the task. This
prevents an update arriving between 'nothing to read' and 'start waiting' from
being missed. Commands use the same rule; stop takes precedence over retry.

Merge repeated notifications while a task is already scheduled. Process at most
one account update per GTK task dispatch and yield to the context before taking
another, including when more data is already pending. Do not create a busy async
loop under continuous account changes. When no data, commands or timed requests
are pending, these tasks remain asleep. The ten-second health timer wakes the worker
for a real GOA request; it does not poll command flags or refresh the UI by a timer.
No per-change task/source is needed; no GTK references cross into the worker.

The UI compares the newest state with rows actually displayed before that update.
If an account is still available, keep its selection. If it is currently confirmed
disabled/absent, hide it and generate a notice. Intermediate switches are not
recorded or replayed.

If account data really becomes incomplete or exceeds the limits, publish an explicit
problem and require a fresh successful check. Do not mark ordinary update merging
as a service failure and do not publish a truncated list as complete.

## Limits

Initial engineering limits; validate them during implementation.

| Resource | Limit / behavior |
|---|---|
| Active checks | One bootstrap or account check at a time, five seconds total per attempt |
| GOA health check | Every ten seconds while running, one request if idle; skip busy ticks and continue after failure |
| Manual retry | Reuse pending work or start a fresh limited attempt after failure |
| New GOA process | Start a new recovery attempt; do not accumulate checks during repeated restarts |
| Current account data | Up to 4,096 records, 4 KiB per normalized string, 16 MiB total |
| Pending UI data | One account list/status, no history of changes |
| Commands | One retry flag and a separate stop flag |
| Notifications | One waiting task/waker per direction; new data or commands wake it, repeated changes merge |
| Timers | Ten-second GOA health check plus request deadlines and shutdown deadline; none for checking flags or refreshing the UI |
| Shutdown test | Worker finishes within one second in the isolated fixture; GTK never waits for it |

Optional display fields that are too large use a neutral fallback. Do not truncate
an ID to make it fit. Required-field or whole-list failures stay explicit. These
limits bound application-owned data, not GIO's internal D-Bus message allocations.
The required 30-account fixture is not a product maximum.

Use GLib's existing context executor and standard Rust Future/Waker support. No
additional runtime or channel dependency is needed for this latest-value interface.
Only the health-check timer repeats. Deadline/teardown timers end with their
operation. Remove all of them on stop; no health request is issued after shutdown.

## Shutdown

Stop is independent of retry/data traffic. Invalidate pending requests, cancel calls,
disconnect signals and remove timers on the worker. Allow expected cancellation
callbacks to finish within a limited cleanup period before releasing its context.
Avoid strong callback cycles. Release shared bus references instead of closing a
connection used elsewhere. Wake/close waiting consumers and clear stored wakers.
GTK cancels its consumer task on the GTK thread and uses weak window references;
late completions cannot reopen the window. Unexpected worker exit is a visible
service problem if Mailbag is still running.

## Required tests (PR 1)

Use private D-Bus services with host activation disabled and synthetic accounts.
Test:

- Healthy empty versus no GOA, even after successful client construction.
- Every event/property row above, including invalidated properties, is exercised;
  signal-driven updates arrive before the next health tick. Events during initial
  list acquisition are not missed or overwritten.
- Service restart/replacement, stalled activation or account acquisition, fresh activation on retry,
  failed recovery and obsolete replies/client callbacks.
- One account error isolated; missing/duplicate IDs; optional display errors.
- Both orders of MailDisabled and Mail-interface changes; explicit disable amid
  unrelated errors.
- Paused UI with disable→enable and removal→confirmed-reappearance: only the final
  available state is applied; the row and selection remain and no toast is produced.
- An applied disabled/absent state followed by reappearance: selection stays cleared.
- Worker progress without GTK; pending/error/success delivery without property changes.
- Quiet state has only the ten-second health request and no periodic command/UI polls.
  A silent/hanging GOA process triggers the failure state by the due tick plus its
  five-second deadline while the worker runs normally. Drop a fixture event and
  verify the full check corrects account state, then verify later signals still work.
  Check busy-tick skipping, no extra automatic attempts after failure, recovery with
  no owner change, manual/automatic check coalescing and no post-shutdown health call.
- Updates/commands racing with wait
  registration are delivered; bursts schedule no growing task queue. Continuous
  updates yield to other GTK work. Stop wakes an idle worker and a waiting UI consumer.
- Rapid retry presses, manual retry after failure, and stop during connection,
  check, idle failure and heavy updates.
- 10,000 transient changes and oversized replies: pending data stays within limits,
  no truncated list is accepted and a later complete check restores availability.

Use short injected deadlines or a controlled clock in tests. Always stop the private
bus process and enforce an outer test deadline. Domain tests then verify which rows
and notices result; the transport does not duplicate those rules.
