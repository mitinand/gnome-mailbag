# F01 Validation Quickstart

This guide targets F01 on top of approved UI commit `7a69c49`. Portion 2 provides
the adapter’s initial acquisition, field validation and private-bus tests. Ongoing
account updates, recovery, account policy, Settings and UI integration remain
pending. Run the full sequence after implementation; initial-client tests do not
prove the remaining F01 behavior.

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
cargo test --locked -p mailbag accounts::
cargo test --locked -p mailbag settings::
```

GOA restart/recovery fixtures keep their session bus running; recovery after destruction of the entire desktop bus is outside F01.

The goa-adapter entry point runs the initial-client tests; the mailbag accounts and
settings test modules remain planned. The client tests start isolated D-Bus fixtures with service activation directories disabled and connect explicitly to those buses; they never replace the host GOA name or modify real accounts. Each fixture enforces an outer deadline and cleans up its daemon. Use synthetic identities only.

Expected coverage:

1. Healthy empty versus absent GOA; all three provider keys, unsupported providers, duplicate presentation and Microsoft 365 without IMAP.
2. Exercise every signal/property listed in the GOA contract, including events during initial discovery. Add/remove/disable/re-enable; both orders of MailDisabled/Mail-interface changes; one malformed account isolated from valid accounts. Verify event-driven updates before the next ten-second check.
3. Owner loss/replacement/recovery, stalled activation/account acquisition, retry activation, same-owner obsolete-client callbacks, malformed membership, late replies, explicit disable amid unrelated errors, retained rows and selected ID.
4. Pause UI updates, then disable/re-enable or remove/restore an account and confirm its final state: keep the visible row and selection, with no toast. Separately apply a disabled/absent update before restoring the account: expect one toast and cleared selection. Test 10,000 transient changes, resource-limit failure and recovery without an event history.
5. Retry after failure, repeated retry coalescing and worker progress without GTK/default-context iteration; stop during each pending phase. Verify that quiet state issues only the ten-second health request, with no frequent command/UI polling, and that updates arriving as a consumer starts waiting are not missed.
6. Notices for selected and unselected hidden rows: single-account display label, one combined count-based toast for mixed removal/Mail-disablement, deduplication, no false/cross-run notices, and a new notice after confirmed reappearance.
7. Exact Settings action body; absent/denied/hanging/error/malformed reply; shared pending launch and no late UI action after quit.
8. Hang GOA without changing its process name: the periodic request times out and preserves known rows/selection with a problem indicator. Resume replies after a failed check: periodic checks still recover, with no extra automatic attempts between ticks. Skip busy ticks, coalesce manual checks, and cancel the periodic timer at shutdown. Suppress one fixture change signal, verify that the next list check repairs current state, then emit another real change to verify subscriptions still work. Healthy checks cause no loading flash or success toast.

See [observation contract](contracts/observation.md) for budgets and [UI contract](contracts/ui.md) for expected presentation. Do not substitute live destructive account operations for missing synthetic tests.

## Graphical checks

In a GNOME graphical session, run the graphical tests separately from the headless gate:

```bash
cargo test --locked -p mailbag -- --ignored --test-threads=1
```

The approved UI already has an ignored graphical smoke test. Extend graphical coverage for F01; fixture-based graphical tests must still use private GOA/Settings connections. Confirm the output contains the intended graphical cases and no ignored/skipped case is reported as passed.

Check stable focus/selection under rename and outages; problem explanations by hover, click, touch and Enter/Space; Retry Check and Online Accounts via keyboard/touch; no fabricated mail or sync state. At 360 logical units and through 720sp/1100sp breakpoints, the list status must be visible and actions reachable. Repeat with enlarged text and high contrast. Keep geometry and action placement consistent with approved UI.

## Installed Flatpak

```bash
./scripts/build-flatpak.sh --install
flatpak info --user --show-permissions io.github.mitinand.Mailbag
flatpak run io.github.mitinand.Mailbag
```

Expected: the existing Generic IMAP account is discovered, selectable and marked as having mail reading not implemented. The GOA client is compiled into Mailbag; it does not install a separate daemon or require libgoa. Native GIO/GLib comes from the selected runtime; GOA and Settings run on the host. Its discovery does not request credentials. The installed manifest adds only the two named talk permissions; no network/broad-bus/host filesystem permission is introduced.

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

PR 1 must pass the observation and account-policy matrix while keeping the application buildable. PR 2 must pass the full matrix and installed/accessible UI checks before F01 is called complete. Account hiding does not test or imply deletion of stored mail. Real integration checks remain pending at the end of planning.
