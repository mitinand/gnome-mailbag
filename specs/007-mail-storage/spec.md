# Feature Specification: Mail Storage

**Feature**: `007-mail-storage`
**Created**: 2026-09-25
**Status**: Implemented and accepted on the installed build on 2026-09-26
([plan](plan.md), Post-implementation). Challenged 2026-09-25; FR-013
aligned on 2026-09-26 with 006 as corrected that day. The decisions taken at sizing and at the
specification challenge are recorded under Clarifications.
**Input**: Every piece of mail the window shows comes from a local store and
from nowhere else. A load writes what it received into the store; the window
reads the store. Today each account's received batch lives in memory for one
run: a restart or a failed refresh leaves the list empty, and nothing can be
read without the server. With the store, stored mail is there at the next
start and without a network, a failed refresh leaves the mail on screen, and
every later capability (read and star, folders, conversations, search)
changes or reads one place instead of a batch in the window.

**Scope**: This is the complete storage specification for Mailbag, written
for the target application with many accounts, several folders per account
and local changes. It owns what is stored, where, how it stays whole and
private, how it follows Online Accounts, and how the window reads it. It does
not own how a folder is synchronized with its server: what a load fetches
stays as each provider feature defines it (the newest 100 Inbox messages), and
a completed load replaces what the store holds for that folder. Whatever waits
for a layer that does not exist yet is marked deferred in FR-014 and gets no
plan decisions, tasks or code until that layer exists.

## User Scenarios & Testing

### User Story 1 — Stored mail is there without the server (Priority: P1)

The user has refreshed an account's Inbox at some point. Later Mailbag starts
while the network is down, or the mail server is unreachable. Selecting the
account shows the messages of its last completed load, in the same order,
with the same read state, and opening one shows the same text, or the same
reason why there is none, as before. Nothing is fetched to show them.
**Independent Test**: load an Inbox from a scripted server, stop the server,
start a new window over the same store, select the account; compare the rows
and each message's reader content with those before the restart, and check
that no connection was attempted.

**Acceptance Scenarios**:

1. **Given** an account's Inbox was loaded before a restart, **when** the user
   selects the account with the server unreachable, **then** its stored
   messages are shown and no load starts; Refresh Inbox stays the only thing
   that loads.
2. **Given** a stored message whose text was received, **when** the user opens
   it, **then** the reader shows that text in full, as after the load.
3. **Given** a stored message whose content was explained (only HTML,
   encrypted, unknown character set, text not returned), **when** the user
   opens it, **then** the reader shows the same status page as after the load
   (006 FR-006).
4. **Given** an account whose Inbox was never loaded, **when** the user selects
   it, **then** the list says that no mail is loaded, never that the Inbox is
   empty.

### User Story 2 — A refresh replaces the stored Inbox (Priority: P1)

The account's stored messages are on screen and the user refreshes. The rows
stay while the load runs, with the sidebar's spinner. When the load completes,
the stored Inbox becomes exactly what the load delivered: a message the server
no longer lists among the newest is gone, a new one is there, read states are
the server's. The list shows the new rows and the reader closes.
**Independent Test**: two scripted loads of the same account with different
messages and read states; check the rows during the second load, after it,
and after a restart.

**Acceptance Scenarios**:

1. **Given** stored rows are on screen, **when** the user refreshes, **then**
   the rows stay and the spinner runs until the load ends.
2. **Given** the second load lists a new message and no longer lists an old
   one, **when** it completes, **then** the list holds the new message and not
   the old one, the reader is closed, and a restart shows the same list.
3. **Given** the server refused to finish the message list, **when** the load
   ends, **then** the rows that arrived replace the stored Inbox and the
   incomplete-list banner is shown (006 US3); after a restart the same rows
   are shown without the banner (Clarifications).
4. **Given** a load of another account runs, **when** the user selects this
   account, **then** this account's stored rows are shown, and the other
   account's result is stored under the other account only.

### User Story 3 — A failed refresh keeps the stored mail (Priority: P1)

This is 006 User Story 2, built here. The account's stored messages are on
screen and a refresh fails: the server is unreachable, it rejects the sign-in,
Online Accounts gives no credential, or the store cannot take the result. The
messages stay, and the banner above them names the failure; its button opens
the failure dialog. The store is unchanged. An account with nothing stored
shows the failure page instead, as 006 User Story 1 describes.
**Independent Test**: a stored Inbox and a scripted server that rejects the
sign-in; check the rows, the banner and its dialog, the stored content after a
restart, then a successful refresh.

**Acceptance Scenarios**:

1. **Given** stored rows are on screen and the server rejects the sign-in,
   **when** the refresh ends, **then** the rows stay, the banner says the
   sign-in was rejected, and its dialog offers Online Accounts.
2. **Given** the banner is shown, **when** a later refresh completes, **then**
   the banner is gone and the rows are the fresh ones.
3. **Given** the banner is shown, **when** the user selects another account and
   comes back, **then** the banner is there again with the same rows.
4. **Given** nothing is stored for the account, **when** its refresh fails,
   **then** the failure page takes the list's place (006 FR-006).
5. **Given** a refresh fails or is cancelled, **when** Mailbag restarts,
   **then** the stored rows are those of the last completed load.

### User Story 4 — An account's mail leaves with the account (Priority: P2)

The user removes an account in Online Accounts or turns its Mail off. Its
stored mail is deleted, whether Mailbag was running at the time or not. An
Online Accounts service that restarts or fails to answer deletes nothing: only
a complete answer proves that an account is gone
([001](../001-goa-account-observation/spec.md) FR-009).
**Independent Test**: a store holding two accounts and scripted Online
Accounts answers: a complete list without one of them, a failed read, a
service restart; check which account's mail remains after each.

**Acceptance Scenarios**:

1. **Given** Mailbag runs and a complete Online Accounts answer no longer lists
   an account, or lists it with Mail off, **then** that account's stored mail
   is deleted, and a load still running for it stores nothing.
2. **Given** an account was removed while Mailbag was not running, **when**
   the first complete answer after the start arrives, **then** its stored mail
   is deleted.
3. **Given** the Online Accounts service disappears or its account list cannot
   be read, **then** no stored mail is deleted.

### User Story 5 — A store that cannot be used (Priority: P3)

Rarely the store cannot do its job: the disk is full when a load's result is
written, the file was damaged, or a newer or older build wrote it with a
different structure. While Mailbag is unreleased the store holds only what the
servers can give again, so a store that cannot be used at start is discarded
and the next refresh fills it; a failure after the start is the failure of the
operation that met it, shown under 006.
**Independent Test**: a write that fails, a store file that is not a store, a
damaged store and a store written with a different structure; check what the
window shows and what the record says.

**Acceptance Scenarios**:

1. **Given** writing a load's result fails, **when** the load ends, **then** it
   is a failed load: the banner over the stored rows, or the failure page when
   nothing is stored, and the stored rows are unchanged.
2. **Given** the store was written with a different structure, is not a store
   or is damaged, **when** Mailbag starts, **then** the store starts empty, one
   record line says it was discarded and why, and every account shows that no
   mail is loaded until refreshed.

### Edge Cases

| Situation | Required visible result | Basis |
|---|---|---|
| Mailbag quits, crashes or loses power while a load's result is written | The next start shows the Inbox of some completed load, never part of one; it may be the previous load | FR-010 |
| A completed load finds the Inbox empty | "Inbox is empty", also after a restart | FR-006 |
| Every listed message disappeared during the load | A failed load (002); the store is unchanged | 002 FR-009, FR-004 |
| An account is removed while its load runs | The load is cancelled and stores nothing; none of the account's mail remains | 002 FR-008, FR-007, FR-008 |
| A refresh is incomplete and then a later one fails | The banner names the failure: it describes the latest refresh only | FR-005 |

## Clarifications

### Session 2026-09-25 (sizing)

- Q: Does "everything the window shows comes from the store" include the
  account list and failures? → A: No. Mail comes from the store: rows, reader
  content, read state, and later folders and counts. The account list comes
  from Online Accounts, the only account authority (001); the store keeps its
  mail under each account's Online Accounts ID and nothing else about the
  account. Load state and failures live in memory (006 FR-007).
- Q: How much of an Inbox is stored? → A: What a load delivers today, the
  newest 100 messages. Fetching a whole folder, filling it the first time and
  finding what changed on the server belong to the synchronization feature,
  not to storage (FR-014).
- Q: How does a load change the store? → A: It replaces the folder's stored
  messages as a whole, after the load completed, in one step. No comparison
  with what was stored.
- Q: What does the window show during a refresh? → A: The stored rows and the
  spinner. The reader closes when the rows are replaced; keeping the open
  message is not built. The rows stay so that 006 User Story 2 holds; clearing
  them at the start of a refresh would leave nothing for its banner.
- Q: Is an incomplete list remembered across a restart? → A: No. It is stored
  as received; after a restart it looks complete until the next refresh. A
  stored flag was weighed and not added: the case is rare and the next refresh
  replaces it.
- Q: What happens when the store's structure changes? → A: Until the first
  release, the store is discarded at start and filled again by refreshing; no
  conversion (FR-012). The structure's version is derived from its
  definition, so a change needs no manual step. A Reset control was proposed
  and not built: nothing needs it while every refresh replaces the stored
  Inbox and a changed structure resets itself.
- Q: Which folders will Gmail synchronize? → A: When folders arrive, Gmail's
  messages come from All Mail, plus Trash and Spam, which All Mail leaves
  out; labels are memberships, not folders to download again. Evidence:
  inside a label's folder `X-GM-LABELS` leaves that label out, while All Mail
  lists every label ([004 research §5](../004-gmail-integration/research.md)).
  Built now: the Inbox only, addressed as today.
- Q: Are thread identifiers captured now? → A: No; they wait for
  conversations. Since every refresh replaces the stored Inbox, the next
  refresh after that feature fills them.

### Session 2026-09-25 (specification challenge)

- Q: Is the folder, membership and identity model built now? → A: No. It
  stays this specification's target model (FR-003) and is built with the
  first feature that reads it: folders and labels for membership in several
  folders, read and star for addressing a message on its server. Today every
  stored message belongs to one folder, its account's Inbox, and nothing
  reads a message's identity except one record line. The decision of
  2026-09-24 put the model into this feature before a changed structure could
  simply be discarded (FR-012); with that rule, building it later costs one
  refresh.
- Q: Is a damaged store kept for the user to delete? → A: No. Until the first
  release the store holds only what the servers give again, so a store that
  is not a store, is damaged or has another structure is discarded at start
  like a changed structure. The file is checked for damage at start, because
  damage inside the file shows only when that part is read and would
  otherwise fail every later load. The rule "never delete a store
  automatically" returns with unsent local changes (FR-014(c), (g)).
- Q: Does the store keep its own list of accounts? → A: The specification
  states the rule the user sees (FR-008); how a late result is kept out is
  the plan's choice.
- Applied without a question: Online Accounts has no partial answer (an
  answer is accepted whole or is one failure, 001 FR-008), so "an incomplete
  answer" is not a separate case; the record rules of 003 and the panic and
  cancellation rules of 006 are referenced, not repeated; the half-second
  number and the check of the store file for credentials were dropped (no
  source for the number; no stored value can hold a credential); a refresh
  with nothing stored shows that the Inbox is loading; a deletion that fails
  is repeated at the next complete answer. A separate failure for one
  message's stored content was written and then dropped at planning: the
  window reads a message's content together with the list, so it cannot fail
  on its own.

### Session 2026-09-26 (plan challenge)

- Q: A failed read of the stored Inbox is on screen and the user refreshes;
  which failure does the list show? → A: The refresh's: a refresh forgets the
  failed read, so the list says that the Inbox is loading and then shows the
  refresh's own outcome, the newest failure (FR-013).

## Requirements

### Functional Requirements

**One source**

- **FR-001 — The window shows stored mail only**: Every message row, every
  reader content and every read state the window shows MUST be read from the
  store. A load's result MUST reach the window only through the store: the
  load writes, then the window reads. The account list keeps coming from
  Online Accounts (001); a load's state and its failure stay in memory and are
  never stored (006 FR-007).
- **FR-002 — What is stored**: For each account, identified by its Online
  Accounts ID, the store holds its Inbox as the latest completed load left it.
  For each message: its provider identity (FR-003), the list fields (subject,
  sender, recipients, received date), its read state as the server last
  reported it, and its reader content, which is either the received text in
  full or the reason the reader shows none (002 FR-004, 006 FR-006). The store
  holds no password, token or other credential, no server reply, no failure,
  no load state and no account name or address.

**Data model**

- **FR-003 — Folders, identity and membership**: Built now: each account has
  at most one stored folder, its Inbox, and every stored message belongs to
  it. A message keeps the identity its load reports (an IMAP UID, Gmail's
  message identifier, Microsoft 365's immutable identifier) as a value for the
  record; nothing addresses a stored message by it.
  The target model, deferred with the features that first read it (FR-014(b),
  (c)): a folder is the unit of synchronization, belongs to one account, and
  carries its provider identity (an IMAP mailbox name, a Gmail label's folder,
  a Microsoft 365 folder) and its own state; for IMAP that state includes
  UIDVALIDITY. A message has the identity its provider defines: Gmail's
  message identifier ([004](../004-gmail-integration/spec.md) FR-004),
  Microsoft 365's immutable identifier
  ([005](../005-microsoft-graph-integration/spec.md) FR-004); a Generic IMAP
  message has none beyond its place in a folder. Membership is a relation
  between a message and a folder, not a field of the message: it carries the
  IMAP UID, which is valid only with the folder's UIDVALIDITY, and a message
  may belong to several folders. No stored message is ever addressed by a UID
  of another UIDVALIDITY.

**Loads and the window**

- **FR-004 — A completed load replaces the folder**: When a load completes, the
  folder's stored messages MUST become exactly the load's messages, in one
  step: no reader of the store ever sees part of the old and part of the new
  Inbox. A failed or cancelled load MUST leave the store unchanged. A load
  whose list the server did not finish, or whose service offered more than one
  request holds, completes with the messages it received (006 US3).
- **FR-005 — The window during and after a refresh**: While a refresh runs,
  the stored rows stay and the sidebar's spinner runs; with nothing stored the
  list says that the Inbox is loading, as today. When a load completes the
  window reads the store again: the list shows the stored rows and the reader
  closes. A failed load leaves the store unchanged (FR-004): stored rows stay
  with the banner that names the failure (006 FR-013(a)); with nothing stored
  the failure page takes the list's place (006 FR-006). The banner describes
  the latest refresh of the account and nothing older: its failure, or its
  incomplete list, or nothing; two list notices are never shown at once.
- **FR-006 — Empty is proven, not assumed**: The list says the Inbox is empty
  only when a completed load stored an empty Inbox for the account; with no
  completed load stored it says that no mail is loaded. This holds across
  restarts (006 FR-002).
- **FR-007 — The right account**: A load's result MUST be stored only under
  the account it was started for, never for an account whose mail was deleted
  (FR-008). The rows on screen MUST always be the selected account's; a result
  that arrives for another account changes nothing on screen (002 FR-007).

**Accounts**

- **FR-008 — Mail leaves with its account**: When a complete Online Accounts
  answer (001 FR-009) no longer lists an account, or lists it with Mail off,
  that account's stored mail MUST be deleted and MUST NOT be written again
  until a complete answer lists it with Mail on. An account removed while
  Mailbag was not running loses its mail at the first complete answer after
  the start. A failed read and a service that disappears or restarts MUST NOT
  delete anything (001 FR-010). A deletion that fails is written to the record
  and happens at the next complete answer.

**The store itself**

- **FR-009 — Private and local**: The store lives in Mailbag's private data
  directory, which only the user can open. It holds no credential (FR-002),
  and what it holds enters the record only as 003 permits for received mail.
  It is not encrypted by Mailbag; the platform's disk encryption covers it.
  Deleting mail from the store is not a secure erase.
- **FR-010 — Whole after any interruption**: After a quit, a crash or a power
  loss, the store holds the state after some completed load, possibly not the
  latest one, and never part of a load. A load acknowledged as stored survives
  a quit and a crash; after a power loss it may be missing (constitution III:
  the next refresh obtains it again).
- **FR-011 — Never in the window's way**: Reading and writing the store MUST
  NOT block the window (constitution V). Selecting an account shows its
  stored rows without a visible wait, and the window stays responsive while a
  load's result is written.
- **FR-012 — A store that cannot be used at start, before the first release**:
  When the store was written by a build whose store structure differs, is not
  a store, or is damaged, Mailbag MUST discard it at start, begin with an
  empty store, and write one record line saying why. Stored data is never
  converted before the first release; upgrading a populated store waits for
  release readiness (FR-014(g)). Once local changes exist (read and star),
  discarding also drops changes not yet sent to the server; this is accepted
  until the first release.
- **FR-013 — The store's failures after the start**: A failure after the start
  is the failure of the operation that met it, declared under 006 (FR-001,
  FR-012). A result that cannot be written makes the load fail (FR-004),
  shown like any failed load. A list that cannot be read for the selected
  account leaves nothing to show, so the failure page takes the list's place,
  as for a failed load; its Retry reads the stored list again, since reading
  is the operation that failed (006 FR-003). Panics and cancellation follow
  006 FR-014 and FR-010.

**Deferred**

- **FR-014 — Deferred, with the layer each waits for**:
  (a) *Synchronization*: fetching a whole folder, its first fill in portions,
  and finding what changed on the server (IMAP CONDSTORE/QRESYNC or UID
  ranges, Gmail's modification sequences and label changes, Microsoft Graph's
  delta queries), with the folder state they need (the last UID, the highest
  modification sequence, the delta link); a list that shows a whole folder.
  Until then a load delivers the newest 100 and replaces the stored folder.
  (b) *Folders and labels*: other folders, their discovery, roles, counts,
  navigation and the combined Inbox; the folder with its provider identity
  and membership as a relation (FR-003); for Gmail, All Mail plus Trash and
  Spam as the synchronized folders and labels as memberships
  (Clarifications).
  (c) *Read and star*: a message addressed on its server by its identity and,
  for IMAP, by its UID with the folder's UIDVALIDITY (FR-003); local changes,
  their durability before the server confirms them, and a store changed by
  something other than a load, which the window then learns of without a
  load's completion. Until the first release a discarded store may still
  lose unsent changes (FR-012); consent comes with release readiness (g).
  (d) *Conversations*: Message-ID, References, In-Reply-To, Gmail's thread
  identifier and Microsoft 365's conversation identifier.
  (e) *Content cache*: HTML, inline resources, attachments, previews, and
  keeping content beyond what the latest load delivered.
  (f) *Background synchronization*: loads the user did not start.
  (g) *Release readiness*: upgrading a populated store instead of discarding
  it (FR-012), with the user's consent when it holds unsent changes.

## Success Criteria

### Measurable Outcomes

- **SC-001**: After a completed load of each provider's scripted server and a
  restart with the server stopped, selecting the account shows the same rows
  in the same order with the same read states, every message opens with the
  same text or status page, and no load starts (US1; FR-001, FR-002).
- **SC-002**: After two scripted loads with different messages, the stored
  Inbox equals the second load's messages exactly, before and after a restart;
  no row of one account is ever shown for another (US2; FR-004, FR-007).
- **SC-003**: A load that fails at each step, a cancelled load and a load whose
  result cannot be written each leave the stored rows exactly as before. A
  failed load, the unwritable result included, shows the banner over stored
  rows or the failure page without them; a cancelled load shows nothing
  (006 FR-010) (US3, US5; FR-004, FR-005, FR-013).
- **SC-004**: A complete Online Accounts answer without an account, or with its
  Mail off, leaves none of its stored mail, and a load of that account that
  ends afterwards stores nothing; a failed read and a service restart delete
  nothing; an account removed before the start loses its mail at the first
  complete answer (US4; FR-007, FR-008).
- **SC-005**: A store written with a different structure, a file that is not a
  store and a damaged store each start empty with one record line naming the
  reason (US5; FR-012).
- **SC-006**: A write interrupted by a failure leaves the previous Inbox
  complete (FR-010).
- **SC-007**: Review finds no store access on the window's thread, and on the
  installed build selecting an account with 100 stored messages shows them
  with no visible wait while the window keeps redrawing during a load's write
  (FR-011).
- **SC-008**: The record of a load, a restart and a deletion holds none of 003
  SC-002's private markers (FR-009).

## Assumptions

- The load sequences of 002, 004 and 005 stay as they are: what they fetch,
  their limits, their failures. Only what happens to their result changes.
- Amendments to closed features: 002 FR-006's stage rule (mail in memory for
  the run, no application mail files, a new run obtains mail again) and
  SC-004's stage part are superseded; its password rule stays. 002 FR-008's
  "MAY discard received mail when the account service fails" and FR-009's "a
  failed refresh leaves the list empty" are superseded by FR-005 and FR-008.
  002's Clarification "What does a refresh keep? Nothing" is superseded by
  FR-005. 004 and 005 refer to 002 FR-006 and follow the same change. 006 User
  Story 2 and FR-013(a) are built here, and the store adds its failure kinds
  under 006. 001 is unchanged: its complete answer drives FR-008.
- No layout change: the banner, the failure page, the empty-state page, the
  reader's status page and the sidebar spinner exist.
- The size budget approved at sizing and raised on 2026-09-26: at most 650
  net new production lines,
  about 450 test lines, one new crate and one new dependency, no thread or
  timer of Mailbag's own. The plan's size table checks it.
- Out of scope: a Reset control, a stored "incomplete" flag, keeping the open
  message across a refresh, a refresh at start, storing accounts' names or
  failures, and everything FR-014 defers.
- The [constitution](../../.specify/memory/constitution.md) governs this
  feature.
