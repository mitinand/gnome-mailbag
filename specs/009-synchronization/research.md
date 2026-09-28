# Research: Synchronization

Decisions that had alternatives or needed a check. Each names what it rests
on: a source line, a document, or a probe run at the feature-start or while
planning (2026-09-28). Probe results are stated without the accounts they
ran on.

## §1 One cycle, two parts: learning changes and storing portions

**Decision**: A cycle is written as the steps of spec FR-001: open the
folder, learn the server's changes, store them. Learning changes is one
function per way of learning them (the IMAP base method, the Microsoft 365
delta query); each delivers `FolderPortion`s ([contract](contracts/synchronization.md)).
One store operation, `Store::store_portion`, writes any portion in one
transaction and does not know which way produced it.

**Rationale**: two ways exist today, so the shared shape is not speculative;
CONDSTORE later adds a third way and one field of folder state (spec
FR-015(e)) without touching the store, the worker or the window. The
store has no provider rule: a Generic IMAP renumbering is expressed by
identities (§2), not by a reset kind of portion.

**Alternatives**: a store operation per kind of change (removals, read
state, arrivals), called by each provider: rejected, since every provider
would then own the order and the transaction boundaries (constitution IV); a
trait for the ways of learning changes: rejected, since the two ways run in
different provider sequences chosen once by provider (004 plan D1), and a
trait with one caller adds nothing.

## §2 IMAP: the listing and its proof

**Decision**: After EXAMINE, a cycle runs one `UID FETCH 1:* (UID FLAGS)`
(Gmail adds `X-GM-MSGID`) and keeps, per message, its UID, whether it is
`\Seen` and Gmail's identifier. The listing is complete when the command
ended with OK; a NO, a BAD or a lost connection makes it incomplete. When
EXAMINE reports no messages the listing is empty and complete without a
command, since servers answer `1:*` in an empty mailbox differently.
Removal needs a complete listing (spec FR-004); nothing else is compared.

**Identity**: a Generic IMAP message's identity is
`imap:<folder>/<UIDVALIDITY>/<UID>`, the numbering version taken from the
cycle's EXAMINE. After the server renumbers a folder, every stored identity
of the old version is absent from the listing and leaves with its proof,
and every message of the new version is an arrival: no old row, text or
open reader is ever attached to a new message, also when the window's
coalesced reads skip the moment between. No numbering version is stored
per folder, and no reset rule exists. (External review, 2026-09-29; it
replaces a stored `uid_validity` with a reset portion, which could match
the rows of a store written without a version to new messages.)

**Rationale**:
- RFC 3501 §7.4.1 lets a server send EXPUNGE during a UID command and leave
  the expunged message out of the answer (checked), so a complete listing
  that is shorter than EXISTS is still right. A count check would only
  delay correct removals (spec Clarifications).
- A message a non-conforming server leaves out wrongly returns at the next
  complete listing: arrivals are "listed but not stored", not "UID above the
  last one", so the error heals itself.
- The listing costs one short line per message: measured at under a second
  and well under a megabyte for real folders of several thousand messages
  on three providers, so 100 000 messages are a few megabytes and around
  ten seconds (inferred).
- The fork's FETCH stream reports a NO or BAD completion as its last item
  (async-imap `fix/fetch-completion-status`); `uid_search` does not (it
  stops at the tag and ignores its status, checked in `parse_ids`), so
  SEARCH is not used for proof.

**Alternatives**: `UID SEARCH ALL` for presence plus a flag FETCH: two
commands, and SEARCH's completion is not checked by the fork; ESEARCH is not
parsed by the fork. CONDSTORE: deferred (spec FR-015(e)).

**Streaming**: the listing is read from the command's stream and reduced to
a small record per message as it arrives, instead of collecting every
response first as today's `collect_fetches` does; a FETCH response without a
UID, which the fork also passes on for unsolicited flag updates, is skipped; 100 000 responses are then
about 2 MB in memory instead of tens of megabytes (inferred from the
response sizes).

## §3 IMAP: arrivals and their texts, a portion at a time

**Decision**: Arrivals are the listed messages the store lacks, taken
highest UID first in portions of 100. For each portion: `UID FETCH` of the
list fields (today's row items) by UID set; then, for the rows whose
INTERNALDATE lies within 30 days of the cycle's start, the part structures
and the text parts as today's batch reads them (002 FR-004); then the
portion is stored. Rows older than 30 days are stored with
`ReceivedContent::NotDownloaded`. A message missing from the row answer
disappeared and is skipped without failing (spec Edge Cases), and a group
of structures that all disappeared is skipped likewise. A NO that ends a
portion's row FETCH keeps the rows received, stores them, and ends the
cycle as an incomplete list (`IncompleteList::ServerRefused`) without the
completed state, as today's `MessageList.refusal` does, so a folder never
looks complete while the server withheld messages. The sequence-number
FETCH goes.

**Rationale**:
- Texts with their rows (spec FR-003): the newest messages are readable as
  soon as listed, and a stop leaves no stored message of the last 30 days
  without its text.
- 100 is today's batch, whose timing on real servers is known; one portion
  with texts took seconds on the slowest server measured (19 texts per
  second).
- Structures are never asked for a whole folder in one command: on one
  server `UID FETCH 1:* (BODYSTRUCTURE)` over a folder of several thousand
  messages took 15 minutes, while groups of 100 ran at 92 messages per second (measured).
- The portion size bounds the work a stop loses (spec FR-010), not the time
  quitting takes (§8).

**Alternatives**: every row first, texts afterwards: rejected by the spec
challenge (the newest message said "not downloaded" during a fill). Larger
portions for rows without text: kept as an optional mechanism, for a first
fill of a very large folder that proves slow.

## §4 Gmail: label folders by the same method

**Decision**: A Gmail label folder runs §2 and §3 with `X-GM-MSGID` in the
listing. A listed message's identity is `gmail:<X-GM-MSGID>`. Before a portion's
rows are fetched, the cycle asks the store which of the portion's
identities the account already holds (`Store::stored_identities`, one
query on the unique `(account, identity)` index); those messages are only
related to this folder with their listed read state, and nothing of them is
fetched again (spec FR-005: "messages the store lacks"). A row without `X-GM-MSGID` is
left out of the cycle and written to the record: Google documents the
attribute on every message, and a message is never guessed from its place.

**Rationale**: spec FR-006 and Clarifications; the listing's cost grows by
one number per line.

**Alternatives**: All Mail with labels: rejected in the spec.

## §5 Microsoft 365: the delta query, texts by date range, and resumption

**Decision**:
- A cycle reads `GET /me/mailFolders/{id}/messages/delta` with `$select` of
  the list fields and `isRead`, `$orderby=receivedDateTime desc` on a first
  reading, `Prefer: odata.maxpagesize=500` and `IdType="ImmutableId"`; it
  follows `@odata.nextLink` until `@odata.deltaLink`. Each page is a
  portion.
- An entry for a message the account also holds in another folder is not
  applied from the entry, which may be older than that folder's state (a
  message read in A, moved to B and marked unread there, then an old
  entry of A's round): the message is read with `GET /me/messages/{id}`
  including `parentFolderId`, its current fields and read state are
  stored, and it is related to this folder only if `parentFolderId` names
  it (external review, 2026-09-29). This happens only while a moved
  message is still listed in its old folder, so the extra requests are
  few.
- An entry with `@removed` removes the message from the folder. An entry
  that carries every selected field is a listed message: an arrival or a
  full update. Any other entry carries only what changed: its `isRead`, when
  present, sets the read state of a stored message; when it changed other
  selected fields, or names a message the store lacks, the message is read
  with `GET /me/messages/{id}` before it is stored (spec FR-007); a 404
  there means the message is gone meanwhile and it is left out. Entries of
  one page are merged per message in their order, so a later partial entry
  never drops an earlier `isRead`; a later page wins over an earlier one.
- Texts on a first fill: a page of the first reading is ordered by
  received date, so the folder holds no message between its earliest and
  latest dates that the page lacks; its messages within 30 days get their
  texts in one request
  `GET /me/mailFolders/{id}/messages?$filter=receivedDateTime ge A and receivedDateTime le B&$select=id,body&$top=500`
  with `Prefer: outlook.body-content-type="text"` (the list's default page
  is 10 messages), A and B being those messages' earliest and latest
  dates; messages with an equal date outside the page are ignored.
- Texts in a round of changes: its arrived messages are scattered in time
  (a message moved in from a month ago beside today's), so each text is
  read by the message's identifier; a date range could cover a month of
  mail for two messages (external review, 2026-09-29).
- Position: the `@odata.deltaLink` is saved with the portion that completes
  the cycle. During the first fill of a folder never refreshed, each page's
  portion saves its `@odata.nextLink` as the place to continue (spec
  FR-008, FR-010).
- A 410, or a 4xx whose `error.code` is `syncStateNotFound` (compared
  without case), means the saved position or place is no longer accepted;
  the cycle then reads the whole folder, keeps the listed identities in
  memory, and at its end removes the stored messages it did not list; if it
  stops, the next cycle starts it again (spec FR-007, FR-010).

**Rationale**:
- The delta query returns created, updated and removed items, updates may
  carry only the changed property, and replays and reordering are allowed
  (Graph "delta query overview", "Replays"; checked). An update carrying
  only `isRead` was observed.
- Pages are capped at 512 items whatever `maxpagesize` asks (measured); a
  first reading of a folder of a few thousand messages took 20 to 114
  seconds and single pages
  up to 15 seconds (measured), so resuming a first fill is worth one stored
  link.
- Only `receivedDateTime desc` is supported as an order (checked).
- Resources deleted before a reading began are not returned by it (checked),
  so only a completed full reading proves which stored messages are gone;
  hence a re-reading runs within one cycle.
- A date range with `body` as text returned the range's messages with text
  bodies in one request (probe, 2026-09-28); fetching a text per message
  would take one request each, and the list query has no documented filter
  by identifier.
- "Sync from now" exists only for Microsoft Entra resources (checked), so a
  folder's first position always costs a full reading.

**Checked while planning**: the service accepted a saved `@odata.nextLink`
after a pause of 15 minutes and of 45 minutes, answering with the next 500
messages in 29.3 s and 3.3 s (probe, 2026-09-28). A place the service
rejects later is handled as a rejected position (full reading, spec
FR-007).

**A continued first fill reads one more round**: after the last page of
a first fill continued from a saved place, the cycle reads the round of
changes after it before it completes (one request when nothing changed),
so changes made during the pause are included as far as the service
reports them (spec FR-001, FR-007). The service documents that changes can
reach delta answers with a delay ("replication delays", checked), so no
cycle promises more than the service reports.

**Open check before the Microsoft 365 portion**: whether a change made
during a paused first fill is reported at all is not documented; a probe
with the maintainer changes a message in another client while the probe
waits (a change that stays: a message marked read and left read, and a
message moved to another folder; a change undone before the probe resumes
would prove nothing), then finishes the reading and reads the next round.
If the change is never reported, a continued first fill ends with a full
re-reading instead.

**Alternatives**: `$select=body` on the delta query: downloads every
message's text, against spec FR-009; JSON batching of per-message requests:
more code for the same result as a date range.

## §6 The store: folder state, portions, reads without text

**Decision** ([data-model.md](data-model.md)):
- `folder` gains `server_position`; `loaded` becomes `synchronized` ("the
  folder's latest cycle completed"). No numbering version is stored (§2).
  The first portion of a cycle that has messages to fetch sets
  `synchronized` to 0, so a stopped cycle that left no row is shown as "no
  mail loaded", never as an empty folder.
- `membership` loses `position`: rows are ordered by the message's received
  date, newest first, then by the stored row's id. No index is added: the
  read starts from the folder's memberships and sorts them (to be confirmed
  with `EXPLAIN QUERY PLAN` in portion 2).
- The content code `not_downloaded` joins the schema's `CHECK`.
- `Store::read_folder_sync(folder)` returns the folder state and the stored
  identities with their read state, once per cycle.
- `Store::store_portion(folder, portion, cancelled)` writes one portion in
  one transaction under the store's lock, after the cancellation check that
  loads use today: removals, orphaned messages, read states, arrivals (an
  arrival never replaces a stored text with `NotDownloaded`), memberships,
  and the folder state when the portion carries one.
- `Store::read_folder_rows(folder)` returns the list fields and read state
  of the folder's messages without their text; `None` when no cycle
  completed and no row is stored ("no mail loaded").
- `Store::read_message_content(account, identity)` returns one message's
  content, read when the message is opened.
- `replace_mailbox`, `read_mailbox` and the load order go.

**Rationale**: reading 100 000 list rows took about 0.1 s and one text 29 µs
(measured at the feature-start and for 007), so the list never carries text
and the reader reads one message on opening. The store already serializes
writers behind one lock, so parallel cycles later (spec FR-002(d)) write
portion after portion without change. The existing `load_cancelled` check
under the lock keeps 007 FR-007 and FR-008 for every portion.

**Alternatives**: a stored "listed in this reading" mark per relation, to
resume Microsoft 365 re-readings: rejected with the spec's rule that
re-readings restart.

## §7 The worker reports portions; the window re-reads the shown folder

**Decision**: The mail worker's outcome channel carries
`LoadEvent::PortionStored` any number of times before the final
`LoadEvent::Finished(LoadResult)`. Today it holds one message and the result
is sent with `try_send`; it becomes unbounded and the window receives in a
loop, so the final result is never dropped behind an unread portion event.
The window, on a portion of any folder of the shown folder's account (a
Gmail label and a moved Microsoft 365 message share messages across
folders, as the completed load already rules today), reads the shown
folder's rows again while the rows and the banner on screen stay as they are (a "read
due" mark, not the state of a first read, which hides the banner); a
portion that arrives while a read runs marks one more read when it ends, so
reads never pile up and no timer is needed.

**Rationale**: FR-003 and FR-013; the numbered-read pattern exists for
stored mailboxes (007, 008).

**Alternatives**: the worker passes the portion to the window: two sources
of rows, against 007 FR-001 ("the window shows stored mail only").

## §8 Quitting within a second

**Decision**: Unchanged mechanism, now a requirement (spec FR-010): closing
the window drops the running load's handle, the worker drops the cycle's
future, which closes its connection, and nothing joins the worker thread
(`main.rs`, `connect_destroy`). A portion being written finishes or rolls
back with the process (007 FR-010). A test starts a cycle against a
scripted server that stops answering, cancels it and checks that the
cancellation is reported within a second; the installed build is checked by
hand (quickstart).

**Rationale**: the store's transaction is the only work that cannot stop at
once; a portion's write took milliseconds (measured for 100 000 rows in
portions of 500: 0.8 s in all).

## §9 The list widget

**Decision**: The messages list becomes a `GtkListView` over a
`GtkSingleSelection`, as Workbench's "List View" demo builds it: a
`GtkBuilderListItemFactory` whose template is the row form. `message-row.ui`
becomes a `GtkListItem` template; its labels and the unread dot bind to the
properties of a row object, `MessageItem` (identity, sender, subject, date
text, unread), which the window creates from the stored rows. The model is
updated by the difference between the shown and the stored rows, compared
by identity: a changed read state is set on the listed object in place;
arrived and removed messages change the list in one splice between the
common beginning and end; the selected row and the open message are two
identities kept apart (a row selected with the arrow keys need not be the
open one), and each is found again by its own after a splice (spec
FR-013).

**Rationale**:
- A list box builds every row: about 1 s per 1 000 rows (measured for 007);
  a list view builds only visible rows, and a model of 100 000 items was
  built in 0.2 s (probe).
- A list view reuses row widgets while scrolling, so a row must be bound to
  its item rather than filled once. The demo's template with bindings keeps
  the whole row in the form, as AGENTS.md asks, and the code only creates
  row objects.
- Properties make a read-state change an update of one object, with no
  splice; splices remain for arrivals and removals, which during a first
  fill happen at the list's end and later at its top, so the common
  beginning or end covers almost the whole list.

**Alternatives**: a signal factory that builds `message-row.ui` per row and
fills it in code, as the list box does today: with reused rows it needs a
row widget class or data attached to widgets, so it is not simpler;
rebuilding the model on each read: 100 000 items per portion, and the
selection lost.

**Unknown until portion 1**: whether Cambalache edits a list item template
with property bindings; if not, that form is edited as text.

## §10 `MailboxChanged` stays for one case

**Decision**: The failure kind and its wording stay. Of its four producers
today, three go: rows of a sequence-number FETCH that all vanished
(`reader.rs`, `fetch_rows`), structures that all vanished
(`fetch_structures`) and a batch whose messages all vanished
(`imap_batch.rs`, `load_batch_from_rows`); a vanished message is a missing
answer to a UID FETCH, which the cycle skips. One producer remains: after a structure the
parser cannot read, the reader reconnects, and if the folder's UIDVALIDITY
changed meanwhile the numbers the cycle holds name other messages, so the
cycle stops with `MailboxChanged` (`reader.rs`, `reconnect`). The next
cycle's identities carry the new numbering version (spec FR-005).

**Rationale**: storing a message's fields under an identity of the old
numbering would attach them to the wrong message. The earlier draft of this
section said the kind had no producer; the plan challenge found the
reconnect.

## §11 Microsoft Graph's wait limit

**Decision**: The wait per request rises from 30 to 60 seconds (the
maintainer, 2026-09-28). A second saved place, after 45 minutes, answered in
3.3 seconds, so the spread between requests is wide.

**Rationale**: pages of 15 seconds were measured, and a page read from a
saved place after a 15-minute pause took 29.3 seconds (probe, 2026-09-28),
at the edge of today's limit. A request that exceeds the limit stops the
cycle, and the stored portions and the saved place make the next Refresh
continue (FR-010), so the cost of a too short limit is a failure banner and
a second Refresh; the cost of a longer one is a real outage noticed after
60 seconds instead of 30.

## §12 Considered and not handled

- Messages flagged `\Deleted` but not expunged are listed like others; how
  they are shown belongs to moving and deleting.
- A server that answers `UID FETCH 1:*` in an empty mailbox with an error
  is avoided by §2's EXISTS rule rather than handled.
- The unsolicited-response channel of the fork drops responses beyond 100;
  the cycle reads no unsolicited response.

## §13 Renewing access during a cycle

**Decision**: When a request of a running cycle on an OAuth account meets
a refused access after the cycle's first successful request (a 401 from
Microsoft Graph) or the end of its session by the server (Gmail's BYE),
the cycle asks for the account's access once more. When Online Accounts
hands out a different token it makes one attempt to continue: on
Microsoft 365 it repeats the request with the new token; on Gmail it drops
the reader, opens the folder again with the new access through
`MailboxReader::open`, compares `uid_validity()` with the one it holds (a
change is `MailboxChanged`), and repeats the interrupted request (the
listing or the portion). A different token does not prove the cause:
Online Accounts renews a token that has less than ten minutes left,
whatever ended the session, so a BYE Gmail sent for its limits can,
rarely, be followed by one reconnect; 004's amendment says so. With the
same token, and after a second refusal, the refusal stands with its real
reason: a refused sign-in on Microsoft 365, Gmail's own text on Gmail. A
password account is not renewed: a lost connection ends the cycle as
today, and the next Refresh continues.

**What `mailbag-imap` must tell**: today `command_failure` turns NO, BAD
and BYE into one failure (`session.rs`); the cycle needs to know that the
server ended the session, so `ImapError` carries that the session ended
with BYE (a small addition in `mailbag-imap`; external review,
2026-09-29).

**How the worker asks**: the mail worker cannot call Online Accounts, whose
adapter belongs to GTK's context. `MailLoader` gives each load a renewal
channel: the cycle sends a request with a reply channel, a task on GTK's
context asks the adapter with the same request the load started with, and
sends the new access back. A load cancelled meanwhile drops the request.
Apart from the BYE mark, the renewal adds nothing to `mailbag-imap`,
`mailbag-graph` or the window.

**Rationale**:
- Online Accounts has no signal for a renewed or expired token: its
  `OAuth2Based` interface offers only `GetAccessToken`, returning the token
  and its lifetime, and the account interface has no signals (checked by
  introspecting the running service, 2026-09-28).
- It renews a token only when asked with less than ten minutes left (GOA
  source, `goaoauth2provider.c`), so a cycle that started with a token
  eleven minutes from expiry and runs for fifteen meets a refusal.
- A first fill of a large Microsoft 365 folder takes tens of minutes
  (§5), and Gmail closes an OAuth session when its token expires (004
  research, secondary sources).
- Answering a refusal covers expiry, revocation and a wrong clock with one
  mechanism; predicting expiry from the lifetime would still need it.

**Alternatives**: renewing before expiry from the returned lifetime: the
adapter discards the lifetime today, and a refusal would still need
handling; telling Gmail's expiry from its limits by the BYE's text: Google
documents no wording; a failure shown and the next Refresh continuing:
rejected by the maintainer, since the user would see a false sign-in
failure.
