# F01 Validation Quickstart

This guide targets F01 on top of approved UI commit `7a69c49`. Portions 2–4c provide
the shared account contract, GOA adapter, account display/selection rules and
headless tests. Portion 5 connects account rows, status, retry and exclusion notices to GTK.
Settings and installed-Flatpak acceptance remain pending.

## Prerequisites and baseline

Use the repository development setup in [README](../../README.md). Initial installed acceptance is Fedora 44, GNOME 50, Wayland, x86_64, GNOME runtime 50. No broader platform support is implied. Native tooling includes `dbus-daemon` for private test buses; on Fedora it is supplied by `dbus-daemon`. Do not put the real GOA daemon under a destructive fixture.

From the repository root, verify the implementation checkout includes the approved UI and the intended F01 changes:

```bash
git branch --show-current
git merge-base --is-ancestor 7a69c494dbb97c10fe1baf8db88a5296d271278e HEAD
./scripts/check.sh
```

Each of the two planned PRs runs `scripts/check.sh`. At the approved baseline it already checks the whole Cargo workspace. Confirm that behavioral tests actually ran; a zero-test result is not sufficient after F01 is implemented.

## Automated observation and policy

```bash
cargo test --locked -p goa-adapter
cargo test --locked -p mailbag account_
```

GOA restart/recovery fixtures keep their session bus running; recovery after destruction of the entire desktop bus is outside F01.

The goa-adapter entry point runs field, event, recovery, timing and shutdown tests.
The mailbag accounts tests exercise display/selection rules using synthetic account
lists without D-Bus or GTK. The settings test module remains planned. The client tests start isolated D-Bus fixtures with service activation directories disabled and connect explicitly to those buses; they never replace the host GOA name or modify real accounts. Each fixture enforces an outer deadline and cleans up its daemon. Use synthetic identities only.

Acceptance criteria are listed once in [spec.md](spec.md#acceptance). Component
protocol cases belong to [the GOA contract](contracts/observation.md#verification).
The goa-adapter suite includes adapter-to-AccountList cases for malformed identities,
pending checks and actual superseded/applied exclusions, in addition to isolated
rules. Do not substitute live destructive account operations for synthetic tests.

## Graphical checks

In a GNOME graphical session, run the graphical tests separately from the headless gate:

```bash
cargo test --locked -p mailbag account_ui_transitions -- --ignored --test-threads=1
cargo test --locked -p mailbag empty_window_and_about -- --ignored --test-threads=1
```

Run these cases in separate processes because GTK initialization belongs to one
thread. The account UI test applies synthetic snapshots without contacting host
services; it covers status transitions, stable row objects, selection, focus,
problem popovers, both retry controls, bounded notices and collapsed navigation.
The window/dialog smoke test also runs without GOA. These tests do not establish
physical keyboard/touch input or installed-host behavior. Future service-backed
graphical fixtures must use private GOA/Settings connections. Confirm each case
actually ran; ignored/skipped cases are not passing evidence.

Check stable focus/selection under rename and outages; problem explanations by hover, click, touch and Enter/Space; Retry Check and Online Accounts via keyboard/touch; no fabricated mail or sync state. At 360 logical units and through 720sp/1100sp breakpoints, the list status must be visible and actions reachable. Repeat with enlarged text and high contrast. Keep geometry and action placement consistent with approved UI.

## Installed Flatpak

```bash
./scripts/build-flatpak.sh --install
flatpak info --user --show-permissions io.github.mitinand.Mailbag
flatpak run io.github.mitinand.Mailbag
```

Expected: the existing Generic IMAP account is discovered, selectable, with no development-stage message or claim that its mailbox is empty. The GOA client is compiled into Mailbag; it does not install a separate daemon or require libgoa. Native GIO/GLib comes from the selected runtime; GOA and Settings run on the host. Its discovery does not request credentials. The installed manifest adds only the two named talk permissions; no network/broad-bus/host filesystem permission is introduced.

Open Online Accounts from the existing menu. Repeat with Settings closed, already open on another panel, and on another workspace; verify the panel actually appears rather than relying on a successful D-Bus reply. Test the account-empty button using the synthetic graphical fixture. Verify actual host presentation from the empty-state button when the maintainer provides a disposable supported desktop account setup with no eligible accounts; do not remove personal accounts just to reach it.

For reversible permission-negative checks, first close all Mailbag instances (otherwise activation can reach an already-running instance with different permissions):

```bash
flatpak run --no-talk-name=org.gnome.Settings io.github.mitinand.Mailbag
```

Activate Online Accounts: expect a visible error and a usable window. Close that instance, then run:

```bash
flatpak run --no-talk-name=org.gnome.OnlineAccounts io.github.mitinand.Mailbag
```

Expect service-unavailable state and Retry Check, never a confirmed account-empty state. Close it and launch normally to verify discovery is restored. These overrides affect only that launch and do not change accounts or saved application permissions. Hanging-service timeouts are tested on private fixtures, not by stopping real Settings/GOA services.

Synthetic lifecycle UI cases must also cover removing/disabling the last displayed account: the ordinary empty state and Settings button remain, with one exclusion toast. Adding/re-enabling an eligible account replaces the empty state without Welcome or a synchronization claim. Real Mail toggles or GOA restarts are optional, maintainer-supervised validation only.

## Evidence recorded in each PR

Record commit/build, environment and runtime revision, check commands/results, behavioral test counts, installed permissions and actual observed UI outcomes. Separate headless synthetic, graphical synthetic and installed-host evidence. Mark unavailable touch/desktop/test services explicitly unverified. Do not include account identifiers, addresses, screenshots with private data or secrets in public evidence. Do not create a standalone verification diary file.

PR 1 must pass the observation and account-policy matrix while keeping the application buildable. PR 2 must pass the full matrix and installed/accessible UI checks before F01 is called complete. Account hiding does not test or imply deletion of stored mail. Settings integration and installed-host checks remain pending.
