# Account State in Memory

All state belongs to the current Mailbag run. There is no database, credential
store, previous-run account history or cache-deletion record.

## Data and responsibilities

| Data | Meaning | Responsible component |
|---|---|---|
| GoaAccountId | Valid, nonempty GOA ID; never display or log it | GOA client validates it; Mailbag uses it as the account key |
| GoaAccountDetails | Provider, display fields, MailDisabled, Mail presence and AttentionNeeded | GOA client reads and validates fields |
| GoaAccountList | Latest accepted account details, whether the list is complete, check status and errors | GOA client |
| AccountCheck | Current request and reason (startup, user retry, recovery or periodic check), expected GOA process/client instance, deadline and cancellation | GOA client thread |
| VisibleAccount | ID, last usable display fields, availability and problem explanation | Mailbag accounts module |
| AccountSelection | Selected GOA ID or none | Mailbag accounts module |
| AccountHiddenNotice | Single-account display label or group count; one combined removed-or-Mail-disabled explanation | Mailbag accounts module; UI presents it |
| SettingsLaunch | Idle, pending or failed; request ID, deadline, cancellation and safe error | Settings module |

There is one pending GoaAccountList for the UI and a wakeup for its waiting task. A newer accepted update replaces
it. Only current account state is needed; intermediate switches are not recorded.

## Validating account fields

- GOA Id identifies the account. A display name or email address does not.
- Required fields for a newly available row: valid unique ID and provider type,
  explicit MailDisabled=false, present Mail interface and valid AttentionNeeded.
- AttentionNeeded=true keeps an otherwise eligible row with a repair explanation.
- The application maps imap_smtp, google and ms_graph to its three planned providers.
  Other providers are unsupported; Microsoft 365 does not require IMAP.
- Missing or wrongly typed required values are unknown/error, never false defaults.
- ProviderName, PresentationIdentity, icon and EmailAddress are display information.
  An empty address is valid. Invalid optional display data gets a neutral fallback.
- Display text is plain text. Accept safe themed icons only; a provider-supplied
  file/URI does not authorize opening a file or downloading an image.
- Identical display values do not merge accounts. If necessary, add a stable
  current-run display number to distinguish otherwise identical rows; do not expose
  internal IDs or invent addresses.

A malformed account can be isolated if its ID and the rest of the account list
are trustworthy. Missing/new ambiguous IDs or duplicate IDs make the list unsuitable
for confirming account absence. Within the same GOA process, a known object path
can attribute a damaged record to its existing ID. Do not reuse that mapping after
a process replacement without checking it again.

Read a valid MailDisabled=true independently of unrelated fields. Missing Mail
alone never means disabled.

## Applying an account update

The application compares its visible rows with the newest accepted client state.
It updates existing rows by ID and produces notices only for rows this update hides.

| Latest state | Result |
|---|---|
| Supported, enabled and valid new account | Add a row; do not automatically select it |
| Unsupported, disabled or unverified new account | Keep hidden; explain applicable reasons in the empty state |
| Valid display change | Update the existing row; preserve selection and focus |
| Known account needs attention or has a temporary problem | Keep its row and selection with a problem indicator |
| GOA/list unavailable or incomplete | Keep known rows unconfirmed; do not infer removal from missing data |
| Current trustworthy MailDisabled=true | Hide the row, clear its selection and produce a notice |
| Current complete list confirms absence | Hide the row, clear its selection and produce a notice |
| Account confirmed available after a row was hidden | Add it again without restoring the old selection |
| An old request/client returns late | Ignore the result before it becomes application state |

If disable→enable or removal→reappearance is confirmed before the UI takes the
update, it applies the final available state. There is no row removal, selection
reset or toast just to replay the earlier switch. If the UI already applied the
disabled/absent state, later reappearance cannot undo the selection reset.

An error never overwrites accepted data with an empty list. Keep usable display
facts during errors, but mark availability unconfirmed. Previously accepted explicit
disablement is not changed to enabled by a failed check; current list uncertainty
cannot establish a new removal. Do not use an older full list to pretend a failed
current membership check succeeded.

## Checking and retry status

Distinguish initial checking, checking again, ready, unavailable/incomplete, and
stopping. A failed recheck does not destroy the last usable display information.
A successful recheck clears only the problems it resolved, not an unrelated request
for attention from GOA. Every published status change has a new update number,
including pending/failure when no account property changed.

A routine ten-second background check does not change confirmed availability merely
by being pending. Keep its request status separate from visible loading; if Retry
Check joins it, show pending state at the retry controls. Its failure uses the same
unavailable state as any other failed account check. Periodic checks continue after failure, with no parallel requests or growing queue.

Exact request guards and limits belong to [the GOA contract](contracts/observation.md).

## Notices

Only an applied visible-to-hidden change caused by confirmed absence or disabled
Mail produces an account-hidden notice. Selection is irrelevant. Combine rows hidden
by the same update without splitting by cause. Use one removed-or-Mail-disabled
explanation: a single account is named, a group is counted. Keep the single display
label only until the notice is dismissed or merged into a group. Repeated unchanged state produces
no new notice because the rows are already hidden. A row shown again can later be
hidden by another update and produce a new notice.

Keep one active toast and one pending combined message. Do not retain all notices
or labels for the session. No toast is generated for an intermediate switch that
never hid a row, an unavailable service, an initially hidden account or a change
inferred from previous application runs.

## Future mail storage

Hiding an account and deleting stored messages are separate functions. F01 only
hides rows. A later mail-storage feature defines retention/deletion and interrupted
operation handling; no cache wipe is prescribed here. Likewise, continued mail work
with usable credentials during a GOA-only outage remains future-feature behavior.
