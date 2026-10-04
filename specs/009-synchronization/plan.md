# Implementation Plan: Synchronization

**Branch**: `claude/sync` | **Feature**: `009-synchronization`
**Date**: 2026-09-28 | **Spec**: [spec.md](spec.md)
**Status**: Approved on 2026-09-29 (tasks T001). The maintainer's decisions of 2026-09-28 are recorded
under "Decisions for the maintainer". Supporting documents:
[research.md](research.md), [data-model.md](data-model.md),
[contracts/synchronization.md](contracts/synchronization.md),
[quickstart.md](quickstart.md).

## Size

Budget agreed at the feature-start on 2026-09-28 and raised by the
maintainer the same day, when the plan added renewing access during a
cycle, then raised again on 2026-09-29 after the IMAP cycles came out larger
than estimated: at most 2 000 production lines; the test lines came out
about 1.45 times the 1 500 planned, which the maintainer accepted on
2026-09-29; no thread, timer or queue of the feature's own;
no new dependency; no change to the IMAP library forks. The estimates below
include doc comments and formatting, which the estimates of 007 and 008
left out and came out 1.6 to 1.9 times low. Reassess with the maintainer
before exceeding the budget or about 1.5 times an item's estimate; the size
so far is compared with this table at every review pause.

| Item | Budget | This plan (estimate) |
|---|---|---|
| New modules and production lines | ≤ 1 600 net | ≈ 1 550 (≈ 1 340 before the plan challenge, which found the Microsoft 365 cycle low and added partial delta entries, row refusals and the event loop; then relating messages another folder stored ≈ 25 and renewing access ≈ 75; after the external review, the numbering version in the identity −20, reading moved messages again ≈ 15, texts of change rounds by identifier ≈ 12, the BYE mark ≈ 10, one more round after a continued fill ≈ 8, reads on batches of the account ≈ 3): `mailbag-domain` ≈ 70 (`FolderState`, `FolderBatch`, `MessageListRow`, `NotDownloaded`; `MailboxChanged` goes); `mailbag-imap` ≈ 170 (the listing, rows by UID, the empty mailbox; the sequence-number rows go); `mailbag-graph` ≈ 210 (the delta request and its full and partial entries, the rejected position, texts by date range, one message; `list_mailbox_messages` goes); `mailbag-providers` ≈ 540 (the IMAP cycle ≈ 170, the Microsoft 365 cycle ≈ 200, batches and load events ≈ 60, the 30-day selection ≈ 30, renewing access ≈ 75; the newest-100 path goes); `mailbag-store` ≈ 215 (schema, the cycle read, stored identities of a batch, the batch write, rows without text, one message's content; `replace_mailbox` and `read_mailbox` go); `mailbag` ≈ 320 (the list view, its row object and the row template ≈ 100, the update by difference with read states changed in place and the selection kept ≈ 80, reading a message on opening ≈ 40, reads on batches ≈ 40, "not downloaded" wording ≈ 20, load events ≈ 40) |
| Call sites or existing files touched | — | imap `lib.rs`, `reader.rs`, `fetch_responses.rs`, `test_server.rs`; graph `lib.rs`, `reply.rs`, `test_server.rs`; providers `lib.rs`, `batch.rs`, `worker.rs`, `imap.rs`, `gmail.rs`, `imap_batch.rs`, `microsoft365.rs`, `store_load.rs`, new `cycle.rs`; store `schema.sql`, `lib.rs`, `folders.rs`, `content.rs`; domain `lib.rs`; mailbag `window_ui.rs`, `mail_ui.rs`, `refreshes.rs`, `failure_declarations.rs`. Forms: `mailbag.ui` (the messages list and its factory) and `message-row.ui` (a list item template) |
| New crates | 0 | 0 |
| New threads, timers, queues | 0 | 0: the worker's existing outcome channel carries the batch events; the window's numbered reads coalesce |
| New state, types, error types | — | Domain: `FolderState`, `FolderBatch`, `MessageListRow`, `ReceivedContent::NotDownloaded`; providers: `LoadEvent`; imap: `FolderListing`, `ListedUid`; graph: `ChangesFrom`, `ChangePage`, `NextPage`, `MessageChange`, `GraphFailure::PositionRejected`; window: `MessageItem`, the row object. `IncompleteList::MoreAvailable` goes; `FailureKind::MailboxChanged` stays for a reconnect (research §10) |
| New fields in existing data | — | `folder.server_position`, `folder.fill_place`, `folder.synchronized` (was `loaded`); a Generic IMAP identity gains the numbering version; `membership.position` goes; content code `not_downloaded` ([data-model.md](data-model.md)) |
| Changes to other features' contracts or documents | 007, 008, 002, 004, 005, 006 | As the spec's Amendments; 006 loses `MoreAvailable` and its wording |
| New dependencies | 0 | 0 |
| Tests | ≤ 1 500 | ≈ 1 340: imap ≈ 180 (listing complete, refused, lost; EXPUNGE during the listing; empty mailbox; rows by UID and their refusal), graph ≈ 180 (pages, removed and partial entries, rejected position, text range, one message, a 401), providers ≈ 460 (cycles against the scripted servers: SC-003 to SC-006, resumption, cancellation within a second, related messages, renewing access), store ≈ 220 (batches, kept texts, reads, orphans), window ≈ 200 (list updates keep the selection and the reader, reads on batches, "not downloaded", a 100 000-row model); scripted-server additions ≈ 100 |

## Summary

Refresh Mailbox runs a cycle that brings the selected folder into agreement
with its server (spec FR-001). A cycle has two parts: learning the server's
changes, which is one function per way (the IMAP base method for Generic
IMAP and Gmail, the delta query for Microsoft 365), and storing them as
batches, which is one store operation for every way (research §1). On IMAP
the cycle compares one listing of every message's number and read state
with the stored folder (§2), then fetches the missing messages highest UID
first, a hundred at a time, with the text of those received in the last 30
days (§3). On Microsoft 365 it reads the delta pages, fetching texts for a
page by its date range (§5). The worker reports each stored batch, and
the window reads the shown folder again and updates its list by the
difference, keeping the selection and the open message (§7, §9). The
messages list becomes a list view that builds only the visible rows. The
list reads no text; a message's content is read when it is opened.
Quitting never waits for a cycle (§8).

## Minimal version

| Step | What it does | Cost |
|---|---|---|
| The domain | `FolderState`, `FolderBatch`, `MessageListRow`, `NotDownloaded`; `MoreAvailable` removed | ≈ 70 |
| The store | Folder state columns, no position, `not_downloaded`; `read_folder_sync`, `stored_identities`, `store_batch`, `read_folder_rows`, `read_message_content`; the replace-and-read pair removed | ≈ 215 |
| IMAP | `list_messages` streamed into a small record per message with the completion; rows by UID with their refusal; EXISTS 0 without a command | ≈ 170 |
| Microsoft Graph | Delta pages with removed, listed and partial entries; a rejected position; texts by date range; one message | ≈ 210 |
| The cycles | IMAP (Generic and Gmail through one function with an identity rule, known messages related) and Microsoft 365; batches of 100; the 30-day selection; load events; renewing access | ≈ 540 |
| The window | List view with a row object and the row as a list item template; update by difference, read states in place; content read on opening; reads on batches; wording | ≈ 320 |

## How a cycle runs across the components

```mermaid
sequenceDiagram
    participant W as Window (GTK thread)
    participant L as Loader (Online Accounts)
    participant K as Mail worker
    participant S as Server
    participant D as Store

    W->>L: Refresh Mailbox: start_load(folder)
    L->>K: access granted, run the cycle
    K->>D: read_folder_sync(folder)
    D-->>K: folder state, stored identities and read state
    K->>S: open the folder; list every message (IMAP) or read changes (Graph)
    S-->>K: listing or first page
    loop each batch
        K->>S: list fields and texts of the batch's messages
        S-->>K: rows and texts
        K->>D: store_batch (one transaction, cancellation checked under the lock)
        K-->>W: BatchStored
        W->>D: read_folder_rows (GIO pool; one read at a time, one more if due)
        D-->>W: rows
        W->>W: update the list by difference; keep the selection and the reader
    end
    K->>D: store_batch with the completed state
    K-->>W: Finished(Stored)
    Note over W,K: Closing the window drops the load: the worker drops the cycle<br/>and its connection; nothing waits (FR-010)
```

## Function map

**`mailbag-providers`** — the entry point and the cycles.

- `worker::run_load(kind, target, store, cancelled, events)`: for
  `LoadTarget::Mailbox(folder)` calls `synchronize_folder`; the folder list
  is unchanged.
- `cycle::synchronize_folder(kind, batches) -> LoadResult`: chooses the
  provider once (004 plan D1) and runs one of the two cycles; `batches` is
  the folder's `BatchWriter`.
- `cycle::imap::synchronize_imap_folder(access, identity_rule, batches)`:
    1. `ImapFolder::open` — SELECT (EXAMINE until 011 FR-008); the folder's
     numbering version, which Generic IMAP identities carry.
  2. `batches.read_folder_sync()` — the stored state and identities.
  3. `ImapFolder::list_messages` and `identify` — the listing, whether it
     completed, and each message's identity (`imap:<folder>/<UIDVALIDITY>/
     <UID>`, or `gmail:<X-GM-MSGID>` with a Gmail message without it left
     out).
  4. `listing_changes` — removals if completed, read-state changes, and
     the folder marked not completed when messages are missing; one batch.
  5. `ImapFolder::fetch_arrivals` — for each hundred missing messages,
     highest UID first: `batches.stored_identities` (messages another
     folder stored are only related), rows by UID for the others, and
     `read_contents` for those within 30 days; one batch each.
  6. the completed state, if the listing completed; `batches.finish`.
- `cycle::graph::synchronize_graph_folder(access, service_url, renewal,
  batches)`:
  1. `batches.read_folder_sync()`.
  2. `where_to_start` — saved position, a first fill's saved place, or a
     first reading.
  3. for each page of `read_message_changes`: `merge_per_message`, then
          `GraphService::batch_from_changes` (removed entries; read states of
          messages only this folder holds (*until 2026-10-05*: since then every
     entry about a stored message in a round reads the message, and a
     round naming more than `ROUND_READINGS_LIMIT` (100) stored messages
     turns into a whole reading as after a rejected position, spec
     FR-007); listed messages; unknown ones, and
     any message another folder holds (`batches.identities_in_other_folders`),
     read with `read_message` and kept only if it is in this folder now,
     leaving it otherwise; a removal met with another entry for the
     message in one page is read again likewise;
     texts by `read_texts_received_between` on a first reading's page, by
     `read_message_text` in a round of changes); the page's place saved
     when this is a first fill; one batch.
  4. on `PositionRejected` of a saved link: a full reading that keeps the
     listed identities in memory; a rejected first reading fails.
  5. a first fill continued from a saved place reads one more round of
     changes.
  6. the completed state with the delta link, and for a full re-reading the
     removal of what it did not list; `batches.finish`.
  `GraphService::request` repeats a request once after a 401, only with a
  renewed token.
- `cycle::recent_limit(cycle_start)` — the 30-day limit, computed once per
  cycle.
- `renewal::answer_renewals(account_id, request)` — on GTK's context,
  answers a load's `AccessRenewal::renew` with the Online Accounts request
  the load started with (research §13).
- `store_load::BatchWriter::store(batch)` — writes, then sends
  `LoadEvent::BatchStored` (built as `StoreChanged` since 011); a
  cancelled write ends the cycle as cancelled.
  `BatchWriter::finish` writes the cycle's record line.

**`mailbag-imap`**

- `MailboxReader::list_messages(row_items) -> FolderListing` — `UID FETCH
  1:* (UID FLAGS [X-GM-MSGID])` read as a stream; empty without a command
  when the count is 0.
- `MailboxReader::fetch_rows_by_uid(uids, row_items) -> MessageList` —
  replaces the sequence-number rows; a missing message is left out, a
  refusal travels with the rows.

**`mailbag-graph`**

- `read_message_changes(service_url, token, from) -> ChangePage`.
- `read_texts_received_between(service_url, token, folder_id, from, to)`.
- `read_message_text(service_url, token, id)`.
- `read_message(service_url, token, id)` — with the folder the message is
  in now.

**`mailbag-store`**

- `read_folder_sync`, `stored_identities`, `identities_in_other_folders`,
  `store_batch`, `read_folder_rows`, `read_message_content`
  ([contract](contracts/synchronization.md)).

**`mailbag` (the window)**

- `WindowUi::start_load` passes an event handler: `BatchStored` (built
  as `StoreChanged` since 011) for any
  folder of the shown folder's account calls `read_shown_mailbox_again`,
  which keeps the rows and the banner on screen and marks one more read
  when a read is running; `Finished` as today.
- `MessageItem` — the row object: identity, sender, subject, date text
  and whether it is unread, as properties the row template binds.
- `MailUi::show_rows(account, rows)` — `update_list_by_difference`: set
  the read state of listed messages in place, keep the common start and
  end of the list, splice the middle for arrived and removed messages,
  select the open message again by identity or close the reader when it
  is gone.
- `MailUi::open_message(identity)` — reads the content on GIO's pool and
  shows it, its reason, or "not downloaded".

## Optional mechanisms

Each is left out of the minimal version; the situation that would require
it and its cost are named.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Renewing access before it expires, from the lifetime Online Accounts returns | Refusals mid-cycle prove frequent enough to be visible in time | ≈ 20 lines |
| Larger batches for rows without text | The first fill of a very large folder takes too long because each hundred rows is one round trip | ≈ 10 lines |
| A Refresh that stops the running cycle | Waiting for a long first fill to refresh another folder proves a problem (spec Clarifications) | ≈ 30 lines and tests; amends 008 FR-012 |
| Reading a round's named messages in one `$batch` of 20 (Microsoft 365) | A round names dozens of stored messages, each read in its own request of about 0.35 s, so a refresh after a day of reading on another client takes many seconds; the record `changed messages read again` shows it. Not a priority: the gain is time only, the request count and the service's limits are the same, and above a hundred named messages the cycle reads the whole folder instead (FR-007, 2026-10-05) | ≈ 80 lines (the batch body, the answers by id, a status and a 429 per part) and the scripted service's `$batch` |
| CONDSTORE | Background synchronization, or a real folder whose listing makes Refresh slow (spec FR-015(e)). *Built on 2026-10-04 as part of the state pass (below)* | Estimated before the amendment at ≈ 80 lines and one state column with the fork parsing `[NOMODSEQ]`; built with four columns and no fork change, since the fork passes `[NOMODSEQ]` through as text (the amendment's table) |

## Decisions for the maintainer

1. **The form changes** (spec Assumptions; AGENTS.md "UI layout"):
   approved 2026-09-28. The messages list in `mailbag.ui` becomes a
   `GtkListView`, and `message-row.ui` becomes the row's list item template
   (2). The row keeps its look; portion 1 shows a render of the list before
   and after.
2. **How the row is built** (research §9): as Workbench's "List View" demo
   does, decided 2026-09-28: `message-row.ui` becomes a list item template
   whose labels bind to a small row object's properties. A list view
   reuses row widgets while scrolling, so building the form per row and
   filling it in code, as the list box does today, would need a row widget
   class or data attached to widgets anyway. Whether Cambalache edits a
   list item template with bindings is checked in portion 1.
3. **`MailboxChanged`**: approved for removal on 2026-09-28 on the
   plan's claim that nothing produced it any more; the plan challenge found
   a producer (a reconnect that meets another UIDVALIDITY, research §10),
   so it stays for that case; the maintainer was told the same day.
4. **A probe with the maintainer before the Microsoft 365 portion**
   (research §5), agreed 2026-09-28: whether a change made in another
   client during a paused first fill is reported by the first delta round
   after it. The probe starts a reading and waits; the maintainer marks one
   message read in the web interface and back; the probe then finishes the
   reading and reads the next round. If the change arrives only with the
   round after the reading, a resumed first fill reads that round within
   the same cycle; if it never arrives, a resumed first fill ends with a
   full re-reading (≈ 10 lines either way).
5. **After the plan challenge**, decided 2026-09-28: messages another
   folder already stored are related without fetching (research §4); the
   wait per Microsoft Graph request rises to 60 seconds (research §11);
   access is renewed once when refused during a cycle (research §13,
   spec FR-011), and the production budget rises to 1 600 lines for it.
   At the final review (2026-09-30) a probe showed that Gmail checks the
   token only at sign-in, and the Gmail half was removed.
6. **After the consistency analysis**, decided 2026-09-28: access is
   renewed only when Online Accounts hands out a different token, so a
   Gmail BYE for its limits or a revoked sign-in still ends the cycle as
   004 FR-003 and 005 FR-002 require (research §13); "Text not received"
   loses its Retry, which would repeat a Refresh that no longer fetches a
   stored message's text (006 amended); a message moved on Microsoft 365
   stays listed in its old folder until that folder's next cycle, as 008
   FR-004 is amended to say.
7. **After the external review**, decided 2026-09-29: the numbering
   version is part of a Generic IMAP identity, and the folder's stored
   version and the reset rule go (research §2); FR-001 holds within the
   service's guarantee, and a continued first fill reads one more round
   (research §5); an entry for a message another folder holds is applied
   from the message read again with its current folder (research §5);
   texts of change rounds are read by identifier (research §5); renewing a
   Gmail session is one attempt with a different token, and a second
   refusal keeps its real reason (research §13; superseded on 2026-09-30,
   decision 5: Gmail sessions are not renewed).
8. **After the independent review**, decided 2026-09-30: any 4xx
   answering a saved link, but the token's 401 and the throttling 429,
   rejects the position, since the service documents its codes only by
   example (research §5); a removal met with another entry for a message
   in one page is read again, and a message read again that the service
   places elsewhere leaves the folder (research §5; spec FR-004, FR-007);
   a refusal the server marks temporary (RFC 5530 `UNAVAILABLE`) of a
   batch's structures or texts fails the cycle instead of storing the
   batch as unreadable (research §3; spec FR-009); a text the service does
   not return never replaces a stored text (data-model.md; spec FR-009).

## Portions and review pauses

One commit each, with its tests and `scripts/check.sh`; stop after each for
the maintainer's review and compare the size with the table above.

1. **The list as a list view.** The two form changes, the row object and
   the row template, the update by difference keeping the selection and
   the reader, the list
   read without text and the content read on opening, against today's
   loads (newest 100, clear and refill). A render of the list for the
   maintainer. Suggested commit: "Show messages in a list view".
2. **The store for cycles.** The domain types, the schema, `store_batch`,
   `read_folder_sync`; 007 and 008 documents amended first; today's loads
   keep writing through `replace_mailbox`, adjusted to the schema, until
   portion 4 removes its last caller. Suggested commit: "Store folder
   batches".
3. **IMAP cycles.** The listing and rows by UID in `mailbag-imap`, the IMAP
   cycle for Generic IMAP and Gmail with related messages, load events and
   reads on batches, renewing Gmail's access after an ended session,
   `MoreAvailable` kept until portion 4; 002, 004 and 006 amended first.
   Suggested commit: "Synchronize IMAP folders".
4. **Microsoft 365 cycles.** The probe of Decision 4; the delta pages with
   partial entries, texts by date range, the saved place, the rejected
   position, renewing the token after a 401, the 60-second wait;
   `MoreAvailable` and `replace_mailbox` removed with their last callers;
   005 and 006 amended first. Suggested commit: "Synchronize Microsoft 365
   folders".
5. **Polish.** "Not downloaded" wording checked in the reader, the record's
   counts, remaining document alignments, the quickstart on the installed
   build. Suggested commit: "Finish synchronization".

After portion 5: the GUI tests one by one, `simplify-review` on the branch
diff in a fresh subagent, then the manual checks of
[quickstart.md](quickstart.md).

## Technical Context

**Language/Version**: Rust 1.95, edition 2024, as the workspace.
**Primary Dependencies**: gtk4 0.11, libadwaita 0.9, glib/gio 0.22; the
async-imap and imap-proto forks at their pinned revisions; soup3 0.9;
rusqlite 0.40 without default features. No new crate.
**Storage**: the 007 store with 008's tables, whose structure changes; an
existing store is discarded at start (007 FR-012).
**Testing**: `cargo test` with the scripted IMAP server (the listing, UID
rows, EXPUNGE during a command), the scripted Graph service (delta pages,
a 410 or another 4xx answering a saved link, date-range texts), the store in memory, and
the GTK tests one per process.
**Target Platform**: GNOME desktop, native and Flatpak.
**Project Type**: desktop application.
**Performance Goals**: spec SC-001 (first rows within 5 s), SC-002 (nothing
changed within 3 s), SC-007 (100 000 rows listed), SC-009 (quit within 1 s).
**Constraints**: no store access on GTK's thread; one load at a time; no
thread, timer or queue of the feature's own; the approved layout unchanged
except the two form changes.
**Scale/Scope**: folders up to 100 000 messages; batches of 100.

## Constitution Check

- **I. Necessary complexity only**: each step of the minimal version serves
  a spec requirement; CONDSTORE, larger batches, stopping a running cycle
  and renewing before expiry are listed as optional with their situations.
  The count check and the unread text rule were removed at the spec
  challenge; `MoreAvailable` goes with its last producer, and
  `MailboxChanged` keeps only its reconnect producer.
- **II. Clear language and concrete names**: `FolderBatch`,
  `FolderState`, `MessageListRow`, `store_batch`, `read_folder_rows`,
  `list_messages`, `read_message_changes`, `synchronize_imap_folder`; the
  spec's *cycle* and *batch* are defined in its Scope.
- **III. Explicit failures and truthful state**: removal only with proof;
  a refused listing is an incomplete list; "not downloaded" is shown, never
  an empty message; a stopped cycle keeps its stored batches and says so
  while Mailbag is open.
- **IV. One owner per business rule**: the store owns how a batch is
  written; each provider cycle owns how its server's changes become
  batches; the 30-day rule lives in one function.
- **V. Responsive, bounded work**: the store is read and written off GTK's
  thread; the list builds only visible rows; one connection at a time;
  structures and texts asked for a hundred messages at a time; quitting
  waits for nothing.
- **VI. Evidence before completion**: scripted scenarios for SC-003 to
  SC-006 and cancellation; GTK tests for the list; the installed-build
    checks of the quickstart, including quitting during a fill.
- **VII. Gmail first** (added 2026-10-05): CONDSTORE on Gmail by the
  announced capability (spec Clarifications 2026-10-04), measured on
  folders under 800 messages; the maintainer ruled on 2026-10-05 that it
  stands (Clarifications 2026-10-05).
- **VIII. A fact about a message comes from the server's report of it**
  (added 2026-10-05): the state pass's numbers decide how much is listed,
  never what is proven; a command is confirmed by the flags read after it
  (011 FR-007(d)); on Microsoft 365 a round's entry names a stored
  message and the message is read (FR-007).

No violation to justify.

## Project Structure

### Documentation (this feature)

```text
specs/009-synchronization/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/synchronization.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code

```text
crates/
├── mailbag-domain/src/lib.rs          # FolderState, FolderBatch, MessageListRow
├── mailbag-imap/src/                  # reader.rs listing and rows by UID
├── mailbag-graph/src/                 # lib.rs, reply.rs: delta, texts, one message
├── mailbag-providers/src/             # cycle.rs, cycle/imap.rs, cycle/graph.rs, renewal.rs, imap_texts.rs, load.rs (new or renamed), worker.rs, store_load.rs
├── mailbag-store/src/                 # schema.sql, lib.rs, folders.rs, content.rs
└── mailbag/
    ├── resources/ui/                  # mailbag.ui, message-row.ui
    └── src/                           # window_ui.rs, mail_ui.rs, mail_ui/message_item.rs (new), failure_declarations.rs
```

## Documents amended before implementing

In the portion that needs them, before its code (AGENTS.md): portion 1:
002 contracts/ui.md, 007 FR-005, 008 FR-010 (the list view, the reader
kept open); portion 2: 007 FR-003, FR-004, FR-010, FR-014 and its data
model, 007 FR-002, 008 FR-004 (with the Generic IMAP identity), FR-007,
FR-012, FR-013(b), Key Entities, Assumptions, SC-002, its data model and
contract; portion 3: 002 FR-002, FR-003 and
contracts/imap-reading.md, 004 FR-003 (one attempt after an ended
session) and its deferred Gmail model,
006 User Story 3; portion 4: 005 FR-002, FR-003, FR-006, FR-008 (renewal
after a 401, pages), 006 (`MoreAvailable`); portion 5: 006 FR-008 (the
banner during a refresh, at the acceptance). Each amended document gets a
status line naming this feature.

## Post-implementation

The manual checks of [quickstart.md](quickstart.md) ran on the installed
Flatpak build on 2026-09-29 and 2026-09-30 with a Generic IMAP, a Gmail
and a Microsoft 365 account; all ten steps pass after these fixes, found
at the acceptance:

- On Microsoft 365 the newest message of each page of a first reading got
  no text: the service keeps dates finer than the seconds it shows, so a
  range ending at the latest shown second left that message out. The
  range now ends a second later (research §5).
- The banner stayed over the rows while a refresh of the same mail ran; it
  now goes while the refresh runs and returns only if it fails or ends
  short (006 FR-008). The failure page already behaved so.

Observed and kept: a Gmail message's read state in All Mail follows a
change made after it left the refreshed label only when All Mail itself
is refreshed, since a cycle learns only its own folder's messages (019
refreshes every folder). A lost network is noticed on IMAP after the
30-second socket limit of 002, on Microsoft 365 at once, since each page
is a new request. A first fill on one IMAP server reads about 90 ms per
older message (research §3); Microsoft 365 pages that took 20 s, once
81 s, were the service's load at that hour (research §5). A text-only
edit of a Microsoft 365 draft comes as a listed entry and gets the new
text (research §5).

## Amendment 2026-10-02: one connection, faster

Decided at a feature-start on 2026-10-02 after the message list's
acceptance (research §14), built on `claude/connections` in three
portions, each reviewed and committed by the maintainer, then checked on
the installed build with accounts of each provider. No behaviour changed
for the user beyond speed; FR-009's refusal wording follows the one
command.

| Item | Budget | Came out |
|---|---|---|
| Production lines | ≤ 220 changed, net ≤ +150 | ≈ 365 changed, net +73 after the simplify-review (the retry apart after a refused row command was not in the estimate); accepted |
| Files touched | imap `transport.rs`, `session.rs`, `reader.rs`, `fetch_responses.rs`, `lib.rs`, `test_server.rs`; providers `imap_texts.rs`, `cycle/imap.rs`; `Cargo.toml`, `Cargo.lock`, `cargo-sources.json` | as planned |
| Threads, timers, dependencies | 0 | 0 |
| New types | one handle on the stream for the compression swap | none: the IMAP library hands out its stream (`Connection::get_mut`), `GioStream::compress` swaps in place |
| Fork commits | 1 (imap-proto) | 1: `NIL` encoding read as 7BIT |
| Other features' documents | 002 contracts/imap-reading.md, 004 research | amended after the live check |
| Tests | ≤ 250 lines | ≈ 270: compression (3), the `NIL` encoding (1), the row command with structures and its retry apart (rewritten) |

Function map changes: `MailboxReader::fetch_structures` is gone;
`fetch_rows_by_uid` asks for the structures and, when the command does not
answer for every message, `read_unanswered_apart` reads their rows and
then each structure alone (`fetch_structures_apart`, the former isolation
loop); `read_contents` takes the rows. `sign_in_session` gained
`compress_session` after the capabilities. The live check's numbers are
in research §14.

## Amendment 2026-10-04: the state pass

Decided at a feature-start on 2026-10-04 after the live check of
[Read and star](../011-read-and-star/spec.md) (research §15; spec
Clarifications 2026-10-04), for branch `claude/state-pass`, from
`claude/read-star`, whose full listing after a cycle's commands this
amendment replaces. Documents first (tasks T039), then four portions.
Built on 2026-10-04 (tasks T040–T045, four commits) and checked on the
installed build the same day (T046); the final passes' findings (T047)
this section records as "built as".

### Size

Budget proposed on 2026-10-04: at most 250 production lines and 400 test
lines; no thread, timer or queue of the feature's own; no new dependency;
no change to the IMAP library forks (checked: the pinned async-imap
already carries `exists`, `uid_next`, `uid_validity`, `highest_modseq` and
`select_condstore`, and `uid_fetch` takes the query text as given); four
columns on `folder`; the window and Microsoft 365 untouched.

| Item | Budget | This amendment (estimate) |
|---|---|---|
| New modules and production lines | ≤ 250 net | ≈ 200 (≈ 185 before the challenges of 2026-10-04, which added the pending changes to the pass's plan and the conversion of the opening's numbers): `mailbag-imap` ≈ 60 (the opening with the CONDSTORE parameter when announced, the opening's numbers, opening the same folder again, the listing of changed flags sharing the listing's body); `mailbag-domain` ≈ 20 (`FolderNumbers`, a field of `FolderState`, its privacy-safe `Debug`); `mailbag-store` ≈ 20 (four columns read and written with the state); `mailbag-providers` ≈ 100 net (the pass's plan with three outcomes and the pending changes, running a pass and storing its numbers, the second pass at the end, the numbers converted ≈ 130; `confirm_sent_changes` and the listing's state helper −30) |
| Call sites or existing files touched | — | imap `session.rs`, `reader.rs`, `lib.rs`, `test_server.rs`; domain `lib.rs`; store `schema.sql`, `folders.rs`, `lib.rs`; providers `cycle/imap.rs`, `cycle/pending.rs` (the confirm's caller), tests |
| New threads, timers, queues | 0 | 0 |
| New state, types, error types | — | `MailboxNumbers { uid_validity, message_count, uid_next, highest_modseq }` in `mailbag-imap`, what the opening returned, and `FolderNumbers` of the same shape in `mailbag-domain`, what the folder stores, the cycle converting, since the IMAP crate does not depend on the domain crate (as 011's `StoreFlag`); the pass's plan (nothing, the changed flags, every message); `ImapFailure::MailboxChanged`, which the reader already raises when a reconnection meets another numbering version, raised likewise when the second opening does |
| New fields in existing data | 4 columns | `folder.uid_validity`, `folder.message_count`, `folder.uid_next`, `folder.highest_modseq` ([data-model.md](data-model.md)) |
| Changes to other features' contracts or documents | 011, 002, 007 | 011 FR-007(e), plan and research (the confirm is the second pass); 002 contracts/imap-reading.md (the opening and the listing); 007 FR-014(a) |
| New dependencies | 0 | 0 |
| Tests | ≤ 400 | ≈ 385 (≈ 330 before the challenges, which measured the cycle tests at 30–60 lines each): the scripted server ≈ 85 (UIDNEXT in the opening, a CONDSTORE knob with HIGHESTMODSEQ, per-message mod-sequences raised by `UID STORE` and by a scripted change, `CHANGEDSINCE` cut off the items before `split_items`, a message gone after the first listing, a flag changed between connections); store ≈ 30; imap ≈ 50; providers ≈ 220 (SC-011's outcomes as successive cycles on one fixture, SC-012 apart, a pending star on a quiet CONDSTORE server, the 011 tests of the listing after commands rewritten for the second pass) |

### Minimal version

| Step | What it does | Cost |
|---|---|---|
| The folder's numbers | `FolderNumbers` in the domain, converted from the IMAP crate's `MailboxNumbers`; four nullable columns written with the folder's state and read with it; kept through a folder list replacement | ≈ 40 |
| The reader | `open` selects with `(CONDSTORE)` when the signed-in capabilities announce it; `numbers()`; `reopen()` selects the same folder again in the session; `list_changed_flags(since, row_items)`, `UID FETCH 1:* (UID FLAGS [X-GM-MSGID]) (CHANGEDSINCE since)`, streamed as `list_messages` | ≈ 60 |
| The pass | `pass_plan(numbers, reference, pending)`: nothing, the changed flags, or every message; the plan runs its listing, stores removals (a complete listing of every message only), flag states and the numbers in one batch, and hands back the listed and the missing messages | ≈ 75 |
| The cycle | open → read → pass → end the wishes the listing shows → send → batches with sends → when batches were fetched or commands sent: reopen, the pass again against the first pass's numbers (which confirms no command since the reading after the command of 2026-10-04, 011 FR-007(d); until then it settled the sent changes it shows, the folder left not completed when that pass listed messages it did not fetch → finish | ≈ 55 net |

### Function map changes

- `session::select_mailbox` (built as `open_selected`, which the opening
  and `reopen` share, tasks T042): `select_condstore` when `capabilities` include
  CONDSTORE, else `select`; `MailboxSession` keeps the opening's numbers.
- `MailboxReader::numbers() -> MailboxNumbers`; `reopen() -> Result<(),
  ImapError>`: when the session was closed after an unreadable structure
  it reconnects as every command does, otherwise it runs the same SELECT
  on the open session (RFC 3501 §6.3.1: a SELECT deselects first); it
  replaces the numbers and the message count, and a numbering version
  other than the first opening's is `ImapFailure::MailboxChanged`, the
  rule `reconnect` already owns; `list_changed_flags(since: u64,
  row_items) -> FolderListing` shares `list_messages`' body (the query
  differs by the `CHANGEDSINCE` modifier) and the EXISTS = 0 shortcut; its
  `messages` are the changed ones only.
- `cycle::imap::pass_plan(numbers: &MailboxNumbers, reference:
  Option<&FolderNumbers>, pending: &[PendingChange]) -> PassPlan` (built
  as `pass_plan(FolderNumbers, Option<FolderNumbers>, bool)`, the
  opening's numbers converted once by the caller):
  `Everything` when there is no reference, the numbering version differs,
  a number is missing on either side, or `pending` is not empty (011
  addresses a change by the UID the listing shows); `Nothing` when all
  four numbers are equal; `ChangedFlags { since }` when EXISTS and UIDNEXT
  are equal and both HIGHESTMODSEQ are present. The reference is the
  stored numbers of a synchronized folder, or, at the second pass, the
  first pass's numbers when its listing completed and every message it
  showed missing was stored; the second pass gets no pending changes:
    since the reading after the command (2026-10-04) a command is confirmed
  by the flags read right after it (011 FR-007(d)), and the pass learns
  the folder's own change (until then
  the pass confirmed the commands, which one that changes nothing
  escaped). The pending changes read for the plan are the ones
  `end_changes_the_listing_shows` compares with the listing, read once.
- `cycle::imap::run_state_pass(server, reference, batches, plan) ->
  StatePass { listed: Vec<ListedMessage>, missing: Vec<&ListedMessage>,
  refusal }` (built as `run_state_pass(server, stored, numbers, batches,
  plan) -> StatePass { listed, refusal }`; the missing messages borrow
  from `listed`, so the caller computes them again from it): `Nothing`
  lists nothing and writes the completed state only when the folder is
  not yet marked so with these numbers, otherwise nothing; `ChangedFlags`
  calls `list_changed_flags` and stores flag states with the numbers,
  removing nothing; `Everything` calls `list_messages` and stores what
  `listing_changes` proves, removals only when the listing completed. A
  completed listing always writes the folder's state with the numbers:
  synchronized when nothing is missing, not completed otherwise (today's
  helper writes none for a synchronized folder with nothing missing, so
  the numbers would never advance); a refused listing writes the state as
  today and leaves the numbers as stored (built as: the numbers travel
  only with the completed state, and a not-completed state carries none,
  since nothing reads the numbers of a folder not completed; maintainer's
  decision at the final review, 2026-10-04).
- `cycle::imap::synchronize_imap_folder`: the order above;
  `confirm_sent_changes` goes, its settle of the sent changes
  (`settle_sent_changes`) runs on the second pass's listed messages
  (superseded later the same day: `settle_sent_changes` goes too and the
  sending step settles by the flags it reads, 011 plan "Amendment
  2026-10-04 (later)"); a
  second pass that listed messages it did not fetch ends the cycle with
  the folder not completed, so the next cycle lists and fetches them.
- Record lines: the pass's outcome at info ("nothing changed", "changed
  flags listed", "folder listed") with counts; the numbers at debug.

### Decisions for the maintainer

1. **CONDSTORE where the server announces it, Gmail included** (spec
   Clarifications 2026-10-04): Google's documentation does not describe
   it; the capability does, and for COMPRESS=DEFLATE this feature already
   lets the announced capability decide (research §14). Alternative: the
   base method on Gmail, which then gets only the EXISTS and UIDNEXT check
   and the second pass, never the flag skip. Decided on 2026-10-04: the
   capability decides.
2. **The second pass only after batches or commands**: a synchronized
   folder with nothing missing and nothing sent ends with its first pass.
   Alternative: a second pass always, one round trip more per refresh for
   nothing. Recommended as written.
3. **No cadence inside the cycle** (spec Clarifications 2026-10-04):
   decided on 2026-10-04.

After the challenges of 2026-10-04 (two fresh sessions: the spec's
requirements, the plan's mechanisms), applied the same day: a first pass
of a folder with pending changes lists every message, since the changes
are addressed by the listing's UIDs; a second pass that lists messages
it does not fetch leaves the folder not completed, since its stored
numbers would otherwise hide them from every later pass; the numbering
version joins every comparison; the second pass compares with the first
pass's numbers, so an undisturbed fill ends with one round trip; `reopen`
reconnects first when the session was closed; the opening's numbers are
the IMAP crate's own type, converted by the cycle; a completed listing
always writes the state with the numbers; the NOMODSEQ case is a two-line
knob of the scripted server (with the completion text alone it would
still send HIGHESTMODSEQ, found in portion 8c); the providers' tests are
estimated at the measured 30–60 lines each.

### Optional mechanisms

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Arrivals alone by `UID FETCH <stored UIDNEXT>:* (UID FLAGS …)`, removals ruled out when EXISTS grew by exactly the arrivals | New mail is the most common change, and on a large folder each arrival costs the whole listing today; measured at 0.6 s for 9 000 messages, estimated at seconds for 100 000 (research §15) | ≈ 30 lines |
| A flags fetch of the sent UIDs alone as the second pass's listing on a server without CONDSTORE | The second pass lists every message's flags there after each refresh with a command; 0.6 s for 9 000 messages | ≈ 25 lines |
| Fetching within the cycle the arrivals the second pass finds | A long fill ends with mail that arrived meanwhile listed but not fetched until the next cycle, the folder left not completed | ≈ 15 lines; or 020's slicing |
| Addressing a pending Gmail message by `UID SEARCH X-GM-MSGID` when the pass lists nothing | Every refresh after the user read or starred a message lists the folder in full; on a large Gmail folder that is the whole listing's cost per such refresh (a Generic IMAP identity carries its UID and needs no search) | ≈ 40 lines; about 0.2 s per pending message on Gmail (011 research §2) |

### Portions

1. **Documents** (T039): this amendment, spec, research §15, data model,
   contract, quickstart, the 011, 002 and 007 amendments; the maintainer's
   decision 1; then `spec-challenge` in a fresh session. *Pause.*
2. **The folder's numbers** (T040–T041): domain, schema, store. *Pause.*
3. **The reader** (T042–T043): the opening, the numbers, `reopen`, the
   changed-flags listing, the scripted server's CONDSTORE. *Pause.*
4. **The cycle** (T044–T045): the pass, the second pass, the 011 tests
   rewritten; SC-011, SC-012. *Pause.*
5. **Final passes** (T046–T047): check, size against the budget, the
   quickstart's steps 11–12 on the installed build, one consistency
   analysis, `simplify-review` in a fresh session.
