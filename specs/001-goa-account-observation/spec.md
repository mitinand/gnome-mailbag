# Feature Specification: Observe GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation`
**Created**: 2026-09-12 · **Revised**: 2026-09-15
**Status**: Approved revised design; implemented, manual acceptance remains

Mailbag shows mail accounts configured in GNOME Online Accounts, explains their
availability, follows changes and opens system account settings. It observes
accounts only; it does not authenticate to mail services or read mail.

## User stories

1. **US1 — See available accounts (P1).** Start with existing GNOME accounts,
   identify and select them, and understand empty states and account problems.
   Verify this independently of Settings and mail access (FR-001–005, FR-014–015).
2. **US2 — Trust account changes (P1).** Add, disable, remove or rename an account,
   interrupt GOA and retry observation. Verify retention, recovery and notices
   against the same displayed list (FR-006–010, FR-012, FR-017).
3. **US3 — Open system account management (P2).** Open Online Accounts from the
   menu and empty-state button; handle unavailable or unresponsive Settings.
   Verify both entry points independently of GOA (FR-011, FR-015–016).

## Requirements

These requirements own the user-visible behavior. Contracts describe its data and
protocol representation; other feature documents reference these rules.

- **FR-001**: Mailbag MUST use GNOME Online Accounts as the sole account authority. Account creation, removal, Mail enablement and account repair MUST remain in the system interface.
- **FR-002**: Mailbag MUST recognize Generic IMAP, Google and Microsoft 365 as the providers in this feature's planned mail scope. The separate consumer Microsoft/Outlook.com provider and other providers MUST remain unsupported; a display name or email domain MUST NOT alone determine support.
- **FR-003**: An account MUST be presented as confirmed available only when its existence, supported provider, enabled Mail and available Mail service are confirmed. A previously known row MUST remain visible during temporary problems with explicitly unconfirmed availability; row presence alone MUST NOT claim availability. Attention requirements MUST remain distinct from disablement and removal. Initially excluded accounts MUST remain hidden; the empty state offers guidance for adding a mail account or enabling Mail without listing unsupported providers.
- **FR-004**: Mailbag MUST present each account with one display label from GOA, without a provider or email-address subtitle. Use compact symbolic icons that follow the interface theme. When no display label is available, use the email address, then “Mail account” if neither is available. Account identity and selection MUST remain stable when display information changes; duplicate display names MUST NOT merge accounts and MUST remain distinguishable. A click, tap or keyboard activation selects an account; pointer hover MUST NOT select it.
- **FR-005**: Mailbag MUST distinguish discovery in progress, no configured accounts, no eligible mail accounts, an individual account problem, service-wide unavailable/incomplete information, account attention and failure to obtain account details. Already listed accounts with temporary problems MUST retain their rows with a problem icon in the message-count position. Discovery or retained row presence MUST NOT imply successful authentication, synchronization or an empty mailbox. An observation failure alone MUST NOT be described as invalid credentials or stopped mail access; the explanation MUST identify the inability to check account state. When no account rows remain and observation establishes an account-empty state, the existing status area MUST explain the applicable reasons and offer an Online Accounts button, both at startup and after confirmed exclusion of the last displayed account. Guidance MUST distinguish adding an account, enabling Mail and other known causes; loading or uncertainty MUST NOT be presented as confirmed absence. F01 MUST NOT introduce a separate Welcome screen, require account setup to access the normal interface or quit, or announce initial synchronization.
- **FR-006**: Account addition, confirmed removal, Mail enablement, attention and display-information changes MUST be reflected without restarting Mailbag. The UI MUST apply the latest confirmed account state; intermediate changes superseded before the UI update need not be replayed. If the latest state confirms the account is enabled and present, a brief earlier disablement/removal MUST NOT by itself hide its row, clear selection or produce a toast.
- **FR-007**: Explicit Mail disablement in an applied full account list MUST hide the affected account. Missing Mail service without explicit disablement MUST remain a distinct account problem: keep a previously known row marked as unconfirmed, and do not describe it as disabled or removed. At startup, an enabled supported account whose Mail service is missing MUST produce an explanation and Retry Check, even if no rows can yet be displayed.
- **FR-008**: A failed account-list request or a violation of its required data contract MUST produce one list-read error. Keep the previously accepted accounts and selection, marking availability as unconfirmed with problem icons. A cold start without an accepted list MUST show service unavailability, without fabricated rows or a healthy empty-account claim. Do not recover individual records from a rejected reply. Unsupported providers and a missing Mail service are valid account states, not malformed replies.
- **FR-009**: A successful full account-list request MUST replace the previous source data and clear the read error. It confirms absence and allows eligible accounts to recover. Normal account removal MUST update the list without a service-wide error. After a failed request, a later account-service event or Retry Check starts recovery. Recovery without a new event may require Retry Check; F01 does not detect a silent idle failure or poll for recovery.
- **FR-010**: When applying the current confirmed state excludes the selected account, Mailbag MUST hide its row, clear selection and show a neutral state without automatically choosing another account. Temporary account problems or service-wide uncertainty MUST preserve existing rows and selection with explicit problem indicators; they MUST NOT be treated as confirmed exclusion.
- **FR-011**: The existing Online Accounts menu action and the account-empty status button MUST open the same system Online Accounts panel. Failed or unresponsive launches MUST produce a visible error; repeated pending activations MUST NOT accumulate launch attempts.
- **FR-012**: Users MUST remain able to navigate and quit while discovery, recovery or Settings launch is pending or failing. Reads MUST use a bounded service-call timeout; repeated triggers MUST NOT create parallel reads or a queue of attempts. The GOA problem explanation and the account-service status area MUST offer Retry Check. Only an explicit Retry Check shows “Checking…” at retry controls; automatic reads leave those controls unchanged. Initial discovery has its own loading state. Starting a read MUST preserve the previous result, account availability and unresolved error until a new result arrives. An event or Retry during a read requests one follow-up read; multiple such triggers coalesce. Failure alone MUST NOT schedule a retry. Successful automatic reads MUST NOT show a success toast. Retry MUST NOT request credentials or initiate mail authentication.
- **FR-013**: F01 MUST NOT request or retain passwords/tokens, attempt mail authentication, connect to mail servers, fetch messages, change remote mail, or persist application account/mail data. Diagnostics and fixtures MUST NOT expose personal account details, credentials or mail.
- **FR-014**: Integration MUST preserve the approved application layout, dimensions, spacing, adaptive behavior and action/menu placement, with the agreed account problem indicator occupying the message-count position. Account selection, state explanations and the agreed Retry Check action MUST use the existing account and status areas and the indicator explanation. The Online Accounts button MUST occupy the existing account-empty status area while preserving the menu action and surrounding layout; unrealized mail actions MUST remain unavailable.
- **FR-015**: Account navigation, explanations and Online Accounts action MUST be usable by keyboard and at the existing narrow width, with enlarged text and high contrast. Hovering over a problem icon MUST show a tooltip; clicking, tapping, or pressing Enter/Space on the focused icon MUST open the same problem explanation. The icon and Retry Check action MUST be keyboard-accessible; Retry Check MUST also be usable by touch. Essential actions MUST NOT depend on hover/right-click; application-authored text MUST be English.
- **FR-016**: The feature MUST operate in the installed application with only the host access required for account observation and opening Settings. Broad filesystem/bus access, host-command escape and direct keyring access MUST NOT be introduced as integration shortcuts.
- **FR-017**: When confirmed GOA removal or explicit Mail disablement hides an account previously displayed in the current run, Mailbag MUST show an informational toast regardless of selection. Each toast MUST include the account's display label and use one combined explanation that the account was removed or Mail was turned off in Online Accounts. The message MUST NOT claim that remote mail was deleted or offer an in-app Undo of the system decision. Several exclusions MUST produce separate account toasts through the standard toast queue, without grouping or counting accounts. Each applied transition from displayed to hidden MUST be notified once; superseded intermediate changes that never hide a row MUST NOT generate a toast; subsequent observations, manual retries and further changes to an already excluded account MUST NOT repeat it. A later confirmed reappearance followed by a new exclusion is a new event. Service outages, incomplete information and initially excluded accounts MUST NOT generate this toast. F01 MUST NOT persist account history or notification receipts to detect or replay exclusions across application runs.

## Implementation boundary

Account rules use the public account data contract exported by `goa-adapter`.
Its internal `account_model` module owns the normalized data and validity checks;
the adapter translates GOA into that format. Mailbag owns provider support,
display, selection and notices. Application wiring selects the adapter. There is
no separate model crate, source registry, universal trait or second account authority.
See [the account contract](contracts/accounts.md).

The 2026-09-15 decisions replace the earlier worker and partial-record design.
Use a typed account record and accept a complete reply or one read error. The
observer runs in the application's main GLib context. Changes trigger a full
read; signals do not supply account fields. The [plan](plan.md) and
[GOA contract](contracts/observation.md) define the interface and read scheduling.

## Acceptance

| Criterion | Required evidence |
|---|---|
| SC-001 | FR-001–004 supported/unsupported providers, duplicate labels, fallback labels and stable identity |
| SC-002 | FR-006–009 events during reads, list-read errors, missing Mail, restart and recovery |
| SC-003 | Every FR-005 status and transition to/from the account-empty state |
| SC-004 | Both FR-011 entry points open the actual installed Settings panel and explain failures |
| SC-005 | FR-014–015 input, focus, 360-width, enlarged-text and high-contrast matrix |
| SC-006 | FR-010/012 selection, manual-only pending controls, repeated triggers, failed reads and responsive shutdown |
| SC-007 | FR-013/016 scope, safe diagnostics and installed permissions |
| SC-008 | FR-017 individual account notices, repeated observations, unselected accounts and superseded switches |

Automate the 17 scenarios defined in [quickstart.md](quickstart.md), with one test
per scenario. Test transport on private buses and AccountList rules separately
without D-Bus. Graphical and installed-host evidence remains separate; component
tests do not establish it.
Commands and manual acceptance are in [quickstart.md](quickstart.md); task ownership
and review boundaries are in [tasks.md](tasks.md).

## Assumptions and future work

- The supported acceptance environment is Fedora 44, GNOME 50, Wayland, x86_64,
  GNOME runtime 50. The approved UI baseline is commit `7a69c49`.
- GOA recovery assumes the desktop session bus remains running. Whole-bus recovery
  is outside F01. Fixture sizes are test inputs, not product limits or latency targets.
- All retained account/selection data belongs to this run. Cross-run notifications,
  cached mail, queued mail actions, Welcome/onboarding and initial synchronization
  require later features. F01 does not add storage for any of them.
- Hiding accounts and deleting stored mail are separate. Later storage work defines
  retention, deletion and crash recovery; F01 prescribes no automatic cache wipe.
- Credentials, mail synchronization, continued mail access during GOA failure,
  background application lifetime and any credential runtime/thread choice belong
  to later features. F01 sets no policy for them.
- Governing principles: [constitution](../../.specify/memory/constitution.md).

## Supported failure and transition cases

| Situation | User-visible result | Basis |
|---|---|---|
| GOA is absent, restarts or a read fails | Preserve known rows; show a read error if loading fails; recover on an event or Retry | GOA lifecycle and GIO calls; [research](research.md#full-list-reads) |
| Mail is enabled before its interface appears | Explain missing Mail and offer Retry; update when the interface appears | GOA configuration reload; [research](research.md#mail-availability) |
| Account information changes during a read | A follow-up read obtains the current list; identity remains stable | [Read scheduling](contracts/observation.md#events-and-request-ordering) |
| A reply violates the data contract | One visible read error; preserve the last accepted list | Constitution III; [account contract](contracts/accounts.md#validation-and-diagnostics) |
| Settings is missing, denied or does not answer | One launch-error toast; allow another attempt | [Settings contract](contracts/ui.md#settings-launch-protocol) |

Per constitution I, do not expand these into speculative recovery requirements.
Partial-record rescue, reconstruction of changed IDs, notification grouping,
periodic health checks and work while the main context is not dispatched are out
of scope. Producer guarantees and the retained Mail distinction are recorded in
[research](research.md).

## Account UI wording and behavior

The account-empty page uses one title, “No mail accounts”, with guidance to add a
mail account or enable Mail. It does not list unsupported providers. A failed
account-list request is a separate error, never an empty-account claim.
Before selection show “Select an account”; after selection leave the status area
blank unless a list request failed. Do not show development-stage messages or
claim that the mailbox is empty. Show the shared request failure once in the
status area. Row explanations describe only the affected account; an unconfirmed
row refers to the list check without repeating its detailed error. Offer Retry
Check for missing account details, and Online Accounts for required attention.
