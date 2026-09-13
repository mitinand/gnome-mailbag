# Feature Specification: Observe GNOME Mail Accounts

**Feature**: F01 / `001-goa-account-observation` · **Branch**: `codex/goa`
**Created**: 2026-09-12 · **Status**: Approved scope; implementation in progress

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
- **FR-004**: Mailbag MUST show enough account identity and provider information to distinguish eligible accounts. Account identity and selection MUST remain stable when display information changes; duplicate display names MUST NOT merge accounts.
- **FR-005**: Mailbag MUST distinguish discovery in progress, no configured accounts, no eligible mail accounts, an individual account problem, service-wide unavailable/incomplete information, account attention and failure to obtain account details. Already listed accounts with temporary problems MUST retain their rows with a problem icon in the message-count position. Discovery or retained row presence MUST NOT imply successful authentication, synchronization or an empty mailbox. An observation failure alone MUST NOT be described as invalid credentials or stopped mail access; the explanation MUST identify the inability to check account state. When no account rows remain and observation establishes an account-empty state, the existing status area MUST explain the applicable reasons and offer an Online Accounts button, both at startup and after confirmed exclusion of the last displayed account. Guidance MUST distinguish adding an account, enabling Mail and other known causes; loading or uncertainty MUST NOT be presented as confirmed absence. F01 MUST NOT introduce a separate Welcome screen, require account setup to access the normal interface or quit, or announce initial synchronization.
- **FR-006**: Account addition, confirmed removal, Mail enablement, attention and display-information changes MUST be reflected without restarting Mailbag. The UI MUST apply the latest confirmed account state; intermediate changes superseded before the UI update need not be replayed. If the latest state confirms the account is enabled and present, a brief earlier disablement/removal MUST NOT by itself hide its row, clear selection or produce a toast.
- **FR-007**: Explicit Mail disablement in the current applied state MUST hide the affected account without waiting for unrelated account information to recover. A newer confirmed enabled state received before the UI update supersedes the earlier disablement under FR-006. Missing Mail service without confirmed disablement MUST be represented as a temporary problem: keep a previously known row marked as unconfirmed rather than treating it as explicitly disabled or removed.
- **FR-008**: Temporary account-service loss or an untrustworthy account list MUST NOT confirm removal or a healthy empty account set. Mailbag MUST keep rows known during the current run and their selection, marking availability as unconfirmed with problem icons. A cold start without account information MUST show service unavailability without fabricated or persisted account rows. If the account list is trustworthy, incomplete/invalid information for an individual account MUST be isolated and MUST NOT make other verified accounts unavailable.
- **FR-009**: After recovery, a complete trustworthy current account list is required to confirm absence; valid current information for each account is required to renew its confirmed availability. An invalid account MUST NOT prevent unaffected accounts from recovering when the list itself is trustworthy. Resolved problems MUST clear their indicators; confirmed removed or explicitly disabled accounts MUST be hidden. Older results and loss of observation continuity MUST NOT restore outdated eligibility.
- **FR-010**: When applying the current confirmed state excludes the selected account, Mailbag MUST hide its row, clear selection and show a neutral state without automatically choosing another account. Temporary account problems or service-wide uncertainty MUST preserve existing rows and selection with explicit problem indicators; they MUST NOT be treated as confirmed exclusion.
- **FR-011**: The existing Online Accounts menu action and the account-empty status button MUST open the same system Online Accounts panel. Failed or unresponsive launches MUST produce a visible error; repeated pending activations MUST NOT accumulate launch attempts.
- **FR-012**: Users MUST remain able to navigate and quit while discovery, recovery or Settings launch is pending or failing. Individual attempts MUST be bounded; failure or overload MUST be explicit, and later recovery MUST accept only confirmed current state. In addition to automatic recovery, the GOA problem explanation MUST offer Retry Check; when no account rows are known, the account-service status area MUST offer the same action. Manual retry MUST initiate or reuse one bounded observation attempt, show pending state, and leave failure visible if unresolved. Repeated activation MUST NOT accumulate parallel attempts. Manual retry MUST remain possible after failure and MUST NOT request credentials or initiate mail authentication. In addition to listening for changes and checking at startup, Mailbag MUST check GOA every ten seconds when idle, skip ticks while a check is pending, and continue periodic checks after failure without scheduling fast automatic retries. Starting or joining any check MUST preserve the last observation result and account availability until new evidence arrives. A pending retry MUST keep its unresolved error visible. Routine background checks MUST NOT flash loading or show a success toast. Their failures MUST follow FR-008; successful checks MUST apply current account data and clear only resolved problems.
- **FR-013**: F01 MUST NOT request or retain passwords/tokens, attempt mail authentication, connect to mail servers, fetch messages, change remote mail, or persist application account/mail data. Diagnostics and fixtures MUST NOT expose personal account details, credentials or mail.
- **FR-014**: Integration MUST preserve the approved application layout, dimensions, spacing, adaptive behavior and action/menu placement, with the agreed account problem indicator occupying the message-count position. Account selection, state explanations and the agreed Retry Check action MUST use the existing account and status areas and the indicator explanation. The Online Accounts button MUST occupy the existing account-empty status area while preserving the menu action and surrounding layout; unrealized mail actions MUST remain unavailable.
- **FR-015**: Account navigation, explanations and Online Accounts action MUST be usable by keyboard and at the existing narrow width, with enlarged text and high contrast. Hovering over a problem icon MUST show a tooltip; clicking, tapping, or pressing Enter/Space on the focused icon MUST open the same problem explanation. The icon and Retry Check action MUST be keyboard-accessible; Retry Check MUST also be usable by touch. Essential actions MUST NOT depend on hover/right-click; application-authored text MUST be English.
- **FR-016**: The feature MUST operate in the installed application with only the host access required for account observation and opening Settings. Broad filesystem/bus access, host-command escape and direct keyring access MUST NOT be introduced as integration shortcuts.
- **FR-017**: When confirmed GOA removal or explicit Mail disablement hides an account previously displayed in the current run, Mailbag MUST show an informational toast regardless of selection. The message MUST use one combined explanation that the account was removed or Mail was turned off in Online Accounts. A single-account notice MUST include its display label; a group MUST use the account count without splitting notices by cause. The message MUST NOT claim that remote mail was deleted or offer an in-app Undo of the system decision. Simultaneous exclusions MUST be grouped into one toast even when some accounts were removed and others had Mail turned off. Each applied transition from displayed to hidden MUST be notified once; superseded intermediate changes that never hide a row MUST NOT generate a toast; subsequent observations, manual retries and further changes to an already excluded account MUST NOT repeat it. A later confirmed reappearance followed by a new exclusion is a new event. Service outages, incomplete information and initially excluded accounts MUST NOT generate this toast. F01 MUST NOT persist account history or notification receipts to detect or replay exclusions across application runs.

## Implementation boundary

Account rules use the public account data contract exported by `goa-adapter`.
Its internal `account_model` module owns the normalized data and validity checks;
the adapter translates GOA into that format. Mailbag owns provider support,
display, selection and notices. Application wiring selects the adapter. There is
no separate model crate, source registry, universal trait or second account authority.
See [the account contract](contracts/accounts.md).

The 2026-09-13 review authorizes simplifying the existing PR 1 implementation:
separate pending work from the last result, discard malformed/ambiguous identity
records without reconstructing IDs from old paths, ignore unrelated service
changes, centralize data limits, share snapshots and remove the publication
counter. A malformed record cannot supply a disablement fact for a guessed account.
Unaffected identifiable records can still supply facts; an incomplete list cannot
confirm absence. [The GOA contract](contracts/observation.md) defines this boundary.

## Acceptance

| Criterion | Required evidence |
|---|---|
| SC-001 | FR-001–004 account/provider matrix, including 30 distinct accounts with duplicate labels |
| SC-002 | FR-006–009 events, individual/list errors, restart, recovery and stale replies |
| SC-003 | Every FR-005 status and transition to/from the account-empty state |
| SC-004 | Both FR-011 entry points open the actual installed Settings panel and explain failures |
| SC-005 | FR-014–015 input, focus, 360-width, enlarged-text and high-contrast matrix |
| SC-006 | FR-010/012 selection, retry, silent failure, periodic recovery and responsive shutdown |
| SC-007 | FR-013/016 scope, safe diagnostics and installed permissions |
| SC-008 | FR-017 single/group notices, deduplication, unselected accounts and superseded switches |

Automate behavioral and failure cases using synthetic accounts and private
services. Include adapter-to-account-list tests so separately correct components
cannot disagree about pending, malformed or superseded updates. Graphical and
installed-host evidence remains separate; component tests do not establish it.
Commands and manual acceptance are in [quickstart.md](quickstart.md); task ownership
and review boundaries are in [tasks.md](tasks.md).

## Assumptions and future work

- The supported acceptance environment is Fedora 44, GNOME 50, Wayland, x86_64,
  GNOME runtime 50. The approved UI baseline is commit `7a69c49`.
- GOA recovery assumes the desktop session bus remains running. Whole-bus recovery
  is outside F01. Thirty accounts is an acceptance fixture, not a product maximum.
- All retained account/selection data belongs to this run. Cross-run notifications,
  cached mail, queued mail actions, Welcome/onboarding and initial synchronization
  require later features. F01 does not add storage for any of them.
- Hiding accounts and deleting stored mail are separate. Later storage work defines
  retention, deletion and crash recovery; F01 prescribes no automatic cache wipe.
- In future mail features, after account inclusion/configuration was confirmed
  during the current run, loss of contact with GOA alone does not stop server
  requests or invalidate already obtained credentials. Mail work, including remote
  actions, continues using the last confirmed configuration while an existing
  authenticated connection or usable credentials permit it. Work requiring
  unavailable credentials or reauthentication waits; credential rejection is
  handled as an authentication failure, not ignored. Recovery reconciles account
  changes before further work uses newly observed state; confirmed Mail
  disablement/removal stops account work; this does not by itself prescribe
  deletion of stored mail. This deliberately allows GOA changes to take effect
  after observation recovers. It does not change cold-start confirmation or F01's
  prohibition on credentials, mail access and persistence. Later mail features
  must validate continuation with usable credentials, waiting without usable
  credentials, and stopping on confirmed exclusion.
- Governing principles: [constitution](../../.specify/memory/constitution.md).

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
Settings action implementation remains in the next portion.
