# Implementation Plan: Mail Storage

**Branch**: `claude/storage` | **Feature**: `007-mail-storage`
**Date**: 2026-09-25, revised 2026-09-26 | **Spec**: [spec.md](spec.md)
**Status**: Implemented on `claude/storage` and accepted on the installed
Flatpak build on 2026-09-26 (see Post-implementation). Paused on 2026-09-25
because the plan declared the store's failures in `mailbag-providers`;
revised on 2026-09-26 on top of
006's amendment of that day: `mailbag-domain` holds the application's shared
definitions, a lower layer hands the application domain values only, and
006's portion 6 builds that crate on this branch before 007's portions. The
plan challenge's findings were decided by the maintainer on 2026-09-26: a
late result is refused by the load's own cancellation (research §6), and a
refresh forgets a failed read, so the list shows the newest failure; the
budget was raised to 650 lines. The specification's decisions are settled
and are not reopened here.

## Size

The budget agreed at sizing on 2026-09-25, and this plan's estimate after
reading the code and counting what moves. 006's portion 6 has its own table
in [006's plan](../006-error-handling/plan.md) and is not counted here.
Reassess with the maintainer before exceeding the budget or about 1.5 times
an item's estimate; at every review pause the size so far is compared with
this table.

| Item | Budget | This plan (estimate) |
|---|---|---|
| New modules and production lines | ≤ 650 net new (raised from 600 on 2026-09-26) | `mailbag-domain` ≈ 45 new (`Message` ~20, the store's three kinds ~8, `catch_panic` and `Failure::from_panic` ~20) and ~30 moved in (`AccountId`, `DisplayFields`); `mailbag-store` ≈ 365 (store and its operations ~170, opening and discarding ~80, schema ~30, content codes ~50, SQLite's errors as `Failure` ~35); `mailbag-providers` ≈ +60 (worker write ~40, `store_load.rs` ~20 new and ~45 moved from the window); `mailbag` ≈ +130 (window reads and account deletion ~105, the three storage kinds worded ~25, Retry by carrier ~25, main ~8, list ~5) and ~40 removed (`AccountInbox`, batch handling). **Net ≈ 600**; the plan challenge judged the first estimate high (its own sketch ≈ 450–480) |
| Call sites or existing files touched | — | Rust: providers `lib.rs`, `batch.rs`, `worker.rs`, `imap_batch.rs`, `microsoft365.rs`, new `store_load.rs`; mailbag `main.rs`, `inbox.rs` (renamed `refreshes.rs` at the final review), `window_ui.rs`, `mail_ui.rs`, `failure_declarations.rs`, `failure_dialog.rs`; `goa-adapter` and `mailbag-content` (the moved definitions; imports of `AccountId` in about 14 files change their path only). Build and checks: workspace `Cargo.toml`, the new crate's manifest and three existing ones, `Cargo.lock`, `cargo-sources.json`, `scripts/check.sh` (the store's dependency rule), `scripts/setup.sh`, `.github/workflows/check.yml` and the README's prerequisites (`sqlite-devel`). No form changes |
| New crates | 1 | 1: `mailbag-store` (`mailbag-domain` comes with 006's portion 6) |
| New threads, timers, queues | 0 | 0: GIO's pool for the window's calls, the existing mail worker for writes |
| New state, types, error types | — | Types: `Store`, `InboxWrite`, `RefreshOutcome` (replaces `AccountInbox`), `RetriedOperation`; in the domain `Message` and three `FailureKind` variants. Built (2026-09-26) with a few private helpers besides: the store's `StoreError { Sqlite, File }` and `StoreOperation`, the window's `ShownInbox`, `StoredInbox` and `ShownMail`. State: in the window, the shown account's stored Inbox and the number of the latest read; no state inside `Store` beyond its file's path and its connection (research §6) |
| New fields in existing data | — | `LoadResult::Received(batch)` becomes `LoadResult::Stored { incomplete }`; persisted: two tables ([data-model.md](data-model.md)) |
| Changes to other features' contracts or documents | 002; 006 | 002 spec, data model and UI contract (memory-only mail, refresh clears the list); 006 spec (User Story 2 and FR-013(a) built) and contract (the store's kinds, a new carrier, Retry by carrier); 001 data model and account contract (`AccountId` lives in the domain crate); 003 FR-004's example amended by 006. The quickstarts of 003–006 and the README name the log file's new place (commit `08c276d`, from the PR review); otherwise 004 and 005 unchanged |
| New dependencies | 1 | `rusqlite` 0.40 without default features: `libsqlite3-sys` and four small crates, all MIT or Apache-2.0. `deny.toml` stays as it is: the Zlib licence approved at sizing came only with the default statement cache ([research §1](research.md)) |
| Tests | ≈ 450 | ≈ 450 net: store ~200 (in memory and on a temporary file); domain ~20 (a panic caught with its place); providers ~90 (the sequence tests call the load directly, so their assertions stay; worker writes, storage failure, late result); window ~200 (the scripted loader writes into a test store; US1–US5 scenarios; a read failure's Retry reads again); `inbox/tests.rs` rewritten, about −150 +100 |

## Summary

A new crate, `mailbag-store`, keeps each account's Inbox as the latest
completed load left it, in one SQLite file in Mailbag's private data
directory. The mail worker writes a load's messages into the store in one
transaction and reports only how the load ended; the window reads the shown
account's stored Inbox through GIO's thread pool and builds the list and the
reader from it. A refresh keeps the rows on screen; a completed load
replaces them, a failed one leaves them under 006's banner. A complete
Online Accounts answer deletes the mail of accounts that are gone or have
Mail off, and a late result for such an account is refused. A store with
another structure, or one that is not a store or is damaged, is discarded at
its first use. Nothing else reaches the window: `LoadResult` carries no mail.

Every layer speaks the domain's definitions (006 research §1): the message
the store keeps and the window shows is the domain's `Message`, the account
is the domain's `AccountId`, and the store's failures are domain `Failure`s
the window words like any other. The window chooses what Retry repeats.

## Minimal version

Everything below is built after 006's portion 6, in the three portions of
the last section.

| Step | What it does | Cost |
|---|---|---|
| The domain's additions | `AccountId` and `DisplayFields` moved in; `Message`; the store's three kinds; `catch_panic` and `Failure::from_panic` | ~45 new, ~30 moved |
| The store | `mailbag-store`: `Store` holds the file's path and one connection behind a mutex opened at first use; `replace_inbox`, `read_inbox`, `delete_other_accounts`; content codes; SQLite's errors turned into `Failure`; no wording | ~365 lines, ~200 lines of tests |
| Opening and discarding | Create `mailbag/` with mode 0700, or give an existing one that mode (built after the PR review, 2026-09-26), open, check version and damage, discard with one warning line, create the schema ([research §4](research.md)) | inside the store's ~365 |
| Loads write | The worker turns the batch into `Message`s and calls `replace_inbox` inside the load's panic guard, with its cancellation as the store's check; `Stored { incomplete }`, `Failed(failure)` with the store's failure, or `Cancelled` when the load was cancelled meanwhile; the "Inbox load finished" record line moves here from the window | ~+60 in providers |
| The window reads | On selecting an account and after a completed load of the shown account, read its Inbox through `gio::spawn_blocking` inside `catch_panic`; show rows, "Inbox is empty", "No mail loaded" or a failure page; rows stay while loading; the banner shows the latest refresh's failure or incomplete list | ~+105 in the window |
| Wording and Retry | `declare_failure` words the store's three kinds; `show_action_button` takes the operation Retry repeats; `app.read-stored-inbox` reads the shown Inbox again ([research §10](research.md)) | ~50 |
| Accounts leave | On a complete answer, `delete_other_accounts` with the accounts present with Mail on; deleted accounts and failures go to the record | inside the window's ~105 |

Not built: a store thread, a change notification, migrations, reading one
message's content separately, a statement cache, a Reset control, folders,
membership, UIDVALIDITY, Gmail labels, thread identifiers (spec FR-014).

## Function map

The entry points and their steps, as the code will read.

**`mailbag-domain`** (additions of 007)

- `AccountId`, moved from `goa-adapter` with `as_str`; its `TryFrom<&str>`
  refuses an empty identifier with the domain's `EmptyAccountId`, which
  `goa-adapter`'s reader keeps turning into its `InvalidReply`.
- `DisplayFields`, moved from `mailbag-content`.
- `Message { identity: String, fields: DisplayFields, received_unix:
  Option<i64>, seen: bool, content: ReceivedContent }`, with a `Debug` that leaves the
  fields and the text out.
- `FailureKind::StorageFull`, `MailNotSaved`, `StoredMailUnreadable`.
- `catch_panic(work) -> Result<T, String>` and `Failure::from_panic(kind,
  panic)`, the failure of the operation a panic stopped (research §9).

**`mailbag-store`**

- `Store::at(path: PathBuf) -> Store`: no I/O; the file opens at first use.
  `Store::in_memory()` for tests.
- `Store::replace_inbox(&self, account: &AccountId, messages: &[Message],
  load_cancelled: impl FnOnce() -> bool) -> Result<InboxWrite, Failure>`: lock;
  a load cancelled by then → `InboxWrite::LoadCancelled`, nothing written;
  else one transaction: delete the `inbox` row, insert it, insert the
  messages in order, commit → `InboxWrite::Stored` (research §6).
- `Store::read_inbox(&self, account: &AccountId) ->
  Result<Option<Vec<Message>>, Failure>`: `None` without an `inbox` row; the
  messages in load order with their content otherwise.
- `Store::delete_other_accounts(&self, current_accounts: &BTreeSet<AccountId>)
  -> Result<Vec<AccountId>, Failure>`: in one transaction, delete the `inbox`
  row of every stored account not in `current_accounts`, the accounts of
  that answer; return those accounts for the record (research §6,
  "Deletions in order").
- `with_connection(&self, operation, work)`: runs the work with the locked
  connection, opened by `open_store` at first use, and hands a failure on as
  the operation's; a poisoned lock is taken over, since a transaction a panic
  interrupted was rolled back.
- `open_store(path)`: `create_private_directory`, `Connection::open`,
  `examine_existing` → `Usable | Empty | Discard(reason)`;
  `discard_store(path, reason)` removes the three files and logs;
  `create_schema` runs the schema text and sets `user_version`; the
  connection settings of the data model.
- `schema_version() -> i32`: FNV-1a over the schema text.
- `content_columns(&ReceivedContent)` and `content_from_columns(kind,
  detail)`: the codes of the data model; an unknown code is a read failure.
- `storage_failure(operation, &StoreError) -> Failure`: a full disk is
  `StorageFull` whatever the operation, otherwise the kind by operation;
  `Failure: <kind>` and `SQLite: <code>: <text>`, or `File: <error>` when the
  file system failed while the directory or the files were prepared, as the
  technical details; a debug line with the same.

**`mailbag-providers`**

- `MailLoader::new(accounts, store: Arc<Store>)`: as today; the worker gets
  the store.
- `worker::load_catching_panics(kind, &store, &cancelled)`: inside the
  existing guard, the provider's load and then `store_load::store_batch(&store,
  batch, || cancelled.is_closed())`.
- `store_load::store_batch(store, batch, load_cancelled) -> LoadResult`:
  `message` for each received message (identity text, fields, date, read
  state, content); `replace_inbox`; `Stored` → `log_received_batch` (moved
  from `inbox.rs`) and `LoadResult::Stored { incomplete }`; `LoadCancelled` →
  `Cancelled`; a failure → the load's error line with `cause` naming the
  kind, then `Failed(failure)`.

**`mailbag/src/failure_declarations.rs`**

- `declare_failure`: arms for `StorageFull` ("advice: free disk space"),
  `MailNotSaved` and `StoredMailUnreadable`, all with Retry (wording final in
  code).

**`mailbag/src/failure_dialog.rs`**

- `show_action_button(button, action, retried: RetriedOperation)`: Retry's
  action name from the operation: `RefreshInbox` → `app.refresh-inbox`,
  `ReadStoredInbox` → `app.read-stored-inbox`; `present(parent, failure,
  retried)` passes the same operation to the dialog's button.

**`mailbag/src/window_ui.rs`**

- `WindowUi::new(builder, loader, store)`; the `read-stored-inbox` action,
  published by `main.rs` beside `refresh-inbox`.
- `show_selected_account(self: &Rc<Self>)`: on selection, reads the
  selected account's Inbox unless it is the one on screen or being read, so
  selecting it again keeps the open message; every write to it is followed
  by a read anyway.
- `read_shown_inbox(self: &Rc<Self>)`: the selected account; a new read
  number; spawn on GTK's context: `run_on_pool(StoredMailUnreadable,
  read_inbox)`, which is `gio::spawn_blocking(catch_panic(work))` and is
  shared with the deletion; a caught panic becomes the read's own failure,
  `StoredMailUnreadable` with the panic in its details (006 FR-014); a
  failure writes the read's
  error line; keep the answer only if its number is still the latest, so
  neither another account's answer nor an older read of the same account
  replaces what a newer read found; `render`.
- `render`: account page first; then, for the selected account, the first
  that applies: stored rows → the list, with the banner for the latest
  refresh's failure or incomplete list; a read in flight → the list's page
  with no rows, so no older state shows while it runs; a load running →
  "Loading Inbox"; a read failure → the failure page with
  `ReadStoredInbox`; a failed refresh → the failure page with
  `RefreshInbox`; an empty stored Inbox → "Inbox is empty", with the banner
  for an incomplete list; nothing stored → "No mail loaded". After a refresh forgets a read failure, nothing is read
  and nothing is in flight, so the list says "Loading Inbox" while the load
  runs and shows the refresh's own outcome after it.
- `refresh_inbox`: as today, and it forgets the shown account's read
  failure, so the list shows the refresh's outcome, the newest failure.
- `finish_load`: record the outcome; a `Stored` outcome of the shown account
  → `read_shown_inbox`.
- `apply_account_update`: as today, the exclusion's cancellation first, so
  a load finishing meanwhile finds itself cancelled under the store's lock
  (research §6); and it forgets what was read of an account no longer
  shown, whose mail may be deleted, dropping a read still running for it;
  then on a complete answer `delete_removed_accounts(update)`:
  `delete_other_accounts` with the answer's accounts present with Mail on, through
  `run_on_pool` (research §6, "Deletions in order"); each
  deleted account and a failure go to the record. It does not use
  `shows_account`: an account whose Mail service is missing is not shown,
  and keeps its mail (spec FR-008).

**`mailbag/src/refreshes.rs`**

- `Refreshes`: `outcomes: BTreeMap<AccountId, RefreshOutcome>` and the
  running load. `begin_load` keeps the outcome, so the banner stays during a
  refresh (006 FR-008); `finish_load` records `Stored(incomplete)` or
  `Failed(failure)`, ignores `Cancelled` and a result whose load was
  cancelled by an exclusion; `discard_excluded` forgets outcomes and cancels
  as today.

**`mailbag/src/mail_ui.rs`**

- `show_inbox(account_id, &Rc<[Message]>)`: today's `show_batch`; rows and
  the reader read from the shown Inbox, kept with its account in
  `listed_inbox`; the "message opened" line names the account and the
  message's identity; the reader's status page keeps `RefreshInbox` for
  Retry.

**`mailbag/src/main.rs`**

- `connect_account_updates`: `Arc::new(Store::at(user_data_dir/mailbag/
  mail.sqlite))`, given to `MailLoader` and `WindowUi`; `read-stored-inbox`
  published and removed with the window, like `refresh-inbox`.

## Optional mechanisms

None is planned. Each would need the situation named beside it.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Reading a message's content when it opens | Stored texts so large that reading an Inbox shows a wait or holds too much memory | ~40 lines: a second read, a message replaced meanwhile, a reader failure |
| rusqlite's statement cache | Preparing statements shows up in a measurement | A feature flag; `foldhash` brings the Zlib licence into `deny.toml` |
| A busy timeout | A second process opens the same store; the application runs once per session today | 1 line |
| A thread of Mailbag's own for the store | GIO's pool, shared with GIO's own work, delays the window's reads visibly | ~60 lines ([research §2](research.md)) |
| Reclaiming file space | The file keeps growing; whole-Inbox replacement reuses freed pages, so it does not | 1 line (`auto_vacuum`) |

## Portions and review pauses

One commit each, with its tests and `scripts/check.sh`; stop after each for
the maintainer's review and compare the size with the table above.

0. **Failures as domain values.** 006's portion 6 (006 tasks T028–T032),
   with 006's own budget. Suggested commit: "Hand failures to the application
   as domain values".
1. **The store.** The domain's additions (`AccountId` and `DisplayFields`
   moved, `Message`, the store's kinds with their wording, since the
   declarations' `match` is exhaustive, `catch_panic`, `Failure::from_panic`)
   and `mailbag-store` with everything in its function map and its tests; the
   workspace, `rusqlite`, `cargo-sources.json`, the store's dependency rule
   in `check.sh`, `sqlite-devel` in `setup.sh`, CI and the README. The
   application does not use the store yet. The 001 data model is amended
   first. Suggested commit: "Add the mail store".
2. **Loads write, the window reads.** The worker writes;
   `LoadResult::Stored`; Retry by carrier and
   `app.read-stored-inbox`; the window reads through GIO's pool, keeps rows
   during a refresh, shows the banner over rows after a failed one, the
   failure page for a read failure; `main.rs` creates the store. The 002
   documents and 006's spec and contract are amended first. Tests: provider
   sequences called directly; the worker's write, storage failure and panic;
   the window's US1–US3 and US5 scenarios through the scripted loader
   writing into a test store; a read failure's Retry reads again. Suggested
   commit: "Show mail from the store".
3. **Accounts leave.** `delete_removed_accounts` on complete answers (the
   late-result refusal itself is built with the worker's write in portion
   2). Tests: US4 through scripted Online Accounts updates; a result
   arriving after the exclusion stores nothing. Suggested commit: "Delete the stored mail of removed accounts".

After portion 3: the GUI tests one by one, `simplify-review` on the branch
diff in a fresh subagent, then the manual checks of
[quickstart.md](quickstart.md) on the installed build.

## Technical Context

**Language/Version**: Rust 1.95, edition 2024, as the workspace.
**Primary Dependencies**: gtk4 0.11, libadwaita 0.9 (`v1_8`), glib/gio
0.22; `rusqlite` 0.40 without default features, against the system SQLite
(GNOME 50 runtime 3.50.4; Fedora 44 3.51.2).
**Storage**: one SQLite file, `<user data directory>/mailbag/mail.sqlite`,
WAL; [data-model.md](data-model.md).
**Testing**: `cargo test` with the scripted IMAP server, the scripted Graph
service, the Online Accounts test double and stores in memory or in a
temporary directory; the window through the scripted loader; manual checks
in [quickstart.md](quickstart.md).
**Target Platform**: GNOME desktop, native and Flatpak.
**Project Type**: desktop application.
**Constraints**: no store access on GTK's thread; no thread or timer of
Mailbag's own; no widget built in code, no form change; nothing that is not
mail stored; domain values only between layers and the window (006
contract); the budget above.

## Constitution Check

- **I. Necessary complexity**: every piece answers a spec requirement with a
  situation that happens today: offline reading (US1), the stale banner
  (US3), a late result after a removal (research §6), a damaged file
  (research §4), a panic in a read (006 FR-014), a Retry that repeats the
  read that failed (research §10). The domain's additions are what the store
  shares with the other layers today. The target model, change
  notification, migrations and a store thread are deferred or optional with
  their situations.
- **II. Clear language and names**: `Store`, `replace_inbox`, `read_inbox`,
  `delete_other_accounts`, `Message`,
  `RefreshOutcome`, `RetriedOperation`, `InboxWrite::LoadCancelled`,
  `catch_panic`, `StorageFull`,
  `MailNotSaved`, `StoredMailUnreadable`; the record's lines name the account
  and the reason.
- **III. Explicit failures and truthful state**: "Inbox is empty" only after
  a completed load stored an empty Inbox; a failed write is a failed load; a
  read failure is a failure page, never "No mail loaded"; a discarded store
  is logged with its reason.
- **IV. One owner**: the domain crate owns the shared definitions; the store
  owns the stored form, the reading of SQLite's codes and the check that
  keeps a cancelled load's result out; `mailbag` owns every failure's wording and what Retry repeats; the
  worker owns writing a load; Online Accounts owns which accounts exist.
- **V. Responsive, bounded work**: SQL runs on the mail worker or GIO's
  pool, never on GTK's thread; the whole shown Inbox is at most 100
  messages.
- **VI. Evidence**: tests per portion; the probes of [research.md](research.md);
  the installed build checked by [quickstart.md](quickstart.md).

Gates pass; no violation to justify.

## Project Structure

### Documentation (this feature)

```text
specs/007-mail-storage/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── checklists/requirements.md
└── tasks.md
```

No contract document of its own: the store's interface is the functions of
the function map, in domain types; what the store gives the window for its
failures is added to 006's contract.

### Source Code

```text
crates/mailbag-domain/                     from 006 portion 6; 007 adds
                                           AccountId, DisplayFields, Message,
                                           three kinds, catch_panic
crates/mailbag-store/                      new crate: Cargo.toml, src/lib.rs,
                                           src/open.rs, src/content.rs,
                                           src/failure.rs, src/schema.sql, tests
crates/goa-adapter/src/account_model.rs    AccountId moved out
crates/mailbag-content/src/lib.rs          DisplayFields moved out
crates/mailbag-providers/src/worker.rs     write after the load
crates/mailbag-providers/src/store_load.rs new: batch → store, record lines
crates/mailbag-providers/src/batch.rs      LoadResult::Stored
crates/mailbag-providers/src/lib.rs        MailLoader with the store
crates/mailbag/src/failure_declarations.rs the store's three kinds
crates/mailbag/src/failure_dialog.rs       Retry by carrier
crates/mailbag/src/main.rs                 the store's place, the read action
crates/mailbag/src/window_ui.rs            reads, pages, account deletion
crates/mailbag/src/refreshes.rs            refresh outcomes
crates/mailbag/src/mail_ui.rs              list and reader from the stored Inbox
scripts/check.sh, scripts/setup.sh, .github/workflows/check.yml, README.md,
Cargo.toml, Cargo.lock, cargo-sources.json
```

## Documents amended before implementing

| Document | Change | Portion |
|---|---|---|
| specs/006-error-handling/ (research, contract, plan, tasks, spec status) | Failures reach the application as domain values; 006's portion 6 | done 2026-09-26, before 0 |
| specs/001-goa-account-observation/data-model.md; contracts/accounts.md | `AccountId` lives in `mailbag-domain`; the domain refuses an empty identifier and `goa-adapter` turns that into its `InvalidReply` | 1 |
| specs/006-error-handling/contracts/failure-declaration.md | The store's kinds (`StorageFull`, `MailNotSaved`, `StoredMailUnreadable`), produced by the store | 1 |
| specs/006-error-handling/contracts/failure-declaration.md | A new carrier, a stored Inbox that cannot be read, with Retry reading it again; Retry's operation chosen by carrier (`RetriedOperation`) | 2 |
| specs/006-error-handling/spec.md | User Story 2 and FR-013(a) marked as built by 007 | 2 |
| specs/002-imap-integration/spec.md | FR-006's stage rule, FR-008's "MAY discard", FR-009's empty list after a failed refresh, SC-004's stage part and the Clarification "What does a refresh keep?" marked as amended by 007 | 2 |
| specs/002-imap-integration/data-model.md | "In-Memory Data": the batch no longer reaches the window; Refresh no longer clears the list | 2 |
| specs/002-imap-integration/contracts/ui.md | "Refresh clears the list, selection and reader before loading" replaced by 007 FR-005 | 2 |

## Post-implementation

Acceptance on 2026-09-26 by the maintainer, with the maintainer's accounts
and the Flatpak built from the branch and installed
(`scripts/build-flatpak.sh --install`), following
[quickstart.md](quickstart.md). Every step gave the expected result.

| Step | Result |
|---|---|
| 1. Stored mail without a network (US1) | Passed: the same rows, read states and texts after a restart offline; no load started |
| 2. A refresh replaces the Inbox (US2) | Passed: rows kept with the spinner, the new message stored, the reader closed; selecting the shown account again kept the open message |
| 3. A failed refresh keeps the mail (US3) | Passed: rows under the banner, Retry in its dialog; no banner after a restart |
| 4. An account leaves (US4) | Passed: no stored row for the account with Mail off; with Mail on again it showed "No mail loaded" until refreshed |
| 5. A damaged store (US5) | Passed: one warning line, discarded as not a store; every account without mail until refreshed |
| 6. No visible wait (FR-011, SC-007) | Passed |
| 7. Privacy (FR-009) | Passed: `700` |
| 8. The record (SC-008) | Passed: no subject, sender, text, password or token; the deletion named by the account's identifier |

Verified by tests only: a store written with another structure and a
damaged store with a valid header (the store's tests); a write that fails
(`MailNotSaved`, the store's and the providers' tests); a full disk
(`StorageFull`: the store's size limit makes SQLite report `SQLITE_FULL`,
and the previous Inbox stays whole, added at the final review); a failed
load over a stored Inbox leaves it as it was (the providers' test); a stored
Inbox that cannot be read and its Retry (`a_store_that_cannot_be_read`); a
panic in store work on GIO's pool, which is the failure of that work's own
kind (the window's test, added at the final review); a load that ends after
its account was excluded; an empty page whose service offered more; the
banner absent after a restart and back after another account (the window's
graphical test). Killing the process seven times during a write left the
previous or the new Inbox whole each time (checked 2026-09-26).

Not verified: a power loss during a write (FR-010); a deletion that fails
and happens again at the next complete answer; a lock poisoned by a panic
inside the store's own code, which `with_connection` takes over.

### Size

Measured on 2026-09-26 with `git diff --numstat`, a line moved between files
counted once; comments and blank lines included, as for 006. Accepted by the
maintainer at the final review on 2026-09-26.

| Part | Budget | Measured |
|---|---|---|
| 007, production | ≤ 650 net | +1032 after the final review (Rust +969; `schema.sql` and manifests +63); without comments and blank lines about +740 |
| 007, tests | ≈ 450 | +730 after the final review |

The difference comes from requirements rather than extra mechanisms (final
simplify review): the window's reading states, numbered reads and the read
failure's own Retry make `window_ui.rs` +287 against about 130 estimated;
the store is about 490 against 365. The final review itself added +23
production and +26 test lines net.

### Final review (2026-09-26)

Four reviews in fresh contexts (consistency, convergence, simplicity,
correctness) found no data loss, no mail shown for the wrong account and no
leak into the record. Applied: the order of the list's states puts a read in
flight first, so no older page shows while it runs; a read's panic is the
read's own failure; the schema and its version are created in one
transaction; tests for a full disk, the store's three failures' wording, a
failed load over a stored Inbox, a panic on the pool and two banner checks;
names (`Refreshes` in `refreshes.rs`, `delete_other_accounts`,
`listed_inbox`) and the shared `TestDirectory`; tests that repeated a
`match` line for line removed. Kept by the maintainer's decision: Retry of a
failed read reads the store again (006 FR-003), and the loads keep their own
`ReceivedMessage` until the labels feature (004 FR-005).
