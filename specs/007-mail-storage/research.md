# Research: Mail Storage

Decisions that had alternatives or needed a probe. Probes ran on 2026-09-25
outside the repository, against the GNOME 50 runtime installed on the
maintainer's machine and on the host (Fedora 44).

## 1. SQLite through rusqlite, with the system library

**Decision**: `rusqlite` 0.40 with `default-features = false`, linked against
the SQLite the platform provides: the GNOME 50 runtime in the Flatpak, the
distribution's library on the host. No `bundled` feature.

**Evidence** (checked):

- The GNOME 50 runtime ships `libsqlite3.so` 3.50.4 and the SDK its
  `sqlite3.pc`; the runtime's library is compiled with `ENABLE_FTS5` and
  `THREADSAFE=1`, and write-ahead logging (WAL) works: `sqlite3` inside the
  runtime created the `-wal` and `-shm` files and answered an FTS5 query.
  Fedora 44 has `sqlite-devel` 3.51.2.
- `rusqlite` 0.40.2 links the system library without `bundled`. Its default
  features pull in `hashlink`, `hashbrown` and `foldhash`, the last under the
  Zlib license, which `deny.toml` does not allow; they serve only
  `prepare_cached`, a statement cache. With `default-features = false` the
  tree is `libsqlite3-sys` (MIT) with `bitflags`, `smallvec`,
  `fallible-iterator` and `fallible-streaming-iterator` (MIT or Apache-2.0),
  and at build time `pkg-config` and `vcpkg` (MIT or Apache-2.0). The
  licence list stays as it is; the Zlib addition approved at sizing is not
  needed.
- `STRICT` tables need SQLite 3.37; both libraries are newer.

**Why**: the domain needs SQL with transactions now (FR-004, FR-010) and
full-text search later (the search feature), which FTS5 in the runtime covers.
The system library takes its security updates from the runtime.

**Alternatives**: the `bundled` feature (compiles and ships a second SQLite,
updated only with Mailbag); `sqlx` (asynchronous, needs a Tokio or async-std
runtime, which the project avoids, see 002 research §3); `diesel` (an object
mapper with its own migrations, far more than two tables need); a key-value
store (no SQL, no full-text search).

## 2. Where the SQL runs

**Decision**: one connection behind a mutex inside `mailbag_store::Store`,
shared as `Arc<Store>`. The mail worker calls the store directly on its own
thread, after a load. The window calls it through `gio::spawn_blocking`, which
runs the call on GIO's thread pool and returns a future on GTK's context. No
thread of Mailbag's own.

**Evidence** (checked with a prototype):

- A read started from a GLib main context through `gio::spawn_blocking`
  while another thread held the mutex for 500 ms returned after that thread
  released it; the main context ran a 50 ms timeout nine times meanwhile, so
  it was not blocked.
- `gio::spawn_blocking` wraps the call in `catch_unwind` and returns a panic
  as `Err` (gio 0.22.9, `task.rs:519`). A panic inside a transaction rolled
  it back; the poisoned mutex stays usable through
  `PoisonError::into_inner`.
- `rusqlite::Connection` is `Send` (rusqlite 0.40.2 `lib.rs:364`), so a
  mutex makes it shareable.
- Cost on the maintainer's machine, WAL with `synchronous=NORMAL`: writing
  100 messages of 5 KB in one transaction 1.2–1.6 ms; reading 100 rows
  45–52 µs; about 550 KB on disk.

**Why**: the window never waits for a load's network work, only for another
store call, which takes milliseconds (FR-011). The store's functions stay
ordinary synchronous functions, tested without threads.

**Alternatives**: a store thread of Mailbag's own with a request channel
(works equally in the prototype; more code for the same result); all store
work on the mail worker with a copy of the stored mail in the window's
memory (proposed at the specification challenge: the window would wait for a
running load, or keep a second copy of the mail, which the goal rules out);
SQL on GTK's thread (constitution V).

## 3. A message's content is read with the list

**Decision**: reading an account's Inbox returns every stored message with
its list fields and its reader content, as the in-memory batch holds today.
Opening a message reads nothing more.

**Why**: one read per shown Inbox, no second asynchronous path for the
reader, no race between a message being opened and its Inbox being
replaced. Memory is what the batch uses today, and only the shown account's
Inbox is held. A separate failure for one message's stored content therefore
cannot happen and was dropped from the specification at planning.

**Alternative**: read the content when a message opens (less memory with very
large texts; a second read path, a stale-key case and a reader failure).
Listed as optional in the plan.

## 4. The structure's version, and what is discarded at start

**Decision**: the schema is one SQL text in the store crate. Its version is a
32-bit FNV-1a hash of that text, kept in the database header's `user_version`
("the user-version integer at offset 60 in the database header",
sqlite.org/pragma.html). The codes a column may hold (the content kinds) are
`CHECK` constraints in the same text, so changing them changes the version.

At the first use of the store, which is the start of FR-012: the first
Online Accounts answer or the first selected account reaches the store
right after Mailbag starts:

| Found | Result |
|---|---|
| No file, or a file without tables and version 0 | Create the schema |
| The same version, `PRAGMA quick_check` answers `ok` | Use it |
| Another version, or tables with version 0 | Discard: structure changed |
| SQLite reports `NotADatabase` | Discard: not a store |
| SQLite reports `DatabaseCorrupt`, or `quick_check` finds errors | Discard: damaged |
| Any other failure (the directory cannot be created, permission, I/O) | No discard; the call fails and the next call tries again (FR-013) |

Discarding removes the file and its `-wal` and `-shm` files, writes one
warning line naming the reason, and creates an empty store. `quick_check`
"runs in O(N) time" (sqlite.org/pragma.html); the store is a few hundred
kilobytes per account.

**Why**: FR-012 as decided at sizing and at the challenge; the hash needs no
manual step when the schema changes.

**Alternatives**: a version number raised by hand (forgotten easily, tests do
not notice); migrations (none before the first release, FR-012).

## 5. Durability

**Decision**: `journal_mode=WAL`, `synchronous=NORMAL`. The SQLite
documentation (sqlite.org/pragma.html, checked): "Transactions are durable
across application crashes regardless of the synchronous setting or journal
mode"; "A transaction committed in WAL mode with synchronous=NORMAL might
roll back following a power loss or system crash." This is FR-010 as
written: after a power loss the latest load may be missing, never half of it.

**Alternative**: `synchronous=FULL` (a sync per commit) protects a load the
next refresh obtains again anyway.

## 6. A late result for a removed account

**Situation**: Online Accounts' complete answer removes an account while its
load is finishing. The window cancels the load, but the worker may already be
past its last network step, and its write could land after the deletion
(FR-007, FR-008).

**Decision** (the plan challenge's, accepted 2026-09-26): `replace_inbox`
takes the load's cancellation as a check and runs it under the store's lock,
before its transaction; a cancelled load writes nothing
(`InboxWrite::LoadCancelled`). The worker passes `|| cancelled.is_closed()`
on its cancellation channel. The window cancels an excluded account's load
before it asks the store to delete that account's mail, so whichever takes
the lock first, the result stays out: a write that came first is erased by
the deletion, a write that comes later finds its load cancelled.
`keep_accounts` only deletes and keeps no state.

**Why**: the store's calls from the window run on GIO's pool, where the
order of two tasks is not guaranteed; a set of accounts kept in the store
could be overwritten by an older answer applied last, and every refresh of an
account that was just enabled would then end silently as cancelled. The
cancellation needs no order between answers, and a late result becomes a
case of FR-004's rule that a cancelled load changes nothing.

**Deletions in order** (decided 2026-09-26, after the final review): each
deletion keeps the accounts of its own answer. Two complete answers close
together send two deletions to GIO's pool, which may run them in either
order; an older deletion could erase fresh mail only if it waited in the
pool for a whole refresh, seconds, which nothing in the application causes
(inferred). A set shared with the window, read under the store's lock, was
built first and removed: it did not keep the mail of an account whose Mail
was turned off by mistake and on again, since a deletion runs within
milliseconds, and when both answers came before the deletion it skipped the
deletion FR-008 requires. The window's reads are numbered, and only the
answer to the latest read is shown, because an older read's answer would
replace what the user sees.

**Checked**: `async_channel::Receiver::is_closed` is true once the only
sender, the load handle's cancellation, is dropped (async-channel 2.5.0);
`gio::spawn_blocking` runs its work on GIO's shared pool (gio 0.22.9
`task.rs:519`).

**Alternatives**: the accounts of the latest complete answer kept in the
store and checked under the same lock (the first version of this plan: out
of order answers leave a stale set); a table of accounts (removed at the
specification challenge); numbering the answers so the store ignores an older
one (four more lines to repair the stale set); the deletion sent through the
mail worker's queue (a second request kind in the worker).

## 7. Where the store lives

**Decision**: `<user data directory>/mailbag/mail.sqlite`, from
`glib::user_data_dir()`: `~/.var/app/io.github.mitinand.Mailbag/data/` in
the Flatpak, `~/.local/share/` on the host. The store creates the `mailbag`
directory with mode 0700.

**Evidence** (checked): Flatpak created `~/.var/app` and the application's
directories with mode 755; on this machine the home directory's mode 700 is
what keeps them private. Mode 0700 on the store's own directory keeps FR-009
where a home directory is readable by others.

## 8. Types and crates

Revised on 2026-09-26 after 006's amendment of the same day ([006 research
§1](../006-error-handling/research.md)): `mailbag-domain` holds the
application's shared definitions, and a lower layer hands the application
domain values only. 006's portion 6 creates the crate with the failure, the
content outcome and the short list; 007 builds on it.

**Decision**:

- `mailbag-store`, a new crate below `mailbag-providers`: providers (the mail
  worker) write, the window reads, and the store depends on neither. It
  depends on `mailbag-domain`, `rusqlite` and `tracing` only.
- 007 adds to `mailbag-domain` the definitions the store shares with the
  other layers:
  - `AccountId`, moved from `goa-adapter`: the store keys its data by it and
    must not depend on Online Accounts for it. The domain's `TryFrom<&str>`
    refuses an empty identifier; `goa-adapter` turns that refusal into its
    `InvalidReply` for an answer it cannot accept;
  - `DisplayFields`, moved from `mailbag-content`: the store keeps the list
    fields and must not depend on the MIME decoding crate for them;
    `mailbag-content` still decodes them;
  - `Message`: a message as the application keeps and shows it (the
    identity text for the record, the list fields, the received date, the
    read state, the content). Providers make it from what a load received,
    the store keeps it, the window shows it;
  - the store's failure kinds: `StorageFull`, `MailNotSaved` and
    `StoredMailUnreadable`.
- The store returns its failures as the domain's `Failure`: it reads
  SQLite's code (`DiskFull` → `StorageFull`; another failed write →
  `MailNotSaved`; a failed read → `StoredMailUnreadable`), writes SQLite's
  error into the technical details and a debug line, and writes no wording
  (006 contract).
- `ReceivedBatch` becomes private to `mailbag-providers`, so the window
  cannot receive mail that did not come through the store (FR-001): a
  `LoadResult` carries no mail.

**Alternatives**: the store's own message and error types (`StoredMessage`,
`StoreError`, the first version of this plan: the window would match a
second lower layer's types, which 006 now rules out); the store depending on
`goa-adapter` and `mailbag-content` for `AccountId` and `DisplayFields` (the
storage layer would pull in D-Bus and MIME decoding for two definitions); the
store depending on the provider crate and taking a `ReceivedBatch` (the store
would depend on the IMAP and Graph crates, and the worker would need the
window to write for it); the window writing the batch (the mail would reach
the window before the store).

## 9. Panics outside the mail worker

006 research §4: the code that sends work to a thread catches its panics
there, inside the work, where the hook's thread-local slot holds the message
and the place; the hook and its slot live in `mailbag-domain` (006 portion 6);
the helper for work outside the mail worker comes with 007.

**Decision**: `mailbag-domain` gets `catch_panic(work) -> Result<T, String>`,
which installs the hook once, runs the work under `catch_unwind` and returns
the message and place the hook kept for that thread, and
`Failure::stopped(panic: Option<String>)`, which builds the failure of kind
`Stopped` with the technical lines the worker writes today. The window wraps
each store call it sends to GIO's pool in the helper; a write on the worker
stays inside the load's existing guard.

**Why**: 006 FR-014 asks for the message and the place on every worker
thread; the payload that `gio::spawn_blocking` returns carries only the
message (checked, gio 0.22.9 `task.rs:519`). The window's first store read
may come before the mail worker ever started, so the helper installs the
hook itself.

**Alternatives**: the payload's message without the place (against 006
FR-014); the helper in `mailbag-providers` (the window would call the
loading crate for a rule that has nothing to do with loading).

## 10. Retry's operation for a failure the window reads itself

006's contract: Retry "runs the failed operation, which the window chooses
from the carrier"; every carrier in 006 is a load, so the mapping in
`failure_dialog::show_action_button` was fixed to `app.refresh-inbox`, and
006 left the choice by carrier to 007.

**Decision**: `show_action_button` takes the operation Retry runs, from the
channel that shows the failure: a load's failure, a short list and a
message's content keep `app.refresh-inbox`; a stored Inbox that cannot be
read uses a new application action, `app.read-stored-inbox`, which reads the
shown account's Inbox again (spec FR-013). The failure dialog receives the
same operation from the channel that opened it. The read's failure is the
store's `Failure`, or `Failure::stopped` for a caught panic; the window needs
no type of its own for it.

**Why**: a Retry that starts a network refresh does not repeat a failed read,
and without a network it would show the same page again while the load's
failure only reaches the record (found at the plan challenge).

**Alternatives**: no action on the read failure (006 FR-014 gives a panic's
failure Retry); Retry as a refresh (a different operation from the one that
failed).
