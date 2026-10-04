# Tasks: Read and star

**Feature**: `011-read-and-star`
**Created**: 2026-10-03 · **Branch**: `claude/read-star` · **Status**:
Documents written on 2026-10-02, challenged the same day (the spec's
requirements and the plan's mechanisms, in fresh sessions) and analysed
for consistency on 2026-10-03; approved on 2026-10-03 (T001) with the
toast for a change the store cannot write; T002 applied the same day.

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table, the
function map and the portions, [research](research.md) owns the decisions
with alternatives and the facts checked on live servers, the
[data model](data-model.md) owns the stored form, [quickstart](quickstart.md)
owns the manual acceptance. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one
portion, run its checks, compare the size with the plan's table, report and
stop. The maintainer creates commits and PRs. Do not start code before
document approval. Nothing committed may name where an idea came from
outside this repository, nor any account or server of a person.

Phases follow the plan's portions, with one change of order against the
plan's first draft: the read side of the flags (the server's star parsed
from IMAP listings and rows and from Microsoft 365 entries) joins portion
2, so that the store holds a true star from the first portion on and the
cycles compile against the new batch shape; portion 3 holds the write
primitives. Story labels trace tasks to US1 (a change lasts and reaches
the server), US2 (reading marks read, for good), US3 (a change during a
long refresh), US4 (the same change on every provider) and US5 (the
server refuses, the connection breaks). Tests are part of every portion
and live beside their modules; no test pins wording except the privacy
invariants.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| 1. Documents | T001–T002 | docs(flags): specify and plan read and star | Read and star |
| 2. Stored flags | T003–T010 | feat(store): keep the star and the user's pending flag changes | Read and star |
| 3. The wire | T011–T017 | feat(imap,graph): change flags on the server | Read and star |
| 4. The cycle sends | T018–T023 | feat(sync): send pending flag changes with the cycle | Read and star |
| 5. The window | T024–T029 | feat(ui): star, mark unread and durable read on opening | Read and star |
| 6. Final passes | T030–T033 | (per review) | Read and star |

## Phase 1: documents and review (portion 1)

- [x] T001 STOP: present specs/011-read-and-star/ (spec.md, plan.md,
  research.md, data-model.md, quickstart.md, checklists/requirements.md,
  this tasks.md) and wait for explicit maintainer approval before any
  code change. The challenge decisions are in spec.md Clarifications and
  plan.md "Decisions for the maintainer"; the channel of a change the
  store cannot write (spec FR-011, a toast) was decided here on
  2026-10-03.
- [x] T002 After approval, amend the earlier documents, each with a status
  line naming this feature, as plan.md "Documents amended before
  implementing" lists: specs/009-synchronization/spec.md FR-001 (a cycle
  sends pending changes under 011 FR-007 and otherwise reads; "and their
  read state" gains the star), FR-002(a) (stored mail also changes by the
  user's pending change, kept apart from the server state), FR-005 ("its
  number and read state" gains the star), FR-015(a) (built; the lifecycle
  is 011's, sending after the listing) and the "How a cycle runs"
  flowchart (the pending node loses "deferred" and moves after the
  listing's states on IMAP and after each page on Microsoft 365, repeated
  before each batch and once before the end);
  specs/009-synchronization/data-model.md ("not stored": "read and star
  adds them" goes, UIDs stay unstored; "pending changes" → the three
  columns; the cycle's read, the batch's steps 3 and 5 and the row read
  name both flags, the equal-value rule and the effective values; the
  cycle's pending query);
  specs/009-synchronization/contracts/synchronization.md (`FolderBatch`
  flag states, `FolderSync` flags, `MessageListRow.flagged`,
  `ListedUid.flagged`, `MessageChange::Changed.flagged`, `MessageItem`'s
  `starred`, the sending step in the cycle's order,
  `LoadEvent::StoreChanged`, and "a cycle writes only through
  `Store::store_batch`" → "through `store_batch`, `settle_flags` and
  `drop_pending_flags`, and reads `read_pending_changes`; nothing reaches
  the window with data"); specs/007-mail-storage/spec.md FR-002 (the star
  and the pending wanted values among what is stored), FR-003 (no UID on
  a membership) and FR-014(c) (built), data-model.md (the columns);
  specs/010-message-list/spec.md FR-002 (the star before the date),
  FR-009 (durable; the in-window record retired), FR-011(b) (built), Key
  Entities (effective state), data-model.md (the row object's read state
  from the store's effective value; the read-in-window set goes);
  specs/002-imap-integration/contracts/imap-reading.md (the Inbox row,
  the "EXAMINE completion" of the Metadata section, isolation step 2 and
  the ALERT paragraph: `SELECT`; the Finish row and SC-002's note: the
  flag commands of 011 FR-007 are the only mail-changing commands);
  specs/008-folders/spec.md FR-013(d) (built note);
  specs/006-error-handling/spec.md FR-006 (the toast row also carries a
  change to stored mail that could not be written). Then STOP for the
  documents commit.

## Phase 2: stored flags (portion 2)

Goal: the store keeps the server's star next to the read state and the
user's pending wanted values, reads rows as the effective state, ends an
equal pending value at every server write, and lists a folder's pending
changes; the cycles store the star the servers report. Nothing is sent
and the window changes nothing yet.

- [x] T003 [US1] In crates/mailbag-domain/src/lib.rs: `MessageFlag {
  Seen, Flagged }`, `MessageFlags { seen: bool, flagged: bool }`,
  `FlagChanges { seen: Option<bool>, flagged: Option<bool> }` (what a
  server report named), `PendingChange { identity: String, flag:
  MessageFlag, wanted: bool, server_value: bool }`; `Message.flagged: bool` and
  `MessageListRow.flagged: bool`; `FolderBatch.read_states` becomes
  `flag_states: Vec<(String, FlagChanges)>` and `known_arrived:
  Vec<(String, MessageFlags)>`;
  `ServerStep::ChangeFlags`; the privacy-safe `Debug` outputs updated; in
  crates/mailbag/src/failure_declarations.rs the three wording arms for
  `ServerStep::ChangeFlags` ("Message not changed on the server"; "The
  mail server refused to change this message."; "The mail server stopped
  responding while changing this message."); every constructor in the
  workspace's tests updated.
- [x] T004 [US1] In crates/mailbag-store/src/schema.sql the columns
  `message.flagged INTEGER NOT NULL CHECK (flagged IN (0, 1))`,
  `message.seen_pending INTEGER CHECK (seen_pending IN (0, 1))` and
  `message.flagged_pending INTEGER CHECK (flagged_pending IN (0, 1))`;
  in src/folders.rs `read_listed_rows` selects `COALESCE(seen_pending,
  seen)` and `COALESCE(flagged_pending, flagged)`; `read_folder_identities`
  returns `HashMap<String, MessageFlags>` of the server values and
  `FolderSync.stored` takes that type (src/lib.rs); `set_read_states`
  becomes `set_flag_states(transaction, account, &[(String,
  FlagChanges)])` writing only the named flags (`seen = COALESCE(?seen,
  seen)`) with `seen_pending = CASE WHEN ?seen IS NOT NULL AND
  seen_pending = ?seen THEN NULL ELSE seen_pending END` and the same for
  `flagged`; `store_arrived`'s upsert writes `flagged` and the two
  `CASE`s in its `DO UPDATE`; `relate_known` writes both flags the same
  way; new `read_pending_changes(connection, folder_id) ->
  Vec<PendingChange>` over the folder's memberships where either pending
  column is not null, each with its server value;
  `write_pending_flag(transaction, account, identity, flag, wanted)` as
  one `UPDATE` setting the pending column to `wanted` whatever the server
  column holds (research §14); `settle_flags(transaction, account,
  identities, flag, value)` (server := value; pending := NULL where it
  equals value); `drop_pending_flags(transaction, account, identities,
  flag, refused)` (pending := NULL where it equals refused); the
  `Store` methods `read_pending_changes(folder)`, `write_pending_flag`,
  `settle_flags`, `drop_pending_flags` in src/lib.rs, the writes failing
  as `FailureKind::MailNotSaved` and the read as `StoredMailUnreadable`.
- [x] T005 [US4] In crates/mailbag-imap/src/lib.rs `ListedUid.flagged:
  bool` and `MessageRow.flagged: bool`; in src/fetch_responses.rs
  `\Flagged` read next to `\Seen` for the listing and the rows
  (`Flag::Flagged`); in src/test_server.rs `FixtureMessage.flagged: bool`
  (default false) written into `FLAGS (...)` with `\Seen`.
- [x] T006 [US4] In crates/mailbag-graph/src/lib.rs `flag` joins
  `CHANGE_FIELDS`; `GraphMessage.flagged: bool` from `flag.flagStatus ==
  "flagged"` (`complete` and `notFlagged` read false); `MessageChange::
  Changed { flagged: Option<bool>, .. }`, and `read_change` in
  src/reply.rs treats `flag` like `isRead`: a partial entry carrying
  only `isRead` or `flag` is not "other fields"; in src/test_server.rs
  `delta_entry`, `inbox_message` and `stored_message` carry `flag` with
  `flagStatus`.
- [x] T007 [US4] In crates/mailbag-providers/src/cycle/imap.rs
  `ListedMessage.flags: MessageFlags`, `listing_changes` emits
  `flag_states` where either flag differs from the stored server value,
  `fetch_arrivals` sets `Message.flagged` from the row and `known_arrived`
  with both flags; in src/cycle/graph.rs `merge_per_message` merges
  `flagged` as it merges `is_read`, `batch_from_changes` emits a
  `FlagChanges` from a partial entry with only the flags it names (the
  other `None`, never taken from the cycle's snapshot: a message may come
  in two pages of one round, research §14), `stored_message` sets
  `flagged`; in
  src/store_load.rs `BatchCounts.read_states` becomes `flag_states`.
- [x] T008 [P] [US1] Tests in crates/mailbag-store/src/tests.rs: rows
  read the effective state for each combination of server and pending
  values; `write_pending_flag` stores a wish, also one equal to the
  server value, and a newer wish replaces the older; a batch's flag write
  ends an equal pending value, leaves a differing one and leaves a flag
  the report did not name, through `flag_states`, an upsert and a
  related known message; `read_pending_changes` lists only the folder's
  non-null values, as `(identity, flag, wanted, server_value)`; `settle_flags`
  ends a pending value equal to the sent value and keeps a differing one;
  `drop_pending_flags` the same with the refused value; a second
  `Store::at` over the same file reads the pending state again (spec
  SC-002's store half).
- [x] T009 [P] [US4] Tests: crates/mailbag-imap/src/tests/ (a listing and
  a row with `\Flagged` read into `flagged`; without it false);
  crates/mailbag-graph/src/tests.rs and src/reply.rs tests (`flag` in the
  selected fields; `flagged` from `flagged`, `notFlagged` and
  `complete`; a partial entry with `flag` only is not "other fields");
  crates/mailbag-providers/src/tests.rs (an IMAP listing whose flag
  changed stores the new star; a Microsoft 365 partial entry with `flag`
  changes the stored star and keeps the read state).
- [x] T010 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md (domain ≈ 35, store ≈ 90, the read side of imap ≈ 10
  and graph ≈ 15, providers ≈ 20; tests ≈ 185); report, suggest the
  commit and wait before portion 3.

## Phase 3: the wire (portion 3)

Goal: a mailbox is opened for writing, the IMAP reader can set and clear
`\Seen` and `\Flagged` by UID set, the Microsoft 365 client can update a
message's read mark and follow-up flag, and both scripted servers accept,
refuse and record those commands. No cycle uses them yet.

- [x] T011 [US1] In crates/mailbag-imap/src/session.rs `examine_mailbox`
  becomes `select_mailbox` sending `SELECT` (the fork's `select`, the same
  `Mailbox` out); `open_mailbox` and therefore `MailboxReader::reconnect`
  use it; doc comments no longer say read-only; in src/test_server.rs the
  log's `examined_mailboxes` becomes `opened_mailboxes` and records the
  command name; a test asserts `SELECT` is sent and `EXAMINE` is not.
- [x] T012 [US1] [US5] In crates/mailbag-imap/src/lib.rs
  `ImapStep::StoreFlags` and `pub enum StoreFlag { Seen, Flagged }` (the
  crate does not depend on `mailbag-domain`; the sender maps
  `MessageFlag` to it); in src/reader.rs `MailboxReader::store_flags
  (&mut self, uids: &[u32], flag: StoreFlag, set: bool) ->
  Result<Option<ImapError>, ImapError>`: one `UID STORE <set>
  +FLAGS.SILENT (\Seen)` (or `-FLAGS.SILENT`, `\Flagged`), the UID set
  written as `1,5,9`; the answer drained with `collect_fetches` (Gmail
  still sends a `FETCH` line); a `NO`/`BAD` completion returns
  `Some(refusal)`, an error at `StoreFlags` built where every reader error
  is, so the reply has the sign-in name replaced and the alerts are kept
  (changed in portion 4); a lost connection fails at
  `ImapStep::StoreFlags`; `notices.collect` as other commands; in
  crates/mailbag-providers/src/failure.rs `ImapStep::StoreFlags →
  ServerStep::ChangeFlags`.
- [x] T013 [US1] [US5] In crates/mailbag-imap/src/test_server.rs: the
  flags of a session's messages become mutable state (a `RefCell` map
  `uid → (seen, flagged)` seeded from the fixture, read by the `FLAGS`
  answers); `UID STORE` parsed (UID set with commas,
  `+FLAGS`/`-FLAGS` with or without `.SILENT`, the flag list), applied,
  recorded in `commands` with its full text, and answered `{tag} OK` plus
  an untagged `FETCH (UID n FLAGS (...))` line when the setup's
  `store_echoes_fetch` is set (Gmail's behaviour); setup knobs
  `store_completion: Option<String>` (a scripted `NO` or `BAD` with
  `{tag}`) and `store_fault: Option<StoreFault>` (`CloseAfterApplying`: apply
  and close the connection without the completion; `CloseBeforeApplying`: close
  before applying; `HoldCompletion(receiver)`: apply, then send the
  completion only when the test signals, so a test can write a pending
  change while the command is in flight).
- [x] T014 [US1] [US4] [US5] In crates/mailbag-graph/src/lib.rs
  `FlagUpdate { Read(bool), Starred(bool) }` and
  `update_message_flags(service_url, access_token, id, update) ->
  Result<(), GraphError>`: `build_request` takes the method and an
  optional JSON body (`set_method`, `set_request_body_from_bytes` with
  `application/json`), the ImmutableId preference, `PATCH
  /me/messages/{id}` with `{"isRead": …}` or `{"flag": {"flagStatus":
  "flagged" | "notFlagged"}}`; 200 is success, any other status the
  refusal `GraphFailure::Refused { status, code }` as today (the sender
  tells a 5xx apart, T020); the record line names the path and the
  field, never the message. In src/test_server.rs `ReceivedRequest.method`
  and `.body`; `PATCH /me/messages/{id}` applies `isRead` and `flag` to
  the scripted message and answers 200 with it, or the setup's scripted
  answer (`patch_answer: Option<ScriptedAnswer>` for 400, 404, 429 or
  504).
- [x] T015 [P] [US1] [US5] Tests in crates/mailbag-imap/src/tests/
  (a new `flags.rs` beside `mailboxes.rs`): `store_flags` sends the
  expected command text for each flag and direction and a UID set; an
  `OK` with and without the echoed `FETCH` line returns `None`; a `NO`
  and a `BAD` return the reply with the code; a connection closed before
  the completion fails at `StoreFlags`; the scripted server's flags
  change and show in a later `FETCH`.
- [x] T016 [P] [US4] [US5] Tests in crates/mailbag-graph/src/tests.rs:
  the request's method, path, headers (`Content-Type`, `Prefer`,
  `Authorization`) and body for each `FlagUpdate`; a 200 is `Ok`; a 400,
  a 404 and a 429 are `Refused` with their status and code; the scripted
  message carries the new value afterwards.
- [x] T017 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md (imap ≈ 60, graph ≈ 40; tests ≈ 200 with the
  scripted servers' ≈ 100); report, suggest the commit and wait before
  portion 4.

## Phase 4: the cycle sends (portion 4)

Goal: a cycle sends the folder's pending changes after storing its
listing, before each batch of missing messages and once before closing,
settles accepted commands, drops refused ones and tells the window, and
leaves the rest for the next listing (spec FR-006 to FR-010, FR-012).

- [x] T018 [US5] In crates/mailbag-providers/src/load.rs
  `LoadEvent::BatchStored` becomes `StoreChanged` ("the cycle changed the
  folder's stored state, a batch or a dropped pending change; the window
  reads again"); src/store_load.rs, src/tests.rs,
  crates/mailbag/src/window_ui.rs and crates/mailbag/src/mail_ui/tests.rs
  follow the rename.
- [x] T019 [US1] [US5] In crates/mailbag-providers/src/store_load.rs
  `BatchWriter::pending_changes() -> Result<Vec<PendingChange>,
  LoadResult>`, `settle(identities, flag, value)` and
  `drop_pending(identities, flag, refused)` with the cycle's failure
  mapping; `drop_pending` sends `LoadEvent::StoreChanged`;
  `BatchCounts.settled` (accepted changes and those the server already
  had) counted in `settle` and written by `finish`.
- [x] T020 [US1] [US4] [US5] New crates/mailbag-providers/src/cycle/
  pending.rs, declared from src/cycle.rs: `send_imap_changes(reader:
  &mut MailboxReader, listed_uids: &HashMap<String, u32>, batches: &mut
  BatchWriter) -> Result<(), CycleEnd>`: a pending change whose wanted
  value equals its server value → `settle` with that value, no command;
  the others the listing shows, grouped by `(flag, wanted)`, one
  `store_flags` per hundred UIDs; `Ok(None)` → `settle(uids, flag,
  wanted)`; `Ok(Some(reply))` → `drop_pending(uids, flag, wanted)` then
  `Err(CycleEnd::Failed(LoadFailure::Imap(...)))` with the reply at
  `ImapStep::StoreFlags`; `Err(error)` → `Err` with the pending untouched.
  `send_graph_changes(service: &mut GraphService, batches) ->
  Result<(), CycleEnd>`: the same equal-value settle; one
  `GraphService::update_flags(id, update)` per pending change, which runs
  `update_message_flags` through `request` (the renewal applies once as
  for any request); 200 → `settle`; a 4xx other than the refused token →
  `drop_pending` then `Err` with the refusal; a 5xx → `Err` with the
  pending untouched, an unknown outcome (spec FR-009, research §14).
  Record lines count sent and refused changes at info with identities at
  debug, never a subject.
- [x] T021 [US1] [US3] [US4] In crates/mailbag-providers/src/cycle/imap.rs
  `synchronize_imap_folder`: the listing's `identity → uid` map kept from
  `identify`; after `batches.store(&listing_changes(...))` the loop
  `loop { send_imap_changes; the next chunk of missing messages or break;
  fetch_arrivals; store }`, so a change is sent before each batch and
  once before the cycle finishes; `ImapFolder` exposes the reader to the
  sender. In src/cycle/graph.rs `synchronize_graph_folder`: after a
  round's last page is stored, and after each stored page of a first fill
  or full re-reading, `send_graph_changes` (`GraphService::update_flags`
  of T020).
- [x] T022 [P] [US1] [US3] [US4] [US5] Tests in
  crates/mailbag-providers/src/tests.rs against the scripted servers:
  SC-001's server side (a stored pending change of each kind reaches the
  IMAP server as exactly one `UID STORE` with that UID and flag, and the
  Microsoft 365 service as one `PATCH` with that body, within one cycle;
  afterwards the server columns hold the value and nothing is pending);
  two pending stars go in one IMAP command, 250 pending stars in three
  commands, two Microsoft 365 changes in two requests; a wish equal to
  the server value ends without a command; a pending change written
  while the scripted server holds the completion (the opposite value)
  survives the acceptance and is sent by the next sending step (research
  §14); SC-003 (a fixture of 300 messages in batches of 100: a
  pending change written after the first batch is stored appears in the
  command log before the second batch's `UID FETCH`; one written after
  the last batch before `LOGOUT`); SC-004 (`CloseAfterApplying`: the cycle
  fails, the pending stays; the next cycle's listing ends it and the log
  holds one `STORE`; `CloseBeforeApplying`: the next cycle sends it once);
  SC-005 (a scripted `NO`: the pending is dropped, `StoreChanged` is
  received after the drop, the cycle ends `Failed` with a failure naming
  `ServerStep::ChangeFlags` and carrying the reply; the next cycle sends
  nothing for it; a `BAD` the same); SC-006 (a Gmail fixture with one
  message under two label mailboxes: a pending read under label A is
  sent by A's cycle once, B's cycle finds the server agreeing and sends
  nothing); Microsoft 365: a change is sent after the round, a 400
  refusal drops and fails, a 504 fails the cycle and leaves the pending
  change for the next cycle, which sends it again, a refused token is
  renewed once as for any request, a message reported in two pages of one
  round (read state, then star alone) keeps both; a pending change whose
  message the listing lacks is left
  pending and not sent; a cycle whose connection the scripted server
  refuses leaves the pending change untouched and the next cycle sends
  it (SC-002's server half); after a Microsoft 365 change the next round's
  partial entry and full entry leave the effective state as it is.
- [x] T023 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md (providers ≈ 150; tests ≈ 240); report, suggest the
  commit and wait before portion 5.

## Phase 5: the window (portion 5)

Goal: the star toggle, Mark as Unread and the header menu's Mark as Read
and Mark as Unread change the open message; read on opening stores its
change; the row shows the star; every row state comes from a read of the
store (spec FR-001 to FR-004).

- [x] T024 [US1] Form, edited as text (a list item template; Cambalache
  cannot open it, 010 research §9), presented as a diff with a rendering
  for approval before the code is written: crates/mailbag/resources/ui/
  message-row.ui gains, in the first line's box between `sender` and
  `time`, a `GtkImage` `star` (`starred-symbolic`, `pixel-size` 14,
  `margin-start` 6, `accessible-role` presentation, `valign` center,
  `visible` bound to `starred` of `MessageItem`, style class `warning`
  for the star's colour as libadwaita names it); nothing else in the row
  changes. Workbench: no demo fits a mail row; the icon is the stock
  `starred-symbolic` the envelope's toggle uses.
- [x] T025 [US1] In crates/mailbag/src/mail_ui/message_item.rs the
  `starred` property (get, set) seeded from `row.flagged` in `new`;
  `read_state_text` says "Unread, starred" / "Read, starred" when
  starred; `set_unread` and `set_starred` notify it.
- [x] T026 [US1] [US2] In crates/mailbag/src/mail_ui.rs: a
  `gio::SimpleActionGroup` named `message` inserted on `reader_stack`
  (the envelope's toggle and menu are its descendants) with `star` (stateful,
  boolean state, `change-state` → `change_flag(MessageFlag::Flagged,
  state)`) and `mark-unread` (→ `change_flag(Seen, false)`), no
  `mark-read`, since no form names it; `change_flag(flag, wanted)`: for
  `Seen = false` `drop_pending_read` first of all (research §14); then the
  `flag_change` callback for the open message
  (`connect_flag_change(impl Fn(&AccountId, &str, MessageFlag, bool))`),
  without a check of the shown state, so two quick opposite changes are
  both written; the timer, renamed `mark_read_after_opening`, calls
  `change_flag(Seen, true)` while the open row is unread; `InWindow.read`, `unread_in_window` and the
  read-in-window arguments removed (`unread` is `!row.seen`);
  `update_list_by_difference` sets `starred` beside `unread`, in place
  (`lists_same_message` leaves it out, or the row would animate away and
  back); `show_envelope` sets the `star` action's state and the toggle's
  icon (`starred-symbolic` while starred, `non-starred-symbolic`
  otherwise) from the item's `starred`; the envelope's menu is enabled; the
  old `in_window.removed` stays as it is.
- [x] T027 [US1] [US2] [US5] In crates/mailbag/src/window_ui.rs:
  `connect_flag_change` → the change joins a `VecDeque` of pending writes
  and one `run_on_pool(store.write_pending_flag(...))` runs at a time, the
  next starting when it ends (research §14); after each,
  `read_shown_mailbox_again()` on success, or on failure the toast
  "Message not changed. Try again." (spec FR-011; one constant,
  `MESSAGE_NOT_CHANGED` in failure_declarations.rs) without any row
  change; no load starts;
  `LoadEvent::StoreChanged` handled as `BatchStored` was; the mail pane
  owns the `app.mark-scope-read` and `app.mark-scope-unread` actions
  calling its `change_flag` for the open message (moved there at the
  simplification review), and crates/mailbag/src/main.rs publishes them
  through the window.
- [x] T028 [P] [US1] [US2] [US5] Tests in crates/mailbag/src/mail_ui/
  tests.rs, one GUI test per process: pressing the star stores
  `flagged_pending` for the open message, the row's `starred` and the
  toggle follow after the re-read, and the test loader records no load
  (SC-001's window side); Mark as Unread on a message counted read stores
  `seen_pending = 0`, the message stays open, the dot returns and no
  timer marks it read again within two seconds; an opened unread message
  gets `seen_pending = 1` between 0.8 and 1.5 s after opening and not
  within half a second, and a message opened and left within the second
  stores nothing (SC-007); two changes made within one frame (star, then
  unstar) are written in that order and the store ends unstarred; the
  header menu's actions act on the open message; a `StoreChanged` event
  followed by a `Finished(Failed)` with
  `ServerStep::ChangeFlags` shows the row as the store has it and the
  failed-refresh banner with the server's words; a store that refuses the
  write (a read-only file) shows the toast and changes no row.
- [x] T029 STOP: run ./scripts/check.sh, git diff --check and each GTK
  test one per process; compare the size with plan.md (mailbag ≈ 130;
  tests ≈ 235) and the budget (≤ 600 / ≤ 850); show the row's rendering;
  report, suggest the commit and wait.

Amended on 2026-10-03 at the review of portion 5 (spec FR-002, FR-004,
Clarifications "the window's review"): the row's star moves under the
date and becomes a control.

- [x] T034 Amend spec.md (status, Clarifications, FR-002, FR-004),
  specs/010-message-list/spec.md (status, FR-002), plan.md (decision 6,
  the row's star in the function map, the optional "star from the row"
  gone) and this file.
- [x] T035 STOP: crates/mailbag/resources/ui/message-row.ui: the star
  leaves the first line and ends the second, after the subject, in a
  place every row keeps (a fixed width); its icon follows a `star-icon`
  property of `MessageItem` (filled while starred, the outline while the
  pointer is over the row, none otherwise); a click gesture claims the
  press and stars or unstars on release; a motion controller on the row
  tells the item the pointer is over it. Presented as a diff with a
  rendering for approval before the code is written.
- [x] T036 In crates/mailbag/src/mail_ui/message_item.rs the `pointed`
  and `star-icon` properties; in src/mail_ui.rs the row handlers
  (`star_pressed` claims the press, `star_row` asks for the opposite
  star, `row_pointed`/`row_unpointed`) and `change_row_flag(identity,
  flag, wanted)`; a GUI test in src/mail_ui/tests.rs: the row's star
  stores the change of its message, open or not, without opening it, the
  outline shows only while pointed, and the subject keeps its width. Then
  STOP with ./scripts/check.sh, the GTK tests one per process and the
  live rendering.

## Phase 6: final passes (portion 6)

- [x] T030 Consistency analysis (`speckit-analyze`) in a fresh session,
  once; document fixes applied, scope-adding findings brought to the
  maintainer.
- [x] T031 Each GTK test of the branch one per process; the
  simplification review of the branch diff in a fresh session; findings
  reported, not applied, until the maintainer decides; the accepted ones
  applied with ./scripts/check.sh.
- [x] T032 The quickstart's installed-build checks with the maintainer
  (SC-008 with an account of each provider; the record checked for
  privacy: identities and flags, no subject); findings fixed within this
  portion; the amendments of T002 checked against the built behaviour.
- [x] T037 From T032 (spec FR-007 and 009 FR-001, amended 2026-10-04):
  in crates/mailbag-providers/src/cycle/imap.rs a cycle whose command
  was accepted lists the folder once more after its last sending step and
  stores the removals and flag states it proves (no arrivals, no state);
  `send_imap_changes` returns whether a command was accepted; the scripted
  IMAP server gains a mailbox that lists only flagged messages, as Gmail's
  Starred label; a test: unstarring there takes the message out of the
  stored folder in the same cycle.
- [x] T033 STOP: final report with the size against the budget (≤ 600
  production, ≤ 850 test lines), what was verified and how, and the open
  items.

## Phase 7: the implementation's review (2026-10-04)

The outside review of the built branch (research §15); the maintainer
decided every point in this feature, two quick clicks on a star as a
recorded limitation.

- [x] T038 Documents (portion 7a): research §15 and the notes on §11,
  §12, §14; spec FR-001, FR-002, FR-004, FR-007, FR-009, Edge Cases,
  Key Entities, SC-004, Assumptions and the Clarifications of
  2026-10-04; data-model; plan; quickstart; 009 spec FR-015(a) and its
  flowchart, 009 data-model. STOP for the maintainer's review.
- [x] T039 [P] In crates/mailbag-store: `set_flag_states`,
  `store_arrived` and `relate_known` write the server value and leave the
  pending columns; `read_pending_changes` returns `(identity, flag,
  wanted)` and `PendingChange` in crates/mailbag-domain loses its server
  value; store tests: a report leaves an equal pending value.
- [x] T040 In crates/mailbag-providers/src/cycle/: `send_imap_changes`
  compares each wish with the value the cycle last sent, otherwise the
  listing's; ends a wish equal to the listing's value without a command,
  skips one equal to the value sent, sends the others and records what it
  sent; an OK settles nothing; `confirm_sent_changes` replaces
  `store_listing_after_commands`: it lists again, stores what the listing
  proves, settles the sent changes it shows and returns its refusal, with
  which the cycle ends incomplete; `send_graph_changes` sends every
  pending change; `changes_to_send` goes. The scripted IMAP server
  ignores a store on a UID the mailbox does not list and can refuse the
  listings after an accepted store. Tests: a command the server ignored
  (a UID the mailbox no longer has) leaves the change pending; a change
  whose answer was lost ends by the next listing without a command
  (kept); a refused listing after the commands ends the cycle
  incomplete; a 504, the wish taken back and an empty round still send
  the request; a page reporting the wish and a later page replaying the
  old value leave the change for the request; the tests that relied on
  an OK or a report settling are rewritten.
- [x] T041 In crates/mailbag/resources/ui/message-row.ui the star loses
  its `accessible-role`; no test (two quick clicks stay a limitation,
  research §15.4).
- [x] T043 From the second review of 2026-10-04 (spec Clarifications; in a
  fresh session: the window, the behaviour, readability, size,
  architecture, security): in crates/mailbag/src/mail_ui.rs the reader
  header's menu button (`demo_button`) is no longer made insensitive, and
  `app.mark-scope-read` / `app.mark-scope-unread` are enabled while a
  message is open (spec FR-002); the GUI test asserts both;
  specs/002-imap-integration/contracts/ui.md amended.
- [x] T044 From the same review: in
  crates/mailbag-providers/src/cycle/pending.rs
  `end_changes_the_listing_shows`, called once after the listing is
  stored, ends the wishes the listing shows; `send_imap_changes` no longer
  compares with the listing, whose values are old by a later step (spec
  FR-007, Edge Cases; research §15); the race test also marks the message
  unread, a value the listing shows, and expects the command before the
  next batch.
- [ ] T042 STOP (portion 7b with T043 and T044): ./scripts/check.sh, the
  GTK tests one per process, the size of the change, a suggested commit
  message; PR #17 updated by the maintainer.

## Dependencies

- T001 before everything; T002 after approval; each STOP (T002, T010,
  T017, T023, T029, T033) waits for the maintainer.
- Portion 2 before 3 (the batch shape and the store's writes), 3 before 4
  (the commands), 4 before 5 (the window needs `StoreChanged` and the
  store's writes); 6 last.
- Within a portion, tasks marked [P] touch different files from the tasks
  before them and may run in parallel once those are done.

## Implementation strategy

Each portion is a reviewable, testable increment: after portion 2 the
store holds the real star of every message and the user's wishes can be
written and read; after portion 3 the commands exist and are tested
against the scripted servers; after portion 4 a refresh sends what the
user wanted (the first end-to-end slice, exercised by writing a pending
value directly into the store); after portion 5 the user can do it from
the window. The maintainer's live check (T032) covers SC-008 on the
installed build.
