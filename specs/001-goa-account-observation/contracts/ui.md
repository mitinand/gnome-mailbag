# Account UI and Settings Contract

Account rules: `mailbag::accounts`. GTK display: `mailbag::account_ui`. Settings launcher: `mailbag::settings`. This contract applies to the approved UI at `7a69c49` and the clarified [feature specification](../spec.md).

## Existing surfaces

| Surface | F01 integration |
|---|---|
| `folder_tree` (`GtkListView`) | Flat account model; reuse `folder-row.ui`, no fabricated child folders. |
| `folder_details` (`AdwActionRow`) | Plain account identity/provider text. Stable row objects keyed by GOA ID. |
| `folder_badge` position | No healthy message count; a focusable problem button occupies the agreed suffix when needed. |
| `list_stack` existing `empty` page | Assign an ID to its `AdwStatusPage`; project account status text and relevant actions here. |
| `mail_split` / `list_page` | Show list page initially and on account activation so status is visible when collapsed. |
| `folders_split` | Preserve breakpoints and sidebar action; close an overlaid sidebar after account activation. |
| `toasts` | Confirmed-exclusion notices and Settings launch failures. Persistent account problems remain in status/icon explanations. |
| Existing `app.accounts` menu item | Register the action and share its launcher with the empty-state Online Accounts button. |
| `reader_stack`, mail actions, sync status | Neutral reader and unavailable mail controls; no mail loading, progress, success checkmark or synchronization claim. |

Preserve 1440×900 defaults, 360×294 minimum, 1100sp/720sp breakpoints, pane fractions, spacing, menus and existing action positions. Changes to visible navigation state and the agreed status/button content do not redesign the layout. No separate Welcome, setup gate or first-run state is introduced.

## Selection and focus

Use `GtkSingleSelection` with autoselect disabled and unselection allowed. The policy owns `selected_id`; GTK selection translates to ID-based commands. Reconcile by ID rather than clearing/recreating the list or remembering a row index. Presentation updates, individual problems and GOA outages preserve selection. An update that actually hides the selected row clears it; later reappearance does not restore it; another account is not automatically selected.

A problem icon has an accessible name describing the account problem, a tooltip and a keyboard-focusable button. Hover shows the explanation text; click, touch, Enter or Space opens the same explanation with relevant actions. Icon activation must not accidentally activate another account or open mail. If a resolved problem removes a focused icon, move focus to its surviving row. If a focused row disappears, focus the account list or the relevant status action without selecting another account. Close/reanchor an explanation whose account was excluded; never leave a detached popover referring to another row after list recycling.

Use plain text (`use-markup=false`); provider-supplied text is data. Explain duplicate presentation without exposing internal GOA IDs. All application-authored strings are English. High contrast, enlarged text and narrow layouts must retain essential actions.

## Status and pending work

Present AccountPage, row problems and excluded reasons from AccountList according
to FR-005; widgets do not recalculate eligibility. The shared
[account contract](accounts.md) defines last_check and check_pending. Use pending
state at retry controls without changing an established status page, row
availability, selection or unresolved error. First discovery has no previous result.
Show the common check error once in the status area; keep account-specific explanations at the row. Follow the wording and action rules in the specification.

## Notices

AccountList returns AccountHiddenNotice under [FR-017](../spec.md#requirements).
Present Single(label) using the existing disambiguated label, or Group(count), with
one combined removed-or-Mail-disabled wording. Keep one active toast and one pending
aggregate, releasing single labels when dismissed or aggregated. Settings failures
share this bounded presentation owner; retain their explanation separately so an
exclusion burst cannot hide a failed user action. No desktop notification is added.

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

Use asynchronous bus acquisition and method invocation with a shared 5-second overall deadline, expected reply-type validation, cancellable and attempt serial. Only one attempt can be pending across both entry points. Keep the window usable; pending activations reuse the existing attempt. On timeout/unavailable/access-denied/protocol failure, end pending state and show a visible safe explanation. Retain the last launch failure in the status explanation until retry/success so a burst of exclusion toasts cannot silently displace the failed user action. The user can retry after completion. On application shutdown, cancel and ignore expected cancellation and late completion; use weak UI references.

A successful reply means Settings accepted the action, not that Mailbag observed a visible panel. Do not show a success toast. Installed GNOME/Wayland acceptance must verify presentation with Settings closed and already open on another panel/workspace. Empty platform data is the initial supported call; any activation metadata needed to satisfy this target must come from a valid current user interaction, not fabricated or reused tokens.

## Sandbox contract

Add only these permissions to the approved manifest:

```yaml
- --talk-name=org.gnome.OnlineAccounts
- --talk-name=org.gnome.Settings
```

Existing Wayland and GPU permissions remain. No network, keyring, home/filesystem, blanket session/system bus or host-command access is needed. Record installed permissions separately from source-manifest review.

## Acceptance ownership (PR 2)

Automated model/GTK checks cover row identity, no auto-selection, focus after updates/exclusion, icon activation, retry/button access, neutral reader, absence of fake counts and narrow list-page visibility. Fake Settings checks assert exact parameter nesting, error/timeout, pending coalescing, shutdown cancellation and late completion.

Installed checks prove the existing Generic IMAP row, both Settings entry points and actual panel presentation, keyboard/mouse/touch explanations, 360-width and breakpoint behavior, enlarged text/high contrast, plus synthetic exclusion grouping. A machine without the appropriate display/input path reports that case unverified rather than equating click with touch.
