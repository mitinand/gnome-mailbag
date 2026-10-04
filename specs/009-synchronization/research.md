# Research: Synchronization

Decisions that had alternatives or needed a check. Each names what it rests
on: a source line, a document, or a probe run at the feature-start or while
planning (2026-09-28). Probe results are stated without the accounts they
ran on.

## §1 One cycle, two parts: learning changes and storing batches

**Decision**: A cycle is written as the steps of spec FR-001: open the
folder, learn the server's changes, store them. Learning changes is one
function per way of learning them (the IMAP base method, the Microsoft 365
delta query); each delivers `FolderBatch`s ([contract](contracts/synchronization.md)).
One store operation, `Store::store_batch`, writes any batch in one
transaction and does not know which way produced it.

**Rationale**: two ways exist today, so the shared shape is not speculative;
CONDSTORE later adds a third way and one field of folder state (spec
FR-015(e)) without touching the store, the worker or the window. The
store has no provider rule: a Generic IMAP renumbering is expressed by
identities (§2), not by a reset kind of batch.

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
replaces a stored `uid_validity` with a reset batch, which could match
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

## §3 IMAP: arrivals and their texts, a batch at a time

**Decision**: Arrivals are the listed messages the store lacks, taken
highest UID first in batches of 100. For each batch: one `UID FETCH` of the
list fields (today's row items) and the part structures by UID set
(*amended 2026-10-02, §14: the structures were a second command*); then,
for the rows whose INTERNALDATE lies within 30 days of the cycle's start,
the text parts as the newest-100 load read them (002 FR-004), and the
preview pieces of the others (010 research §3); then the batch is stored. Rows older than 30 days are stored with
`ReceivedContent::NotDownloaded`. A message missing from the row answer
disappeared and is skipped without failing (spec Edge Cases), and a group
of structures that all disappeared is skipped likewise. A NO that ends a
batch's row FETCH keeps the rows received, stores them, and ends the
cycle as an incomplete list (`IncompleteList::ServerRefused`) without the
completed state, as today's `MessageList.refusal` does, so a folder never
looks complete while the server withheld messages. The sequence-number
FETCH goes. A NO that left messages of the row command unanswered first
asks for them again apart, their rows in one command and then each
structure on its own (002 contracts/imap-reading.md, isolation), so that
a server which refuses to describe one damaged message for good leaves
that message its row, stored as unreadable (002's rule); the messages it
withholds still make the list incomplete (*amended 2026-10-02, §14*). A NO
the server marks temporary (RFC 5530 `UNAVAILABLE`) on a structure asked
for on its own or on a batch's texts fails the cycle as a temporarily
unavailable server (006) and stores nothing of the batch, so the next
cycle fetches it again; any other NO there keeps 002's rule, the message
stored as unreadable (independent review, 2026-09-30: with the old rule a
passing refusal left up to a batch of messages without text until the
content cache, and Retry is gone).

**Rationale**:
- Texts with their rows (spec FR-003): the newest messages are readable as
  soon as listed, and a stop leaves no stored message of the last 30 days
  without its text.
- 100 is the newest-100 load's size, whose timing on real servers is known; one batch
  with texts took seconds on the slowest server measured (19 texts per
  second).
- Structures are never asked for a whole folder in one command: on one
  server `UID FETCH 1:* (BODYSTRUCTURE)` over a folder of several thousand
  messages took 15 minutes, while groups of 100 ran at 92 messages per second (measured).
- The batch size bounds the work a stop loses (spec FR-010), not the time
  quitting takes (§8).

**Alternatives**: every row first, texts afterwards: rejected by the spec
challenge (the newest message said "not downloaded" during a fill). Larger
batches for rows without text: kept as an optional mechanism, for a first
fill of a very large folder that proves slow.

**Measured on the installed build (2026-09-30)**: on one server the first
read of any header of an older message costs about 90 ms, whatever the
items (header fields, ENVELOPE or the whole header: 9–11 s per 100), and a
second read of the same messages 0.2 s; flags and dates alone take 0.2 s
per 100. A first fill of 9 000 messages there takes about 15 minutes, the
newest rows first. Larger batches do not help, since the cost is per
message; several connections at once would, and belong with 019.

## §4 Gmail: label folders by the same method

**Decision**: A Gmail label folder runs §2 and §3 with `X-GM-MSGID` in the
listing. A listed message's identity is `gmail:<X-GM-MSGID>`. Before a batch's
rows are fetched, the cycle asks the store which of the batch's
identities the account already holds (`Store::stored_identities`, one
query on the unique `(account, identity)` index); those messages are only
related to this folder with their listed read state, and nothing of them is
fetched again (spec FR-005: "messages the store lacks"). A row without `X-GM-MSGID` is
left out of the cycle and written to the record: Google documents the
attribute on every message, and a message is never guessed from its place.
Such a row, if its message is stored, removes it with the listing's proof,
and the next listing that carries the identifier brings it back as an
arrival (spec, Considered and out of scope; a review asked on 2026-09-30).
Google's documentation and the public bug trackers of other clients name
no case of a missing identifier, and a listing our parser cannot read
fails the cycle instead of dropping the row.

**Rationale**: spec FR-006 and Clarifications; the listing's cost grows by
one number per line.

**Alternatives**: All Mail with labels: rejected in the spec.

## §5 Microsoft 365: the delta query, texts by date range, and resumption

**Decision**:
- A cycle reads `GET /me/mailFolders/{id}/messages/delta` with `$select` of
  the list fields and `isRead`, `$orderby=receivedDateTime desc` on a first
  reading, `Prefer: odata.maxpagesize=500` and `IdType="ImmutableId"`; it
  follows `@odata.nextLink` until `@odata.deltaLink`. Each page is a
  batch.
  Page size (measured 2026-09-30): a page takes about 0.35 s plus 5 ms per
  message (50 in 0.6 s, 500 in 3 s), so smaller pages lengthen a full
  reading; the same pages once took 20 s and one 81 s, the service's
  load at that hour.
- An entry for a message the account also holds in another folder is not
  applied from the entry, which may be older than that folder's state (a
  message read in A, moved to B and marked unread there, then an old
  entry of A's round): the message is read with `GET /me/messages/{id}`
  including `parentFolderId`, its current fields and read state are
  stored, and it is related to this folder only if `parentFolderId` names
  it; otherwise it leaves this folder, since the service placed it
  elsewhere or no longer finds it (external review, 2026-09-29;
  independent review, 2026-09-30). This happens only while a moved
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
  An `@removed` entry met with another entry for the same message in one
  page is trusted neither way: the service documents that an entity can
  appear several times and in no certain order, and a removal, once
  reported, is not reported again, so a message read and then moved out
  could stay listed for good if the later entry won; the message is read
  again like an entry that changed other fields, and leaves the folder
  when the service does not place it here (independent review,
  2026-09-30). Across pages the order stands: a removal on one page and a
  change on a later one apply in turn.
- Texts on a first fill: a page of the first reading is ordered by
  received date, so the folder holds no message between its earliest and
  latest dates that the page lacks; its messages within 30 days get their
  texts in one request
  `GET /me/mailFolders/{id}/messages?$filter=receivedDateTime ge A and receivedDateTime lt B&$select=id,body&$top=500`
  with `Prefer: outlook.body-content-type="text"` (the list's default page
  is 10 messages), A being those messages' earliest date and B a second
  after their latest; messages with an equal date outside the page are
  ignored. The service keeps dates finer than the seconds it shows: a
  range ending `le` the latest shown date missed that message (probe,
  2026-09-30).
- A message of the last 30 days whose list fields the service reports
  again (a listed entry, or a partial one that changed other fields) gets
  its text again: a draft edited in another client keeps its identity,
  and so does the message once sent (spec FR-009; external review,
  2026-09-29). Probe, 2026-09-29: a draft whose text alone was edited in
  Outlook on the web came in the next round as a listed entry. A text the
  service then does not return leaves the stored one in place
  (data-model.md): a full reading reports every message's fields again,
  and a message deleted between its page and the text request, or one the
  range answer misses, would otherwise lose the text it had (independent
  review, 2026-09-30).
- Texts in a round of changes: its arrived messages are scattered in time
  (a message moved in from a month ago beside today's), so each text is
  read by the message's identifier; a date range could cover a month of
  mail for two messages (external review, 2026-09-29).
- Position: the `@odata.deltaLink` is saved with the batch that completes
  the cycle, as `server_position`. During the first fill of a folder never
  refreshed, each page's batch saves its `@odata.nextLink` as `fill_place`,
  the place to continue (spec FR-008, FR-010). Each page that is not a
  reading's last marks the folder not completed and leaves
  `server_position` as it was, so an interrupted round starts again from
  it and is never taken for a first fill (external review, 2026-09-29: one
  field held both links, told apart by `synchronized`).
- A 410, or any other 4xx answering a saved link except the token's 401
  and the throttling 429, means the saved position or place is no longer
  accepted: the service documents a 410 and "a 40X-series error with error
  codes such as `syncStateNotFound`" for a token it no longer holds (delta
  query overview, "Token duration", checked 2026-09-30), so the code is not
  relied on; a refused first reading is its own failure, and a link refused
  for another reason costs one full reading that meets the same refusal
  (independent review, 2026-09-30: with the narrow rule a folder whose link
  the service refused with another code could never be refreshed again).
  The cycle then reads the whole folder, keeps the listed identities in
  memory, and at its end removes the stored messages it did not list; if it
  stops, the next cycle starts it again (spec FR-007, FR-010): with no
  saved link and rows stored, a first reading is such a full reading,
  never a first fill that removes nothing (final review, 2026-09-30).

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

**Checked before the Microsoft 365 portion** (probe with the maintainer,
2026-09-29, one run): a delta reading of the Inbox was paused after its
first page of 50; meanwhile the maintainer marked a message of that page
read, moved another to a different folder, and changed the subject of a
draft in a Drafts reading that had completed. The continued reading
(3 425 entries) reported none of these changes. The next round reported
each: the moved message as `@removed`, the read state as an entry carrying
only `isRead`, and the draft as an entry with every selected field. So a
continued first fill reads one more round, as below, and needs no full
re-reading.

**Alternatives**: `$select=body` on the delta query: downloads every
message's text, against spec FR-009; JSON batching of per-message requests:
more code for the same result as a date range.

## §6 The store: folder state, batches, reads without text

**Decision** ([data-model.md](data-model.md)):
- `folder` gains `server_position` and `fill_place`; `loaded` becomes
  `synchronized` ("the folder's latest cycle completed"). No numbering version is stored (§2).
  The first batch of a cycle that has messages to fetch sets
  `synchronized` to 0, so a stopped cycle that left no row is shown as "no
  mail loaded", never as an empty folder.
- `membership` loses `position`: rows are ordered by the message's received
  date, newest first, then by the stored row's id. No index is added: the
  read starts from the folder's memberships and sorts them. Confirmed in
  portion 2 with `EXPLAIN QUERY PLAN` (SQLite 3.51.2, a store of
  200 000 messages, 100 000 in the folder): a search of `membership` by its
  primary key's prefix, the message by its row id, and a temporary B-tree
  for the order; the read took 26 ms.
- The content code `not_downloaded` joins the schema's `CHECK`.
- `Store::read_folder_sync(folder)` returns the folder state and the stored
  identities with their read state, once per cycle.
- `Store::store_batch(folder, batch, cancelled)` writes one batch in
  one transaction under the store's lock, after the cancellation check that
  loads use today: removals, orphaned messages, read states, arrivals (an
  arrival never replaces a stored text with `NotDownloaded`), memberships,
  and the folder state when the batch carries one.
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
batch after batch without change. The existing `load_cancelled` check
under the lock keeps 007 FR-007 and FR-008 for every batch.

**Alternatives**: a stored "listed in this reading" mark per relation, to
resume Microsoft 365 re-readings: rejected with the spec's rule that
re-readings restart.

## §7 The worker reports batches; the window re-reads the shown folder

**Decision**: The mail worker's outcome channel carries
`LoadEvent::BatchStored` any number of times before the final
`LoadEvent::Finished(LoadResult)`. Today it holds one message and the result
is sent with `try_send`; it becomes unbounded and the window receives in a
loop, so the final result is never dropped behind an unread batch event.
The window, on a batch of any folder of the shown folder's account (a
Gmail label and a moved Microsoft 365 message share messages across
folders, as the completed load already rules today), reads the shown
folder's rows again while the rows and the banner on screen stay as they are (a "read
due" mark, not the state of a first read, which hides the banner); a
batch that arrives while a read runs marks one more read when it ends, so
reads never pile up and no timer is needed.

**Rationale**: FR-003 and FR-013; the numbered-read pattern exists for
stored mailboxes (007, 008).

**Alternatives**: the worker passes the batch to the window: two sources
of rows, against 007 FR-001 ("the window shows stored mail only").

## §8 Quitting within a second

**Decision**: Unchanged mechanism, now a requirement (spec FR-010): closing
the window drops the running load's handle, the worker drops the cycle's
future, which closes its connection, and nothing joins the worker thread
(`main.rs`, `connect_destroy`). A batch being written finishes or rolls
back with the process (007 FR-010). A test starts a cycle against a
scripted server that stops answering, cancels it and checks that the
cancellation is reported within a second; the installed build is checked by
hand (quickstart).

**Rationale**: the store's transaction is the only work that cannot stop at
once; a batch's write took milliseconds (measured for 100 000 rows in
batches of 500: 0.8 s in all).

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
common beginning and end; the open message is found again by its identity
after a splice and its row selected, or the reader closes when it is gone
(spec FR-013). One click or Enter opens a message, as the list box did: the
list view activates on a single click and its rows are not selectable by
the user, so the selected row is always the open message; the arrow keys
move the focus, which GTK keeps on its row (maintainer's decision
2026-09-29; checked in GTK 4.22.5 `gtklistfactorywidget.c`: with
`single-click-activate` a selectable row is selected on hover, and a row
that is not selectable ignores the pointer's and the keys' selection).
Tab leaves the list after one row (`tab-behavior` `item`) instead of
visiting every row. The row texts are made when a shown row reads them:
building 100 000 row objects with their dates formatted took 0.76 s on
GTK's thread in a release build, and 0.13 s without.

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
rebuilding the model on each read: 100 000 items per batch, and the
selection lost.

**Checked in portion 1** (Cambalache 1.0.3, 2026-09-29): Cambalache does
not edit this form, so `message-row.ui` is edited as text. Cambalache reads
`<template>` as a new class whose base class is its `parent` attribute. Without
`parent`, the project loads but the form does not open. With
`parent="GObject"`, loading the property bindings fails and the whole
project with it (`cmb_db.py` looks up the base class `object`, which its
catalog lacks). GTK refuses any other `parent`: a list item template's
parent must be `GObject`.

## §10 `MailboxChanged` stays for one case

**Decision**: The failure kind and its wording stay. Of its four producers
today, three go: rows of a sequence-number FETCH that all vanished
(`reader.rs`, `fetch_rows`), structures that all vanished
(`fetch_structures`, itself gone on 2026-10-02 when the structures joined
the row command, §14) and a newest-100 load whose messages all vanished
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
cycle, and the stored batches and the saved place make the next Refresh
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

**Decision**: When Microsoft Graph refuses the token of a running cycle
(a 401), the cycle asks Online Accounts for the account's access once more.
When Online Accounts hands out a different token, the cycle repeats the
request once with it. With the same token, which Online Accounts gives
again for a token just handed out, and after a second refusal, the
refusal stands as a refused sign-in. A Gmail session is not renewed: a
session Gmail ends stands with Gmail's own reason (004 FR-003), and a
password account is not renewed either; a lost connection ends the cycle,
and the next Refresh continues.

**Gmail, checked at the final review (2026-09-30)**: the plan assumed that
Gmail ends an OAuth session when its token expires (004 research,
secondary sources) and renewed the session after a BYE. A probe signed in
to Gmail's IMAP with one token and kept it: 18 minutes after the token
expired the open session still answered `FETCH` of flags and header
fields every 4 minutes, while a new sign-in with the same token was
refused (`AUTHENTICATIONFAILED`). Gmail checks the token at sign-in only,
so a long cycle is not cut by the expiry; the renewal of Gmail sessions,
its BYE mark in `mailbag-imap` and `MailboxReader::reopen` were removed
(the maintainer's decision).

**How the worker asks**: the mail worker cannot call Online Accounts, whose
adapter belongs to GTK's context. `MailLoader` gives each Microsoft 365
load a renewal channel: the cycle sends a request with a reply channel, a
task on GTK's context asks the adapter with the same request the load
started with, and sends the new access back. A load cancelled meanwhile
drops the request. The renewal adds nothing to `mailbag-imap`,
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
  (§5), and Microsoft Graph checks the token with every request.
- Answering a refusal covers expiry, revocation and a wrong clock with one
  mechanism; predicting expiry from the lifetime would still need it.

**Alternatives**: renewing before expiry from the returned lifetime: the
adapter discards the lifetime today, and a refusal would still need
handling; a failure shown and the next Refresh continuing:
rejected by the maintainer, since the user would see a false sign-in
failure.

## §14 One connection, faster (amendment of 2026-10-02)

**Decision**: three changes to how a cycle uses its one connection
(FR-012), decided at a feature-start on 2026-10-02 after the message
list's acceptance had measured the cycle on real servers, and built as a
small amendment: build, look at the installed build, then write down.
Budget: at most 220 production lines changed, no thread, timer or
dependency, one fork commit; the size came out about 365 changed lines,
net +73 after the simplify-review, accepted by the maintainer.

1. **The parser accepts `NIL` as a part's encoding.** One server answers
   `NIL` where RFC 3501 wants a string, for a part without a
   Content-Transfer-Encoding header. The imap-proto fork rejected the
   whole FETCH answer, so the reader reconnected and read the rest of the
   batch one message at a time (002's isolation): 20 to 28 s per episode,
   23 episodes and 202 s in one fill of 5 983 messages, and those messages
   stayed without text and preview. The fork now reads `NIL` as 7BIT, the
   default RFC 2045 §6.1 gives the absent header (fork commit `6adb660`,
   tag `mailbag-2026-10-02`). Text is decoded from the part's own MIME
   header, so the field changes nothing else.
2. **Compression when the server announces it** (`COMPRESS=DEFLATE`,
   RFC 4978). After the capabilities of the signed-in session the reader
   sends `COMPRESS DEFLATE`; on OK, GIO's `ZlibCompressor` and
   `ZlibDecompressor` in raw form go between the session and the TLS
   stream as converter streams, swapped inside the stream handle the IMAP
   library holds and hands out (`transport.rs`, `GioStream::compress`); the
   handle keeps the TLS connection itself, since GIO's TLS input and output
   streams do not keep it alive (found when a review removed the field: the
   next command failed). NO or BAD leaves the connection as it is. Gmail announces it, the two Generic IMAP
   servers probed do not (checked 2026-10-01); Google's IMAP documentation does not mention it
   (checked 2026-10-02), so the capability decides, for any server.
   Rejected: the IMAP library's own `compress` feature, which adds the
   async-compression crate and changes the session's type.
3. **Rows and structures in one command.** `UID FETCH <uids> (… BODYSTRUCTURE)`
   per batch instead of two commands: a server spends about as much on a
   second command for the same messages as on the first (measured
   2026-10-01 per message: rows and structure apart 10 + 10 ms on Generic
   IMAP server A, 95 + 105 on server B, 33 + 34 on Gmail; in one command 10, 148 and 31).
   When the command does not answer for every message, because the server
   refused some or the parser rejected one structure, the messages it did
   not answer for, or answered without a structure, are read again apart:
   their rows in one command, then each structure on its own, so that one
   message the server cannot describe keeps its row (§3; 002
   contracts/imap-reading.md, isolation; the second case added after the
   review of PR #16: a server may answer rows before structures).

**Measured on the installed build** (2026-10-02, accounts of each
provider, a fill from an empty store of the same folders as the message
list's acceptance, from the record's timestamps):

| Folder | Before | After |
|---|---|---|
| Gmail Inbox, 763 messages | 54.5 s; texts 48.3 s | 31.5 s; texts 27.8 s; `compression enabled` in the record |
| Generic IMAP server A, Inbox, 5 983 | 744.8 s; structures 202 s; 23 reconnections; 25 empty previews | 547.6 s; 0 reconnections; 15 empty previews, pages without words (010) |
| Generic IMAP server B, Inbox, 9 322 | not filled in the application before | 1 587.6 s; rows with structures 1 100 s, 118 ms per message; texts 487 s |

A refresh of a folder where nothing changed ends in 0.4 to 2.4 s on every
account, with the listing only. No warning in the record; no subject,
address or text in it.

**Left for a measurement, not built**: a batch above 100 messages helps
only where the cost is per round trip (Gmail), not per message (server A,
§3), and grows the work a stop loses (FR-010); SASL-IR and Gmail's
untagged CAPABILITY after sign-in save about two round trips per refresh;
several connections per account belong to background synchronization
(020): many accounts with several connections each load a server, and
Gmail allows 15.

## §15 The state pass (amendment of 2026-10-04)

**Decision**: an IMAP cycle learns the folder's state in a *state pass*
(spec FR-005): the four numbers the opening returns (UIDVALIDITY, EXISTS,
UIDNEXT, HIGHESTMODSEQ), compared with the folder's stored ones, decide
what to list: nothing, the changed flags (`CHANGEDSINCE` where CONDSTORE
serves, every message's flags otherwise) or every message. The pass runs
at the cycle's start and, after batches or commands, once more before the
cycle closes; it stores the numbers it started from with what its listing
proved. How often passes run during a fill belongs to background
synchronization (020).

**Why**: the live check of 011 (2026-10-04) found three holes with one
root. The cycle's single listing at its start was its only view of the
server while a first fill ran minutes (6 and 19 minutes on the two Generic
IMAP servers of §14): a folder a cycle's own command changed stayed out of
agreement until 011 added a full re-listing after commands; changes
another client made during a fill showed only at the next refresh; and
each fix inside one long cycle patched the same premise. The maintainer
named it: one snapshot must not stand for a minutes-long process. A pass
that costs one round trip when nothing changed can be run again, and the
second pass at the cycle's end gives the agreement 011 bought with a full
listing.

**Checked**:

- RFC 3501 §2.3.1.1: the next unique identifier "MUST NOT change unless
  new messages are added to the mailbox" and "MUST change whenever new
  messages are added to the mailbox, even if those new messages are
  subsequently expunged"; so with UIDNEXT equal nothing arrived, and with
  EXISTS equal too nothing left. §6.3.1: a SELECT of the selected mailbox
  deselects it first, so the second pass opens the folder again in the
  same session. §6.4.8: a UID the mailbox lacks is ignored (011).
- RFC 7162 §3.1.2.1 and §3.1.2.2: once a CONDSTORE enabling command was
  issued, the server MUST return HIGHESTMODSEQ, or NOMODSEQ for a mailbox
  without persistent mod-sequences, with every successful SELECT; after
  NOMODSEQ a FETCH with CHANGEDSINCE is rejected with BAD. §3.1.4.1: the
  CHANGEDSINCE modifier returns only messages whose mod-sequence is
  higher than the one given. §3.2: only QRESYNC requires the mailbox's
  mod-sequence to rise on an expunge, so removals rest on EXISTS and
  UIDNEXT.
- The async-imap fork at its pinned revision: `Mailbox` carries `exists`,
  `uid_next`, `uid_validity` and `highest_modseq`, read from the SELECT
  response codes; `select_condstore` sends `SELECT … (CONDSTORE)`;
  `uid_fetch` sends the query text as given, so `(UID FLAGS) (CHANGEDSINCE
  n)` needs no change. imap-proto parses `HIGHESTMODSEQ` and `MODSEQ`,
  and a response code it does not know, such as `[NOMODSEQ]`, passes as
  text, leaving `highest_modseq` empty. No fork change.
- Google's IMAP documentation does not describe CONDSTORE (the IMAP
  extensions page and the IMAP, POP and SMTP page, checked 2026-09-28 and
  2026-10-04); Gmail announces it after sign-in (004 research), and its
  HIGHESTMODSEQ is one for the account (004 research).

**Measured** on 2026-10-04 with a read-only probe (EXAMINE, STATUS, SEARCH
and FETCH of UID and FLAGS only) over an account of each IMAP provider,
medians of three runs; the Gmail folders held under 800 messages, so
Gmail at scale is unknown:

| | Gmail, 766 messages | Generic IMAP server A, 6 004 | Generic IMAP server B, 9 322 |
|---|---|---|---|
| A round trip (NOOP), an opening, a STATUS | 0.17 s each | 0.25 s each | 0.08–0.2 s each |
| `UID FETCH 1:* (UID FLAGS)` | 0.46 s, 37 KB (60 KB with `X-GM-MSGID`) | 0.63 s, 326 KB | 0.60 s, 385 KB |
| `UID SEARCH UNSEEN`, `FLAGGED`, `ALL` | one round trip each | one round trip each | 1.6 s, 0.2 s, 0.3 s |
| `CHANGEDSINCE`, nothing changed | one round trip | one round trip | no CONDSTORE |
| `CHANGEDSINCE`, 60 and 184 changed | 0.42 s, 2.9 KB | 0.25 s, 10 KB | — |
| `UID FETCH <100 uids> (UID FLAGS)` | one round trip | one round trip | one round trip |
| Announced | CONDSTORE, ESEARCH; no QRESYNC | CONDSTORE, QRESYNC, ESEARCH | none of them |

A listing costs about 45 bytes and 60 µs per message on the Generic
servers, so a folder of 100 000 messages about 6 s and 4.5 MB (inferred);
on Gmail about 0.4 ms per message between 13 and 766 messages (not
verified beyond). Gmail's `CHANGEDSINCE` is small in bytes and about as
slow as the full listing when anything changed, and one round trip when
nothing did; server A's is one round trip either way.

**Alternatives**:

- A listing before every batch, as first considered: on a server without
  CONDSTORE a listing per hundred messages adds about 5 % to a fill of
  9 000 messages and doubles one of 100 000. Not taken as a rule of the
  cycle: the cadence of passes is the scheduler's (020).
- A pass once a minute during a fill: proposed and withdrawn the same
  day, a constant standing in for the scheduler that does not exist yet;
  correctness never depended on it.
- Flags by `SEARCH UNSEEN` and `SEARCH FLAGGED`, the UID set by ESEARCH:
  a round trip each, as much as the listing of a few thousand messages,
  and on server B three times the listing; ESEARCH compressed the UID set
  to 198 bytes on Gmail and to 25 KB on server A, whose UIDs are sparse.
  Not taken; the listing stays where the numbers say something changed.
- QRESYNC: announced by server A only, not by Gmail. Not taken.
- A flags fetch of the sent UIDs alone to confirm commands: one round
  trip everywhere; optional (plan), since the second pass covers it and
  costs the same on CONDSTORE servers.
- Arrivals alone by `UID FETCH <stored UIDNEXT>:*`, with removals ruled
  out when EXISTS grew by exactly the arrivals: optional (plan); new mail,
  the most common change, would cost a round trip instead of the listing.

**CONDSTORE on Gmail**: 009 decided on 2026-09-28 to use the base method
only on Gmail, since Google does not document CONDSTORE; on 2026-10-02 it
decided for COMPRESS=DEFLATE, likewise undocumented by Google, that the
announced capability decides (§14). Decided on 2026-10-04: the same rule
for CONDSTORE, for any server (the maintainer's decision).

**Left to 020**: how often a pass runs during a fill; slicing a fill into
short cycles, each with its own pass (the newest-first order means
everything above a UID is stored, so a continued fill could list only the
rest); fetching, within the cycle, the arrivals a second pass finds (they
wait for the next cycle, as the listing after commands of 011 left them,
and the folder stays not completed until then).

**The challenges of 2026-10-04** (two fresh sessions, the requirements
and the plan's mechanisms) found four holes, closed the same day:

1. *Pending changes need the listing.* 011 addresses a change by the UID
   the cycle's own listing shows and stores no UID; a pass that lists
   nothing, or only the flags another client changed, leaves a star made
   before a quiet refresh unsent until something else changes in the
   folder. A first pass of a folder with pending changes lists every
   message; the cost is today's listing on exactly the refreshes that
   carry a change. Addressing a Gmail message by `UID SEARCH X-GM-MSGID`
   instead (≈ 0.2 s each, 011 research §2) is optional in the plan; a
   Generic IMAP identity carries its UID and would need no search.
2. *A second pass that lists an arrival it does not fetch* stores numbers
   that already count it; the next pass finds them equal and the message
   is never fetched. Such a pass leaves the folder not completed.
3. *The numbering version* belongs in every comparison: a renumbered
   folder without expunges shows the old EXISTS and UIDNEXT, and RFC 3501
   §2.3.1.1's guarantees hold only "unless the unique identifier validity
   also changes".
4. *The second pass compared with the stored state*, which the first
   batch of a fill marks not completed, so every fill ended with a full
   listing. It compares with the first pass's numbers instead: the store
   then holds every message that listing showed, since a refused row
   command ends the cycle before the second pass.

Also checked by the plan's challenge: inside a FETCH stream the fork
forwards EXISTS, EXPUNGE and RECENT as typed unsolicited items and the
rest as `Other`, which the session's notice collector alone reads; UIDNEXT
and HIGHESTMODSEQ never arrive unsolicited, so a second SELECT is the way
to fresh numbers; a session closed after an unreadable structure must
reconnect before it (the reader does so for every command);
mod-sequences are 63-bit values, which SQLite's INTEGER holds; the
NOMODSEQ case needs no server knob, since the scripted opening's
completion text takes an untagged line.
