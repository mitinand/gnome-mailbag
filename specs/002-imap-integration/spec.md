# Feature Specification: IMAP Integration

**Feature**: `002-imap-integration`
**Created**: 2026-09-16
**Revised**: 2026-09-19
**Status**: Approved by the maintainer 2026-09-19
**Input**: On an explicit refresh, load recent Inbox message metadata and plain-text body parts into
memory without attachment contents, fill the message list, and open the received
text without another mail request.
Message previews, HTML reading and persistent storage are separate future features.

The focus is IMAP integration. The existing UI makes its results visible
and testable; this feature does not design the final synchronization experience.
It extends [account observation](../001-goa-account-observation/spec.md).

**Scope**: Only the Inbox of one selected Generic IMAP account, with no remote
mail changes. Other folders, other providers and a combined Inbox are outside
this feature.

The latest 100 messages, newest-first ordering, text acquisition during batch
loading, loading only on explicit refresh, memory-only mail retention per account
and the wait limit are temporary development and acceptance choices. They exist
to evaluate the integration, so they stay minimal; later features may replace
them without retaining these mechanisms.

The lasting guarantees are secure connections (FR-010), reading without remote
changes (FR-005), no passwords on disk or in diagnostics (FR-006, FR-009), no mail
shown for another or confirmed excluded account (FR-007–008), no false empty
result or partial load presented as complete (FR-003), and responsive operation
without endless loading or crashes (FR-009). Technical mechanisms and concrete
limits belong to the plan. Temporary mechanisms need to preserve these guarantees,
not establish the final synchronization or storage design.

## Clarifications

### Session 2026-09-16

- Q: Should message content be obtained during Inbox loading or when a message
  is opened? → A: Obtain it during Inbox loading and keep it in memory. Opening
  uses the received content. Defer a separate on-demand body fetcher and its
  reconciliation scenarios until the synchronization mechanism is designed.
- Q: Should eager loading download complete messages or only the parts needed
  for text reading? → A: Obtain message metadata, the description of its parts
  and the needed plain-text body parts during batch loading, without attachment
  contents. Separate attachment retrieval is a future feature.
- Q: Which loading choices constrain later features? → A: The Scope distinguishes
  lasting guarantees from this stage's temporary batch, acquisition, retention,
  replacement and limit choices. Later features may replace the temporary mechanisms.
- Q: What installed permissions may this feature add? → A: Network access only,
  beyond F01 FR-016; no added filesystem, direct secret-store or host-command access.

### Session 2026-09-17

- Q: Does this stage need an application-defined download size limit? → A: No.
  Keep the 100-message window, selective text acquisition and an explicit failure
  when a response stops arriving.
- Q: What if neither encryption mode is enabled in the account settings? → A:
  Stop before requesting a password or opening a connection and explain the
  missing encryption setting. GOA's own SSL fallback for this case is internal
  to GOA and does not change the meaning of the flags it exports.
- Q: How much accessibility acceptance is required in 002? → A: Preserve F01's
  accessibility and check the new Refresh Inbox menu item and message rows by
  keyboard. Do not repeat the full input-method and narrow-width matrix.
- Q: Can a message whose part structure cannot be read be left out of the
  batch? → A: No. It keeps its row; opening it explains that its content could
  not be read. List fields do not depend on the part structure, so no message
  is skipped and no skipped count is shown. An interrupted transfer still fails
  the load.

### Session 2026-09-19

- Q: What starts a load? → A: Only Refresh Inbox. Selecting an account shows the
  batch received for it earlier in this run, or that nothing has been loaded;
  selection never loads.
- Q: What does a refresh keep? → A: Nothing. It clears the account's list and
  reader, loads a new batch and shows it. A failed load leaves the list empty
  and names the failing step.
- Q: What happens to received mail when the account is removed, Mail is disabled
  or the account service fails? → A: It may be discarded; a later refresh loads
  it again. A load that finishes after removal or Mail disablement cannot
  restore that account's mail.
- Q: Must the reader explain text it does not display in full? → A: No. It may
  show only the beginning of a long text; this stage shows that text was received.

## User Scenarios & Testing

### User Story 1 — Load recent incoming mail (Priority: P1)

Select a Generic IMAP account, load its recent messages with Refresh Inbox and
inspect the list.

**Why this priority**: This verifies account access, message acquisition and
the visible result of IMAP integration.
**Independent Test**: Load unchanged Inboxes with 0, 1, 100 and 101 messages.
Verify received content without opening the reader.

**Acceptance Scenarios**:

1. **Given** an enabled Generic IMAP account is selected, **when** the user
   activates Refresh Inbox, **then** Mailbag loads the batch, including message
   content, and shows up to 100 rows with sender, subject, date and read/unread
   appearance.
2. **Given** a successfully loaded empty Inbox, **when** the result appears,
   **then** Mailbag shows an empty Inbox. An account not loaded yet, a pending
   load or a failed load never claims emptiness.
3. **Given** more than 100 messages, **when** loading completes, **then** only the
   latest 100 appear, newest first by Inbox addition order.
4. **Given** an account was loaded earlier in this run, **when** the user selects
   another account and returns, **then** its received list appears again without
   loading. An account not loaded in this run shows that nothing has been loaded.

### User Story 2 — Read received text (Priority: P1)

Select a received message and show its plain-text content in the existing reader.

**Why this priority**: This makes the content obtained through IMAP inspectable
without adding a separate network operation for opening.
**Independent Test**: Finish loading, then open several received messages,
including one with both plain-text and HTML versions and attachments. Verify that
opening sends no request and no attachment contents were downloaded.

**Acceptance Scenarios**:

1. **Given** a loaded message with a plain-text body, **when** opened or
   reopened, **then** its received text appears without a network request.
   The reader MAY show only the beginning of a long text.
2. **Given** both plain-text and HTML versions, **when** opened, **then** the
   plain-text version appears. HTML-only mail gets an explanation that HTML
   reading is unavailable; no HTML source or converted HTML is displayed.
3. **Given** unread messages, **when** downloaded, listed and opened,
   **then** they remain unread on the server and no other flags or membership change.
4. **Given** a message has a plain-text body and attachments, including a text
   file, **when** its batch is loaded, **then** the body is available for local
   reading without downloading attachment contents or treating the attached
   text file as the body.

### User Story 3 — Refresh and explain a failed load (Priority: P2)

Load the selected Inbox again with Refresh Inbox and see why a load failed.

**Why this priority**: Repeating the IMAP operation allows integration and its
failures to be checked without restarting the application.
**Independent Test**: Start with a received batch, change the server Inbox,
then exercise a successful refresh, a failed refresh and a repeated refresh.

**Acceptance Scenarios**:

1. **Given** messages arrived, disappeared or changed read status elsewhere,
   **when** the user activates Refresh Inbox, **then** the list and reader are
   cleared, and after loading the new batch reflects the server result without
   duplicate rows.
2. **Given** a refresh fails, **then** the list stays empty and one explanation
   names the failing step. The previous batch is not kept.
3. **Given** a failed load, **when** its cause is resolved and the user refreshes
   again, **then** loading can succeed without restarting Mailbag.
4. **Given** loading is pending, **when** the user navigates, switches accounts or
   quits, **then** the interface remains usable. Refresh Inbox is unavailable
   until the load ends, and its result belongs to the account it was started for.
5. **Given** a server certificate fails validation, **when** mail access is
   attempted, **then** Mailbag reports a secure-connection failure and does not
   send the password, even if GOA allows certificate errors for that account.
6. **Given** account settings specify no encryption, **when** loading is
   attempted, **then** Mailbag identifies the unsupported security setting
   before requesting a password or connecting to the mail server.
7. **Given** an account requires upgrading its connection to encryption,
   **when** that upgrade is unavailable or fails, **then** loading stops with
   a secure-connection explanation and no cleartext sign-in or fallback.

### Edge Cases

These cases affect acquisition or the received batch. Opening received text
has no network failure or server-identity reconciliation of its own.

| Situation | Required visible result | Basis |
|---|---|---|
| Subject is absent, or sender/subject text cannot be fully decoded | Show a neutral fallback or replacement characters; keep the row usable | Sender-controlled mail fields; [mail format](https://datatracker.ietf.org/doc/html/rfc5322#section-3.6) |
| Text uses a transfer encoding or a non-Unicode character set | Display decoded text; UTF-8, Windows-1251 and KOI8-R are acceptance examples | [MIME encodings](https://datatracker.ietf.org/doc/html/rfc2045#section-6) |
| Received mail has attachments, several body alternatives or no readable plain-text body | Show the actual plain-text body, not an attached text file; explain unsupported or undecodable content for that message | [MIME body parts](https://datatracker.ietf.org/doc/html/rfc2046#section-5.1) |
| A message disappears while its batch is being obtained | Do not fabricate an empty message for a missing server result; the batch may contain fewer messages | Another client can remove mail; [IMAP message access](https://datatracker.ietf.org/doc/html/rfc9051#section-6.4.9) |
| The server does not return one message's data, while other messages load | Keep that message's row with an explanation; load the other messages normally | Servers can end a request with a failure after answering for the other messages, for example for a damaged message |
| The server answers for part of the message list and then refuses the command | Keep the rows that arrived and say that the list is incomplete, with the server's reason; a short list is never shown as complete, and one refused message never costs the whole batch | A missing row explains nothing by itself, unlike a missing structure or text |
| Account settings/password retrieval, connection, certificate validation, sign-in or mail retrieval fails | Identify the failing step once and stop dependent work; the list stays empty | These are separate stages of the loading operation |
| The user switches accounts while a load is pending | The load continues; its result appears only for the account it was started for | Account selection remains available during loading |
| GOA confirms removal or Mail disablement during loading | Clear that account's received mail; a late result cannot restore it | Existing [account lifecycle](../001-goa-account-observation/spec.md) |
| GOA temporarily fails after mail has been received | Received mail may be discarded; a later refresh loads it again | This stage does not preserve mail across account-service failures |
| A response stops arriving | End the load with an explicit timeout explanation; never present a partial transfer as complete | A connection can stop making progress; constitution III and V |

## Requirements

### Functional Requirements

- **FR-001 — Account scope**: Mailbag MUST read only the selected Generic IMAP
  account's Inbox, using GNOME Online Accounts (GOA) as the sole source of
  account settings and passwords. Account management and observation retain
  their existing rules. The introductory scope defines this stage's boundaries.
- **FR-002 — Development batch**: Each load MUST select up to the latest 100
  messages added to that Inbox and display received entries newest first by
  Inbox addition order, not by the displayed date.
  Missing messages during acquisition may reduce the result. For an unchanged
  Inbox, all selected messages MUST be represented, including messages whose
  part structure cannot be read; those get a content explanation (FR-004).
  Rows MUST show sender, subject, the server-reported received date
  (`INTERNALDATE`) and observed read/unread state. Missing or undecodable display
  text MUST NOT prevent other rows from appearing.
  The count and ordering are provisional for later features, but fixed for
  this stage's acceptance.
- **FR-003 — Loading and refresh**: Only activating Refresh Inbox MUST start a
  load. It MUST clear the selected account's list and reader, obtain a batch
  including message content and show it when complete. Selecting an account
  MUST NOT start a load; it shows the batch received for that account earlier in
  this run, if any. A failed load leaves the list empty; it MUST NOT publish a
  partial batch as complete. When the server answered part of the message list
  and then refused the command, Mailbag MUST show the messages it received and
  MUST report that the list is incomplete, with the server's reason; losing the
  whole batch over one refused message is not an acceptable answer either.
  Not loaded, loading, a successful empty result, an incomplete result and
  failure MUST be distinguishable.
- **FR-004 — Received text**: Mailbag MUST obtain message metadata and the
  description of its parts, then download the plain-text body parts needed for
  reading during batch loading. It MUST NOT download
  attachment contents. Attachment retrieval, viewing and saving are outside scope.
  Opening MUST use received content, without a separate body request or a check
  of current server state. Display text as inert content; do not use an attached
  text file as the body, render or convert HTML, load external content or generate
  list previews. Text MUST be shown the way its format defines: plain text marked
  `format=flowed` is unflowed before display, and a related set is read from the
  root its `start` parameter names. Unsupported or undecodable content MUST have a message-specific
  explanation. The reader MAY show only the beginning of a long text.
- **FR-005 — No remote changes**: Downloading, listing, refreshing and opening
  MUST NOT change message flags, contents or folder membership, including
  implicit read marking. Unimplemented mail actions MUST remain unavailable.
- **FR-006 — Memory and password storage**: Mailbag MUST NOT write passwords to
  disk, including temporary files; this guarantee is independent of the current
  stage. For this stage, Mailbag keeps each account's received batch (display
  fields and received text or content explanations) in memory until that
  account's next refresh, its discarding or exit. It MUST NOT create an
  application-managed disk store or temporary files for account connection
  details, lists or bodies. A new run MUST obtain mail again.
- **FR-007 — Correct view**: A load's result MUST be stored only for the account
  it was started for, and the list MUST show only the selected account's batch.
  The reader MUST show the selected entry's received content.
- **FR-008 — Account changes**: When GOA confirms that an account was removed or
  its Mail was disabled, Mailbag MUST discard that account's received mail, and a
  load still running for it MUST NOT restore it. Mailbag MAY also discard
  received mail when the account service fails. Retry Check remains an account
  observation action, separate from Refresh Inbox.
- **FR-009 — Failures and bounded work**: A failed load MUST report the actual
  failing step and permit a subsequent manual attempt. Failure to obtain a
  password from GOA MUST NOT be described as the mail server rejecting sign-in.
  Stop work that depends on the failed step; do not present cascading errors for
  steps that could not run. Unsupported or undecodable content belongs to its
  message and MUST NOT discard other successfully received messages. Mailbag
  MUST remain usable during slow or failed work and MUST NOT crash because a
  load fails. A stalled load MUST end with an explicit timeout failure, not
  endless loading or silently truncated content. This stage MUST NOT add an
  application-defined download size limit. Refresh Inbox MUST be unavailable
  while a load is running. Diagnostics MUST NOT contain credentials or personal mail.
- **FR-010 — Secure connection**: Passwords and mail MUST be transferred over an
  encrypted connection with a valid server certificate, with no fallback to
  cleartext. Mailbag MUST NOT bypass certificate validation, including when GOA
  permits certificate errors. Validation failure MUST stop the connection before
  password transmission and produce a secure-connection explanation.
- **FR-011 — UI for integration**: Populate the approved list and text reader and
  expose loading results and failures. Preserve the existing layout, adaptive
  behavior, keyboard and pointer/touch access, enlarged text and high contrast.
  Account-service explanations MUST remain accessible. The UI only makes the
  integration result visible; polishing final synchronization UX and designing
  UI interaction with a future database are outside this feature.
- **FR-012 — Refresh entry point**: The application menu MUST contain a
  **Refresh Inbox** item immediately after **Synchronization Status**. It loads
  the selected Generic IMAP account's Inbox. It is unavailable while a load is
  running and when no eligible Generic IMAP account is selected, including Google
  and Microsoft 365 accounts; for those, nothing claims an empty Inbox.
  This menu addition was approved for this feature.
- **FR-013 — Installed permissions**: Network access MUST be the only added
  permission beyond the baseline in [F01 FR-016](../001-goa-account-observation/spec.md#requirements).
  The installed application MUST support this feature without added filesystem
  access, direct access to the secret store or permission to execute host commands.

### Key Entities

- **Mail account**: The selected GOA account and its current permission to use Mail.
- **Received batch**: Up to 100 messages obtained by one completed load for that
  account, retained in memory until that account's next refresh, its discarding
  or exit.
- **Received message**: Its identity within the batch's Inbox, display fields,
  observed read status and decoded text or a content explanation. Display
  fields and content must refer to the same message, independent of row position.
- **Opened message**: The selected entry in the received batch. Opening is a
  local display action, not a new mail acquisition.

## Success Criteria

### Measurable Outcomes

- **SC-001**: For unchanged test Inboxes containing 0, 1, 100 and 101 messages,
  a refresh shows exactly 0, 1, 100 and 100 correct rows respectively, newest
  first by Inbox addition order, with zero duplicates or previews (US1; FR-001–003).
- **SC-002**: Opening and reopening received messages produces zero network
  requests. HTML-only messages explain the unsupported view;
  acquisition and display produce zero remote mail changes. Loading messages
  with file attachments, including text files, downloads zero attachment contents
  while making the supported body text readable (US2; FR-004–006).
- **SC-003**: A refresh clears the list and reader, then reflects arrivals,
  removals and changed read status. A failed refresh leaves the list empty with
  an explanation of the failing step; refreshing again recovers without an
  application restart. A refresh the server answered only in part keeps the rows
  it delivered and names the server's reason, so an incomplete list is never
  shown as complete. Selecting an account never starts a load (US3; FR-003,
  FR-009).
- **SC-004**: Verify storage and diagnostics as two distinct checks.
  **Permanent guarantee:** Inspection finds zero application-created password
  files, including temporary files, and no passwords, other credentials or
  personal mail in diagnostics. **This stage only:** Inspection finds zero
  application-created files containing mail, including temporary files, and after
  quitting and restarting without network access, no previous mail is restored
  (FR-006, FR-009).
- **SC-005**: Across account switching during a load and confirmed exclusion,
  zero results appear for the wrong account or restore excluded mail (FR-007–008).
- **SC-006**: Accessibility established by F01 does not regress. The new
  Refresh Inbox menu item and message rows are accessible by keyboard.
  Navigation and quitting remain
  usable during stalled loading; failed steps are distinguished from unsupported
  content in a received message (FR-009, FR-011–012).
- **SC-007**: Accounts with a valid server certificate can complete secure
  access. Invalid-certificate cases result in zero password transmissions,
  including when GOA permits certificate errors, and show an explanation.
  Inspection of the installed application's permissions finds network access
  as the only addition to the F01 FR-016 baseline (FR-010, FR-013).

## Assumptions

- The maintainer approved the guarantees and stage-only choices distinguished
  in Scope, selective text acquisition without attachment contents, loading only
  through Refresh Inbox, the installed-permission boundary and the Refresh Inbox
  menu addition. Approval of temporary mechanisms
  is limited to this stage; it does not bind later features to those mechanisms.
  Remaining technical details belong to planning.
- “Latest” means most recently added to Inbox, not the sender's Date header or
  the displayed `INTERNALDATE`, so displayed dates need not follow row order.
  This stage does not add settings for its batch size or ordering.
- Each account's completed batch is retained in memory for this run; switching
  accounts does not discard it. Raw bytes of received text parts need not remain
  after decoding. Nothing else is cached.
- Each refresh obtains a fresh batch; it does not reconcile received messages
  across runs or revalidate them when opened. Obtaining the description of
  message parts and then their selected contents belongs to this batch load;
  opening a received message requires no further network work.
- Mail access requires a GOA Generic IMAP account with password authentication
  and an encrypted connection supported by its server. Proposed connection modes,
  the credential path and dependencies are described in [the plan](plan.md).
  Access to IMAP settings and passwords requires extending the shared
  [goa-adapter contract](../001-goa-account-observation/contracts/accounts.md).
  Planning must propose that change for maintainer approval before implementation.
- Acceptance uses the existing Fedora/GNOME environment and includes the
  installed application. The maintainer's synthetic-server and live-account
  prototype results from 2026-09-16–17 are accepted evidence for design choices;
  they do not replace acceptance of the integrated Mailbag application.
- [The plan](plan.md) defines waiting behavior and how much text the reader shows.
  This stage has no application-defined download size limit. These are
  stage-specific design choices, not a future database interface.
- Mail servers whose certificate comes from a private certificate authority are
  outside scope in the installed application. Flatpak gives the sandbox the
  runtime's own set of certificate authorities and reserves `/etc`, so a
  certificate authority installed on the host is invisible to Mailbag while GOA,
  which runs on the host, accepts the same account. Supporting those servers
  needs either an extra trust anchor chosen by the user or host access beyond
  the FR-013 baseline; both belong to a later feature. Servers with a publicly
  trusted certificate are unaffected.
- Automatic polling, server push, automatic reconnect/retry loops, older history,
  persistent storage, reading across restarts without a network, previews,
  HTML conversion/rendering, attachment retrieval/opening/saving, decryption, signature
  verification, conversations, search, sending and remote changes are outside scope.
  On-demand body acquisition and configurable prefetch belong to later synchronization
  work. Integration with GNOME system proxy settings is also outside scope.
  No future-only headers, operation journal or persisted cleanup are required.
- The [constitution](../../.specify/memory/constitution.md) governs this feature.
  Proposed implementation portions and review pauses are in [the plan](plan.md).
