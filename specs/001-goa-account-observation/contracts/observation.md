# GOA Client Contract

The adapter supplies [typed account data](accounts.md) to Mailbag. User-visible
rules belong to [FR-001–017](../spec.md#requirements). This contract owns full-list
reads, their triggers and the observer's lifetime.

## Client operations

- `GoaAdapter::start(on_update)` starts observation and returns a handle. The
  callback receives `&AccountUpdate` in the application's main GLib context.
- `refresh_accounts()` requests a read and exposes manual retry progress. If a
  read is active, it requests one follow-up instead of starting another.
- `stop()` cancels pending work and removes subscriptions. Dropping the last
  handle performs the same cleanup. Repeated stop is harmless.

The handle and callbacks are local to the main context. UI actions may share the
handle; no Send/Sync interface, receiver or cross-thread exchange is required.
Callbacks carry ordinary Rust data; GOA/GIO objects remain inside the adapter.

## D-Bus protocol

| Item | Value |
|---|---|
| Bus / destination | Session / org.gnome.OnlineAccounts |
| Root | /org/gnome/OnlineAccounts |
| Full read | org.freedesktop.DBus.ObjectManager.GetManagedObjects |
| Request / expected reply | () / (a{oa{sa{sv}}}) |
| Account interface | org.gnome.OnlineAccounts.Account |
| Mail interface | org.gnome.OnlineAccounts.Mail |

Acquire the session connection asynchronously. Install subscriptions on that
connection in the main context before the initial read. Use the well-known GOA
name, normal bus activation and GIO's default method timeout (`-1`, 25 seconds in
GLib 2.88). There is no GetNameOwner round trip or ObjectManager client.

| Source property | Account field |
|---|---|
| Account.Id | AccountId |
| Account.ProviderType | imap_smtp → ImapSmtp; google → Google; ms_graph → Microsoft365; other valid text → Other |
| Account.MailDisabled | Inverted mail_enabled bool |
| Account.AttentionNeeded | needs_attention bool |
| Mail interface presence | mail_service_available bool |
| Account.PresentationIdentity | display_name |
| Mail.EmailAddress, when available | email_address |

## Events and request ordering

The following all request a full list:

- Startup and explicit Retry Check.
- NameOwnerChanged for org.gnome.OnlineAccounts, from the bus daemon.
- InterfacesAdded or InterfacesRemoved at the GOA root, from GOA.
- PropertiesChanged under the GOA root, from GOA.

Use GIO subscriptions with the sender/interface/path filters above. Signal bodies
do not contribute account facts. A stale signal can at most request another read
of the current well-known service; no application-owned owner or path cache is
needed. Other services are not triggers.

```text
request_read():
    if read_pending:
        refetch_needed = true
    else:
        start_read()

start_read():
    read_pending = true
    asynchronously call GetManagedObjects
    success and complete decoding:
        replace accepted accounts; last_check = Complete
    call or decoding failure:
        keep accepted accounts; last_check = Failed(error)
    read_pending = false
    if refetch_needed:
        refetch_needed = false
        start_read()
    else:
        retry_pending = false
    publish the resulting AccountUpdate
```

Only one read is active. Every trigger during it is represented by the same
follow-up flag. This applies to both successful and failed reads. Once changes
settle and the subsequent read succeeds, the accepted list comes from a request
started after the last received trigger. Results from cancelled/stopped observation
are never published.

Each accepted full list may update the UI under FR-006–010. A signal alone cannot
hide a row, reset selection or create a notice. A read error cannot confirm
absence. Retry presentation follows [the account contract](accounts.md); callbacks
must see the final retry flag when a read sequence finishes.

There is no health timer or automatic retry on failure alone. If the same GOA
process resumes without emitting an event after a timeout, Retry Check is the
recovery path. A later owner/property/interface event also requests a read.

## Parsing and acceptance

Decode the full response into a new account map, without consulting prior source
data. Account properties are required as specified in [the data contract](accounts.md).
Ignore non-account objects. A violation rejects the entire new map; only a
completely decoded response replaces accepted accounts and clears the error.

GOA's producer constructs unique objects and typed properties. Do not reconstruct
IDs, maintain path mappings, merge incomplete replies, rescue valid records from a
bad reply or separately reconcile property changes. Missing Mail is a legitimate
state with the behavior specified in FR-007, not a decoding failure.

## Limits

One pending read and one follow-up flag bound scheduled work. Use the GIO method
timeout and cancellation; retain only the accepted list and the current response.
GIO's existing D-Bus message-size validation remains in force.

There is no application-specific account-count, per-string or aggregate byte
budget in F01. The former 4,096-account limit is removed. Do not add multi-stage
accounting or partial recovery to enforce speculative maxima.

## Delivery and shutdown

The application owns observer lifetime and calls account presentation in the main
context. Keep callbacks short and avoid invoking a consumer while holding a
mutable borrow needed by its possible Retry/stop actions. Widgets remain owned by
the UI. F01 does not introduce background application lifetime.

Stop cancels the active operation, removes subscriptions on the same context and
releases its references. It does not close a shared session connection, wait for
a worker, drain callbacks for a fixed period or emit a source-stopped error. Weak
references or owned cancellation prevent late completion from reaching detached
consumers. Expected cancellation is silent.

## Verification

Keep the private D-Bus service fixtures and dispatch callbacks in a main context.
Use one private-bus test for each scenario: initial accounts and provider mapping;
GOA absent; failed reads retaining accounts; all four signal triggers; one
follow-up for signals during a read with the final reply winning; manual-only
retry_pending and coalesced Retry; whole-read rejection; stop and last-handle drop
suppressing late updates. Test required-field decoding, empty optional strings and
ignored non-account objects together in one unit test.

Test labels, eligibility, notices, failed-read presentation and page states in
AccountList without D-Bus. Do not compile application rules into the adapter.
The graphical test owns row reuse, hover without selection and focus after removal.
Keep private-bus isolation and fixture deadlines. Activation subprocesses and
cross-component UI subprocess tests are not part of this suite.

In ordering tests, a trigger can arrive while a method is pending, before the
fixture constructs its reply. Do not construct an old snapshot, emit newer state
and then return that old snapshot as evidence for GOA behavior. Test commands and installed acceptance are in
[quickstart](../quickstart.md).
