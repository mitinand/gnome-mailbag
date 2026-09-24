# Feature Specification: Microsoft 365 Integration

**Feature**: `005-microsoft-graph-integration`
**Created**: 2026-09-23
**Status**: Implemented on `claude/graph` and accepted live by the
maintainer 2026-09-24; approved 2026-09-23 with the plan after the
feature-start sizing, a read-only probe against the service and the
specification challenge (see Clarifications)
**Input**: On an explicit refresh, load recent Inbox message metadata and text
of the selected Microsoft 365 account into memory, the way
[IMAP integration](../002-imap-integration/spec.md) does for a Generic IMAP
account and [Gmail integration](../004-gmail-integration/spec.md) for a Google
account, signing in with the authorization that GNOME Online Accounts (GOA)
holds for the account and reading mail from the Microsoft Graph web service
instead of an IMAP server. Microsoft documents the service in its
[Outlook mail overview](https://learn.microsoft.com/en-us/graph/outlook-mail-concept-overview),
[list messages](https://learn.microsoft.com/en-us/graph/api/user-list-messages?view=graph-rest-1.0),
[get message](https://learn.microsoft.com/en-us/graph/api/message-get?view=graph-rest-1.0),
[immutable identifiers](https://learn.microsoft.com/en-us/graph/outlook-immutable-id),
[paging](https://learn.microsoft.com/en-us/graph/paging),
[change tracking](https://learn.microsoft.com/en-us/graph/api/message-delta?view=graph-rest-1.0),
[throttling limits](https://learn.microsoft.com/en-us/graph/throttling-limits#outlook-service-limits)
and [error responses](https://learn.microsoft.com/en-us/graph/errors) pages;
this specification follows those pages and names them where a rule comes from
them.

The focus is the integration. The existing UI only makes its results visible
and testable; what is specific to Microsoft 365 is visible in the diagnostic
record of [logging](../003-logging/spec.md), not in the window. This feature
extends [account observation](../001-goa-account-observation/spec.md) and the
IMAP integration, and is the first provider that does not speak IMAP.

**Scope**: Only the Inbox of one selected Microsoft 365 account, with no
remote mail changes. Other folders, other providers, a combined Inbox and
everything the service offers beyond reading the Inbox are outside this
feature. Microsoft 365 is a third mail provider with its own rules, not a
variant of the two IMAP providers: a message is identified by an identifier
that stays the same when the message moves between folders, belongs to exactly
one folder, carries its text as the body the service stores rather than as
MIME parts, is authorized by a token that GOA renews, and is read through web
requests rather than an IMAP session. Those rules are lasting. Where the
visible behaviour equals the IMAP integration's, its rules apply unchanged and
are referenced, not repeated.

The latest 100 messages, newest-first ordering, text acquisition during batch
loading, loading only on explicit refresh, memory-only retention and the wait
limit are the same temporary development and acceptance choices as in the IMAP
and Gmail integrations. They exist to evaluate the integration and stay
minimal; later features may replace them without retaining these mechanisms.

The lasting guarantees are: connections with a verified server certificate
(002 FR-010); reading without remote changes (002 FR-005); no token on disk
or in diagnostics (FR-002); no mail shown for another or a confirmed excluded
account (002 FR-007–008); no false empty result or partial load presented as
complete (002 FR-003); responsive operation without endless loading or
crashes (002 FR-009); the Microsoft 365 identity, content and folder rules
(FR-004–006); Microsoft 365 as a separate provider (FR-007); and refusals
reported with the service's reason and never retried (FR-008). Technical
mechanisms and concrete limits belong to the plan.

## Clarifications

### Session 2026-09-23 (feature-start)

- Q: How is message text obtained? → A: As the body the service renders as
  text, requested together with the message list. No MIME download and no part
  selection: the service does not keep messages as MIME, it stores a body and
  renders it in the format asked for. An HTML-only message therefore shows the
  service's text rendering instead of the IMAP integration's "HTML reading is
  unavailable" explanation.
- Q: Which identifier is the message's identity? → A: The immutable
  identifier, asked for on every request that returns messages, from the first
  load on. The default identifier changes when a message moves between folders.
- Q: Where are Microsoft 365's mechanisms visible at this stage? → A: In the
  record at debug only. No widget, column or wording is added for them.
- Q: Change tracking now? → A: Probed only; the results are recorded in
  Assumptions as evidence for storage and synchronization. No code.
- Q: Folder listing now? → A: None. The Inbox is addressed by its well-known
  role, which the service resolves regardless of the mailbox's language.

### Session 2026-09-23 (specification challenge)

- Q: Follow the service's further pages until 100 messages arrived? → A: No.
  One load is one request; when the service answers with fewer messages than
  the batch size and offers a further page, the batch is shown and reported
  as incomplete through the IMAP integration's existing incomplete-list
  notice. The probe received 100 in one page; a page loop, a rule for a
  message reported twice and a rule for a failed later page are not built.
- Q: What is a message's identity in the batch? → A: A per-provider identity
  kind in one field, as FR-007 says; the reviewer's alternative of an
  optional IMAP number plus a Microsoft 365 field costs the same and adds a
  third optional field. Kept.
- Q: Pass on the wait the service asks for after a refusal? → A: No. A
  refused request is reported with the service's status and error code like
  any refusal; Mailbag never retries on its own. How waits are presented is
  for the error presentation feature.
- Q: A separate secure-connection failure kind, as in 002? → A: No. The
  guarantee stays: verification completes before any request is sent. A
  failed connection, including a failed verification, is one connection
  failure with the platform's reason.
- Q: Record the service's request identifier and time of a refused request?
  → A: No. They serve a support case with Microsoft, which an account using
  GOA's shared application identity does not get; status and code suffice.
- Q: Record the service's attachment indication with each message? → A:
  Deferred to attachments; nothing reads it at this stage.
- Q: A body that arrives as HTML although text was asked for? → A: Nothing
  is built. The service documents that the body arrives in the requested
  form, and the probe confirmed it for 100 messages; acceptance would show
  such a body as markup in the reader.

## User Scenarios & Testing

### User Story 1 — Load recent Microsoft 365 Inbox mail (Priority: P1)

Select a Microsoft 365 account, load its recent messages with Refresh Inbox
and inspect the list, exactly as for a Generic IMAP or Google account.

**Why this priority**: This verifies that the account's authorization from GOA
opens the service and that the Inbox reaches the list through a load that
shares no protocol with the IMAP providers.
**Independent Test**: Load a Microsoft 365 account's Inbox with 0, 1, 100 and
101 messages. Verify received content without opening the reader. No password
is ever requested or stored.

**Acceptance Scenarios**:

1. **Given** an enabled Microsoft 365 account is selected, **when** the user
   activates Refresh Inbox, **then** Mailbag obtains the account's current
   authorization from GOA, requests the batch including message text and shows
   up to 100 rows with sender, subject, received date and read/unread
   appearance.
2. **Given** the loaded Inbox, **when** the batch appears, **then** the rows,
   the empty-Inbox result, retention across account switching and refresh
   behaviour follow the IMAP integration's User Story 1 unchanged; the order
   is newest first by the received time the service reports.
3. **Given** a Microsoft 365 account, a Google account and a Generic IMAP
   account loaded in the same run, **when** the user switches between them,
   **then** each shows its own batch; no Microsoft 365 rule leaks into the
   other providers or the other way round.

### User Story 2 — Read received text (Priority: P1)

Select a received message and show its text in the existing reader.

**Why this priority**: The service delivers text differently from an IMAP
server, and the reading rules of the IMAP integration must hold wherever the
difference is invisible and be stated wherever it is not.
**Independent Test**: Finish loading, then open several received messages:
one sent with both a text and an HTML version, one HTML-only newsletter, one
with attachments including a text file, and one whose body is only a picture.
Opening sends no request and no attachment contents were downloaded.

**Acceptance Scenarios**:

1. **Given** a loaded message, **when** opened or reopened, **then** its
   received text appears without a network request; the reader may show only
   the beginning of a long text.
2. **Given** an HTML-only message, **when** opened, **then** the text the
   service rendered from it appears; no HTML source is shown and Mailbag
   converts nothing itself.
3. **Given** unread messages, **when** listed and opened, **then** they remain
   unread on the service and no other property or folder membership changes.
4. **Given** a message with attachments, **when** its batch is loaded,
   **then** its text is available for local reading without downloading
   attachment contents, and an attached text file is never shown as the body.

### User Story 3 — Sign in with the account's authorization (Priority: P1)

Load a Microsoft 365 account whose authorization GOA holds, and see what
happens when the service or GOA refuses it.

**Why this priority**: Authorization is obtained the way the Gmail integration
obtains it, but it is presented to a web service on every request instead of
once at sign-in. A refused authorization must be told apart from an
account-service failure, because the user's remedy differs.
**Independent Test**: Load with a valid authorization; then remove Mailbag's
access at the Microsoft account's permissions page or sign the account out in
Online Accounts and load again; re-authorize in Online Accounts and load once
more.

**Acceptance Scenarios**:

1. **Given** GOA holds a valid authorization, **when** the user activates
   Refresh Inbox, **then** Mailbag obtains the current access token from GOA at
   that moment and presents it with every request of that load; Mailbag never
   asks GOA for, stores or shows a password for a Microsoft 365 account.
2. **Given** the service refuses the token, **when** the load fails, **then**
   the explanation says that the service rejected the sign-in, gives the
   service's own reason and points to Online Accounts as the place to check
   the account's sign-in; it does not tell the user to change a password.
3. **Given** GOA cannot provide a token, for example because it could not
   renew it or the machine is offline, **when** the load fails, **then** the
   explanation names the account service, and the mail service is not
   contacted.
4. **Given** the user re-authorized the account in Online Accounts, **when**
   they activate Refresh Inbox again, **then** the load can succeed without
   restarting Mailbag.
5. **Given** GOA reports that the account needs attention, **when** the user
   activates Refresh Inbox anyway, **then** the attempt is made, as for the
   other providers; its outcome is reported truthfully.

### User Story 4 — See Microsoft 365's own mechanisms in the record (Priority: P2)

Run Mailbag with logging at debug, load a Microsoft 365 account and read in
the record what the service said about the load and about each message.

**Why this priority**: This feature exists to establish the Microsoft 365
rules that later features build on: identity, content, folder membership and
the service's limits. The window has no place for them yet, so the record is
where they are verified.
**Independent Test**: Load a Microsoft 365 account at debug, note one
message's identifier, move that message to another folder and back in Outlook,
load again and compare.

**Acceptance Scenarios**:

1. **Given** a load at debug, **when** it completes, **then** the record shows
   for each received message its immutable identifier, received time and read
   state, and for the load the request made, its response status, whether
   the service offered further messages, and the number and size of
   messages.
2. **Given** a message moved to another folder and back by the user in
   Outlook, **when** the next load's record is compared with the previous one,
   **then** the message carries the same identifier.
3. **Given** a request the service refused, **when** the failure is recorded,
   **then** the record shows the response status and the service's error
   code.
4. **Given** any level, **when** the record is inspected, **then** it contains
   no token, no message text, no subject and no address.

### Edge Cases

These are the cases whose visible result differs from the IMAP integration.
The IMAP integration's cases about account changes, account switching during
a load, a stalled transfer and a failing step apply to Microsoft 365 as well.
Its cases about decoding sender and subject text, character sets, part
structures and messages that disappear while parts are fetched do not arise:
the service delivers display fields and text as ready values in one answer.

| Situation | Required visible result | Basis |
|---|---|---|
| The service refuses the token (revoked, expired between renewal and use) | The load fails at sign-in with the service's reason; the explanation points to Online Accounts to check the account's sign-in; no retry and no password request | [Error responses](https://learn.microsoft.com/en-us/graph/errors): 401 with an error code |
| GOA cannot provide a token | The load fails at the account-service step; the service is not contacted | GOA renews tokens itself and reports failure; Mailbag has no credentials of its own |
| The service refuses because of its limits, or is temporarily unavailable | The load fails with the service's status and error code as the reason; no automatic retry and no background attempt; the user may refresh later | [Throttling limits](https://learn.microsoft.com/en-us/graph/throttling-limits#outlook-service-limits): limits apply per application identity and mailbox, and the application identity GOA uses is shared by every GNOME application on the machine, so another application's synchronization can use up the allowance |
| The service answers with fewer messages than the batch size and offers a further page | The messages that arrived are shown and the list is reported as incomplete, the way the IMAP integration reports a list the server did not finish; Mailbag does not fetch the further page | [Paging](https://learn.microsoft.com/en-us/graph/paging): a page may hold fewer results than requested; the probe received 100 in one page |
| The mailbox cannot be read through the service, for example because the account has no mailbox at the service or its mailbox is kept on the organization's own server | The load fails at the request step with the service's reason; it is not described as a rejected sign-in | [Error responses](https://learn.microsoft.com/en-us/graph/errors): 403 or 404 with an error code, after a valid sign-in |

## Requirements

### Functional Requirements

- **FR-001 — Account scope**: Mailbag MUST read only the selected Microsoft
  365 account's Inbox, using GOA as the sole source of account identity and
  authorization. Account management, observation and exclusion retain their
  existing rules; a Microsoft 365 account is eligible when it is enabled for
  mail and its mail service is available, like the other providers' accounts.
- **FR-002 — Authorization**: Mailbag MUST present the access token GOA
  provides for the account at the time of the load with every request of that
  load, in the way the service documents for delegated access, and MUST NOT
  use, request or store a password for a Microsoft 365 account. GOA owns
  renewal and re-authorization; Mailbag MUST NOT renew a token or keep one
  between loads. The token is a credential under 002 FR-006 and 003 FR-009:
  never on disk, never in diagnostics, and never in a request's address. A
  refused token MUST be reported as a rejected sign-in with the service's
  reason, distinct from a failure to obtain the token from GOA, and the
  explanation MUST name Online Accounts as the place to check the account's
  sign-in.
- **FR-003 — Acquisition and reading as for IMAP**: The IMAP integration's
  FR-002 (batch), FR-003 (loading and refresh), FR-005 (no remote changes),
  FR-006 (memory), FR-007 (correct view), FR-008 (account changes), FR-009
  (failures and bounded work) and FR-010 (secure connection) MUST hold for a
  Microsoft 365 account, with these readings: "newest first" is the received
  time the service reports, and the service orders the list; the secure
  connection is the encrypted web connection the service requires, with the
  server certificate verified against the system's trust and no exception for
  a certificate error, and verification completes before any request is
  sent, so no token travels over an unverified connection; a failed
  connection, including a failed verification, is reported as a connection
  failure with the platform's reason, without a failure kind of its own. One
  load is one request for the batch, with no request between loads. When the
  service answers with fewer messages than the batch size and offers a
  further page, the messages that arrived are shown and the list is reported
  as incomplete under 002 FR-003's incomplete-list rule; Mailbag MUST NOT
  fetch further pages at this stage and MUST NOT retry a request
  automatically for any reason. The IMAP integration's FR-004 (received
  text) is replaced for this provider by FR-005 below.
- **FR-004 — Microsoft 365 identity**: Each received message MUST carry the
  service's immutable identifier, asked for on every request that returns
  messages. The identity of a Microsoft 365 message is that identifier, which
  is the same in every folder of the mailbox and stable across runs; the
  service's default identifier changes when a message moves between folders
  and MUST NOT be treated as the message's identity. This rule produces no
  mechanism in this feature beyond asking for the immutable form; storage and
  synchronization build on it.
- **FR-005 — Content is the service's body**: The text of a Microsoft 365
  message is the body the service stores, rendered by the service in the form
  Mailbag asks for; at this stage Mailbag asks for text and obtains it
  together with the message list, during batch loading. Mailbag MUST NOT
  download the message in MIME form, select parts, decode transfer encodings
  or convert HTML for this provider. An HTML-only message shows the service's
  text rendering; a message whose rendering is empty shows empty text, and
  Mailbag fetches nothing else to fill it. Attachments MUST NOT be downloaded.
  Display fields (sender, subject, received time, read state) MUST come from the
  service's structured fields, not from decoded headers. Opening MUST use
  received content without a request. Text is displayed as inert content, and
  the reader MAY show only the beginning of a long text.
- **FR-006 — One folder per message**: A Microsoft 365 message belongs to
  exactly one folder; moving it changes that folder and nothing else about its
  identity. Folders MUST be addressed by their well-known role where the
  service defines one, never by their localized display name; this stage
  addresses the Inbox by its role and lists no folders. This rule produces no
  mechanism in this feature.
- **FR-007 — Microsoft 365 is a separate provider**: Microsoft 365's rules
  MUST live in a provider of its own, next to the Generic IMAP and Gmail
  providers, over a service-access layer of its own that shares no code with
  the IMAP protocol layer. The batch every provider delivers MUST carry each
  message's identity as that provider's own kind, so that no provider flag and
  no fabricated stand-in identifier reaches the window; the window uses the
  account's provider type only to decide which accounts can be loaded. The
  three providers' load logic lives in one place outside the window code; the
  structure is described in the plan.
- **FR-008 — Refusals without retries**: When the service refuses a request,
  for any reason including its limits or temporary unavailability, the load
  MUST fail with the service's response status and error code as the reason,
  and Mailbag MUST NOT retry on its own, in the foreground or in the
  background. The wait the service may ask for is not read at this stage; the
  user may refresh again at any time.
- **FR-009 — Record**: Under the logging rules, the record at debug MUST show,
  per load, the request made (its path, never the token and never a query
  value that holds personal data), its response status, whether the service
  offered further messages, and the number and size of messages, and per
  received message its immutable identifier, received time and read state. A
  failed request MUST be recorded at error with its status and the service's
  error code; the service's error text is developer text and belongs to debug
  (003 FR-011). Nothing about Microsoft
  365 appears at info beyond what the IMAP integration records for a load.
- **FR-010 — UI for verification**: Refresh Inbox MUST become available for a
  selected Microsoft 365 account. The approved layout, adaptive behaviour and
  accessibility are preserved; no Microsoft 365-specific widget, column or
  wording is added. The wording changes are the sign-in sentence of the Gmail
  integration, which already serves every provider, and the failure
  explanation of the IMAP integration gaining the service's status and error
  code as the reason, with a title for each of the four ways a service
  request can fail (no connection, no answer in time, a refusal, an answer in
  an unexpected form); the existing incomplete-list notice serves a batch the
  service cut short in a second form; and the account-settings explanation of
  the account service losing its IMAP wording. The existing "Refresh Inbox"
  hint is shown for Microsoft 365 accounts too. The UI only makes the
  integration result visible.
- **FR-011 — Installed permissions**: No permission is added beyond the IMAP
  integration's baseline (002 FR-013); network access and access to Online
  Accounts already exist.

### Key Entities

- **Microsoft 365 account**: A GOA account of the Microsoft 365 provider,
  enabled for mail, with identity and authorization held by GOA. Personal
  Microsoft accounts are added through the same provider and are Microsoft
  365 accounts to Mailbag.
- **Access token**: The authorization GOA hands over for one load. Transient,
  never persisted or recorded, valid for a limited time that GOA manages,
  presented with every request of the load.
- **Received Microsoft 365 message**: A received message of the IMAP
  integration whose identity is the service's immutable identifier, whose
  display fields are the service's structured fields and whose text is the
  service's rendering of the body. It belongs to the folder it was loaded
  from.
- **Mail folder**: A container of messages in the mailbox. The service defines
  well-known roles for the folders it creates (Inbox, Sent Items, Drafts,
  Deleted Items, Junk Email, Archive); only the Inbox role is used here.
- **Change set**: What the service reports as added, changed or removed in a
  folder since a previous round. Probed in this feature, used by storage and
  synchronization.

## Success Criteria

### Measurable Outcomes

- **SC-001**: For a Microsoft 365 account's unchanged Inbox with 0, 1, 100 and
  101 messages, a refresh shows exactly 0, 1, 100 and 100 correct rows, newest
  first by received time, with zero password requests during the run (US1,
  US3; FR-001–003).
- **SC-002**: The IMAP integration's SC-002 (zero requests on opening, zero
  remote changes, zero attachment downloads) and SC-003 (refresh and failure
  behaviour) hold for a Microsoft 365 account, and an HTML-only message shows
  text rather than an explanation (US2; FR-003, FR-005).
- **SC-003**: With the account's authorization refused by the service, a
  refresh fails with an explanation that names a rejected sign-in and Online
  Accounts, and after re-authorizing in Online Accounts a refresh succeeds
  without restarting Mailbag. With the account service unavailable, the
  explanation names the account service and the mail service receives zero
  requests (US3; FR-002).
- **SC-004**: Inspection finds zero application-created files containing a
  token and zero tokens in diagnostics at any level, including a refused
  sign-in at debug (FR-002; 003 FR-009).
- **SC-005**: For a loaded batch at debug, every received message has an
  immutable identifier in the record, and a message the user moved to another
  folder and back keeps the identifier recorded before the move (US4;
  FR-004, FR-009).
- **SC-006**: The record of a refused request contains the status and the
  service's error code, and zero subjects, addresses, message texts or tokens
  (US4; FR-009).
- **SC-007**: A Microsoft 365 account, a Google account and a Generic IMAP
  account loaded in one run show their own batches; the other providers'
  behaviour under 002 SC-001–007 and 004 SC-001–007 is unchanged by this
  feature, and 004 SC-008 is amended so that Microsoft 365 accounts are
  refreshable (US1; FR-007).
- **SC-008**: Against a scripted service that refuses the request, a refresh
  fails with an explanation that states the service's status and error code,
  the service receives exactly one request for that load, and a later manual
  refresh succeeds (FR-008).
- **SC-009**: Refresh Inbox is available for a selected Microsoft 365 account,
  and accessibility established by F01 and the IMAP integration does not
  regress (FR-010).
- **SC-010**: A refresh of the maintainer's account, whose Inbox holds more
  than 100 messages, completes with text within the wait limit of the IMAP
  integration (FR-003).
- **SC-011**: Against a scripted service that answers with fewer than 100
  messages and offers a further page, the batch shows those messages and is
  reported as incomplete, and the service receives exactly one request
  (FR-003).

## Deferred to later features

These belong to Microsoft 365's domain and are described here once so that
later features amend this specification instead of adding a layered one. None
of them gets requirements, plan decisions or code in this feature.

| Microsoft 365 mechanism | What it will be for | Waits for |
|---|---|---|
| The folder hierarchy: folders found by their role and listed with their display names, child folders, hidden folders, item and unread counts | Folder navigation and combined Inbox | Folders, labels and combined Inbox |
| Change tracking: rounds of changes since a saved point, entries for removed messages as proof that a message left the folder, changed messages arriving with only their changed properties | "What changed since the last load" without refetching | Mail storage and synchronization; the probe facts in Assumptions are its evidence |
| Read state and flag changes through the service | Marking read and starring | Read and star |
| Moving, deleting and permanently deleting through the service; the Deleted Items and Archive roles | Archive, trash and permanent deletion | Moving and deleting |
| The body rendered as HTML, with inline pictures as attachments the body refers to | HTML reading | HTML reader |
| The attachment indication on a message, attachment listing and download | Saving and opening attachments | Attachments |
| Following the service's further pages when a list exceeds one answer | Loading more than one answer holds | Mail storage and synchronization |
| The wait the service asks for after a refusal because of its limits | Telling the user when to try again | Error presentation |
| The service's search | Remote search | Search |
| Change notifications the service pushes | Background freshness | Background and notifications |
| The conversation identifier and index | Grouping messages into conversations | Conversations |

## Assumptions

- The maintainer decided on 2026-09-23 (see Clarifications and the
  feature-start sizing): text comes from the service's body, not from MIME;
  immutable identifiers are asked for from the first load; the batch every
  provider delivers carries a per-provider identity kind; the service is
  reached through the platform's web library, which the plan names; the
  size budget agreed on 2026-09-23 is the limit on what this feature builds.
  After the specification challenge of 2026-09-23 the maintainer decided
  (see Clarifications): one request per load with the incomplete-list notice
  instead of a page loop; refusals shown with status and code, without the
  wait; no separate secure-connection failure kind; no request identifier;
  no attachment indication; nothing for a body that arrives in another form
  than asked for, which the service documents as not happening.
- The read-only probe of 2026-09-23 against the maintainer's personal
  Microsoft 365 account in GOA is accepted evidence for the plan: GOA exports
  the account's authorization through its OAuth interface and a Mail interface
  that carries the address but no server settings; the token GOA returns is
  accepted by the service; the newest 100 Inbox messages with their text arrive
  in one page within a few seconds, out of an Inbox of several thousand; the
  text rendering applies to the message list, and the answer's own field, not
  its confirmation header, says which form arrived; immutable identifiers
  differ from default ones and are accepted when one message is requested; an
  invalid token is refused with the documented status and code; change
  tracking with a date filter completes its first round in one page, an
  immediate second round is empty, a message moved to Deleted Items appears as
  removed under the same identifier, and a read-state change arrives with only
  the changed property and the identifier. The first request of a cold run
  once took several seconds, later ones well under a second. Details belong to
  the plan's research.
- The service's limits need no mechanism here: a load makes one request, the limits allow thousands per ten minutes
  per application identity and mailbox, and the application identity GOA uses
  is shared by every GNOME application on the machine, which is why another
  application can use up the allowance. A token lasts about an hour, GOA
  renews it when it is close to expiry, and a load lasts seconds; there is no
  token cache. After the user revokes access at Microsoft, GOA may still hand
  out its cached token until it expires; the refresh then fails at sign-in
  until GOA renews, and Mailbag does not ask GOA to re-check the account.
- Setting up the account belongs to Online Accounts: a personal Microsoft
  account needs nothing beyond signing in; an organization account needs the
  organization's identifier in the Online Accounts dialog; an organization can
  block the shared application identity, which fails in Online Accounts before
  Mailbag sees an account. A mailbox the service cannot serve, for example one
  kept on the organization's own server or an account without a mailbox,
  refuses with a reason and is shown as a service refusal; no requirement is
  added for it.
- Access to the account's authorization without server settings requires
  extending the shared
  [goa-adapter access contract](../002-imap-integration/contracts/goa-access.md)
  with a token-only request. Planning must propose that change for maintainer
  approval before implementation.
- FR-012 of the IMAP integration and FR-009 of the Gmail integration, which
  kept Microsoft 365 accounts ineligible for Refresh Inbox, are amended by
  this feature: Microsoft 365 accounts become eligible.
- Building Mailbag on the host needs the development package of the platform's
  web library, which the README's setup list gains; the installed
  application's runtime already contains the library.
- Acceptance uses the existing Fedora/GNOME environment, the installed
  application and the maintainer's personal Microsoft 365 account in GOA, with
  an HTML-only message, a message with attachments and a message the user
  moves to another folder and back during acceptance.
- Automatic polling, server push, reconnection, older history, persistent
  storage, previews, HTML, attachments, conversations, search, sending,
  shared mailboxes and remote changes are outside scope, as in the IMAP
  integration. Microsoft 365's mechanisms for them are listed under Deferred.
- The [constitution](../../.specify/memory/constitution.md) governs this
  feature. Implementation portions and review pauses are defined in the plan.
