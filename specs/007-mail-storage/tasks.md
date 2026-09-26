# Tasks: Mail Storage

**Feature**: `007-mail-storage`
**Created**: 2026-09-26 · **Branch**: `claude/storage` · **Status**: Documents
ready for review; no code yet.

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table, the
function map and the portions, [research](research.md) owns the decisions
with alternatives, the [data model](data-model.md) owns the stored form,
[006's contract](../006-error-handling/contracts/failure-declaration.md)
owns the failure's shape, [quickstart](quickstart.md) owns the manual
acceptance. Follow [AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses):
implement one portion, run its checks, compare the size with the plan's
table, report and stop. The maintainer creates commits and PRs. Do not
start code before document approval. Nothing committed may mention where an
idea came from outside this repository.

Phases follow the plan's portions. Story labels trace tasks to US1 (stored
mail without the server), US2 (a refresh replaces the stored Inbox), US3 (a
failed refresh keeps the stored mail), US4 (an account's mail leaves with
the account) and US5 (a store that cannot be used). Tests are part of every
portion and live beside their modules; no test pins wording except the
privacy invariants.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(storage): specify and plan mail storage | Mail storage |
| 0. Failures as domain values (006 portion 6) | T002–T003 | refactor(errors): hand failures to the application as domain values | Mail storage |
| 1. The store | T004–T012 | feat(store): add the mail store | Mail storage |
| 2. Loads write, the window reads | T013–T018 | feat: show mail from the store | Mail storage |
| 3. Accounts leave | T019–T021 | feat: delete the stored mail of removed accounts | Mail storage |
| Polish | T022–T024 | (per review) | Mail storage |

## Phase 1: documents and review

- [X] T001 STOP: present specs/007-mail-storage/ (spec.md, plan.md,
  research.md, data-model.md, quickstart.md, checklists/requirements.md,
  this tasks.md) together with the amendments made for it on this branch:
  specs/006-error-handling/ (research §1, §4, §5; contracts/
  failure-declaration.md; plan "Amendment 2026-09-26"; tasks T028–T032;
  spec and quickstart) and specs/003-logging/spec.md FR-004's example; wait
  for explicit maintainer approval before any code change.

## Phase 2: failures as domain values (portion 0)

006's portion 6, planned in 006's documents with its own budget. It creates
`mailbag-domain`, which every later portion uses.

- [X] T002 Implement specs/006-error-handling/tasks.md T028–T031 and mark
  them done there.
- [X] T003 STOP: run 006's T032 (checks, the size against 006's amendment
  table, the wording literals unchanged); report, suggest the commit and
  wait before portion 1.

## Phase 3: the store (portion 1)

Goal: `mailbag-store` and the domain's additions exist and are tested; the
application does not use the store yet.

- [X] T004 Amend the documents first: in
  specs/001-goa-account-observation/data-model.md and contracts/accounts.md,
  mark as amended by 007 that `AccountId` lives in `mailbag-domain`, whose
  `TryFrom<&str>` refuses an empty identifier, which `goa-adapter` turns
  into its `InvalidReply`; in
  specs/006-error-handling/contracts/failure-declaration.md add the store's
  kinds `StorageFull`, `MailNotSaved` and `StoredMailUnreadable`, produced by
  the store.
- [X] T005 Move `AccountId` from crates/goa-adapter/src/account_model.rs
  into crates/mailbag-domain (same derives, `as_str`, `TryFrom<&str>` with
  the domain's error `EmptyAccountId`); make goa-adapter depend on
  mailbag-domain and turn `EmptyAccountId` into its
  `AccountCheckError { "account ID", InvalidReply }` where it reads an
  identifier; switch the imports in goa-adapter, crates/mailbag-providers and
  crates/mailbag (about 14 files, paths only).
- [X] T006 Move `DisplayFields` from crates/mailbag-content/src/lib.rs into
  crates/mailbag-domain; `decode_display_fields` returns it; switch the
  imports in crates/mailbag-providers/src/imap_batch.rs, microsoft365.rs,
  batch.rs and crates/mailbag/src/mail_ui.rs.
- [X] T007 In crates/mailbag-domain add `Message { identity: String, fields:
  DisplayFields, received: Option<i64>, seen: bool, content: ReceivedContent
  }` with a `Debug` that leaves the fields and the text out; the
  `FailureKind` variants `StorageFull`, `MailNotSaved` and
  `StoredMailUnreadable`, each documented with the SQLite condition it comes
  from (research §8); in src/panic.rs `catch_panic(work) -> Result<T,
  String>` (installs the hook once, runs the work under `catch_unwind`,
  returns `take_panic()` or the payload's message) and
  `Failure::stopped(panic: Option<String>)` with the technical lines
  `Failure: Stopped` and `Panic: …`; let `LoadFailure::into_failure` in
  crates/mailbag-providers/src/failure.rs build `WorkerStopped`'s failure
  with `Failure::stopped` (the worker's panic still ends the load through
  `LoadFailure::give_up`, which writes its error line). In crates/mailbag/src/failure_declarations.rs add the
  three arms of `declare_failure` with Retry and, for `StorageFull`, advice
  to free disk space (wording final in code, impersonal, AGENTS.md "UI
  wording").
- [X] T008 Create crates/mailbag-store (Cargo.toml: mailbag-domain,
  `rusqlite = { version = "0.40", default-features = false }`, tracing; the
  workspace lints; add it to the workspace members) with src/schema.sql (the
  two `STRICT` tables, the index and the content codes as a `CHECK`, as in
  data-model.md), src/open.rs (`create_private_directory` with mode 0700,
  `open_store`, `examine_existing` → `Usable | Empty | Discard(reason)` per
  research §4 with `PRAGMA quick_check`, `discard_store` removing the file
  and its `-wal` and `-shm` files with one warning line naming the reason,
  `create_schema`, `schema_version` as 32-bit FNV-1a over the schema text,
  `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`),
  src/content.rs (`content_columns`, `content_from_columns`),
  src/failure.rs (`storage_failure(operation, &rusqlite::Error) -> Failure`:
  `DiskFull` → `StorageFull`, any other failure of a write, its opening
  included → `MailNotSaved`, any failure of a read, its opening included →
  `StoredMailUnreadable`; technical lines `Failure:
  <kind>` and `SQLite: <code>: <text>`; a debug line with the same) and
  src/lib.rs (`Store::at`, `Store::in_memory`, `connection` opening at first
  use and taking over a poisoned lock, `replace_inbox(account, messages,
  load_cancelled)` with the check under the lock before the transaction,
  `read_inbox`, `keep_accounts(current_accounts)` reading the accounts to
  keep through the check under the lock, `InboxWrite { Stored, LoadCancelled
  }`).
- [X] T009 [P] Tests in crates/mailbag-store/src/tests.rs, about 200 lines:
  every content code and field round trips in load order; a replacement
  leaves exactly the new messages; an empty stored Inbox differs from none;
  a cancelled load writes nothing; `keep_accounts` deletes every other
  account's mail and returns those accounts, and reads its set only once it
  holds the lock (a check that records when it ran); a write that fails
  (`PRAGMA query_only`) leaves the previous Inbox whole and returns
  `MailNotSaved`; a store reopened from the same file reads the same Inbox;
  a store with another version, a file of random bytes and a damaged file
  each start empty with one warning line naming the reason
  (tests/support/record.rs); a directory that cannot be created is a
  failure and deletes nothing; the store's directory has mode 0700.
- [X] T010 [P] Tests in crates/mailbag-domain: `catch_panic` returns the
  message and the place of a panic on the calling thread and the work's
  value otherwise; `Failure::stopped` gives the two technical lines.
- [X] T011 Build and checks: Cargo.lock; run scripts/generate-cargo-sources.sh
  for cargo-sources.json; in scripts/check.sh add that mailbag-store depends
  on no GTK, GLib, mailbag, mailbag-content, mailbag-imap, mailbag-graph,
  mailbag-providers or goa-adapter; add the `sqlite3` pkg-config check and
  `sqlite-devel` to scripts/setup.sh, `sqlite-devel` to
  .github/workflows/check.yml and to the prerequisites in README.md; confirm
  `cargo deny check licenses sources` passes with deny.toml unchanged.
- [ ] T012 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md's table (domain ~45 new, store ~365); review
  constitution I/II; report, suggest the commit and wait before portion 2.

## Phase 4: loads write, the window reads (portion 2)

Goal: the window shows only stored mail (US1, US2), a failed refresh keeps
it under the banner (US3), and a store that cannot be read is a failure page
whose Retry reads it again (US5).

- [ ] T013 Amend the documents first: specs/002-imap-integration/spec.md
  (FR-006's stage rule, FR-008's "MAY discard", FR-009's empty list after a
  failed refresh, SC-004's stage part and the Clarification "What does a
  refresh keep?" marked as amended by 007), data-model.md ("In-Memory
  Data"), contracts/ui.md ("Refresh clears the list…" replaced by 007
  FR-005); specs/006-error-handling/spec.md (User Story 2 and FR-013(a)
  built by 007); specs/006-error-handling/contracts/failure-declaration.md
  (the carrier "a stored Inbox that cannot be read" with Retry reading it
  again; Retry's operation chosen by carrier, `RetriedOperation`).
- [ ] T014 [US1] [US2] In crates/mailbag-providers: make `ReceivedBatch`,
  `ReceivedMessage` and `MessageIdentity` private to the crate; replace
  `LoadResult::Received(batch)` with `LoadResult::Stored { incomplete:
  Option<IncompleteList> }`; `MailLoader::new(accounts, store: Arc<Store>)`
  passes the store to `MailWorker`, and `load_catching_panics(kind, &store,
  &cancelled)` runs the provider's load and then the write inside the
  existing guard; create src/store_load.rs with `message(ReceivedMessage) ->
  Message` (identity `uid:<n>`, `gmail:<X-GM-MSGID>` when Gmail's fields are
  present, `graph:<immutable id>`) and `store_batch(store, batch,
  load_cancelled) -> LoadResult`: `replace_inbox`; `Stored` →
  `log_received_batch` moved from crates/mailbag/src/inbox.rs unchanged in
  its lines, then `Stored { incomplete }`; `LoadCancelled` → `Cancelled`; a
  failure → the load's error line through `log_load_failure(account, kind,
  None, None, 0)`, the one function that writes it (006 T029), then
  `Failed(failure)`.
- [ ] T015 [US1] [US2] Tests in crates/mailbag-providers/src/tests.rs: the
  sequence tests call `load_imap_inbox`, `load_gmail_inbox` and
  `load_microsoft365_inbox` directly and keep their assertions on the batch;
  worker tests: for each of the three providers, a load from its scripted
  server stores its messages (read back from an in-memory store, content
  included) and reports `Stored`; a refused list reports `Stored` with the
  refusal; a store whose directory cannot be created makes the load fail
  with `MailNotSaved` and one error line; a load
  cancelled before its write stores nothing; a panic during the write ends
  as `Stopped` and the worker serves the next load; move the record tests of
  `log_received_batch` from crates/mailbag/src/inbox/tests.rs.
- [ ] T016 [US1] [US2] [US3] [US5] In crates/mailbag: failure_dialog.rs gets
  `RetriedOperation { RefreshInbox, ReadStoredInbox }` for
  `show_action_button` and `present`; inbox.rs replaces `AccountInbox` with
  `RefreshOutcome { Stored(Option<IncompleteList>), Failed(Failure) }` per
  account (`begin_load` keeps the outcome, `finish_load` records it and
  ignores a cancelled or excluded load's result, `discard_excluded` forgets
  outcomes); window_ui.rs as in the plan's function map: the store,
  `read_shown_inbox` through `gio::spawn_blocking(catch_panic(read_inbox))`
  with a caught panic as `Failure::stopped` and the read's error line, each
  read numbered and only the latest read's answer kept, `render` in the plan's
  order, a `Stored` outcome of the shown account read again, a refresh
  forgetting the shown account's read failure, the `read-stored-inbox`
  action; mail_ui.rs `show_inbox(&Rc<[Message]>)` in place of `show_batch`,
  the reader's status page with `RefreshInbox`; main.rs creates
  `Arc::new(Store::at(glib::user_data_dir().join("mailbag").join("mail.sqlite")))`
  for `MailLoader` and `WindowUi` and publishes and removes
  `read-stored-inbox` with the window, like `refresh-inbox`.
- [ ] T017 [US1] [US2] [US3] [US5] Tests: the `ScriptedLoader` of
  crates/mailbag/src/mail_ui/tests.rs writes its messages into the window's
  store (in memory, or a temporary file where a restart is simulated) and
  reports `Stored` or `Failed`; rewrite crates/mailbag/src/inbox/tests.rs for
  `RefreshOutcome`; graphical scenarios, each `#[ignore = "requires a
  graphical GTK session"]`: US1 a new window over the same store file shows
  the same rows and reader content and starts no load, says "Inbox is
  empty" for an account whose load stored an empty Inbox and "No mail
  loaded" for one never loaded (FR-006); US2 rows stay with
  the spinner during a refresh and are replaced after it, the reader closed;
  US3 a failed refresh keeps the rows under the banner with the failure's
  title and its dialog, nothing stored shows the failure page; US5 a store
  that cannot be read shows the failure page with Retry bound to
  `app.read-stored-inbox`, and a refresh shows the refresh's own outcome
  instead; a read's answer that arrives after a newer read's is dropped.
- [ ] T018 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  on its own (`cargo test -p mailbag <name> -- --ignored --exact`); compare
  the size with plan.md's table; report, suggest the commit and wait before
  portion 3.

## Phase 5: accounts leave (portion 3)

Goal: an account's stored mail is deleted on a complete answer without it
or with its Mail off, and a late result cannot bring it back (US4).

- [ ] T019 [US4] In crates/mailbag/src/window_ui.rs `apply_account_update`,
  after the exclusion's cancellation and only when `last_check` is
  complete, call `delete_removed_accounts`: the accounts of the answer that
  are present with Mail on (from `AccountUpdate::accounts`, not from
  `shows_account`) replace the window's latest set, shared as
  `Arc<Mutex<BTreeSet<AccountId>>>`, and `keep_accounts` reads it under the
  store's lock, through `gio::spawn_blocking` inside `catch_panic`; one info line per deleted account, one error line
  when the deletion fails (it happens again at the next complete answer).
- [ ] T020 [US4] Tests: a complete answer without an account, or with its
  Mail off, leaves none of its stored mail; a failed read and a not yet
  checked answer delete nothing; an account missing from the first complete
  answer after a restart loses its mail; two deletions run in the reverse
  order of their answers keep the newer answer's accounts (Mail off, then
  on, then a stored Inbox: the older deletion running last deletes
  nothing); a load that finishes after its
  account was excluded stores nothing (in crates/mailbag-store with a
  cancelled check, and through the window with the scripted loader).
- [ ] T021 STOP: run ./scripts/check.sh, git diff --check and the GTK tests
  touched; compare the size with plan.md's table; report and suggest the
  commit.

## Phase 6: polish

- [ ] T022 Run every GTK test of the branch on its own; run `simplify-review`
  on the branch diff in a fresh subagent; bring findings that add scope to
  the maintainer with the cheapest option.
- [ ] T023 The maintainer runs quickstart.md on the installed build
  (`scripts/build-flatpak.sh --install`); record the results, what was not
  verified and the measured size against the budget in plan.md
  ("Post-implementation"); update the status lines of spec.md, plan.md and
  this file.
- [ ] T024 STOP: final report with the size against the budget (≤ 650 net
  production lines, ~450 test lines) and the open items.

## Dependencies

- T001 before everything; each STOP (T003, T012, T018, T021, T024) waits for
  the maintainer.
- Portion 0 (T002) creates `mailbag-domain`; portion 1 needs it.
- In portion 1, T005–T007 before T008 (the store uses `AccountId`,
  `DisplayFields`, `Message` and the kinds); T009 and T010 after T008 and
  T007.
- Portion 2 needs the store; T013 before any code of the portion; T014
  before T016 (the window receives `LoadResult::Stored`).
- Portion 3 needs portion 2's window and the store's `keep_accounts`.

## Parallel opportunities

- T009 and T010 touch different crates.
- Within portion 2, T015 (providers' tests) and T016 (the window) touch
  different crates once T014 is done.
- Review pauses are never parallel with anything.

## Implementation strategy

Portion by portion, each a reviewed commit on `claude/storage`. The first
user-visible value is portion 2 (US1–US3, US5); portion 3 completes the
account lifecycle (US4). The size is compared with the plan's table at
every pause; passing the budget or 1.5 times an item's estimate stops the
work for the maintainer's decision.

## Deferred, no tasks

Everything in spec FR-014: synchronization of a whole folder and change
detection, folders and labels with membership, read and star with local
changes, conversations and thread identifiers, the content cache, background
synchronization, and upgrading a populated store at release readiness. The optional
mechanisms of the plan have no tasks.
