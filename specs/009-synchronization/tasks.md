# Tasks: Synchronization

**Feature**: `009-synchronization`
**Created**: 2026-09-28 · **Branch**: `claude/sync` · **Status**: Documents
approved on 2026-09-29 (T001); portions 1–4 committed on 2026-09-29;
T033–T036 done, the manual checks passed on the installed build on
2026-09-30; T037 reported the same day; T038, the documents after the
independent review, done on 2026-09-30. The amendment of 2026-10-02 (one
connection, faster) was built without tasks, by the feature-start's path
for internal changes: three reviewed portions, the installed-build check,
then the documents (plan "Amendment 2026-10-02", research §14). The
amendment of 2026-10-04 (the state pass) has its tasks in Phase 8, on
branch `claude/state-pass`; its documents were approved, portions
8b–8d built and the final passes T046–T047 done the same day.

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table, the
function map and the portions, [research](research.md) owns the decisions
with alternatives, the [data model](data-model.md) owns the stored form,
[contracts/synchronization.md](contracts/synchronization.md) owns the shared
definitions, [quickstart](quickstart.md) owns the manual acceptance. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one
portion, run its checks, compare the size with the plan's table, report and
stop. The maintainer creates commits and PRs. Do not start code before
document approval. Nothing committed may name where an idea came from
outside this repository, nor any account or server of a person.

Phases follow the plan's batches. Story labels trace tasks to US1 (the
whole folder arrives, newest first), US2 (later refreshes bring only what
changed), US3 (nothing disappears without proof), US4 (an interrupted first
fill continues), US5 (recent mail can be read offline) and US6 (an
account's mail follows Online Accounts). Tests are part of every batch
and live beside their modules; no test pins wording except the privacy
invariants.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(sync): specify and plan synchronization | Synchronization |
| 1. The list as a list view | T002–T008 | feat(ui): show messages in a list view | Synchronization |
| 2. The store for cycles | T009–T014 | feat(store): store folder batches | Synchronization |
| 3. IMAP cycles | T015–T024 | feat(sync): synchronize IMAP folders | Synchronization |
| 4. Microsoft 365 cycles | T025–T032 | feat(sync): synchronize Microsoft 365 folders | Synchronization |
| 5. Polish | T033–T037 | (per review) | Synchronization |
| Convergence | T038 | docs(sync): align documents after the independent review | Synchronization |

## Phase 1: documents and review

- [x] T001 STOP: present specs/009-synchronization/ (spec.md, plan.md,
  research.md, data-model.md, contracts/synchronization.md, quickstart.md,
  checklists/requirements.md, this tasks.md); wait for explicit maintainer
  approval before any code change.

## Phase 2: the list as a list view (portion 1)

Goal: the messages list builds only its visible rows and updates by
difference, and a message's content is read when it is opened; loads still
deliver the newest 100 and replace the folder. On screen, the only change
is that the open message stays open when a load leaves it in the list.

- [x] T002 [US1] Amend the documents first: specs/002-imap-integration/
  contracts/ui.md (the messages list is a list view with a row template;
  the row's read state and hidden preview live in the template),
  specs/007-mail-storage/spec.md FR-005 and specs/008-folders/spec.md
  FR-010 (the reader stays open while its message is listed, spec FR-013),
  and 006's acceptance scenario where Retry closes the reader; each with a
  status line naming this feature. Then, in crates/mailbag-domain/src/
  lib.rs: `MessageListRow { identity, fields: DisplayFields, received_unix,
  seen }` with a privacy-safe `Debug` (identity and seen only), as the
  contract describes.
- [x] T003 [US1] [US5] In crates/mailbag-store/src/lib.rs and
  src/folders.rs: `read_folder_rows(folder) -> Option<Vec<MessageListRow>>`
  (today's order by position, no `content_detail` read; `None` when the
  folder is not loaded) and `read_message_content(account, identity) ->
  Option<ReceivedContent>`; `read_mailbox` stays until nothing calls it,
  then goes in this portion; tests in crates/mailbag-store/src/tests.rs
  for both reads (order, no text in rows, a missing message is `None`).
- [x] T004 [US1] Forms, following Workbench's "List View" demo: in
  crates/mailbag/resources/ui/mailbag.ui the `messages` `GtkListBox`
  becomes a `GtkListView`; crates/mailbag/resources/ui/message-row.ui
  becomes `<template class="GtkListItem">` whose child is the approved row,
  its labels bound to `MessageItem`'s `sender`, `subject`, `date-text`, the
  dot's visibility to `unread`, the list item's accessible description to
  `read-state-text`, and `preview` hidden in the template (research §9);
  spacing, margins and style classes unchanged; the row's inner widgets
  not focusable, so the focus rests on the list's row. The window builds
  the `GtkBuilderListItemFactory` from the form's bytes
  (`include_bytes!`, as every form is loaded) and sets it on the list
  view. Open message-row.ui in
  Cambalache and record in research §9 whether it edits the template and
  its bindings.
- [x] T005 [US1] [US2] In crates/mailbag/src/mail_ui.rs and a new
  src/mail_ui/message_item.rs: `MessageItem` (a GObject with the
  properties of T004 and `identity`, registered before the forms are
  built), made from
  `MessageListRow` with today's text rules (`sender_text`, `subject_text`,
  `received_date_text`, inert text); `MailUi` holds the list view over a
  `GtkSingleSelection` set in code; `show_rows(account, rows)` calls
  `update_list_by_difference`: set `unread` of listed items in place, keep
  the common start and end compared by identity and every shown field,
  splice the middle, then find the open message again by its identity and
  select its row, or close the reader when its message is gone; one click
  or Enter opens the message by its identity, the arrow keys move the
  focus (research §9, decided 2026-09-29).
- [x] T006 [US1] [US5] In crates/mailbag/src/window_ui.rs: the shown
  mailbox is read with `read_folder_rows`; opening a message reads
  `read_message_content` on GIO's pool (a numbered read; an answer for
  another message or account is dropped; a failure is shown in the
  reader's status page with Retry reading the stored mail again, 007
  FR-013's operation); the reader shows the content as today.
- [x] T007 [P] [US1] [US2] Tests: unit tests of `update_list_by_difference`
  (append at the end, insert at the top, removal in the middle, read state
  in place, nothing changed) in crates/mailbag/src/mail_ui/tests.rs; the
  GTK test `mailbox_navigation` extended: a second load that changes one
  row's read state keeps the selection and the open reader, and a load
  that removes the open message closes the reader; a model of 100 000
  items is built and the list scrolled to its end without the test's main
  loop stalling (SC-007; the time is written to the test's output, not
  asserted); a render of the list
  before and after (offscreen paintable to PNG, outside the repository)
  and a keyboard check (arrows move the focus, Enter opens, the focused
  row is outlined) for the maintainer.
- [x] T008 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  one per process; compare the size with plan.md (window ≈ 200, store
  ≈ 40, domain ≈ 15); show the render; report, suggest the commit and wait
  before portion 2.

## Phase 3: the store for cycles (portion 2)

Goal: the store holds folder state and writes batches; loads keep
working through `replace_mailbox`, adjusted to the new schema.

- [x] T009 Amend the documents first: specs/007-mail-storage/spec.md
  (FR-002 not downloaded; FR-003 folder state built as the saved position
  and completion, the numbering version in the Generic IMAP identity, IMAP
  UIDs on relations still deferred; FR-004, FR-010 and FR-014(a), (b), (e),
  (f) pointed at this feature) and its data-model.md;
  specs/008-folders/spec.md (FR-004 with the Generic IMAP identity
  `imap:<folder>/<UIDVALIDITY>/<UID>` and the moved-message lag kept until
  the old folder's next cycle, FR-007, FR-012, FR-013(b), Key Entities'
  membership position and labels, Assumptions' newest 100, SC-002),
  data-model.md and contracts/folders.md; each with a status line naming
  this feature.
- [x] T010 [US1] [US2] [US3] [US5] In crates/mailbag-domain/src/lib.rs:
  `FolderState`, `FolderBatch { removed, read_states, known_arrived,
  arrived, state }`, `ReceivedContent::NotDownloaded`; privacy-safe `Debug`
  (counts and identities only); the reader's wording for `NotDownloaded`
  in crates/mailbag/src/failure_declarations.rs, `declare_content` ("The
  text of this message was not downloaded." with a short reason about the
  last 30 days; no action; impersonal, AGENTS.md "UI wording").
- [x] T011 [US1] [US4] In crates/mailbag-store/src/schema.sql and
  src/content.rs: `folder.server_position`, `folder.fill_place`,
  `folder.synchronized` (was `loaded`); no numbering version column; `membership` without `position`;
  content code `not_downloaded`; check the rows read with `EXPLAIN QUERY
  PLAN` on a store of 100 000 rows and record in research §6 that no index
  is needed, or add the one the plan shows.
- [x] T012 [US1] [US2] [US3] [US4] In crates/mailbag-store/src/lib.rs and
  src/folders.rs: `read_folder_sync(folder) -> FolderSync`,
  `stored_identities(account, identities) -> HashSet<String>`,
  `store_batch(folder, batch, load_cancelled) -> StoreWrite` in the
  order of data-model.md (a full record never replaces a stored content
  with `NotDownloaded`, nor a stored text with `TextNotReturned`, the
  latter since the independent review of 2026-09-30; the state written
  when the batch carries it,
  including the first batch's "not completed"), `read_folder_rows`
  ordered by received date then id;
  `replace_mailbox` adjusted (no position, sets `synchronized`) and kept
  until portion 4.
- [x] T013 [P] [US1] [US2] [US3] [US4] Tests in
  crates/mailbag-store/src/tests.rs: a batch with removals, read states,
  known arrivals and arrivals round-trips; orphans go; a stored text
  survives a `NotDownloaded` arrival from another folder, and a
  `TextNotReturned` record (independent review, 2026-09-30); the state is
  written only when the batch carries it; a cancelled batch writes
  nothing; a failing batch leaves the previous state whole; rows newest
  first by received date; `None` for a folder never synchronized and
  without rows, an empty list for a synchronized empty folder;
  `stored_identities` over two folders of one account; a folder marked
  not completed with no rows reads as `None`; a store of 008's structure
  is discarded at start.
- [x] T014 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md (domain ≈ 55, store ≈ 175); report, suggest the commit
  and wait before portion 3.

## Phase 4: IMAP cycles (portion 3)

Goal: Refresh Mailbox on a Generic IMAP or Gmail folder runs a cycle; the
window follows its batches; Microsoft 365 still loads its newest 100.

- [x] T015 Amend the documents first: specs/002-imap-integration/spec.md
  (FR-002, FR-003) and contracts/imap-reading.md (the `1:*` listing, rows
  by UID, a vanished message skipped), specs/004-gmail-integration/spec.md
  (FR-003: after a session the server ended, one attempt with a different
  token, which may rarely follow an end for Gmail's limits, and a second
  end keeps Gmail's reason; the deferred Gmail model of All Mail with
  labels replaced by label folders), and specs/006-error-handling (User
  Story 3: a refused listing of a cycle; `MailboxChanged` kept for a
  reconnect; "Text not received" without Retry), each with a status line
  naming this feature.
- [x] T016 [US1] [US2] [US3] In crates/mailbag-imap/src/reader.rs,
  src/fetch_responses.rs and src/lib.rs: `MailboxReader::list_messages(
  row_items) -> FolderListing` (`UID FETCH 1:* (UID FLAGS)`, with
  `X-GM-MSGID` for Gmail rows, read from the stream into `ListedUid`
  records, a response without a UID skipped, `refusal` from a NO or BAD,
  an error for a lost connection; an empty listing without a command when
  the mailbox count is 0); `fetch_rows_by_uid(uids, row_items) ->
  MessageList` (the refusal kept; a missing UID left out, no
  `MailboxChanged`); a group of structures that all disappeared is
  skipped (reader.rs, `fetch_structures`); a refusal the server marks
  temporary (RFC 5530 `UNAVAILABLE`) of structures or texts fails the read
  instead of keeping the messages as unreadable (independent review,
  2026-09-30); the sequence-number
  `fetch_rows` and its row collection go; the reconnect's `MailboxChanged`
  stays; `ImapError` tells that the server ended the session with BYE
  (today `command_failure` in src/session.rs folds NO, BAD and BYE into one
  failure; research §13); the BYE mark was removed at the final review,
  2026-09-30, with the renewal of Gmail sessions.
- [x] T017 [P] [US1] [US2] [US3] In crates/mailbag-imap/src/test_server.rs
  and tests: the scripted server answers `UID FETCH 1:*` with flags and
  Gmail identifiers, EXPUNGE during the listing, a NO after some
  responses, a dropped connection, rows by UID with one UID missing, a NO
  after partial rows, an empty mailbox, and a BYE that ends an OAuth
  session after a given command with the next sign-in accepting a new
  token (removed at the final review, 2026-09-30); a `NO [UNAVAILABLE]`
  on structures or texts (independent review, 2026-09-30); tests for each.
- [x] T018 [US1] [US4] In crates/mailbag-providers/src/worker.rs,
  src/lib.rs and src/batch.rs (renamed load.rs in T034): `LoadEvent { BatchStored,
  Finished(LoadResult) }`; the outcome channel unbounded, received in a
  loop; `LoadsMail::start_load(…, on_event)`; the scripted loader of the
  GTK tests and crates/mailbag/src/window_ui.rs adapted (Finished as
  today's report).
- [x] T019 [US1] [US2] [US3] [US4] [US5] In a new
  crates/mailbag-providers/src/cycle.rs, with src/store_load.rs and
  src/imap_batch.rs (built as src/cycle/imap.rs, with
  `synchronize_imap_folder(access, identity_rule, renewal, batches)` and
  `IdentityRule` in place of the two identity functions; see the plan's
  function map): `synchronize_folder` choosing the provider once;
  `synchronize_imap_folder(access, options, identify, folder, store,
  events)` as the plan's function map (open, `read_folder_sync`, the
  listing, the batch of removals and read states when the listing
  completed, which marks the folder not completed when messages are
  missing, arrivals highest UID first in
  hundreds with `stored_identities` for known messages, rows by UID, texts
  within `recent_limit` through today's structure and text path in groups,
  `NotDownloaded` for older ones, the completed state); the Retry of
  `ReceivedContent::TextNotReturned` removed in
  crates/mailbag/src/failure_declarations.rs; the empty-batch
  `MailboxChanged` check in imap_batch.rs goes; `generic_identity`
  (`imap:<folder>/<UIDVALIDITY>/<UID>`, the version from the cycle's
  EXAMINE) and `gmail_identity` (a row without `X-GM-MSGID` left out and
  written to the record); a refused listing ends as `Stored { incomplete:
  ServerRefused }` and removes nothing; a refused row fetch after a
  complete listing ends the same way and keeps the removals the listing
  proved; neither saves the completed state; a temporary refusal of a
  batch's structures or texts fails the cycle as a temporarily
  unavailable server and stores none of the batch (independent review,
  2026-09-30); `store_batch` sends `BatchStored`.
- [x] T020 [US1] [US6] In a new crates/mailbag-providers/src/renewal.rs and
  src/lib.rs: the renewal channel of research §13: `MailLoader::start_load`
  gives the load a sender and answers requests on GTK's context with the
  load's own Online Accounts request; the Gmail cycle, on a BYE after its
  first successful command, asks once: with a different token it makes one
  attempt, opening the folder again with `MailboxReader::open` (since the
  final review `MailboxReader::reopen`), comparing `uid_validity()` (a
  change is `MailboxChanged`) and repeating the
  interrupted request (the listing or the batch); with the same token,
  or after a second end, the cycle ends with Gmail's reason; a cancelled
  load drops the request. At the final review (2026-09-30) a probe showed
  that Gmail checks the token only at sign-in; the Gmail half was removed
  and the channel serves Microsoft 365 only (research §13).
- [x] T021 [P] [US1] [US2] [US3] [US4] [US5] [US6] Tests in
  crates/mailbag-providers/src/tests.rs against the scripted server and an
  in-memory store: a first fill newest first in batches with texts only
  within 30 days; a second cycle that changes nothing and fetches no
  message; arrivals, read-state changes and removals; a removal reported
  during the listing; a refused listing removes nothing and does not
  complete; a complete listing followed by a refused row fetch keeps the
  proven removals, stores the rows received and does not complete; a
  connection lost during the listing removes nothing; a renumbered Generic
  IMAP folder (a new UIDVALIDITY with the same UIDs for other messages):
  the old rows leave, the new messages arrive, and no old row or text is
  attached to a new message; a renumbered Gmail folder matched by identity;
  (rows stored without a numbering version cannot remain: the store is
  discarded when its structure changes, 007 FR-012); a stopped refill leaves the folder "no mail
  loaded", not empty; a first fill of 10 000 scripted messages whose first
  batch is stored before the rest and a second cycle that fetches
  nothing (SC-001, SC-002; times written to the output, not asserted); a message gone between the listing
  and its rows; a Gmail message stored through another label is related
  without fetching; a first fill stopped and continued without fetching
  stored messages again; cancellation reported within a second while the
  server stops answering; a Gmail session ended mid-fill is renewed and
  the fill completes (SC-010), and a BYE with the token unchanged, or a
  second BYE, ends the cycle with Gmail's reason (both removed with the
  Gmail renewal, 2026-09-30); an account excluded
  mid-fill stores no later batch; a temporary refusal of structures or
  texts storing no row and naming the server as temporarily unavailable
  (independent review, 2026-09-30).
- [x] T022 [US1] [US4] In crates/mailbag/src/window_ui.rs: on
  `BatchStored` for any folder of the shown folder's account, read the
  shown folder's rows again while the rows and the banner on screen stay
  (a "read due" mark), as a completed load already rules today; a batch during a
  read marks one more read; the refresh actions stay unavailable while a
  cycle runs.
- [x] T023 [P] [US1] [US4] GTK test in crates/mailbag/src/mail_ui/tests.rs:
  the scripted loader stores batches and reports them; the list grows
  without losing the selection or the open message; a batch of another
  folder of the same account that changes a shared message's read state,
  followed by a failed end, updates the shown folder; and a failure banner
  from the previous refresh does not blink.
- [x] T024 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  one per process; compare the size with plan.md (imap ≈ 170, providers
  ≈ 330 so far, window ≈ 80); report, suggest the commit and wait before
  portion 4.

## Phase 5: Microsoft 365 cycles (portion 4)

Goal: Refresh Mailbox on a Microsoft 365 folder runs a cycle; the newest-100
path, `replace_mailbox` and `MoreAvailable` are gone.

- [x] T025 With the maintainer, the open check of research §5: a probe
  (outside the repository) starts a delta reading of the Inbox, stores its
  next link and waits; the maintainer makes changes that stay: marks one
  message read and leaves it read, moves one message to another folder,
  and edits a draft's subject; the probe finishes the reading and reads the
  next round; record in research §5 whether each change is reported by the
  continued reading, by the round after it, or not at all, and in which
  form (a partial entry or a listed one); if it is never reported, T029
  ends a continued first fill with a full re-reading; the maintainer undoes
  the changes afterwards.
- [x] T026 Amend the documents first:
  specs/005-microsoft-graph-integration/spec.md (FR-002: a token refused
  mid-cycle is asked for once more and a different one used; FR-003 and
  FR-008: pages are read, and the one repeat after renewal is the only
  retry; FR-006) and
  specs/006-error-handling (`MoreAvailable` and its wording removed; User
  Story 3's Microsoft 365 case), each with a status line naming this
  feature.
- [x] T027 [US1] [US2] [US3] [US4] In crates/mailbag-graph/src/lib.rs and
  src/reply.rs: `read_message_changes(service_url, token, from:
  ChangesFrom) -> ChangePage` (the first reading's `$select`, `$orderby`
  and `Prefer` headers, a link followed as given), entries parsed as
  `Removed`, `Listed` (every selected field present) or `Changed { id,
  is_read, other_fields }`; `GraphFailure::PositionRejected` for a 410 or a
  4xx whose `error.code` is `syncStateNotFound` without case (since the
  independent review of 2026-09-30: for a 410 or any other 4xx but 401 and
  429 answering a saved link);
  `read_texts_received_between` with `$top=500` and paging for a first
  fill's page; `read_message_text(id)` for a round of changes;
  `read_message` returning the message with its `parentFolderId`, a 404 as
  `None`; the wait limit at 60 s;
  `list_mailbox_messages` removed.
- [x] T028 [P] [US1] [US2] [US3] [US4] In crates/mailbag-graph/src/
  test_server.rs and src/tests.rs: delta pages with a next link and a
  delta link, removed, listed and partial entries, a repeated entry, a
  410 and a `syncStateNotFound` (since 2026-09-30: a 410, a 400 with any
  code, a 401 and a 429 answering a saved link, and a 400 answering a
  first reading), a date-range text page and its paging, a
  message read by id and a 404, a 401 followed by success with a new
  token; tests for each.
- [x] T029 [US1] [US2] [US3] [US4] [US6] In
  crates/mailbag-providers/src/cycle.rs and src/renewal.rs:
  `synchronize_graph_folder` as the plan's function map (`where_to_start`;
  pages as batches with entries merged per message; partial and unknown
  entries, and any entry for a message the account also holds in another
  folder, read with `read_message` and related to this folder only if its
  `parentFolderId` names it, and since the independent review of
  2026-09-30 leaving the folder otherwise, as does a message whose
  removal meets another entry for it in one page; texts of a first fill's
  page by date range,
  of a change round by identifier; the first fill's place saved with each
  page; a continued first fill reads one more round before completing; a
  rejected position or place starts a full reading that keeps the listed
  identities and removes the others at its end; the delta link saved with
  the completing batch; the outcome of T025 applied); a 401 asks for the
  token once and makes one attempt only with a different token (at the
  final review, 2026-09-30, no longer only after a first success: a token
  just handed out comes back the same); the same token, or a second 401,
  is the refused sign-in.
- [x] T030 [US1] Remove what the cycles replaced:
  `IncompleteList::MoreAvailable` (crates/mailbag-domain/src/lib.rs, its
  producer and its wording in crates/mailbag/src/failure_declarations.rs
  and their tests), `BATCH_SIZE` and the newest-100 path in
  crates/mailbag-providers (batch.rs, imap.rs, gmail.rs, microsoft365.rs,
  store_load.rs), and `replace_mailbox` in crates/mailbag-store.
- [x] T031 [P] [US1] [US2] [US3] [US4] [US6] Tests in
  crates/mailbag-providers/src/tests.rs against the scripted Graph service:
  a first fill page by page with texts only within 30 days; a first fill
  stopped and continued from its saved place; changes, repeated and
  reordered entries, removals, a partial entry changing a stored message's
  fields, a read-state entry for an unknown message; a message read in A,
  moved to B and marked unread there, B refreshed, then A's round with an
  old read-state entry and a removal: B's row stays unread; a round of
  changes whose two arrivals are a month apart reads two texts, not the
  month between; a continued first fill that reads one more round; a
  rejected position
  (rows kept, unlisted removed at the end) and a rejected place whose full
  reading stops and is started again in full (added at the final review,
  2026-09-30); a round without changes costing one page and no message
  (FR-012, final review); a removal met with another entry in one page,
  in either order, and a message read again that the service places
  elsewhere leaving the folder (independent review, 2026-09-30); a token
  refused mid-fill renewed once (SC-010), refused twice or answered with
  the same token is the sign-in failure.
- [x] T032 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  one per process; compare the size with plan.md (graph ≈ 210, providers
  ≈ 540 in all); report, suggest the commit and wait before the polish.

## Phase 6: polish

- [x] T033 [US1] [US2] Record lines (specs/003-logging rules): one line
  when a cycle ends, with counts of listed, removed, read-state changes,
  related, arrived and texts; folder names at debug only; no test pins the
  text.
- [x] T034 Align the documents with what was built: the spec's status, the
  plan's function map and size notes (no measured sizes written back), the
  data model, the contract, and the status lines of the amended specs.
  Rename the spec's *portion* to *batch* in the documents and the code
  (`FolderPortion`, `store_portion`, `PortionStored`), since *portion* also
  names a commit's share of the work (AGENTS.md); done here, after
  portion 4 removed the old newest-100 path (maintainer's decision
  2026-09-29).
- [x] T035 After the GTK tests one by one: `simplify-review` of the branch
  diff in a fresh subagent; bring findings that change behaviour or add
  scope to the maintainer.
- [x] T036 With the maintainer on the installed build: the steps of
  quickstart.md; results written to plan.md under "Post-implementation"
  without sizes.
- [x] T037 STOP: final report with the open items.

## Dependencies

- T001 before everything; each STOP (T008, T014, T024, T032, T037) waits
  for the maintainer.
- Portion 1: T002 before T003 and T005; T004 with T005; T006 after T003
  and T005; T007 after T005 and T006.
- Portion 2: T009 first; T010 before T011 and T012; T013 after T012.
- Portion 3: T015 first; T016 before T019 and T020; T017 with T016; T018
  before T019 and T022; T021 after T019 and T020; T023 after T022.
- Portion 4: T025 before T029; T026 first among the code tasks; T027
  before T029; T028 with T027; T030 after T029; T031 after T029 and T030.

## Parallel opportunities

- T003 ∥ T004 (store and forms); T007's unit tests ∥ the GTK test.
- T017 ∥ T018 (IMAP test server against the load events); T027 ∥ T025.
- T013, T021, T028, T031 each with the module they test once its
  signatures exist.

## Implementation strategy

Portion 1 changes the list on today's loads, so the new widget and the
update by difference are reviewed on known behaviour before any cycle
runs. Portion 2 stands on its own with the store's tests while loads keep
their old write. Portion 3 is the first visible synchronization, for IMAP
and Gmail; Microsoft 365 keeps its newest 100 until portion 4 removes the
old path together with its last caller. The size is compared with plan.md
at every STOP; a portion that will exceed its estimate by half stops before
doing so.

## Deferred, no tasks

Sending pending changes (built by 011, after the listing and not before
it, 011 FR-007) and the rules for moving and deleting (spec FR-015(a),
(b));
background synchronization, cycles in parallel and continuing a first fill
after the start (FR-015(c)); HTML, previews, download on opening and
retention (FR-015(d)); CONDSTORE (FR-015(e); built in Phase 8 on
2026-10-04); a Refresh that stops the
running cycle, larger rows-only batches and renewal before expiry (plan,
Optional mechanisms); upgrading a populated store (FR-015(f)).

## Phase 7: Convergence

- [x] T038 Align the documents with the changes of 2026-09-30 per tasks
  T034 and plan "Documents amended before implementing" (partial): add the
  "*Amended 2026-09-30 by Synchronization*" note at
  specs/002-imap-integration/spec.md FR-004 (a refusal the server marks
  temporary, RFC 5530 `UNAVAILABLE`, stores nothing and fails the cycle,
  009 FR-009); note the independent review's changes in T012 (a text not
  returned never replaces a stored text), T016 and T019 (a temporary
  refusal of structures or texts fails the cycle as a temporarily
  unavailable server), T027 and T028 (`PositionRejected` for a 410 or any
  other 4xx but 401 and 429 answering a saved link), T029 and T031 (a
  removal met with another entry in one page is read again; a message read
  again that the service places elsewhere leaves the folder; the
  round-without-changes test of FR-012); in plan.md, the function map
  (a message read again leaves the folder when it is elsewhere), the
  Technical Context (the scripted service's refusals of a saved link) and
  the Gmail clause of decision 7 marked superseded; in quickstart.md, the
  list of scripted cases (a removal met with another entry, a temporary
  refusal of structures or texts, a text not returned keeping the stored
  one); in spec.md FR-011, a cross-reference to FR-009's failure on a
  temporary refusal.

## Phase 8: the state pass (amendment of 2026-10-04)

Decided at a feature-start on 2026-10-04 (plan "Amendment 2026-10-04",
research §15, spec Clarifications 2026-10-04) on branch
`claude/state-pass`, from `claude/read-star`, whose full listing after a
cycle's commands this amendment replaces. Each portion ends with its
tests, `scripts/check.sh`, the size compared with the plan's table and a
STOP for the maintainer's review.

| Portion | Tasks | Suggested commit subject |
|---|---|---|
| 8a. Documents | T039 | docs(sync): specify the state pass |
| 8b. The folder's numbers | T040–T041 | feat(store): keep the numbers of a folder's latest state pass |
| 8c. The reader | T042–T043 | feat(imap): open with CONDSTORE and list changed flags |
| 8d. The cycle | T044–T045 | feat(sync): learn a folder's state in a pass, twice per cycle |
| 8e. Final passes | T046–T047 | (per review) |

- [x] T039 STOP (approved on 2026-10-04 after the two challenges): present the amended spec.md (status, Scope, US1,
  Edge Cases, Clarifications 2026-10-04, FR-001, FR-004, FR-005, FR-008,
  FR-012, FR-015(c) and (e), Key Entities, the flowchart, SC-011, SC-012,
  Assumptions, Amendments), plan.md ("Amendment 2026-10-04" and the
  CONDSTORE row of Optional mechanisms), research.md §15, data-model.md
  (`folder` columns and rules), contracts/synchronization.md
  (`FolderState`, `FolderNumbers`, the reader), quickstart.md (steps 11
  and 12, the scripted cases), and the amendments of 011 (spec FR-007(e)
  and Clarifications, plan, research §2), 002 contracts/imap-reading.md
  and 007 FR-014(a). Decision taken on 2026-10-04: CONDSTORE on Gmail by
  the announced capability (plan, decision 1). Then `spec-challenge` in
  fresh sessions, the spec's requirements and the plan's mechanisms; wait
  for approval before any code.
- [x] T040 In crates/mailbag-domain/src/lib.rs `FolderNumbers {
  uid_validity: Option<u32>, message_count: u32, uid_next: Option<u32>,
  highest_modseq: Option<u64> }` (the count named as the IMAP session's
  `message_count` is, since `exists` is an SQL keyword) and `FolderState.numbers:
  Option<FolderNumbers>` (every `FolderState { .. }` site of the workspace
  gains the field); in crates/mailbag-store/src/schema.sql the four
  nullable columns on `folder`; `read_folder_state` and
  `write_folder_state` carry them; replacing a folder list keeps them
  (`upsert_folders` leaves unnamed columns alone).
- [x] T041 [P] Store tests: the numbers round-trip with a batch's state,
  stay through a folder list replacement, and are absent for a folder
  that never had a complete pass.
- [x] T042 In crates/mailbag-imap: `MailboxNumbers` (the crate's own
  type, as `StoreFlag` is); `session::select_mailbox` selects with
  `select_condstore` when the signed-in capabilities announce CONDSTORE,
  else `select`, through `open_selected`, which a `MailboxSession` keeps
  the numbers and the CONDSTORE choice of; `MailboxReader::numbers()`;
  `reopen()` (reconnect first when the session was closed after an
  unreadable structure, else `open_selected` again on the open session;
  the numbers replaced; another numbering version is
  `ImapFailure::MailboxChanged` through `require_same_numbering`, which
  `reconnect` shares); `list_changed_flags(since, row_items)` sharing
  `list_messages`' body (`list`), the query gaining `(CHANGEDSINCE
  since)`. The scripted server: UIDNEXT in every opening's answer; a
  `condstore` knob (the capability; HIGHESTMODSEQ in the answer to a
  SELECT with the parameter, which the command log records as `SELECT
  (CONDSTORE)`; a mod-sequence per message, seeded in setup order and
  raised by `UID STORE`, by `seen_from_opening` and by an arrival;
  `CHANGEDSINCE` cut off the items before `split_items`, answered from
  the mod-sequences and recorded among the fetch's items); a `nomodseq`
  knob (two lines: with the completion text alone the server would still
  send HIGHESTMODSEQ); three knobs keyed by the mailbox's opening count
  over the server's life, `arriving_from_opening`, `gone_from_opening` and
  `seen_from_opening`, for SC-011's and SC-012's changes between two
  passes; `uid_validity_after_reconnect` renamed
  `uid_validity_from_second_opening`, keyed the same way (a reconnection
  is a second opening).
- [x] T043 [P] IMAP tests (src/tests/numbers.rs): the opening's numbers;
  SELECT with the parameter only when announced; `CHANGEDSINCE` lists
  only the changed messages and the full listing still lists every one
  (the Gmail items share the listing's code, tested in the Gmail tests);
  a NOMODSEQ opening leaves the mod-sequence empty; `reopen` refreshes
  the numbers after an arrival, a removal and a read elsewhere, and fails
  as `MailboxChanged` on another numbering version.
- [x] T044 In crates/mailbag-providers/src/cycle/imap.rs `pass_plan`
  (nothing, the changed flags, every message; every message also when
  there is no reference, the numbering version differs, a number is
  missing or pending changes exist, as the plan's function map says),
  `run_state_pass` (the plan's listing; removals only from a complete
  listing of every message; flag states; a completed listing always
  writes the state, synchronized when nothing is missing, with the
    numbers converted from the opening's; a refused listing leaves the
  numbers as stored (superseded by T047: a not-completed state carries
  none); the listed and the missing messages returned), and
  `synchronize_imap_folder` in the plan's order: open → read → pass (the
  pending changes read once, handed on to `end_changes_the_listing_shows`)
  → end the wishes the listing shows → send → batches with sends → when
  batches were fetched or commands sent: `reopen`, the pass again against
  the first pass's numbers (none when its listing was refused),
    `settle_sent_changes` on what it listed (superseded by 011 Phase 8: the
  pass confirms no command), the folder left not completed when it listed
  messages it did not fetch → finish;
  `confirm_sent_changes` goes. Record lines: the pass's outcome at info
  with counts, the numbers at debug. Built as planned, with `pass_state`
  owning the folder's state after a pass (not completed while messages
  are missing, completed with the numbers when none are and the listing
  completed, unless marked so already) and `folder_numbers` converting
  the opening's numbers; `end_changes_the_listing_shows` takes the
  pending changes the plan read. After the simplify-review (T047):
  `StatePass` carries `listed` and `refusal` only, the caller converts
  the opening's numbers once and hands them to `pass_plan` and the pass,
  and a `Nothing` pass goes through the one listing path with an empty
  listing, since an empty batch is not stored.
- [x] T045 [P] Cycle tests against the scripted server (src/tests/state_pass.rs), SC-011's
  outcomes as successive cycles on one CONDSTORE fixture: nothing changed
  → one SELECT, no listing; a flag changed → one `CHANGEDSINCE` holding
  that message alone, the flag stored, nothing removed; an arrival, then
  a removal → the listing of every message; a pending star with nothing
  changed on the server → the listing of every message and the command
  sent; a server without CONDSTORE → the listing as before; a folder
  whose fill did not complete → the listing; a changed numbering with the
  same count and next number → the folder refills. SC-012 apart: a first
  fill of 300 during which the scripted server changes a flag and removes
  a message → both stored at the fill's end, the second pass a listing
  of every message, since a message left; an undisturbed fill → a second pass of
  one SELECT; an arrival during the fill → the folder not completed, the
  next cycle fetches it. The 011 tests of the listing after commands
  rewritten for the second pass (Gmail Starred, the ignored UID, the
  refused second listing, the lost answer); the record names the pass's
  outcome. Built with two deviations: the changed-numbering case is a unit
  test of `pass_plan` in cycle/imap.rs, since the scripted server
  renumbers from the second opening on, which is the first fill's second
  pass; the four 011 tests held without change, their fixtures having no
  CONDSTORE; two older cycle tests now count the second pass's opening
  and listing.
- [x] T046 STOP: ./scripts/check.sh, the size against the budget (≤ 250
  production, ≤ 400 test lines), the quickstart's steps 11 and 12 on the
  installed build with the maintainer, the record checked for privacy
  (counts and the pass's outcome; folder names at debug). Done on
  2026-10-04: steps 11 and 12 passed (research §15 "Measured on the
  installed build"); the record carries counts and the outcome at info
    and the folder's name and numbers at debug; the size exceeded the
  budget, accepted by the maintainer.
- [x] T047 Final passes: one consistency analysis and `simplify-review` on
  the branch diff, each in a fresh session; findings reported, scope-adding
  ones brought to the maintainer; the measured facts stay in research §15.
  Done on 2026-10-04: no defect found; applied the behaviour-keeping
  simplifications (`pass_plan` over one type, one listing path in
  `run_state_pass`, the mod-sequence written as the same 64 bits, the
  finish record's `listed` as the listing's length), two test assertions
  (a refused listing keeps the stored state; a not-completed folder is
  listed whole) and the document alignment recorded in the plan's
  function map and here; decided by the maintainer the same day: a
  not-completed state carries no numbers (FR-005(c), `pass_state` with
  two cases), and FR-011 names the complete second pass after a refused
  first listing, which still reports the incomplete list.
