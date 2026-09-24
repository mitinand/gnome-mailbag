# Feature Specification: Gmail Integration

**Feature**: `004-gmail-integration`
**Created**: 2026-09-22
**Revised**: 2026-09-23 after the specification challenge
**Status**: Approved and implemented on `claude/gmail`; live acceptance by the maintainer 2026-09-23
**Input**: On an explicit refresh, load recent Inbox message metadata and
plain-text body parts of the selected Google account into memory, the way
[IMAP integration](../002-imap-integration/spec.md) does for a Generic IMAP
account, signing in with the authorization that GNOME Online Accounts (GOA)
holds for the account instead of a password, and reading the mechanisms that
only Gmail has: a message identifier that stays the same in every folder and
labels. Google documents Gmail's IMAP behaviour in its
[IMAP extensions](https://developers.google.com/workspace/gmail/imap/imap-extensions),
[OAuth 2.0 mechanism](https://developers.google.com/workspace/gmail/imap/xoauth2-protocol)
and [IMAP, POP and SMTP](https://developers.google.com/workspace/gmail/imap/imap-smtp)
pages; this specification follows those pages and names them where a rule
comes from them.

The focus is the integration. The existing UI only makes its results visible
and testable; what is Gmail-specific is visible in the diagnostic record of
[logging](../003-logging/spec.md), not in the window. This feature extends
[account observation](../001-goa-account-observation/spec.md) and the IMAP
integration.

**Scope**: Only the Inbox of one selected Google account, with no remote mail
changes. Other folders, including All Mail, other providers and a combined
Inbox are outside this feature. Gmail is a second mail provider with its own
rules, not a variant of Generic IMAP: a message is identified by Gmail's own
identifier, belongs to several labels at once, and is authorized by a token
that GOA renews. Those rules are lasting. Where Gmail behaves like any IMAP
server, the rules of the IMAP integration apply unchanged and are referenced,
not repeated.

The latest 100 messages, newest-first ordering, text acquisition during batch
loading, loading only on explicit refresh, memory-only retention and the wait
limit are the same temporary development and acceptance choices as in the IMAP
integration. They exist to evaluate the integration and stay minimal; later
features may replace them without retaining these mechanisms.

The lasting guarantees are: secure connections (002 FR-010); reading without
remote changes (002 FR-005); no token on disk or in diagnostics (FR-002);
no mail shown for another or a confirmed excluded account (002 FR-007–008);
no false empty result or partial load presented as complete (002 FR-003);
responsive operation without endless loading or crashes (002 FR-009);
Gmail identity and label rules (FR-004–005); and Gmail as a separate
provider (FR-006). Technical mechanisms and concrete limits belong to the plan.

## Clarifications

### Session 2026-09-22

- Q: Where are Gmail's mechanisms visible at this stage? → A: In the record at
  debug only. No widget, column or wording is added to the window.
- Q: Does Mailbag identify itself to Gmail? → A: Yes; Google asks clients to,
  and it is one command. Decided during implementation (2026-09-23): the
  identification carries the fields Google's example asks for, `name`,
  `version`, `vendor` and `contact`, plus `support-url`; the contact is the
  maintainer's address.
- Q: Where do the two providers' load sequences live? → A: In a provider layer
  of their own, outside the window code; the plan describes the structure.

### Session 2026-09-23 (specification challenge)

- Q: Readable label and folder names now or later? → A: Now, in the cheapest
  form: Mailbag offers the readable-name mode once after sign-in and records
  the server's answer; no state and no decoder of the older encoding.
- Q: Thread identifier now? → A: Deferred to conversations. Nothing reads it in
  this feature, and reading it costs a change to the protocol library fork.
- Q: How is a refused token explained? → A: As a rejected sign-in with Gmail's
  reason, pointing to Online Accounts as the place to check the account's
  sign-in. The same sentence serves every provider; no Gmail-specific failure
  kind travels to the window.
- Q: Which Gmail situations get an edge case? → A: Only those whose visible
  result differs from the IMAP integration: a refused token and a token GOA
  cannot provide. Gmail's limits, session expiry and administrator
  restrictions are refusals with a server reason, already handled by 002.

## User Scenarios & Testing

### User Story 1 — Load recent Gmail Inbox mail (Priority: P1)

Select a Google account, load its recent messages with Refresh Inbox and
inspect the list, exactly as for a Generic IMAP account.

**Why this priority**: This verifies that the account's authorization from GOA
opens a Gmail session and that Gmail's Inbox reaches the list through the same
acquisition as any IMAP server.
**Independent Test**: Load a Google account's Inbox with 0, 1, 100 and 101
messages. Verify received content without opening the reader. No password is
ever requested or stored.

**Acceptance Scenarios**:

1. **Given** an enabled Google account is selected, **when** the user activates
   Refresh Inbox, **then** Mailbag signs in with the account's current
   authorization from GOA, loads the batch including message content and shows
   up to 100 rows with sender, subject, date and read/unread appearance.
2. **Given** the loaded Gmail Inbox, **when** the batch appears, **then** the
   rows, ordering, empty-Inbox result, retention across account switching and
   refresh behaviour follow the IMAP integration's User Story 1 unchanged.
3. **Given** a Google account and a Generic IMAP account loaded in the same run,
   **when** the user switches between them, **then** each shows its own batch;
   no Gmail rule leaks into the Generic IMAP account or the other way round.

### User Story 2 — Read received text (Priority: P1)

Select a received Gmail message and show its plain-text content in the
existing reader.

**Why this priority**: Gmail delivers message content over the same protocol,
so the IMAP integration's reading rules must hold for it without exception.
**Independent Test**: Finish loading, then open several received messages,
including one with both plain-text and HTML versions and attachments. Opening
sends no request and no attachment contents were downloaded.

**Acceptance Scenarios**:

1. **Given** a loaded Gmail message, **when** opened or reopened, **then** the
   IMAP integration's User Story 2 applies unchanged: received text appears
   without a network request, HTML-only mail is explained, unread messages
   stay unread on the server, no other flags or labels change.

### User Story 3 — Sign in with the account's authorization (Priority: P1)

Load a Google account whose authorization GOA holds, and see what happens when
Gmail or GOA refuses it.

**Why this priority**: Authorization is the one step that differs from a
Generic IMAP account. A refused authorization must be told apart from an
account-service failure, because the user's remedy differs: check the account
in Online Accounts rather than wait for the account service.
**Independent Test**: Load with a valid authorization; then revoke Mailbag's
access on the Google account's third-party access page and load again;
re-authorize in Online Accounts and load once more.

**Acceptance Scenarios**:

1. **Given** GOA holds a valid authorization, **when** the user activates
   Refresh Inbox, **then** Mailbag obtains the current access token from GOA at
   that moment and signs in with it; Mailbag never asks GOA for, stores or
   shows a password for a Google account.
2. **Given** Gmail refuses the token, **when** the load fails, **then** the
   explanation says that the server rejected the sign-in, gives Gmail's own
   reason and points to Online Accounts as the place to check the account's
   sign-in; it does not tell the user to change a password.
3. **Given** GOA cannot provide a token, for example because it could not renew
   it or the machine is offline, **when** the load fails, **then** the
   explanation names the account service, not the mail server.
4. **Given** the user re-authorized the account in Online Accounts, **when**
   they activate Refresh Inbox again, **then** the load can succeed without
   restarting Mailbag.
5. **Given** GOA reports that the account needs attention, **when** the user
   activates Refresh Inbox anyway, **then** the attempt is made, as for a
   Generic IMAP account; its outcome is reported truthfully.

### User Story 4 — See Gmail's own mechanisms in the record (Priority: P2)

Run Mailbag with logging at debug, load a Google account and read in the record
what Gmail said about the session and about each message.

**Why this priority**: This feature exists to establish the Gmail rules that
later features build on: identity, labels, server identification and readable
names. The window has no place for them yet, so the record is where they are
verified.
**Independent Test**: Load a Google account at debug and compare the record with
the same messages seen in the Gmail web interface, including one message with a
user label whose name is not in Latin letters.

**Acceptance Scenarios**:

1. **Given** a load at debug, **when** it completes, **then** the record shows
   for each received message its Gmail message identifier and labels as Gmail
   reported them.
2. **Given** a label whose name uses non-Latin letters, **when** it appears in
   the record, **then** it is readable text, not an encoded form.
3. **Given** a load at debug, **when** the session is established, **then** the
   record shows the capabilities Gmail announced, the name, vendor and version
   Gmail gave for its server, that Mailbag sent its name, version, vendor,
   contact and support address, and Gmail's answer to the readable-name
   offer.
4. **Given** the same message opened in the Gmail web interface, **when** its
   identifier is compared with the record, **then** they refer to the same
   message (the web interface shows the identifier in hexadecimal, the record
   in decimal).

### Edge Cases

These are the cases whose visible result differs from the IMAP integration.
The IMAP integration's edge cases apply to Gmail as well; Gmail's refusals for
its limits, an expired session or an administrator's restriction arrive as a
server reason and are shown the way 002 shows any refusal (see Assumptions).

| Situation | Required visible result | Basis |
|---|---|---|
| Gmail refuses the token (revoked, wrong scope, expired between renewal and use) | The load fails at sign-in with Gmail's reason; the explanation points to Online Accounts to check the account's sign-in; no retry loop and no password request | [XOAUTH2 error response](https://developers.google.com/workspace/gmail/imap/xoauth2-protocol#error_response) |
| GOA cannot provide a token | The load fails at the account-service step; Gmail is not contacted | GOA renews tokens itself and reports failure; Mailbag has no credentials of its own |

## Requirements

### Functional Requirements

- **FR-001 — Account scope**: Mailbag MUST read only the selected Google
  account's Inbox, using GOA as the sole source of account settings and
  authorization. Account management, observation and exclusion retain their
  existing rules; a Google account is eligible when it is enabled for mail and
  its mail service is available, like a Generic IMAP account.
- **FR-002 — Authorization**: Mailbag MUST sign in with the access token GOA
  provides for the account at the time of the load, through the OAuth 2.0
  mechanism Google documents for IMAP, and MUST NOT use, request or store a
  password for a Google account. GOA owns renewal and re-authorization;
  Mailbag MUST NOT renew a token or keep one between loads. The token is a
  credential under 002 FR-006 and 003 FR-009: never on disk, never in
  diagnostics. A refused token MUST be reported as a rejected sign-in with
  Gmail's reason, distinct from a failure to obtain the token from GOA, and
  the explanation MUST name Online Accounts as the place to check the
  account's sign-in.
- **FR-003 — Acquisition and reading as for IMAP**: The IMAP integration's
  FR-002 (batch), FR-003 (loading and refresh), FR-004 (received text),
  FR-005 (no remote changes), FR-006 (memory), FR-007 (correct view),
  FR-008 (account changes), FR-009 (failures and bounded work) and
  FR-010 (secure connection) MUST hold for a Google account unchanged. Gmail
  serves IMAP only over an encrypted dedicated port, and GOA stores it that
  way. Mailbag uses one connection per load and none between loads, and MUST
  NOT reconnect or retry automatically when Gmail ends a session or refuses
  because of its limits; Gmail's own reason is shown.
- **FR-004 — Gmail identity**: Each received message MUST carry Gmail's message
  identifier. The identity of a Gmail message is that identifier, which is the
  same in every folder the message belongs to and stable across runs; the
  folder-scoped identifiers of IMAP remain valid only within their folder and
  MUST NOT be treated as the message's identity for Gmail. This rule produces
  no mechanism in this feature; storage and synchronization build on it.
- **FR-005 — Labels as membership**: Each received message MUST carry its
  labels as Gmail reported them for the opened folder. A label is membership: a
  message belongs to any number of labels, and a label contains any number of
  messages. Label and folder names MUST be handled as readable text: once
  after sign-in, before opening a folder, Mailbag offers the server its
  readable-name mode and records the server's answer; when the server
  declines, names are recorded as sent. No decoder of the older name encoding
  is added. This stage records labels; it does not display, count or navigate
  them.
- **FR-006 — Gmail is a separate provider**: Gmail's rules MUST live in a
  provider of its own, next to the Generic IMAP provider, above the shared
  protocol layer, which knows no provider and reports only what the server
  said and what it was asked to fetch. No provider flag MAY reach the protocol
  layer, the content rules or the inside of a load sequence. The window uses
  the account's provider type only to decide which accounts can be loaded,
  as account observation already does for icons. The two providers' load
  logic lives in one place outside the window code; the structure is
  described in the plan.
- **FR-007 — Client identification**: Because Google asks IMAP clients to
  identify themselves and to leave a contact, Mailbag MUST announce its name,
  version, vendor, contact address and support address to Gmail after sign-in
  through the standard identification command and MUST record
  the name, vendor and version the server gives in return, at debug, and
  nothing else from that reply: Gmail's reply also carries the client's
  address and a connection token. A refused identification MUST NOT fail the
  load.
- **FR-008 — Record**: Under the logging rules, the record at debug MUST show,
  per load, the capabilities Gmail announced, the server's name, vendor and
  version and the answer to the readable-name offer, and per received message
  its Gmail message identifier and labels. Label names are folder-like names
  and belong to debug (003 FR-010). Nothing about Gmail appears at info beyond
  what the IMAP integration records for a load.
- **FR-009 — UI for verification**: Refresh Inbox MUST become available for a
  selected Google account and remain unavailable for Microsoft 365 accounts.
  *Amended 2026-09-23 by [Microsoft 365 integration](../005-microsoft-graph-integration/spec.md):
  Microsoft 365 accounts become eligible under that feature's rules.*
  The approved layout, adaptive behaviour and accessibility are preserved; no
  Gmail-specific widget, column or wording is added. The wording changes are
  the sign-in sentence of FR-002, which serves every provider, the explanation
  of a missing authorization from the account service, and the existing
  "Refresh Inbox" hint now shown for Google accounts too. The UI only makes
  the integration result visible.
- **FR-010 — Installed permissions**: No permission is added beyond the IMAP
  integration's baseline (002 FR-013); network access already exists.

### Key Entities

- **Google account**: A GOA account of the Google provider, enabled for mail,
  with settings and authorization held by GOA.
- **Access token**: The authorization GOA hands over for one load. Transient,
  never persisted or recorded, valid for a limited time that GOA manages.
- **Received Gmail message**: A received message of the IMAP integration plus
  its Gmail message identifier and the labels reported for it. Its identity
  across folders and runs is the Gmail message identifier.
- **Label**: A name a message belongs to. Gmail names its system labels with a
  leading backslash (Inbox, Sent, Drafts, Spam, Trash, Starred, Important,
  All Mail); user labels carry user-chosen, possibly nested, possibly
  non-Latin names. The record shows them as Gmail sent them.

## Success Criteria

### Measurable Outcomes

- **SC-001**: For a Google account's unchanged Inbox with 0, 1, 100 and 101
  messages, a refresh shows exactly 0, 1, 100 and 100 correct rows, newest
  first, with zero password requests during the run (US1, US3; FR-001–003).
- **SC-002**: The IMAP integration's SC-002 (zero requests on opening, zero
  remote changes, zero attachment downloads) and SC-003 (refresh and failure
  behaviour) hold for a Google account (US2; FR-003).
- **SC-003**: With Mailbag's access to the Google account revoked, a refresh
  fails with an explanation that names a rejected sign-in and Online Accounts,
  and after re-authorizing in Online Accounts a refresh succeeds without
  restarting Mailbag. With the account service unavailable, the explanation
  names the account service and Gmail receives zero connections (US3; FR-002).
- **SC-004**: Inspection finds zero application-created files containing a
  token and zero tokens in diagnostics at any level, including a refused
  sign-in at debug (FR-002; 003 FR-009).
- **SC-005**: For a loaded batch at debug, every received message has a Gmail
  message identifier in the record; the identifier of a message equals the one
  shown for it in the Gmail web interface; a user label whose name uses
  non-Latin letters appears as readable text (US4; FR-004–005, FR-008).
- **SC-006**: The record of a load at debug contains Gmail's announced
  capabilities, the server's name, vendor and version, and no client address
  or connection token (US4; FR-007–008).
- **SC-007**: A Google account and a Generic IMAP account loaded in one run
  show their own batches; the Generic IMAP account's behaviour under the IMAP
  integration's SC-001–007 is unchanged by this feature (US1; FR-006).
- **SC-008**: Refresh Inbox is available for a selected Google account, stays
  unavailable for a Microsoft 365 account, and accessibility established by
  F01 and the IMAP integration does not regress (FR-009).
  *Amended 2026-09-23 by [Microsoft 365 integration](../005-microsoft-graph-integration/spec.md):
  Refresh Inbox becomes available for Microsoft 365 accounts too.*

## Deferred to later features

These belong to Gmail's domain and are described here once so that later
features amend this specification instead of adding a layered one. None of
them gets requirements, plan decisions or code in this feature.

| Gmail mechanism | What it will be for | Waits for |
|---|---|---|
| All Mail as the account's store: every message once, with its labels | Storage and synchronization of the whole account; deletion proof across labels | Mail storage and synchronization |
| Labels as folders in the sidebar; label counts; the Inbox as the `\Inbox` label; special folders found by their role attribute, never by their localized name | Folder navigation and combined Inbox | Folders, labels and combined Inbox |
| Change detection with modification sequences and label changes | "What changed since the last load" without refetching | Mail storage and synchronization; the probe facts in Assumptions are its evidence |
| Deletion semantics: removing the Inbox label versus deleting, expunge behaviour set in Gmail's settings | Archive, trash and permanent deletion | Moving and deleting |
| Thread identifier | Grouping messages into conversations; reading it needs an accessor in the protocol library fork | Conversations |
| Gmail's own search syntax through IMAP | Remote search | Search |
| Signing in again after a session expires; idle notifications | Background freshness | Background and notifications |

## Assumptions

- The maintainer decided on 2026-09-22 and 2026-09-23 (see Clarifications):
  Gmail's specifics are visible in the record only; Mailbag identifies itself
  to Gmail; readable names in the cheapest form; the thread identifier is
  deferred; the two providers' load logic moves out of the window code into a
  provider layer of its own. The size budget agreed on 2026-09-22 is the limit
  on what this feature builds.
- The maintainer's read-only probe of 2026-09-22 against a Google account in
  GOA is accepted evidence for the plan: sign-in with the documented OAuth
  mechanism works without an initial response; a refused token yields Google's
  documented error exchange; Gmail announces modification sequences, readable
  names, unique identifiers and special-use folders after sign-in; the
  readable-name mode is accepted only before a folder is opened; the message
  identifier of an Inbox message is found in All Mail; label lists omit the
  opened folder's own label; the highest modification sequence of a folder can
  exceed that of every message in it; Gmail's identification reply carries the
  client's address. Details belong to the plan's research.
- Gmail's limits and lifetimes need no mechanism here: a load lasts seconds,
  GOA renews a token when it is close to expiry, sessions last about as long as
  the token, and Gmail's limits on simultaneous clients and daily transfer
  arrive as refusals with a reason. After the user revokes access at Google,
  GOA may still hand out its cached token for up to about an hour; the refresh
  then fails at sign-in until GOA renews, and Mailbag does not ask GOA to
  re-check the account. A Gmail setting that limits how many messages a folder
  shows over IMAP limits the batch; Mailbag does not compensate through another
  folder. Google Workspace accounts are Google accounts to Mailbag;
  administrator restrictions surface as Gmail's refusals.
- GOA exposes a Google account's mail settings with the encrypted dedicated
  port and provides its authorization through its OAuth interface; it renews
  the token itself and reports failure when it cannot. Access to that
  authorization requires extending the shared
  [goa-adapter access contract](../002-imap-integration/contracts/goa-access.md).
  Planning must propose that change for maintainer approval before
  implementation.
- FR-012 of the IMAP integration, which made Google accounts ineligible for
  Refresh Inbox, is amended by this feature: Google accounts become eligible;
  Microsoft 365 accounts remain ineligible until their own feature.
  *Amended 2026-09-23 by [Microsoft 365 integration](../005-microsoft-graph-integration/spec.md):
  that feature makes Microsoft 365 accounts eligible.*
- Acceptance uses the existing Fedora/GNOME environment, the installed
  application and the maintainer's Google account in GOA, with a user label
  whose name uses non-Latin letters on at least one Inbox message.
- Automatic polling, server push, reconnection, older history, persistent
  storage, previews, HTML, attachments, conversations, search, sending and
  remote changes are outside scope, as in the IMAP integration. Gmail's
  mechanisms for them are listed under Deferred.
- The [constitution](../../.specify/memory/constitution.md) governs this
  feature. Implementation portions and review pauses are defined in the plan.
