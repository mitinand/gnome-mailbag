# Design Decisions: GNOME Mail Accounts

Revised 2026-09-15 for the maintainer's F01 simplification decisions. The
[specification](spec.md) owns accepted behavior; contracts own protocol details.
Source checks below use GOA 3.58.1 and GLib 2.88.3, the installed native versions.

## Main context

**Decision:** Run GOA observation in the application's main GLib context using
asynchronous GIO calls and short callbacks. Use existing gio/glib dependencies.

**Reason:** F01 observes a small account list. The main context already applies
account changes to the UI. Blocking it violates constitution V; observation is
not required to continue during that violation. The main loop can run without a
window, although background application lifetime is outside F01.

**Alternatives:** The separate GOA thread existed to satisfy independence from the
main context. That requirement is withdrawn, along with its exchange and shutdown
machinery. GDBusObjectManagerClient is also unnecessary: full reads avoid its proxy
cache and automatic reload lifecycle. Credentials/runtime choices are deferred.

The earlier async ObjectManager initialization issue is real but no longer drives
the design: its default initializer runs synchronous construction in a GTask
thread, attaching internal subscriptions to the default context. See
[ObjectManager initialization](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusobjectmanagerclient.c#L1506)
and [GAsyncInitable](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gasyncinitable.c#L257).

## Full-list reads

**Decision:** Use the maintainer's one-active-read/one-follow-up-flag scheme in
[the GOA contract](contracts/observation.md#events-and-request-ordering). Signals
request a read and never directly modify account data. Address the well-known
service name with automatic activation. Accept a full reply or retain the previous
list with one error.

**Producer evidence:** GOA constructs its daemon in the default main context.
ObjectManager's GetManagedObjects handler synchronously enumerates exported
objects and reads their in-memory skeleton properties. It does not wait for a
separate network/account-details operation. Generated property notifications also
run on their construction context. There is no supported producer path in which
this handler independently remains pending while that same GOA context continues
to produce newer MailDisabled observations.

Sources: [GOA main loop](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/daemon/main.c#L118),
[snapshot handler](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusobjectmanagerserver.c#L854),
[registration context](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusconnection.c#L5710),
[generated notifications](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbus-2.0/codegen/codegen.py#L4354).

GOA loads its configuration before acquiring the service name. The initial
well-known-name read therefore does not intentionally expose a half-loaded startup
list. A name change requests another full read; failure does not turn an empty
proxy cache into a successful result because this design has no proxy cache.
Sources: [daemon construction](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/daemon/goadaemon.c#L117),
[bus-name acquisition](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusnameowning.c#L481).

Private-bus checks of the proposed scheme covered changes during reads, replies
followed by newer changes, coalesced triggers, errors, Retry, owner replacement
and successful removal. No concrete counterexample was found. The scheme is the
chosen transport; no additional owner/counter mechanism is required by this review.

**Trade-off:** A same-owner service can resume after a timeout without emitting a
new event. F01 then needs Retry Check to clear the error. No periodic health check
or retry triggered solely by failure is retained.

## Mail availability

**Decision:** Preserve the distinction between explicit Mail disablement and an
absent Mail interface, including the intermediate UI states in FR-007.

**Reason:** Changing MailDisabled updates the preference before the file-monitor
reload rebuilds the Mail interface. The API explicitly documents asynchronous
interface changes. A private-bus probe with the real GOA daemon and an isolated
synthetic IMAP account confirmed these full-read states:

| Trigger | MailDisabled | Mail interface |
|---|---|---|
| Startup | false | present |
| Disable property change | true | present |
| Subsequent interface removal | true | absent |
| Enable property change | false | absent |
| Subsequent interface addition | false | present |

The corresponding UI hides on confirmed disablement and waits for Mail availability
before adding a newly eligible row. Existing rows with missing Mail remain visibly
unconfirmed. Reading on both property and interface changes reaches the final state.

Sources: [GOA API](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/data/dbus-interfaces.xml#L125),
[saving the preference](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/goabackend/goaprovider.c#L1644),
[configuration reload](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/daemon/goadaemon.c#L212),
[IMAP interface construction](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/goabackend/goaimapsmtpprovider.c#L143).

## Typed data and bounds

**Decision:** Decode ordinary required fields and optional display text. A
contract violation rejects the whole read. Remove partial-record salvage, unknown
booleans, identity repair and unused source display metadata/diagnostic codes.

**Reason:** GOA validates object paths, derives each Id from its account path and
exports the account after construction. ObjectManager stores unique object and
interface keys. Generated properties have XML-declared types; blank display/email
text is valid and uses the label fallback. Unsupported providers remain valid data.
Sources: [account construction](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/src/daemon/goadaemon.c#L678),
[object storage](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusobjectmanagerserver.c#L365),
[property generation](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbus-2.0/codegen/codegen.py#L4022),
[blank email contract](https://gitlab.gnome.org/GNOME/gnome-online-accounts/-/blob/3.58.1/data/dbus-interfaces.xml#L458).

Use GIO's existing wire-message validation and timeout instead of a product account
maximum or a custom byte-accounting/recovery subsystem. In GLib 2.88, the default
method timeout is 25 seconds and wire messages larger than 128 MiB are rejected.
That wire bound is not a promise about total process memory. Source text remains
plain UI data and is excluded from diagnostics; local GOA is not a reason to log
account identifiers or addresses.
Sources: [timeout](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusconnection.c#L2067),
[message validation](https://gitlab.gnome.org/GNOME/glib/-/blob/2.88.3/gio/gdbusmessage.c#L2302).

## Settings and UI

**Decision:** Keep the current Settings D-Bus action and sandbox permissions. Use
one pending flag, the D-Bus method timeout and a weak reference in the task.
A launch failure uses one ordinary toast. Keep account notices in the same queue.

**Reason:** The error belongs to a user action and can be retried. Native toast
queuing is sufficient. The accepted design uses the method timeout without a
separate overall deadline or launcher-owned cancellation. A successful reply
acknowledges the action; installed acceptance must verify the visible panel.
The [UI contract](contracts/ui.md#settings-launch-protocol) owns exact parameters,
geometry, input behavior and permissions.

Sources: [GIO asynchronous call](https://docs.gtk.org/gio/method.DBusConnection.call.html),
[Settings action](https://gitlab.gnome.org/GNOME/gnome-control-center/-/blob/50.4/shell/cc-application.c),
[Flatpak permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).
