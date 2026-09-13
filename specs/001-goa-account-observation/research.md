# Design Decisions: GNOME Mail Accounts

Accepted 2026-09-12; amended 2026-09-13 for direct subscriptions, the shared account
contract and the approved review simplification. Protocol details belong to the
contracts; this document records reasons and trade-offs.

## GIO instead of another binding or runtime

Use locked gio/glib 0.22.9 already required by the application. GIO supplies async
D-Bus calls, activation and subscriptions; Mailbag still needs account validation
and display policy. A GOA binding/libgoa adds a binding/native dependency, licensing,
packaging and maintenance. Direct GIO has lower expected cost for this read-only
surface. Reassess when designing credentials; dependency count alone is not a goal.

## Direct subscriptions on a dedicated context

An isolated startup-race test with GIO 2.88.3 received ObjectManager callbacks only
when the global context was dispatched. Its asynchronous initializer invokes a
synchronous initializer in a GTask thread, whose subscriptions use the default
context. Direct subscriptions on our worker context allow progress independently
of GTK without another executor, proxy cache or reconstruction lifecycle.

Sources: [ObjectManager](https://github.com/GNOME/glib/blob/main/gio/gdbusobjectmanagerclient.c),
[async initializer](https://github.com/GNOME/glib/blob/main/gio/gasyncinitable.c),
[GIO subscriptions](https://docs.gtk.org/gio/method.DBusConnection.signal_subscribe.html).

## Separate observations, pending work and presentation

The shared data contract prevents account rules from interpreting GOA strings or
GIO codes. Pending checks do not invalidate previous observations. Sharing an Arc
avoids copying a complete map at receipt; AccountList borrows it and owns its rows.
One latest snapshot matches FR-006: queued intermediate switches are unnecessary.
A mutex and standard Rust wakers provide the required small exchange using GLib's
existing executor, without another runtime/channel dependency.

## Reject ambiguous identities instead of repairing them

GOA defines Id as immutable. Guessing a missing Id from an old path required path
reassignment, duplicate and recovery rules, and allowed conflicting records to
supply exclusion facts. Stateless decoding now drops ambiguous records; the worker
alone retains previous facts and accepts unambiguous updates. This sacrifices
immediate attribution of a damaged record's MailDisabled value until identity is
verified again. It preserves rows and avoids acting on a guessed identity.

Source: [GOA Account identity](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Account.html).

## Bounded recovery and data

Events cannot reveal a GOA process that remains present but stops answering. An
idle ten-second GetManagedObjects check detects that failure and repairs missed
events. Manual retry and events can recover earlier; fast retry sequences add no
needed behavior. One deadline bounds connection, activation and repeated acquisition.
One data-limit validator handles snapshots and candidate updates; an overflowing
display update can omit optional text while retaining explicit account flags.
Exact limits and acceptance rules are in [the GOA contract](contracts/observation.md).

## Settings and existing UI

Use async org.gtk.Actions Activate so errors and timeouts are observable. Both UI
entry points share the launcher; a successful reply acknowledges the action but
does not prove the panel appeared. Installed Wayland checks must establish that.
No host-spawn fallback or fabricated activation token is introduced.

The existing status page, rows and toast overlay provide the needed surfaces.
Stable row objects and a focusable problem button preserve navigation and input
access without a separate Welcome flow. [The UI contract](contracts/ui.md) owns the
surface mapping, launcher protocol and sandbox permissions.

Sources: [Settings action](https://raw.githubusercontent.com/GNOME/gnome-control-center/main/shell/cc-application.c),
[GApplication protocol](https://wiki.gnome.org/Projects/GLib/GApplication/DBusAPI),
[Flatpak permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).
