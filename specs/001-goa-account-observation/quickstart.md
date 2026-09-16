# F01 Validation Quickstart

This guide defines acceptance for the revised 2026-09-15 design. Account observation,
its UI and the Settings launcher are implemented in portions A/B. Automated and
graphical checks have passed; [tasks.md](tasks.md) tracks the remaining manual and
installed cases. Passing component tests does not establish installed acceptance.

## Prerequisites and baseline

Use [README](../../README.md) for setup. The acceptance environment is Fedora 44,
GNOME 50, Wayland, x86_64 and GNOME runtime 50. Record the host GOA and actual
runtime GLib versions. Native tooling includes `dbus-daemon` for private buses.
The approved UI baseline is `7a69c49`; keep the subsequently approved row/icon
changes described in [the UI contract](contracts/ui.md).

From the repository root:

```bash
git branch --show-current
./scripts/check.sh
git diff --check
```

Run the gate for every implementation portion. Confirm that behavioral tests
actually ran; zero, ignored or skipped tests are not passing evidence.

## Automated observation and policy

```bash
cargo test --locked -p goa-adapter
cargo test --locked -p mailbag settings::
cargo test --locked -p mailbag account_
```

Keep private GOA/Settings fixtures in tests/support. They connect explicitly to
isolated buses, use synthetic identities, enforce outer deadlines and clean up
all child processes. Never
replace the host GOA name or change real accounts in an automated test. Recovery
fixtures keep the private session bus alive; whole-desktop-bus recovery is outside F01.

Transport tests dispatch a GLib main context without a GTK window. Exercise the
[GOA contract](contracts/observation.md#verification), including events during reads,
coalesced triggers, standard timeout/cancellation, owner replacement and Retry.
Keep the producer's synchronous snapshot/reply behavior in ordering tests. A fake
GOA that emits newer state while holding an older snapshot reply does not justify
an application requirement unless the supported producer can do that.

Test each of the eight transport scenarios in
[the observation contract](contracts/observation.md#verification) once. One parser
unit test covers required fields, empty optional strings and non-account objects.
Five AccountList unit tests cover eligibility and missing Mail on a visible row;
removal/disable notices and superseded toggles; failed-read retention; label
fallbacks and duplicate names; page states. These tests do not use D-Bus.

The two Settings tests verify exact action parameters with one pending launch,
and one error notification followed by a new attempt. They use the private
Settings service. The launcher uses the D-Bus method timeout without a separate
overall deadline or task cancellation mechanism.

## Graphical checks

In a GNOME graphical session, run:

```bash
cargo test --locked -p mailbag account_ui_transitions -- --ignored --test-threads=1
```

The test uses synthetic account lists. It checks that updates reuse rows, hover
does not select an account, and focus stays in the list after a row is removed.
No private GOA subprocess or separate window/About test is included.

Manual checks cover keyboard/touch activation, problem tooltips/popovers, focus
when a row or problem icon disappears, 360-unit width, the 720sp/1100sp breakpoints,
enlarged text and high contrast. Match approved geometry and actions. Synthetic
clicks do not establish physical touch or installed-host behavior.

## Installed Flatpak

```bash
./scripts/build-flatpak.sh --install
flatpak info --user --show-permissions io.github.mitinand.Mailbag
flatpak run io.github.mitinand.Mailbag
```

Verify actual discovery of an existing supported account, stable labels and
selection, no development-stage message and no mailbox/sync claim. The adapter
is compiled into Mailbag; it installs no daemon and requests no credentials.
Confirm only the two named service permissions in [the UI contract](contracts/ui.md#sandbox-contract)
in addition to existing Wayland/GPU access.

Open Online Accounts from the menu with Settings closed, on another panel and on
another workspace. Verify the visible panel rather than treating a successful
D-Bus reply as proof. Test the account-empty button with private fixtures and, when
a suitable host setup is available, the real panel. Do not remove personal accounts
just to reach an empty state.

For reversible permission-negative checks, close all Mailbag instances first:

```bash
flatpak run --no-talk-name=org.gnome.Settings io.github.mitinand.Mailbag
```

Activate Online Accounts: expect one error toast and a usable application. Close
that instance, then run:

```bash
flatpak run --no-talk-name=org.gnome.OnlineAccounts io.github.mitinand.Mailbag
```

Expect an account-service error and Retry, never confirmed account absence. Close
and launch normally to verify discovery. Overrides affect only that launch and
change no accounts or stored permissions. Test hangs/cancellation on private buses;
live Mail toggles or GOA restarts require a maintainer-controlled disposable setup.

## Evidence and completion

In the handoff/PR, record the tested revision, environment, commands, test counts,
installed permissions and observed outcomes. Keep synthetic, graphical and
installed evidence separate. Report unavailable input paths or missing suitable
accounts as unverified; leave their tasks unchecked. Do not add an internal
validation diary or personal account data to the repository.

Use SC-001–008 in [spec.md](spec.md#acceptance) as the acceptance map. Documents and
probes do not complete the code migration. Installed/input acceptance remains
pending until the revised implementation is actually checked.
