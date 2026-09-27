# Implementation Plan: Folders

**Branch**: `claude/folders` | **Feature**: `008-folders`
**Date**: 2026-09-27 | **Spec**: [spec.md](spec.md)
**Status**: Approved on 2026-09-27. The budget was raised on 2026-09-27 to the plan's
estimate after the clarification's additions; challenged the same day
(four simplifications applied, one deferral declined by the maintainer's
earlier decision), aligned with the spec and tasks by the consistency
analysis, and corrected after an external review the same day; see Size.
Before the window portion (2026-09-27) the budget was raised to the
forecast, and the empty list's hidden account and the folder-list read's
own Retry were dropped (spec Clarifications).

## Size

The budget agreed at sizing on 2026-09-26 was ≤ 900 production and ≤ 550
test lines; the clarification of 2026-09-27 added Refresh Account as an
action of its own, the membership relation, the full attribute list per
folder, three roles and the hidden-account rule, and the budget was raised
the same day to ≤ 1 050 and ≤ 600. Before the window portion it was raised
again to ≤ 1 760 and ≤ 1 840: the estimates below left out doc comments,
formatting and test fixtures and came out two to three times low; they are
kept as written. Reassess
with the maintainer before exceeding the budget or about 1.5 times an item's
estimate; at every review pause the size so far is compared with this table.

| Item | Budget | This plan (estimate) |
|---|---|---|
| New modules and production lines | ≤ 1 760 net (raised from 900 to 1 050, then before the window portion, on 2026-09-27) | `mailbag-domain` ≈ 70 (`FolderRole` with its order and views, `Folder`, `FolderRef`, `FolderMembership`); `mailbag-imap` ≈ 190 (`list_mailboxes` ~60, `utf7.rs` ~50, opening a named mailbox ~25, UTF-8 names by capability ~15, steps and `MailboxChanged` ~15, reader changes ~25); `mailbag-graph` ≈ 130 (`list_folders` with the delta listing, paging and well-known names ~90, messages of a folder ~20, reply parsing ~20); `mailbag-providers` ≈ 150 (folder-list sequences of the three providers ~60, roles from attributes and well-known names ~30, the Gmail container ~10, batch types and `LoadTarget` ~20, store writes ~30); `mailbag-store` ≈ 180 (schema ~40, `replace_folders` ~50, `replace_mailbox` ~60, `read_folders` and `read_mailbox` ~30, account deletion unchanged); `mailbag` ≈ 230 (sidebar tree rebuilt per account after a folder-list load, sorted by role and collation ~110, shown mailbox and one folder-list read ~60, two actions and Retry ~25, wording ~20, icons ~10; the hidden account ~15 dropped). **Net ≈ 985** after the plan challenge (in-place tree diff −50, per-account numbered reads −20, sorting moved out of the store) and the review of 2026-09-27 (folder-list read failure page and Retry +15, selectable account without openable folders +5, special-use return option and `UTF8=ONLY` +5, delta duplicates and removed entries +10). Forks: async-imap ~20, imap-proto ~15, counted apart |
| Call sites or existing files touched | — | Rust: imap `lib.rs`, `session.rs`, `reader.rs`, `test_server.rs`, new `utf7.rs`; graph `lib.rs`, `reply.rs`, `test_server.rs`; providers `lib.rs`, `batch.rs`, `worker.rs`, `imap.rs`, `gmail.rs`, `imap_batch.rs`, `microsoft365.rs`, `store_load.rs`, new `folders.rs`; store `schema.sql`, `lib.rs`; domain `lib.rs`; mailbag `main.rs`, `window_ui.rs`, `account_ui.rs` (becomes `sidebar_ui.rs`), `accounts.rs`, `refreshes.rs`, `mail_ui.rs`, `failure_declarations.rs`, `failure_dialog.rs`. Forms: `mailbag.ui` (menu), new `account-problem.ui`; resources: one icon and `mailbag.gresource.xml`. Build: `Cargo.lock` and `cargo-sources.json` for the fork revisions |
| New crates | 0 | 0 |
| New threads, timers, queues | 0 | 0: the mail worker runs both load kinds one at a time; the window reads through GIO's pool as in 007 |
| New state, types, error types | — | Domain: `FolderRole`, `Folder`, `FolderRef`; `ServerStep::ListFolders`, `OpenInbox` → `OpenMailbox`, `InboxChanged` → `MailboxChanged`. Providers: `LoadTarget { FolderList, Mailbox(FolderRef) }`, `ReceivedFolderList`. Window: the selection (`Account(id)` / `Mailbox(ref)` / none), the folder lists as one numbered read, one latest refresh outcome per account with its target, `RetriedOperation::RefreshAccount` |
| New fields in existing data | — | Persisted: tables `folder`, `membership`; `message` loses `account`-scoped uniqueness in favour of `(account, identity)`; the `inbox` table goes ([data-model.md](data-model.md)). In memory: `LoadResult` unchanged |
| Changes to other features' contracts or documents | 007, 002, 004, 005, 006 | 007 spec FR-002/FR-003/FR-014(b) and data model (the target model built); 002 FR-002/FR-003/FR-012, 004 FR-003/FR-005, 005 FR-003/FR-006 (a named folder; Refresh Mailbox); 006 spec and contract (wording, the new step, Retry of Refresh Account); the 001 contract is untouched (accounts still come from Online Accounts) |
| New dependencies | 0 | 0; the two fork branches move the pinned revisions |
| Tests | ≤ 1 840 (raised from 550 to 600, then before the window portion, on 2026-09-27) | ≈ 560: imap ~150 (LIST scripting in the test server ~60, listing and names ~60, UTF-7 ~30); graph ~70 (folder routes in the test server, listing, well-known 404); domain ~20 (roles); store ~120; providers ~90 (three sequences, roles, the Gmail container, memberships from two labels); window ~80 (sidebar GTK test, selection and collapse, Refresh Account outcomes); ≈ 560 after the plan challenge and the review of 2026-09-27 |

## Summary

Every account gets its folder list from its server through a maintenance
action of its own, Refresh Account, which stores the list in one step. The
sidebar shows each account as a heading over its folders: the system
folders first in a fixed order with their icons, then the user's folders as
a tree in the locale's order. Selecting a folder shows its stored rows;
Refresh Mailbox loads its newest 100 messages as 007 loads the Inbox today
and stores them under the folder. A message is stored once per account
under its provider identity and belongs to folders through a relation that
carries its position, so a Gmail message with several
loaded labels is one stored message in several folders. Roles come from the
server's attributes and well-known names only, mapped by the providers into
the application's roles. Names are decoded from
modified UTF-7 when the server does not offer UTF-8 names. Failures follow
006 unchanged, with wording that speaks of the mailbox. Two defects of the
pinned IMAP libraries are corrected in the forks first: a refused or cut
LIST that looked complete, and mailbox names with a quote or a backslash
that did not round-trip.

## Minimal version

| Step | What it does | Cost |
|---|---|---|
| Forks | async-imap: a NO or BAD completing LIST ends the name stream with an error, as `parse_fetches` already does. imap-proto: quoted strings are unescaped when parsed | ~20 + ~15, with tests |
| Listing mailboxes | `mailbag-imap`: `list_mailboxes(account, options)` signs in, enables UTF-8 names when the server advertises `UTF8=ACCEPT` or `UTF8=ONLY`, runs `LIST "" "*"` with `RETURN (SPECIAL-USE)` when it advertises `SPECIAL-USE`, returns each mailbox's raw name, attributes and delimiter, and whether names are UTF-8; `MailboxReader::open(account, options, mailbox)` opens a named mailbox. `mailbag-graph`: `list_folders(service_url, token)` reads the change-tracking listing page by page and resolves the six well-known names, tolerating 404; `list_mailbox_messages(service_url, token, folder_id, batch_size)` | ~190 + ~130 |
| The domain | `FolderRole` (nine roles, the sidebar's order, `is_view`), `Folder` (identity, name, parent, role, selectable), `FolderRef` | ~70 |
| The store | Tables `folder`, `message`, `membership`; `replace_folders` (delete folders not listed with their memberships and orphaned messages, update listed ones, insert new); `replace_mailbox` (replace the folder's memberships, upsert messages by identity, drop orphans, mark loaded); `read_folders`; `read_mailbox` | ~180 |
| Providers | `LoadTarget::FolderList` and `::Mailbox(ref)` through the same worker; folder-list sequences per provider producing `Folder`s with roles; the Gmail container dropped; an empty list reported as completed without a write; mailbox loads take the target's identity; the batch carries its folder; two writes | ~150 |
| The window | Sidebar as a tree list: accounts as headings or selectable empty rows, folders in order with icons, the account's subtree rebuilt when its stored list changed, collapse clears the selection; the shown mailbox read from the store; Refresh Mailbox and Refresh Account with their outcomes; wording; the account problem form | ~230 |

Not built: unread counts, whole-folder loads, label-driven memberships, the
combined Inbox, moves and deletes, folder management, expansion memory,
horizontal scrolling, OBJECTID, localized role names (spec FR-013).

## Function map

**`mailbag-domain`**

- `FolderRole { Inbox, Starred, Important, Junk, Trash, Archive, Drafts,
  Sent, AllMail }` with `ORDER`, `is_view()` (Starred, Important, All Mail;
  the rule for the actions feature, spec FR-013(d)); its stored code lives
  in the store, its icon in the sidebar.
- `Folder { identity: String, name: String, parent: Option<String>,
  role: Option<FolderRole>, selectable: bool }`: a folder as a provider
  lists it; `parent` is the parent's identity.
- `FolderRef { account: AccountId, identity: String }`: what a mailbox load
  and the window address.
- `ServerStep::ListFolders`; `OpenInbox` renamed `OpenMailbox`;
  `FailureKind::InboxChanged` renamed `MailboxChanged`.

**`mailbag-imap`**

- `list_mailboxes(account, options) -> Result<MailboxList, ImapError>`:
  connect, sign in, `ENABLE UTF8=ACCEPT` when the post-sign-in capabilities
  hold `UTF8=ACCEPT` or `UTF8=ONLY`, `LIST "" "*"` with `RETURN
  (SPECIAL-USE)` when they hold `SPECIAL-USE`, collect `MailboxName { name, attributes,
  delimiter }`; `MailboxList { names, utf8_names: bool }`. A refused or cut
  LIST is `ImapFailure::Failed(ListMailboxes)` (needs the fork's fix).
- `utf7::decode(name) -> String`: modified UTF-7 to text; a name that cannot
  be decoded comes back as sent (research §3).
- `MailboxReader::open(account, options, mailbox: &str)`: as `InboxReader::open`,
  with EXAMINE of the given name; `fetch_rows`, `fetch_structures`,
  `fetch_text` unchanged; `ImapStep::OpenMailbox`.
- Test server: scripted LIST replies (names, attributes, delimiter, a NO or
  BAD completion) and EXAMINE of a named mailbox.

**`mailbag-graph`**

- `list_folders(service_url, token) -> Result<Vec<GraphFolder>, GraphError>`:
  `GET /me/mailFolders/delta?$select=id,displayName,parentFolderId,isHidden`
  following `@odata.nextLink` until a `deltaLink`, hidden folders left out
  here, an id repeated across pages counted once (the last wins) and
  `@removed` entries dropped; then for each well-known name `GET /me/mailFolders/{name}?$select=id`
  (404 → none); `GraphFolder { id, name, parent_id, well_known:
  Option<WellKnownFolder> }`.
- `list_mailbox_messages(service_url, token, folder_id, batch_size)`: the
  existing request with `/me/mailFolders/{id}/messages`.
- Test server: routes for the delta listing (one or two pages), the
  well-known names (200 or 404) and a folder's messages.

**`mailbag-providers`**

- `LoadsMail::start_load(account, provider, target: LoadTarget, report)`;
  `LoadTarget::FolderList | Mailbox(FolderRef)`.
- `folders.rs`: `imap_folders(names, utf8_names) -> Vec<Folder>` (decode
  names, parent by delimiter, `\Noselect` → not selectable, roles from
  attributes and INBOX, first role mark wins), `gmail_folders(...)` (the same,
  then the container whose children carry roles is dropped and its children
  lifted), `graph_folders(GraphFolder) -> Vec<Folder>` (roles by well-known
  name).
- `imap.rs`, `gmail.rs`, `microsoft365.rs`: `list_<provider>_folders(access)`
  and `load_<provider>_mailbox(access, folder identity)`; the latter is
  today's Inbox load with the named folder.
- `store_load.rs`: `store_folder_list(store, account, folders, cancelled)`
  reports a completed load without touching the store when the list is
  empty (spec FR-001), else writes through `replace_folders`;
  `store_mailbox(store, folder_ref, messages in the load's order, cancelled)`;
  the record lines name the folder's identity.
- `worker.rs`: `LoadKind` carries the target; `run_load` chooses the
  sequence by provider and target.

**`mailbag-store`**

- `replace_folders(&self, account, folders: &[Folder], load_cancelled) ->
  Result<InboxWrite, Failure>`: one transaction: delete `folder` rows of the
  account whose identity is not listed (memberships cascade), delete the
  account's messages left without a membership, update listed rows (name,
  parent, role, selectable; `loaded` kept), insert new rows.
- `replace_mailbox(&self, folder: &FolderRef, messages: &[Message],
  load_cancelled)`: one transaction: delete the folder's
  memberships, for each message insert or update by `(account, identity)`,
  insert the membership, delete the messages that lost their last
  membership, set `loaded`.
- `read_folders(&self, account) -> Result<Vec<Folder>, Failure>`: in no
  particular order; the sidebar sorts (research §7).
- `read_mailbox(&self, folder: &FolderRef) -> Result<Option<Vec<Message>>,
  Failure>`: `None` when the folder is not loaded; the messages by position.
- `delete_other_accounts`: as today over `folder` and `message`.

**`mailbag/src/sidebar_ui.rs`** (today `account_ui.rs`)

- `SidebarUi::new(builder)`: a `TreeListModel` over a `ListStore` of
  account nodes (autoexpand); each account node's child model is its folder
  `ListStore`, each folder's child model its children; `SingleSelection`
  with `autoselect` off; the factory binds the approved row form.
- `apply_update(update)`: account rows in place as today.
- `show_folders(account, folders)`: rebuild the account's child store:
  group by parent, sort siblings by `FolderRole::ORDER` then
  `glib::CollationKey` of the name, fill; the account node itself stays; an
  account whose list holds no folder that can be opened stays a selectable
  row (spec FR-009).
  An unchanged list keeps the rows; a changed one reopens collapsed
  subfolders, which the spec allows (research §9).
- Activation: a selectable folder → `Selection::Mailbox`; an account without
  folders → `Selection::Account`; a heading or a container → nothing.
- `connect_selection_changed`; the tree model's `items-changed` outside the
  sidebar's own rebuilds: when the shown mailbox's row is no longer shown,
  the user collapsed an ancestor, so clear the selection.
- The account problem button from `account-problem.ui`.

**`mailbag/src/window_ui.rs`**

- `refresh_mailbox()`: the selected mailbox's load; `refresh_account()`: the
  folder list of the selected account or the selected mailbox's account;
  both refused while a load runs. `Refreshes` keeps one latest outcome per
  account with its target; it shows when the target is the shown mailbox,
  or the account of the shown mailbox or selected account for a folder-list
  target; Retry repeats the target's action (007 FR-005's "latest refresh
  of the account").
- `finish_load(target, result)`: outcome under its target; a completed
  folder-list load re-reads the folder lists; an empty one wrote nothing,
  so the read changes nothing (the providers' record line says the list
  held no folder); after either, the selected mailbox is
  read when the window does not hold its rows, since the load's start forgot
  a failed read of them (007 FR-013); a completed mailbox load re-reads the
  shown mailbox.
- `read_folder_lists()` at start, after each complete account update and
  after a completed folder-list load: one read of every shown account's
  folders through GIO's pool, with one number so an older answer is
  dropped; a failed read shows the failure page with Details as for stored
  mail that cannot be read, whose Retry (`RetriedOperation::ReadStoredMail`)
  reads the folder lists and the shown mailbox again, and leaves the
  sidebar as it was (spec FR-008, 007 FR-013). After the read, the selection is
  cleared when the shown mailbox is no longer listed or when the selected
  account now has folders (spec FR-010).
- `read_shown_mailbox()`: as 007's read of the Inbox, keyed by `FolderRef`.
- `render()`: the list's title (folder name, account); "no mail loaded" for
  an unloaded folder or a selected empty account; banner or failure page by
  006 for the target's outcome; Retry by `RetriedOperation`.

**`mailbag/src/failure_declarations.rs`**: wording for the two renamed kinds
and the new step; "Refresh Inbox" → "Refresh Mailbox" everywhere.

**`mailbag/src/main.rs`**: `app.refresh-mailbox`, `app.refresh-account`.

## Optional mechanisms

None is planned. Each would need the situation named beside it.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Keeping the roles of known Microsoft 365 folders across Refresh Account, so the well-known lookups run only when a role is unassigned | Refresh Account on Microsoft 365 waits visibly for its listing pages and six requests | ~15 lines |
| Recognizing a renamed IMAP folder by a server identifier | A supported server advertises OBJECTID (spec FR-013(h)) | ~30 lines |
| A capped indentation or horizontal scrolling in the sidebar | Trees deep enough that names become unreadable (spec FR-013(g)) | ~5 lines or a form change |
| Collation keys cached per folder | Sorting the sidebar shows up in a measurement | ~10 lines |

## Portions and review pauses

One commit each, with its tests and `scripts/check.sh`; stop after each for
the maintainer's review and compare the size with the table above.

1. **Listing mailboxes on every provider.** The two fork fixes on their
   branches, pushed and pinned; `list_mailboxes`, `utf7.rs`, the named
   `MailboxReader`, the new step and renamed failure in `mailbag-imap`;
   `list_folders` and `list_mailbox_messages` in `mailbag-graph`; both test
   servers extended; no application change. Suggested commit: "List
   mailboxes on every provider".
2. **Storing folders and memberships.** The domain's additions and the
   store's new schema and operations with their tests; 007's data model and
   spec amended first. Suggested commit: "Store folders and memberships".
3. **Loading folder lists and mailboxes.** `LoadTarget`, the folder-list
   sequences, roles, the Gmail container, mailbox loads by identity, the two
   writes; 002/004/005 amended first. Suggested commit: "Load folder lists
   and mailboxes".
4. **Navigating mailboxes.** The sidebar tree, selection and collapse, the
   two actions and their outcomes, wording and 006's amendment, the forms,
   the icon; the GUI tests. Suggested commit: "Navigate
   mailboxes in the sidebar".

After portion 4: the GUI tests one by one, `simplify-review` on the branch
diff in a fresh subagent, then the manual checks of
[quickstart.md](quickstart.md) on the installed build.

## Technical Context

**Language/Version**: Rust 1.95, edition 2024, as the workspace.
**Primary Dependencies**: gtk4 0.11, libadwaita 0.9, glib/gio 0.22; the
async-imap and imap-proto forks at new pinned revisions; soup3 0.9; rusqlite
0.40 without default features. No new crate.
**Storage**: the 007 store, whose structure changes (three tables); an
existing store is discarded at start under 007 FR-012.
**Testing**: `cargo test` with the scripted IMAP server (LIST added), the
scripted Graph service (folder routes added), the store in memory, and the
GTK tests one per process.
**Target Platform**: GNOME desktop, native and Flatpak.
**Project Type**: desktop application.
**Constraints**: no store access on GTK's thread; one load at a time; no
thread or timer of the feature's own; the approved layout unchanged except
the two form changes named in the spec.
**Scale/Scope**: accounts with tens of folders and windows of 100 messages
now; the model measured at a million messages (research §8).

## Constitution Check

- **I. Necessary complexity only**: every mechanism in the minimal version
  has its situation in the spec (US1–US7, Edge Cases); the optional table
  names what was left out and why. The fork fixes answer defects reproduced
  in source: a cut LIST would delete stored folders with their mail; a
  quoted name would not open.
- **II. Clear language and concrete names**: `FolderRole`, `Folder`,
  `FolderRef`, `list_mailboxes`, `replace_folders`,
  `replace_mailbox`, `read_mailbox`, `LoadTarget`, `refresh_account`; in
  code *folder* names the thing in the list and the store and *mailbox* a
  folder opened for its messages and everything the user sees
  ([contract](contracts/folders.md)).
- **III. Explicit failures and truthful state**: a refused, cut or partly
  read folder list fails the load and changes nothing; an unloaded folder
  says so; an empty list is never invented; failures go through 006.
- **IV. One owner per business rule**: roles are mapped by the providers
  (`folders.rs`), one function per provider; order and icons by the domain's
  `FolderRole`; identity and memberships by the store's two writes; the
  selection by the sidebar.
- **V. Responsive, bounded work**: reads through GIO's pool, writes on the
  worker, one load at a time; Refresh Account adds one command on IMAP and
  the listing's pages plus six requests on Microsoft 365.
- **VI. Evidence before completion**: the tests per crate above, the GTK
  tests, and the quickstart on the installed build.

No violation; Complexity Tracking stays empty.

## Project Structure

### Documentation (this feature)

```text
specs/008-folders/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/folders.md
└── tasks.md            (speckit-tasks)
```

### Source Code

```text
crates/mailbag-domain/src/lib.rs          FolderRole, Folder, FolderRef
crates/mailbag-imap/src/{lib,session,reader,utf7,test_server}.rs
crates/mailbag-graph/src/{lib,reply,test_server}.rs
crates/mailbag-providers/src/{lib,batch,worker,folders,imap,gmail,imap_batch,microsoft365,store_load}.rs
crates/mailbag-store/src/{schema.sql,lib}.rs
crates/mailbag/src/{main,window_ui,sidebar_ui,accounts,refreshes,mail_ui,failure_declarations,failure_dialog}.rs
crates/mailbag/resources/ui/{mailbag,account-problem}.ui
crates/mailbag/resources/icons/scalable/places/mailbag-folder-inbox-symbolic.svg
the async-imap and imap-proto forks pinned in Cargo.toml (fix branches, new revisions)
```

**Structure Decision**: no new crate; the sidebar module is renamed from
`account_ui.rs` to `sidebar_ui.rs` because it now owns folders.

## Documents amended before implementing

- Portion 2: 007 spec (FR-002, FR-003, FR-014(b)) and data model.
- Portion 3: 002 spec (FR-002, FR-003, FR-012), 004 spec (FR-003, FR-005),
  005 spec (FR-003, FR-006).
- Portion 4: 006 spec and contract (wording, `ListFolders`, Retry of
  Refresh Account); the roadmap note is local.
