# GOA Client Contract

The adapter supplies the [shared account data](accounts.md) to Mailbag. User-visible
rules belong to [FR-001–017](../spec.md#requirements); this contract owns transport,
request ordering, data acceptance and resource limits.

## Client operations

- `GoaAdapter::start()` returns cloneable commands and one non-cloneable GoaUpdates
  receiver, then performs work on a dedicated GLib thread.
- `GoaUpdates::next_account_update(&mut self)` waits for the newest shared
  `Arc<AccountUpdate>`, or None after shutdown. The mutable borrow excludes
  overlapping waits. Receiving does not clone the account map.
- `refresh_accounts()` starts a check or joins the active one. All checks expose
  check_pending; the last result changes only on new evidence (FR-012).
- `stop()`, dropping the receiver or dropping the last command handle requests
  shutdown without joining the worker from GTK.

Only ordinary Rust data crosses this boundary. Diagnostic rules are in
[the account contract](accounts.md#validation-and-diagnostics).

## D-Bus protocol

| Item | Value |
|---|---|
| Bus / name | Session / org.gnome.OnlineAccounts |
| Root | /org/gnome/OnlineAccounts |
| Full check | org.freedesktop.DBus.ObjectManager.GetManagedObjects |
| Request / reply | () / (a{oa{sa{sv}}}) |
| Account interface | org.gnome.OnlineAccounts.Account |
| Mail interface | org.gnome.OnlineAccounts.Mail |

Use direct GIO signal subscriptions on the worker's thread-default context before
resolving the unique GOA owner and requesting its list. GIO handles the transport.
When no owner exists, permit normal bus activation and resolve again. No proxy
cache, credential method, account mutation or mail-server access is needed.

| Source property | Shared field |
|---|---|
| Id | AccountId |
| ProviderType | imap_smtp → ImapSmtp; google → Google; ms_graph → Microsoft365; other valid text → Other; invalid/missing → None |
| MailDisabled | Valid inverted boolean → mail_enabled; otherwise None |
| AttentionNeeded | needs_attention |
| Mail interface presence | mail_service_available |
| ProviderName, PresentationIdentity, ProviderIcon | provider_name, display_name, theme icon_name |
| Mail.EmailAddress | email_address |

## Events and request ordering

| Signal | Accepted effect |
|---|---|
| NameOwnerChanged for GOA, from the bus daemon | Invalidate the list and path mappings; check the new owner |
| InterfacesAdded at the GOA root | Recheck when Account or Mail was added |
| InterfacesRemoved at the GOA root | Recheck when Account or Mail was removed; losing a known Account interface makes membership incomplete; missing Mail marks its known account unavailable immediately |
| PropertiesChanged under the GOA root | Apply only properties in the mapping above, on the corresponding Account or Mail interface |

Validate signal sender, path and body. Ignore unrelated properties and interface
changes before invalidating an in-flight check or publishing. Relevant property
signals still advance the internal account-change counter when their supplied
value matches current data: their ordering can invalidate an older full reply.
Publish property updates only when account fields change. Check state transitions
and failure reports also publish updates; the same failure may be reported again.

Invalidated required properties become unknown and request a full check; optional
ones become absent display data. An Id change/invalidation is an identity error:
discard that path mapping and recheck without applying the record to its old ID.
Removing a known Account interface likewise reports Failed(InvalidList), retains
account facts and discards its path mapping until a full check confirms membership.
Unknown paths also require a check before attributing property data to an account.

Capture the account-change counter before owner resolution/acquisition. Request the
list from the unique owner. If a relevant signal intervened, discard the reply and
repeat within the original deadline. One owned request future prevents cancelled
or timed-out replies from being accepted. Weak callback references keep old clients
from affecting new ones. No publication or request-instance counter is needed.

## Parsing and acceptance

The decoder is stateless: it reads one reply, validates records, excludes all
records with conflicting paths or IDs, and returns valid account facts, their path
mappings and any list error. It never reads previous state or guesses a damaged ID
from an old object path. An account-shaped object with a missing interface or ID
makes membership incomplete. A valid ID with another invalid field stays a record.

The worker alone combines observations. A complete snapshot replaces accounts and
path mappings. An incomplete snapshot updates only unambiguous records and keeps
previous account facts for omitted records, with Failed(InvalidList). Only mappings
validated in the new reply remain usable for signals. Previously shown rows follow
FR-008; no ambiguous record can supply an exclusion fact. Later valid full checks
restore availability and confirm removal under FR-009.

Whole-reply or merged-list limit failures retain previous accounts, clear uncertain
path mappings and report DataLimit. For property updates, prepare a candidate
record and validate the resulting collection before committing. If new display
strings exceed the total limit, omit that record's optional display fields while
preserving valid boolean/provider changes and invalidations, then report DataLimit
and request a check. The UI retains its prior usable display during that failure.
There is no mutation followed by restoration of old display fields.

## Limits

| Resource | Limit / behavior |
|---|---|
| Active checks | One at a time; five seconds including connection, activation and rescheduled acquisition |
| Periodic check | Every ten seconds if idle; skip busy ticks without queued catch-up |
| Recovery | GOA events, periodic checks or manual retry; failures add no fast retry sequence |
| Accepted account data | Up to 4,096 records, 4 KiB per normalized string, 16 MiB total retained strings including IDs and paths |
| Pending UI data | One immutable snapshot, replaced by newer data |
| Commands | One refresh flag and one stop flag; stop takes precedence |
| Shutdown fixture | Worker finishes within one second; GTK does not wait |

One validation function counts normalized account/path data for decoded snapshots,
merged candidates and property replacements. Input record counts are also capped
before normalization. Limits govern accepted application data, not GIO message
allocations or transient candidate construction. Never truncate IDs or publish a
truncated list as complete.

The periodic request uses GetManagedObjects to detect silent hangs and repair
missed events. Request pending state follows the shared contract; no separate
presentation status is maintained by the transport. Whole-session-bus recovery
and changing a shared GIO connection's exit-on-close policy remain outside F01.

## Delivery and shutdown

A short mutex protects the latest Arc, pending notification and command flags.
Register and take task wakers under the same lock; wake outside it. Build, copy and
drop account collections outside the lock. Retain the latest shared snapshot after
consumption so unexpected worker exit can report failure with known facts.

GTK consumes at most one snapshot per dispatch and yields before reading another.
There is no per-event task queue or command/UI polling timer. Stop cancels the
request and timer, removes subscriptions, clears wakers and releases shared bus
references without closing a connection used elsewhere. Allow cancelled GIO
callbacks a bounded cleanup period. Expected cancellation is silent; an unexpected
worker exit supplies Failed(SourceStopped) and ends the receiver. Requested
shutdown releases pending data without copying accounts into an unused failure.

## Verification

Private services exercise this protocol without iterating GTK: initial acquisition,
relevant and irrelevant events, malformed identities, owner replacement, stale
replies, activation, deadlines, periodic recovery, command/wakeup races, bounded
bursts and shutdown. Tests across the adapter and AccountList cover pending checks,
conflicting identities and actual superseded/applied exclusions. Test commands and
installed-host limits are in [quickstart.md](../quickstart.md).

Sources: [GOA Account](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Account.html),
[GOA Mail](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Mail.html),
[D-Bus interfaces](https://dbus.freedesktop.org/doc/dbus-specification.html),
[GIO subscriptions](https://docs.gtk.org/gio/method.DBusConnection.signal_subscribe.html).
