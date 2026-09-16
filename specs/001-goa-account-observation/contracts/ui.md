# Account UI and Settings Contract

Account rules: `mailbag::accounts`. GTK display: `mailbag::account_ui`. Settings launcher: `mailbag::settings`. This contract applies to the approved UI at `7a69c49` and the clarified [feature specification](../spec.md).

## Existing surfaces

| Surface | F01 integration |
|---|---|
| `folder_tree` (`GtkListView`) | Flat account model; reuse `folder-row.ui`, no fabricated child folders. |
| `folder_details` (`AdwActionRow`) | One plain account label without a subtitle. Stable row objects keyed by GOA ID. |
| `folder_badge` position | No healthy message count; a focusable problem button occupies the agreed suffix when needed. |
| `list_stack` existing `empty` page | Assign an ID to its `AdwStatusPage`; project account status text and relevant actions here. |
| `mail_split` / `list_page` | Show list page initially and on account activation so status is visible when collapsed. |
| `folders_split` | Preserve breakpoints and sidebar action; close an overlaid sidebar after account activation. |
| `toasts` | Confirmed-exclusion notices and Settings launch failures. Persistent account problems remain in status/icon explanations. |
| Existing `app.accounts` menu item | Register the action and share its launcher with the empty-state Online Accounts button. |
| `reader_stack`, mail actions, sync status | Neutral reader and unavailable mail controls; no mail loading, progress, success checkmark or synchronization claim. |

Preserve 1440×900 defaults, 360×294 minimum, 1100sp/720sp breakpoints, pane fractions, spacing, menus and existing action positions. Changes to visible navigation state and the agreed status/button content do not redesign the layout. No separate Welcome, setup gate or first-run state is introduced.

## Selection and focus

AccountList owns `selected_id`; clicking, tapping or pressing Enter on a row selects
that account by ID and opens the list page. Keep single-click activation, but
disable automatic item selection so pointer hover and focus alone do not select
an account. Reflect the selected ID in `GtkSingleSelection`, with autoselect
disabled and unselection allowed. Reconcile by ID rather than clearing/recreating
the list or remembering a row index. Presentation updates, individual problems
and GOA outages preserve selection. An update that actually hides the selected
row clears it; later reappearance does not restore it; another account is not
automatically selected.

AccountUi keeps rows in ascending AccountId order at startup and after updates.
Added accounts and accounts whose Mail is re-enabled enter at their ordered
position, preserving surviving row objects and the selected account.
AccountUi reconciles row membership and selection. Each model item holds the
account ID and its widgets together; the row updates its own text, problem
explanation and controls from AccountRow. The list factory binds those same widgets.
Account rows use 16-pixel symbolic icons, the `heading` label style and standard
Adwaita spacing. Google and Microsoft 365 use bundled symbolic icons. Google's
system SVG retains its contours with reduced outer padding to match the visible
size of the Microsoft 365 icon. Neither requires GOA icons to be installed.
Generic IMAP uses the system `mail-unread-symbolic` envelope.
GTK recolors the icons for light, dark and high-contrast themes.

A problem icon has an accessible name describing the account problem, a tooltip and a keyboard-focusable button. Hover shows the explanation text; click, touch, Enter or Space opens the same explanation with relevant actions. Icon activation must not accidentally activate another account or open mail. If a resolved problem removes a focused icon, move focus to its surviving row. If a focused row disappears, focus the account list or the relevant status action without selecting another account. Close/reanchor an explanation whose account was excluded; never leave a detached popover referring to another row after list recycling.

Use plain text (`use-markup=false`); provider-supplied text is data. Explain duplicate presentation without exposing internal GOA IDs. All application-authored strings are English. High contrast, enlarged text and narrow layouts must retain essential actions.

## Status and pending work

Present AccountPage, row problems and excluded reasons from AccountList according
to FR-005; widgets do not recalculate eligibility. The shared
[account contract](accounts.md) defines last_check and retry_pending. Only a user
Retry changes its controls to “Checking…”; automatic reads leave their labels and
sensitivity unchanged. Preserve the established page, availability, selection and
unresolved error while retrying. First discovery shows “Loading accounts”.
At cold start, an enabled supported account without Mail shows the account problem
and Retry Check, rather than the confirmed “No mail accounts” page. A successful
normal removal has no global error or transient warning on the surviving rows.
Show the common check error once in the status area; keep account-specific explanations at the row. Follow the wording and action rules in the specification.
Select the existing `list_stack` page explicitly: `empty` when presenting account
status and `messages` for the neutral area after selection. Updating status text
alone must not leave the status page hidden after the last account is excluded.

## Notices

AccountList returns one AccountHiddenNotice per excluded account under
[FR-017](../spec.md#requirements). Present each as a separate toast using the
existing disambiguated label and combined removed-or-Mail-disabled wording.
Use the existing AdwToastOverlay queue for both account notices and Settings launch
failures. Mailbag does not group notices or track active and pending toasts.
Settings failures exist only as ordinary launch-error toasts. Do not retain a
second Settings error in the account status or row explanations.
No desktop notification is added.

## Settings launch protocol

Both `app.accounts` and the status button invoke one launcher:

```text
Destination: org.gnome.Settings (session bus)
Object:      /org/gnome/Settings
Interface:   org.gtk.Actions
Method:      Activate
Body type:   (sava{sv})
Body:        ('launch-panel', [<('online-accounts', @av [])>], @a{sv} {})
Reply:       ()
```

The action parameter inside the variant is `(sav)`, not a bare string. Do not use an account ID or credentials as parameters. No shell command or host-spawn fallback.

Acquire the bus and invoke Settings asynchronously. Use the D-Bus method's default
timeout and validate the unit reply. One pending flag covers both entry points;
ignore repeated activations until completion. On failure, clear the flag and
report one error through on_error so the user can try again. The task holds only
a weak reference to the launcher; it cannot report an error after the launcher
is dropped. No separate overall deadline, stop state or stored task handle is needed.

AccountUi does not retain an Idle/Pending/Failed Settings model or include a
Settings failure in account-list status.

A successful reply means Settings accepted the action, not that Mailbag observed a visible panel. Do not show a success toast. Installed GNOME/Wayland acceptance must verify presentation with Settings closed and already open on another panel/workspace. Empty platform data is the initial supported call; any activation metadata needed to satisfy this target must come from a valid current user interaction, not fabricated or reused tokens.

## Sandbox contract

Add only these permissions to the approved manifest:

```yaml
- --talk-name=org.gnome.OnlineAccounts
- --talk-name=org.gnome.Settings
```

Existing Wayland and GPU permissions remain. No network, keyring, home/filesystem, blanket session/system bus or host-command access is needed. Record installed permissions separately from source-manifest review.

## Acceptance ownership (PR 2)

Automated model checks cover eligibility, notices, labels, failed reads and page states. The graphical test covers row identity, hover without selection and focus after row removal. Fake Settings checks assert exact parameter nesting, one pending launch, one error notification and a new attempt after failure.

Installed checks prove the existing Generic IMAP row, both Settings entry points and actual panel presentation, keyboard/mouse/touch explanations, 360-width and breakpoint behavior, enlarged text/high contrast, plus individual toasts for synthetic exclusions. A machine without the appropriate display/input path reports that case unverified rather than equating click with touch.
