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

At the first use of the store:

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

**Decision**: `Store::keep_accounts` records the accounts of the latest
complete answer, present with Mail on, in the `Store` itself, in memory, and
deletes the mail of every other account; `replace_inbox` checks the account
against that record under the same mutex and stores nothing for an account
outside it. Before the first complete answer there is no record and nothing
is refused; no load can start before it, since an account is selected only
after one.

**Why**: the deletion and the check hold the same lock, so no order of the two
threads lets a late result back in. About eight lines, no table.

**Alternatives**: a table of accounts in the store (removed at the
specification challenge); sending the deletion through the mail worker's
queue behind the running load (a second request kind in the worker, and the
window then depends on the worker to delete mail).

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

**Decision**: a new crate, `mailbag-store`, below `mailbag-providers`:
providers (the mail worker) write, the window reads, and the store depends on
neither. `ReceivedContent` moves from `mailbag-providers` into the store,
which owns its stored form; its declaration becomes a function in
`failure.rs`. `ReceivedBatch` becomes private to `mailbag-providers`, so the
window cannot receive mail that did not come through the store (FR-001): a
`LoadResult` carries no mail.

**Alternatives**: the store depends on the provider crate and takes a
`ReceivedBatch` (the store would depend on the IMAP and Graph crates, and the
worker would need the window to write for it); the window writes the batch
(the mail would reach the window before the store).

## 9. Panics in store calls

**Decision**: on the mail worker, the store write runs inside the load's
existing `catch_unwind` (006 FR-014), so a panic there is a failed load with
its message and place. For the window's calls on GIO's pool, the panic hook is
installed when `MailLoader` is created, before any call, and a small
`run_catching_panic` in `worker.rs` wraps the call on the pool thread and
takes the message and place the hook kept for that thread. A panic in a read
becomes `LoadFailure::WorkerStopped(Some(panic))`, declared as today.

**Why**: 006 FR-014 asks for the message and the place; the payload that
`gio::spawn_blocking` returns carries only the message.
