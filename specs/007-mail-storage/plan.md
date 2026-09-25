# Implementation Plan: Mail Storage

**Branch**: `claude/storage` | **Feature**: `007-mail-storage`
**Date**: 2026-09-25 | **Spec**: [spec.md](spec.md)
**Status**: Draft, paused on 2026-09-25 after the plan challenge and before
its findings were applied. The plan declares the store's failures in
`mailbag-providers`, where 006 placed every declaration; that placement
contradicts 006 FR-001 and FR-012 (each feature declares its failures in its
own code), and 006 is corrected first. The specification's decisions are
settled and are not reopened here.

## Size

The budget agreed at sizing on 2026-09-25, and this plan's estimate after
reading the code and counting what moves. Reassess with the maintainer before
exceeding the budget or about 1.5 times an item's estimate; at every review
pause the size so far is compared with this table.

| Item | Budget | This plan (estimate) |
|---|---|---|
| New modules and production lines | ≤ 600 net new | `mailbag-store` ≈ 370 new (store and its operations ~170, opening and discarding ~80, schema ~30, content codes ~50, error ~40) and ~25 moved in (`ReceivedContent`); `mailbag-providers` ≈ +100 (worker write and panic helper ~55, `store_load.rs` ~20 new and ~45 moved from the window, storage declaration ~35) and ~25 moved out; `mailbag` ≈ +140 (window reads and account deletion ~105, main ~8, list ~5) and ~40 removed (`AccountInbox`, batch handling). **Net ≈ 560**, close to the budget; the window is the likeliest to grow |
| Call sites or existing files touched | — | Rust: providers `lib.rs`, `batch.rs`, `worker.rs`, `failure.rs`, `imap_batch.rs`, `microsoft365.rs` (the moved type's path), new `store_load.rs`; mailbag `main.rs`, `inbox.rs`, `window_ui.rs`, `mail_ui.rs`. Build and checks: workspace `Cargo.toml`, two crate manifests, `Cargo.lock`, `cargo-sources.json`, `scripts/check.sh` (the new crate's dependency rule), `scripts/setup.sh`, `.github/workflows/check.yml` and the README's prerequisites (`sqlite-devel`). No form changes |
| New threads, timers, queues | 0 | 0: GIO's pool for the window's calls, the existing mail worker for writes |
| New state, types, error types | — | Types: `Store`, `StoredMessage`, `InboxWrite`, `StoreError`, `StoreOperation`, `RefreshOutcome` (replaces `AccountInbox`). State: the accounts of the latest complete answer inside `Store`; the shown account's stored Inbox in the window |
| New fields in existing data | — | `LoadResult::Received(batch)` becomes `LoadResult::Stored { incomplete }`; `LoadFailure::Storage(StoreError)` is added; persisted: two tables ([data-model.md](data-model.md)) |
| Changes to other features' contracts or documents | 002; 006 | 002 spec, data model and UI contract (memory-only mail, refresh clears the list); 006 spec (User Story 2 and FR-013(a) built). 001, 003, 004, 005 unchanged |
| New dependencies | 1 | `rusqlite` 0.40 without default features: `libsqlite3-sys` and four small crates, all MIT or Apache-2.0. `deny.toml` stays as it is: the Zlib licence approved at sizing came only with the default statement cache ([research §1](research.md)) |
| Tests | ≈ 450 | ≈ 420 net: store ~200 (in memory and on a temporary file); providers ~90 (the sequence tests call the load directly, so their assertions stay; worker writes, storage failure, late result); window ~180 (the scripted loader writes into a test store; US1–US5 scenarios); `inbox/tests.rs` rewritten, about −150 +100 |

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

## Minimal version

Everything below is built, in the three portions of the last section.

| Step | What it does | Cost |
|---|---|---|
| The store | `mailbag-store`: `Store` holds the file's path, one connection behind a mutex opened at first use, and the accounts of the latest complete answer; `replace_inbox`, `read_inbox`, `keep_accounts`; content codes; `StoreError` naming the operation and SQLite's error | ~370 lines, ~200 lines of tests |
| Opening and discarding | Create `mailbag/` with mode 0700, open, check version and damage, discard with one warning line, create the schema ([research §4](research.md)) | inside the store's ~370 |
| Loads write | The worker converts the batch into `StoredMessage`s and calls `replace_inbox` inside the load's panic guard; `Stored { incomplete }`, `Failed(Storage)`, or `Cancelled` when the account was removed; the "Inbox load finished" record line moves here from the window | ~+100 in providers |
| The window reads | On selecting an account and after a completed load of the shown account, read its Inbox through `gio::spawn_blocking`; show rows, "Inbox is empty", "No mail loaded" or a failure page; rows stay while loading; the banner shows the latest refresh's failure or incomplete list | ~+105 in the window |
| Accounts leave | On a complete answer, `keep_accounts` with the accounts present with Mail on; deleted accounts and failures go to the record | inside the window's ~105 |

Not built: a store thread, a change notification, migrations, reading one
message's content separately, a statement cache, a Reset control, folders,
membership, UIDVALIDITY, Gmail labels, thread identifiers (spec FR-014).

## Function map

The entry points and their steps, as the code will read.

**`mailbag-store`**

- `Store::at(path: PathBuf) -> Store`: no I/O; the file opens at first use.
  `Store::in_memory()` for tests.
- `Store::replace_inbox(&self, account: &AccountId, messages:
  Vec<StoredMessage>) -> Result<InboxWrite, StoreError>`: lock; an account
  outside the latest complete answer → `InboxWrite::AccountRemoved`; else one
  transaction: delete the `inbox` row, insert it, insert the messages in
  order, commit → `InboxWrite::Stored`.
- `Store::read_inbox(&self, account: &AccountId) ->
  Result<Option<Vec<StoredMessage>>, StoreError>`: `None` without an `inbox`
  row; the messages in load order with their content otherwise.
- `Store::keep_accounts(&self, accounts: BTreeSet<AccountId>) ->
  Result<Vec<AccountId>, StoreError>`: record the set; delete every other
  account's `inbox` row; return the accounts whose mail was deleted, for the
  record.
- `connection(&self)`: the locked connection, opened by `open_store` at first
  use; a poisoned lock is taken over, since a transaction a panic
  interrupted was rolled back.
- `open_store(path) -> Result<Connection, StoreError>`:
  `create_private_directory`, `Connection::open`, `examine_existing` →
  `Usable | Empty | Discard(reason)`; `discard_store(path, reason)` removes the
  three files and logs; `create_schema` runs the schema text and sets
  `user_version`; the connection settings of the data model.
- `schema_version() -> i32`: FNV-1a over the schema text.
- `content_columns(&ReceivedContent)` and `content_from_columns(kind,
  detail)`: the codes of the data model; an unknown code is a `StoreError`.

**`mailbag-providers`**

- `MailLoader::new(accounts, store: Arc<Store>)`: installs the panic hook, then
  as today; the worker gets the store.
- `worker::load_catching_panics(kind, &store)`: inside the existing guard, the
  provider's load and then `store_load::store_batch(&store, batch)`.
- `store_load::store_batch(store, batch) -> LoadResult`: `stored_message` for
  each message (identity text, fields, date, read state, content);
  `replace_inbox`; `Stored` → `log_received_batch` (moved from `inbox.rs`) and
  `LoadResult::Stored { incomplete }`; `AccountRemoved` → `Cancelled`;
  an error → `Failed(LoadFailure::Storage(error))`.
- `worker::run_catching_panic(call) -> Result<T, String>`: for the window's
  calls on GIO's pool; the message and place the hook kept on that thread.
- `failure.rs`: `LoadFailure::Storage` declared by operation ("Mail not
  saved" for a write, "Stored mail unreadable" for a read, wording final in
  code), action Retry, advice to free disk space for a full disk; `Failure:
  Storage(Write)` and SQLite's error in the technical details;
  `declare_content(&ReceivedContent)` replaces the method on the moved type.

**`mailbag/src/window_ui.rs`**

- `WindowUi::new(builder, loader, store)`.
- `read_shown_inbox(self: &Rc<Self>)`: the selected account; spawn on GTK's
  context: `gio::spawn_blocking(run_catching_panic(read_inbox))`; on the
  answer, keep it only if that account is still selected; `render`.
- `render`: account page first; then, for the selected account:
  rows → the list, with the banner for the latest refresh's failure or
  incomplete list; no rows yet read → the list's page with no rows; a read
  failure → the failure page; loading → "Loading Inbox"; a failed refresh →
  the failure page; an empty stored Inbox → "Inbox is empty"; nothing stored
  → "No mail loaded".
- `finish_load`: record the outcome; a `Stored` outcome of the shown account
  → `read_shown_inbox`.
- `apply_account_update`: as today; on a complete answer
  `delete_removed_accounts(update)`: `keep_accounts` with the accounts present
  with Mail on, through `gio::spawn_blocking`; each deleted account and a
  failure go to the record. It does not use `shows_account`: an account whose
  Mail service is missing is not shown, and keeps its mail (spec FR-008).

**`mailbag/src/inbox.rs`**

- `InboxController`: `outcomes: BTreeMap<AccountId, RefreshOutcome>` and the
  running load. `begin_load` keeps the outcome, so the banner stays during a
  refresh (006 FR-008); `finish_load` records `Stored(incomplete)` or
  `Failed`, ignores `Cancelled` and a result whose load was cancelled by an
  exclusion; `discard_excluded` forgets outcomes and cancels as today.

**`mailbag/src/mail_ui.rs`**

- `show_inbox(&Rc<[StoredMessage]>)`: today's `show_batch`; rows and the
  reader read from the shown Inbox; the "message opened" line names the stored
  identity.

**`mailbag/src/main.rs`**

- `connect_account_updates`: `Arc::new(Store::at(user_data_dir/mailbag/
  mail.sqlite))`, given to `MailLoader` and `WindowUi`.

## Optional mechanisms

None is planned. Each would need the situation named beside it.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Reading a message's content when it opens | Stored texts so large that reading an Inbox shows a wait or holds too much memory | ~40 lines: a second read, a message replaced meanwhile, a reader failure |
| rusqlite's statement cache | Preparing statements shows up in a measurement | A feature flag; `foldhash` brings the Zlib licence into `deny.toml` |
| Correcting the rights of an existing store directory | The directory existed with wider rights before Mailbag created it | ~3 lines |
| A busy timeout | A second process opens the same store; the application runs once per session today | 1 line |
| A thread of Mailbag's own for the store | GIO's pool, shared with GIO's own work, delays the window's reads visibly | ~60 lines ([research §2](research.md)) |
| Reclaiming file space | The file keeps growing; whole-Inbox replacement reuses freed pages, so it does not | 1 line (`auto_vacuum`) |

## Portions and review pauses

One commit each, with its tests and `scripts/check.sh`; stop after each for
the maintainer's review and compare the size with the table above.

1. **The store.** `mailbag-store` with everything in its function map and its
   tests; the workspace, `rusqlite`, `cargo-sources.json`, the dependency rule
   in `check.sh`, `sqlite-devel` in `setup.sh`, CI and the README. The application does
   not use it yet. Suggested commit: "Add the mail store".
2. **Loads write, the window reads.** `ReceivedContent` moves; the worker
   writes; `LoadResult::Stored`; the storage declaration; the window reads
   through GIO's pool, keeps rows during a refresh, shows the banner over
   rows after a failed one, the failure page for a read failure; `main.rs`
   creates the store. The 002 and 006 documents are amended first. Tests:
   provider sequences called directly; the worker's write, storage failure
   and panic; the window's US1–US3 and US5 scenarios through the scripted
   loader writing into a test store. Suggested commit: "Show mail from the
   store".
3. **Accounts leave.** `delete_removed_accounts` on complete answers; the
   late-result refusal through the worker. Tests: US4 through scripted
   Online Accounts updates; a result arriving after the deletion stores
   nothing. Suggested commit: "Delete the stored mail of removed accounts".

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
mail stored; the budget above.

## Constitution Check

- **I. Necessary complexity**: every piece answers a spec requirement with a
  situation that happens today: offline reading (US1), the stale banner
  (US3), a late result after a removal (research §6), a damaged file
  (research §4). The target model, change notification, migrations and a
  store thread are deferred or optional with their situations.
- **II. Clear language and names**: `Store`, `replace_inbox`, `read_inbox`,
  `keep_accounts`, `StoredMessage`, `InboxWrite::AccountRemoved`,
  `RefreshOutcome`; the record's lines name the account and the reason.
- **III. Explicit failures and truthful state**: "Inbox is empty" only after
  a completed load stored an empty Inbox; a failed write is a failed load; a
  read failure is a failure page, never "No mail loaded"; a discarded store
  is logged with its reason.
- **IV. One owner**: the store owns the stored form and the late-result
  rule; the worker owns writing a load; the window owns what is shown;
  Online Accounts owns which accounts exist.
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
└── tasks.md            (next: speckit-tasks)
```

No contract document: the store's interface is the four functions of the
function map, used by two crates of this repository.

### Source Code

```text
crates/mailbag-store/                      new crate: Cargo.toml, src/lib.rs,
                                           src/open.rs, src/content.rs,
                                           src/error.rs, src/schema.sql, tests
crates/mailbag-providers/src/worker.rs     write after the load; panic helper
crates/mailbag-providers/src/store_load.rs new: batch → store, record line
crates/mailbag-providers/src/batch.rs      LoadResult::Stored, Storage failure
crates/mailbag-providers/src/failure.rs    storage declaration, content function
crates/mailbag-providers/src/lib.rs        MailLoader with the store
crates/mailbag/src/main.rs                 the store's place
crates/mailbag/src/window_ui.rs            reads, pages, account deletion
crates/mailbag/src/inbox.rs                refresh outcomes
crates/mailbag/src/mail_ui.rs              list and reader from the stored Inbox
scripts/check.sh, scripts/setup.sh, .github/workflows/check.yml, README.md,
Cargo.toml, Cargo.lock, cargo-sources.json
```

## Documents amended before implementing

| Document | Change | Portion |
|---|---|---|
| specs/002-imap-integration/spec.md | FR-006's stage rule, FR-008's "MAY discard", FR-009's empty list after a failed refresh, SC-004's stage part and the Clarification "What does a refresh keep?" marked as amended by 007 | 2 |
| specs/002-imap-integration/data-model.md | "In-Memory Data": the batch no longer reaches the window; Refresh no longer clears the list | 2 |
| specs/002-imap-integration/contracts/ui.md | "Refresh clears the list, selection and reader before loading" replaced by 007 FR-005 | 2 |
| specs/006-error-handling/spec.md | User Story 2 and FR-013(a) marked as built by 007 | 2 |
