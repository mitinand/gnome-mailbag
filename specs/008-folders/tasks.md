# Tasks: Folders

**Feature**: `008-folders`
**Created**: 2026-09-27 · **Branch**: `claude/folders` · **Status**: Documents
approved on 2026-09-27 (T001); portion 1 (T002–T010) committed on
2026-09-27; portions 2 and 3 (T011–T023) committed on 2026-09-27. Before
portion 4 the budget was raised and the empty list's hidden account and the
folder-list read's own Retry were dropped (spec Clarifications); portion 4
(T024–T033) committed on 2026-09-27; polish (T034–T037) done on
2026-09-27; the feature is implemented and accepted. Phase 7 (T038–T042),
the sidebar on a list box after the spike of 2026-09-28, done on
`claude/sidebar` and checked on the installed build on 2026-09-28. The texts of done
tasks describe the work as planned: the simplification review (T034) and
the acceptance (plan, Post-implementation) changed what T012–T015, T018,
T019, T021, T022, T026, T028 and T030 describe, and T005's tests use the
RFC 3501 example instead of the names listed; the spec, the data model and
the contract record what was built.

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table, the
function map and the portions, [research](research.md) owns the decisions
with alternatives, the [data model](data-model.md) owns the stored form,
[contracts/folders.md](contracts/folders.md) owns the shared definitions,
[quickstart](quickstart.md) owns the manual acceptance. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one
portion, run its checks, compare the size with the plan's table, report and
stop. The maintainer creates commits and PRs. Do not start code before
document approval. Nothing committed may name where an idea came from
outside this repository, nor any account or server of a person.

Phases follow the plan's portions. Story labels trace tasks to US1 (the
account's folders appear and open), US2 (system folders first), US3
(nested folders), US4 (Gmail labels are folders), US5 (the folder list
follows the server), US6 (the account is a heading) and US7 (a load that
fails). Tests are part of every portion and live beside their modules; no
test pins wording except the privacy invariants.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(folders): specify and plan folders | Folders |
| 1. Listing mailboxes on every provider | T002–T010 | feat(imap,graph): list mailboxes on every provider | Folders |
| 2. Storing folders and memberships | T011–T016 | feat(store): store folders and memberships | Folders |
| 3. Loading folder lists and mailboxes | T017–T023 | feat(providers): load folder lists and mailboxes | Folders |
| 4. Navigating mailboxes | T024–T033 | feat: navigate mailboxes in the sidebar | Folders |
| Polish | T034–T037 | (per review) | Folders |

## Phase 1: documents and review

- [x] T001 STOP: present specs/008-folders/ (spec.md, plan.md, research.md,
  data-model.md, contracts/folders.md, quickstart.md,
  checklists/requirements.md, this tasks.md); wait for explicit maintainer
  approval before any code change.

## Phase 2: listing mailboxes on every provider (portion 1)

Goal: the protocol crates can list mailboxes and open a named one, with the
two library defects fixed; the application behaves as before, its call
sites only renamed.

- [x] T002 In the async-imap fork pinned in Cargo.toml, branch
  `fix/list-completion-status` from `mailbag`: make `parse_names` in src/parse.rs end the stream with
  `Err(Error::No(..))` / `Err(Error::Bad(..))` when the LIST completion is NO
  or BAD and with `Err(Error::ConnectionLost)` when the stream ends before
  the completion, as `FetchResponses` does; unit tests for "one name then
  NO", "one name then BAD", "one name then EOF". Merge into `mailbag`, tag
  `mailbag-2026-09-27`, push (no upstream PR).
- [x] T003 [P] In the imap-proto fork pinned in Cargo.toml, branch
  `fix/unescape-quoted` from `mailbag`: make `quoted` in imap-proto/src/parser/core.rs return the
  unescaped content (`\"` → `"`, `\\` → `\`), keeping a borrowed slice when
  nothing is escaped; adjust `quoted_utf8`, every parser that calls `quoted`
  (rfc3501/mod.rs among them) and the tests in core.rs and
  parser/tests.rs (a LIST with name `"a\"b\\c"` parses to `a"b\c`). Merge
  into `mailbag`, tag, push.
- [x] T004 Pin the new revisions in Cargo.toml `[patch.crates-io]`, update
  Cargo.lock, run scripts/generate-cargo-sources.sh for cargo-sources.json.
- [x] T005 [US1] [US2] In crates/mailbag-imap: add src/utf7.rs with
  `decode(name: &str) -> String` (RFC 3501 §5.1.3 modified UTF-7: `&-` is
  `&`, `&…-` is modified base64 of UTF-16BE; anything undecodable leaves
  the name as sent) and its unit tests (ASCII unchanged, a Cyrillic name, a
  name with `&-`, a truncated group returned as sent); add
  `ImapStep::ListMailboxes` and rename `OpenInbox` → `OpenMailbox`,
  `ImapFailure::InboxChanged` → `MailboxChanged` in src/lib.rs with the
  call sites in session.rs, reader.rs, fetch_responses.rs and tests, and
  the mechanical renames in crates/mailbag-providers (failure.rs, tests.rs)
  so the workspace keeps building.
- [x] T006 [US1] [US2] [US7] In crates/mailbag-imap/src/session.rs and
  lib.rs: split `open_inbox` into `sign_in_session(account, options,
  timeout, notices) -> SignedInSession { session, capabilities, utf8_names:
  bool, connection }` (`capabilities` is the post-sign-in list from one
  CAPABILITY command, since the fork's `authenticate` does not return the
  sign-in reply's list; ENABLE UTF8=ACCEPT sent only when it holds
  `UTF8=ACCEPT` or `UTF8=ONLY`, RFC 6855 §6; `utf8_names` true only when the
  server accepts it)
  and
  `examine_mailbox(signed_in, name) -> MailboxSession` (today's
  `InboxSession`; EXAMINE of the given name, step `OpenMailbox`); add `pub async fn list_mailboxes(account,
  options) -> Result<MailboxList, ImapError>` running `LIST "" "*"`, with
  `RETURN (SPECIAL-USE)` appended when the capabilities hold `SPECIAL-USE`
  (RFC 6154 §2), on the
  signed-in session and collecting `MailboxName { name: String, attributes:
  Vec<String>, delimiter: Option<String> }` into `MailboxList { names,
  utf8_names }`, a refused or cut LIST being `Failed(ListMailboxes)` with the
  server's reply; log the count at info and the names at debug (003 FR-010).
- [x] T007 [US1] In crates/mailbag-imap/src/reader.rs: rename `InboxReader`
  to `MailboxReader` with `open(account, options, mailbox: &str)` (and the
  short-timeout variant) that signs in and examines the named mailbox;
  `fetch_rows`, `fetch_structures`, `fetch_text` unchanged; update the
  crate's tests and doc comments to say mailbox, and the call sites in
  crates/mailbag-providers (imap.rs, gmail.rs, imap_batch.rs) to open
  "INBOX" by name so behaviour is unchanged.
- [x] T008 [US1] [US2] [US7] In crates/mailbag-imap/src/test_server.rs:
  script LIST replies (`FixtureSetup::mailboxes: Vec<(attributes, delimiter,
  name)>` sent as `* LIST (...) "d" name` lines, names quoted or as literals
  as configured, a configurable completion OK / NO / BAD / connection closed),
  EXAMINE of any scripted mailbox name (unknown → NO), and CAPABILITY with or
  without `UTF8=ACCEPT` and its ENABLE reply; tests in
  crates/mailbag-imap/src/tests/: listing returns names, attributes and the
  delimiter; a NO or BAD completion is a `ListMailboxes` failure with the
  reply; a cut connection is a failure; names with a quote and a backslash
  round-trip from LIST to EXAMINE; ENABLE is sent with `UTF8=ACCEPT` and with `UTF8=ONLY` and not
  otherwise; the special-use return option is sent only with `SPECIAL-USE`;
  a named mailbox is examined and its rows fetched.
- [x] T009 [P] [US1] [US2] [US7] In crates/mailbag-graph/src/lib.rs and
  reply.rs: add `list_folders(service_url, token) -> Result<Vec<GraphFolder>,
  GraphError>` reading `GET /me/mailFolders/delta?$select=id,displayName,parentFolderId,isHidden`
  and following `@odata.nextLink` until `@odata.deltaLink`, leaving hidden
  folders out here (the only place), an id repeated across pages counted
  once with the last occurrence winning and `@removed` entries dropped
  (both allowed in the first round), then `GET /me/mailFolders/{name}?$select=id` for `inbox`,
  `drafts`, `sentitems`, `deleteditems`, `junkemail`, `archive` (404 →
  none, any other refusal → the load's failure); `GraphFolder { id, name,
  parent_id: Option<String>, well_known: Option<WellKnownFolder> }`;
  generalize `list_inbox_messages` into `list_mailbox_messages(service_url,
  token, folder_id, batch_size)` with the path `/me/mailFolders/{id}/messages`
  (the id percent-encoded); extend test_server.rs with routes for the delta
  listing (one or two pages, `ScriptedFolders`), the well-known names (200
  with an id, or 404) and a folder's messages by id; tests: the whole tree
  from two pages, a folder repeated on both pages, an `@removed` entry, a
  missing well-known folder, a failing second page, hidden
  folders left out, messages of a named folder.
- [x] T010 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md's table (imap ~190, graph ~130, forks ~35); report,
  suggest the commit and wait before portion 2.

## Phase 3: storing folders and memberships (portion 2)

Goal: the domain names folders and roles, the store holds folders, messages
and memberships; the application keeps its behaviour through thin adapters
that treat the Inbox as the one folder, removed in portion 3.

- [x] T011 Amend the documents first: specs/007-mail-storage/spec.md
  (FR-002 "its folders", FR-003 built now as this feature's FR-004 and
  FR-007 except the folder state synchronization needs, FR-014(b) reduced
  to what stays deferred) and
  specs/007-mail-storage/data-model.md (a note that 008's data model
  replaces its tables).
- [x] T012 [US2] [US4] In crates/mailbag-domain/src/lib.rs: `FolderRole`
  (Inbox, Starred, Important, Junk, Trash, Archive, Drafts, Sent, AllMail)
  with `ORDER`, `is_view()` (doc comment: the rule for moves and deletes,
  spec FR-013(d)), `icon_name()` and `as_code()`/`from_code()` for the
  store; `Folder { identity, name, parent, attributes, role, selectable }`;
  `FolderRef { account, identity }`; `FolderMembership { uid: Option<u32>,
  position: u32 }`; `Message.labels: Vec<String>`; `ServerStep::ListFolders`,
  `OpenInbox` → `OpenMailbox`, `FailureKind::InboxChanged` →
  `MailboxChanged` with the mechanical renames in crates/mailbag-providers
  (failure.rs) and crates/mailbag/src/failure_declarations.rs (wording
  itself unchanged until portion 4); privacy-safe `Debug` for `Folder`
  (identity only);
  unit tests for the order and the views.
- [x] T013 [US1] [US4] [US5] In crates/mailbag-store/src/schema.sql: the
  tables of data-model.md (`folder`, `message` with `labels` one per line and unique
  `(account, identity)`, `membership` with its index); drop `inbox`; role
  codes as a CHECK.
- [x] T014 [US1] [US4] [US5] In crates/mailbag-store/src/lib.rs (and a new
  src/folders.rs if lib.rs grows past reading): `replace_folders(account,
  folders, load_cancelled)` (delete unlisted folders, delete the account's
  messages without a membership, update listed, insert new with loaded 0),
  `replace_mailbox(folder_ref, messages: &[(Message, FolderMembership)],
  load_cancelled)` (delete memberships, upsert messages by `(account,
  identity)` with fields, seen, content and labels, insert memberships,
  delete messages that lost their last membership, set loaded 1; an unknown
  folder is a `MailNotSaved` failure), `read_folders(account) ->
  Vec<StoredFolder>`, `read_mailbox(folder_ref) -> Option<Vec<Message>>`
  (None when not loaded or not in the store, by position); rename
  `InboxWrite` to `StoreWrite`; keep `replace_inbox`/`read_inbox` as thin
  adapters over the new tables (the folder `INBOX` with role Inbox,
  memberships by position) so crates/mailbag-providers/src/store_load.rs
  and crates/mailbag/src/window_ui.rs keep working until portion 3 removes
  them; `delete_other_accounts` over `folder` and `message`.
- [x] T015 [P] [US1] [US4] [US5] Tests in crates/mailbag-store/src/tests.rs
  (rewritten, about 120 net lines): a folder list round-trips with attributes,
  roles, parents and selectable; a second list removes a folder with its
  memberships and its messages that belonged to it alone, keeps loaded on
  kept folders, adds new ones unloaded; a mailbox round-trips by position
  with UIDs and labels, a label with a space among them; a second load of the same mailbox replaces its
  memberships; one message in two folders is stored once and read in both;
  a message removed from its last folder is deleted; a cancelled load writes
  nothing; a write that fails leaves the previous state whole; unloaded is
  `None`, loaded and empty is `Some(vec![])`; `delete_other_accounts`
  removes folders and messages; a store from 007's structure is discarded at
  start.
- [x] T016 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md's table (domain ~70, store ~180); report, suggest the
  commit and wait before portion 3.

## Phase 4: loading folder lists and mailboxes (portion 3)

Goal: a folder-list load and a mailbox load exist for every provider and
write into the store; the store's Inbox adapters of portion 2 go; the
window still calls the old entry point until portion 4 through a temporary
adapter in `mailbag-providers` that loads the Inbox by identity, removed in
portion 4.

- [x] T017 Amend the documents first: specs/002-imap-integration/spec.md
  (FR-002, FR-003, FR-012: a named folder, Refresh Mailbox),
  specs/004-gmail-integration/spec.md (FR-003 a named label, FR-005 labels
  as memberships built as 008 FR-004 says), specs/005-microsoft-graph-integration/spec.md
  (FR-003 a named folder, FR-006 consistent with 008 FR-004).
- [x] T018 [US1] [US2] [US4] In crates/mailbag-providers/src/folders.rs
  (new): `imap_folders(list: MailboxList) -> Vec<Folder>` (name decoded with
  `utf7::decode` unless `utf8_names`; parent by delimiter when the parent is
  listed, else none; `\Noselect` → not selectable; role by the first role
  attribute among `\Flagged \Important \Junk \Trash \Archive \Drafts \Sent
  \All` in the order the server listed them, INBOX by name without regard
  to case (RFC 9051 §5.1) or by `\Inbox`; attributes kept as sent),
  `gmail_folders(list) -> Vec<Folder>` (as `imap_folders`, then a
  non-selectable root container whose children carry role attributes is
  removed and its children get no parent, their names without the prefix),
  `graph_folders(folders: Vec<GraphFolder>) -> Vec<Folder>` (role by
  well-known name; attributes hold the well-known name; parent when the
  parent is in the list); unit tests for each with the edge cases of the
  spec (partial marks, two marks, two folders one role, missing parent,
  the container, a well-known 404).
- [x] T019 [US1] [US4] In crates/mailbag-providers/src/batch.rs, lib.rs,
  worker.rs: `LoadTarget { FolderList, Mailbox(FolderRef) }`;
  `LoadsMail::start_load(account, provider, target, report)` replacing
  `LoadsInbox`; `LoadKind` carries the target; `run_load` runs
  `list_<provider>_folders` or `load_<provider>_mailbox`; `ReceivedBatch`
  gains `folder: FolderRef` and each `ReceivedMessage` its `uid` (already
  `MessageIdentity::ImapUid`) and Gmail labels; `LoadResult` gains
  `EmptyFolderList` (a completed list without any folder, nothing written;
  spec FR-001).
- [x] T020 [US1] [US4] In crates/mailbag-providers/src/imap.rs, gmail.rs,
  imap_batch.rs, microsoft365.rs: `list_imap_folders(access)`,
  `list_gmail_folders(access)` (Gmail options as today),
  `list_microsoft365_folders(access, service_url)`; `load_imap_mailbox(access,
  identity)`, `load_gmail_mailbox(access, identity)`,
  `load_microsoft365_mailbox(access, service_url, identity)` as today's Inbox
  loads with the named folder; message identities `imap:<folder>/<uid>`,
  `gmail:<X-GM-MSGID>`, `graph:<id>`.
- [x] T021 [US1] [US4] [US7] In crates/mailbag-providers/src/store_load.rs:
  `store_folder_list(store, account, folders, load_cancelled)` returning
  `LoadResult::EmptyFolderList` without a write when `folders` is empty and
  calling `replace_folders` otherwise, and `store_mailbox(store, batch,
  load_cancelled)` calling `replace_mailbox`; add the temporary providers
  adapter for the window (the store's Inbox adapters of portion 2 stay with
  it until portion 4, since the window reads `read_inbox` until then); record lines "folder list load finished" with the count
  and "mailbox load finished" with the folder's identity at debug and the
  counts at info; `log_load_failure` names the target.
- [x] T022 [P] [US1] [US2] [US4] [US7] Tests in
  crates/mailbag-providers/src/tests.rs: each provider's folder-list load
  against its scripted server stores the folders with roles (Gmail: the
  container dropped); a cut LIST fails the load and stores nothing; a
  mailbox load by identity stores its messages under that folder; two Gmail
  labels sharing a message store it once with two memberships; a Graph
  second page failure stores nothing; an empty completed list stores
  nothing and reports `EmptyFolderList`; cancellation during a folder-list
  load writes nothing.
- [x] T023 STOP: run ./scripts/check.sh and git diff --check; compare the
  size with plan.md's table (providers ~150); report, suggest the commit and
  wait before portion 4.

## Phase 5: navigating mailboxes (portion 4)

Goal: the sidebar shows accounts and folders, the two actions work, failures
speak of the mailbox (US1–US7).

- [x] T024 Amend the documents first: specs/006-error-handling/spec.md and
  contracts/failure-declaration.md (`ListFolders` and `OpenMailbox` steps,
  `MailboxChanged`, Retry of Refresh Account, the wording rule for the
  mailbox); specs/006-error-handling/quickstart.md and
  specs/007-mail-storage/quickstart.md where they say Refresh Inbox.
- [x] T025 [US6] Forms and resources: in crates/mailbag/resources/ui/mailbag.ui
  rename the menu item to "_Refresh Mailbox" (`app.refresh-mailbox`) and add
  "Refresh _Account" (`app.refresh-account`) under it; add
  crates/mailbag/resources/ui/account-problem.ui declaring the account row's
  problem `GtkMenuButton` (icon `dialog-warning-symbolic`, flat, centered)
  with its `GtkPopover` holding the explanation label (wrap, max 30 chars)
  and the "Retry Check" and "Online Accounts" buttons, as built in code
  today; add crates/mailbag/resources/icons/scalable/places/mailbag-folder-inbox-symbolic.svg
  from the GNOME icon-development-kit's `inbox.svg` with a `.license` file
  (CC0-1.0) and register it in mailbag.gresource.xml. Say in the report that
  Workbench's "List View with a Tree" is the demo followed.
- [x] T026 [US1] [US2] [US3] [US6] Rename crates/mailbag/src/account_ui.rs to
  sidebar_ui.rs (`SidebarUi`): a `TreeListModel` (autoexpand) over a
  `ListStore` of account nodes; each account node's child model is its folder
  `ListStore`, each folder node's its children; nodes are `BoxedAnyObject`s
  holding the key (`Selection::Account(id)` / `Selection::Mailbox(FolderRef)`,
  the same type the selection uses) and the bound widgets from
  folder-row.ui, the row's tooltip carrying the folder's full name (spec
  FR-006); `apply_update` keeps account rows in place as
  today and instantiates account-problem.ui per account row;
  `show_folders(account, Vec<StoredFolder>)` rebuilds the account's subtree
  (group by parent, sort siblings by `FolderRole::ORDER` then
  `glib::CollationKey` of the name, fill; the account row becomes a heading
  when it has a folder that can be opened and stays a selectable row
  otherwise, containers included);
  role icons from `FolderRole::icon_name`, `folder-symbolic` otherwise;
  activation: a selectable folder selects it, an account without folders
  selects it, a heading or container does nothing; the tree model's
  `items-changed` outside the sidebar's own rebuilds clears the selection
  when the user collapsed an ancestor of the shown mailbox; an unchanged
  list keeps the rows; the row of the shown mailbox is marked again after a
  rebuild;
  `Selection { Account(AccountId), Mailbox(FolderRef) }` with
  `selection()`, `connect_selection_changed`.
- [x] T027 [US1] [US5] [US7] In crates/mailbag/src/accounts.rs and
  refreshes.rs: `AccountList` is the one owner of the selection
  (`Selection`); `Refreshes` keeps one latest outcome per
  account with its `LoadTarget` (`outcome_of(account) -> Option<(&LoadTarget,
  &RefreshOutcome)>`), `begin_load(target, cancellation)`,
  `finish_load(target, result)`, `is_loading`, `discard_excluded`.
- [x] T028 [US1] [US3] [US5] [US6] [US7] In crates/mailbag/src/window_ui.rs:
  `refresh_mailbox()` and `refresh_account()` (`app.refresh-mailbox`,
  `app.refresh-account`, both disabled while a load runs, the first also
  without a selected mailbox, the second without a selection);
  `finish_load(target, result)`: a completed folder-list load reads the
  folder lists again; `LoadResult::EmptyFolderList` ends the load as a
  completed one that changes nothing shown; a completed mailbox load of the shown mailbox reads it
  again; after a folder-list read the selection is cleared when the shown
  mailbox is no longer listed or when the selected account now has folders
  (spec FR-010);
  `read_folder_lists()` as one numbered read of every shown account's
  folders on GIO's pool, at start, after a complete account update and
  after a completed folder-list load, a failure shown as the failure page
  with Details as for stored mail that cannot be read, whose Retry
  (`RetriedOperation::ReadStoredMail`, `app.read-stored-mail`, today's
  `app.read-stored-inbox`) reads the folder lists and the shown mailbox
  again while the sidebar keeps what it showed; `ShownMailbox` keyed by `FolderRef` replacing
  `ShownInbox`; `render()`: the list title from mail_ui.rs `show_title(name,
  account label)`, "no mail loaded" for an unloaded folder and a selected
  empty account with the advice "Choose Refresh Account or Refresh Mailbox in
  the main menu.", "Mailbox is empty" for a loaded empty one, "Select a
  mailbox" when nothing is selected, the banner or failure page for the
  account's latest outcome when its target is the shown mailbox or, for a
  folder list, the account; `RetriedOperation::RefreshMailbox |
  RefreshAccount | ReadStoredMail` in failure_dialog.rs; remove the
  temporary bridge: the providers' `LoadsInbox`, `LoadJob::InboxUntilFolders`
  and `store_inbox_until_folders`, the store's `replace_inbox` and
  `read_inbox`, and the `EmptyFolderList` arm refreshes.rs got in portion 3.
- [x] T029 [US7] In crates/mailbag/src/failure_declarations.rs: wording for
  `ServerStep::ListFolders` ("Mailbox list not received", "The mail server
  did not send the mailbox list.", the timed-out variant; renamed from
  "folder list" at the simplification review), `OpenMailbox`
  ("Mailbox not opened", "The mail server did not open this mailbox."),
  `FetchMessages`/`FetchText` texts saying "this mailbox", `MailboxChanged`
  ("Mailbox changed", "The messages being loaded are no longer in this
  mailbox."), every "Refresh Inbox" → "Refresh Mailbox" or "Refresh Account"
  by the carrier, "Loading this mailbox stopped…"; main.rs registers the two
  actions and removes `refresh-inbox`.
- [x] T030 [P] [US1] [US3] [US6] GTK test in crates/mailbag/src/sidebar_ui/tests.rs
  (one per process, `#[ignore]` as the others): accounts appear as
  selectable rows; after folders are shown an account is a heading whose
  activation changes nothing; folders come in the spec's order with the
  roles' icons and user folders by collation (a non-Latin and a Latin name);
  nested folders under their parent; a container cannot be selected; an account whose folders are all
  containers stays selectable;
  collapsing the parent of the selected folder clears the selection; a
  rebuilt subtree reselects the shown mailbox's row; a row's tooltip is the
  folder's full name.
- [x] T031 [P] [US1] [US4] [US5] [US7] GTK tests in
  crates/mailbag/src/mail_ui/tests.rs, where the scripted loader and the
  graphical tests live, through the scripted loader writing into a test
  store: Refresh Account on an empty account shows the folders
  and clears the selection; Refresh Mailbox stores and shows a folder's rows
  while another folder's rows stay; a folder gone from a new list clears the
  selection and its rows; a failed folder list shows the failure page (empty
  account) or the banner (shown mailbox) with Retry repeating Refresh
  Account; a failed mailbox open shows the banner with Retry repeating
  Refresh Mailbox; an empty completed list changes nothing shown;
  a failed folder-list read shows the failure page and its Retry reads the
  folder lists and the shown mailbox again; an older read's answer never
  replaces a newer one's.
- [x] T032 [P] [US2] Unit tests in crates/mailbag/src/accounts/tests.rs and
  refreshes/tests.rs for the selection rules and one outcome per account
  with its target.
- [x] T033 STOP: run ./scripts/check.sh, git diff --check and each GTK test
  one by one; compare the size with plan.md's table (window ~230); report,
  suggest the commit and wait.

## Phase 6: polish

- [x] T034 Run `simplify-review` on the branch diff in a fresh subagent;
  apply what does not add scope, bring the rest to the maintainer. Applied
  on 2026-09-27 with the maintainer's decisions: attributes, labels and
  UIDs no longer stored (spec Clarifications), the empty folder list
  reported as a completed load, one account page for shown accounts, role
  codes in the store and icons in the sidebar, `read_folders` returning
  folders, and "mailbox list" in the interface; the load kind stays on the
  failure record line.
- [x] T035 The manual checks of quickstart.md on the installed build
  (`scripts/build-flatpak.sh --install`); record the results in plan.md
  under a "Post-implementation" heading without sizes.
- [x] T036 Mark the spec's Status implemented and accepted, and the amended
  specs' status lines.
- [x] T037 STOP: final report with the open items.

## Phase 7: the sidebar on a list box (portion 5)

Decided after the spike of 2026-09-28 (spec Clarifications, research §9).

- [x] T038 Documents: spec FR-006, FR-010, Assumptions and Clarifications;
  research §9; the plan's sidebar map; quickstart step 5; these tasks.
- [x] T039 Forms: in crates/mailbag/resources/ui/mailbag.ui `folder_tree`
  becomes a `GtkListBox` (`navigation-sidebar`, single selection,
  `tab-behavior` `item`, GTK 4.18 required); in folder-row.ui the row is an
  `AdwActionRow` with the expander (no focus) and the icon (1 px up) as
  prefixes and the badge as suffix, and the account spacer is an object of
  its own.
- [x] T040 crates/mailbag/src/sidebar_ui.rs: `bind_model` with `create_row`,
  the header function for the spacer, selection by `row-selected` and
  marking by `select_row`, `row-activated` for a narrow window, the row's
  key forwarding, removed nodes letting go of their tree rows; remove the
  item factory, `bound_item`, the click gesture on the name and the tree's
  own key forwarding.
- [x] T041 Tests: crates/mailbag/src/sidebar_ui/tests.rs and the window
  tests' selection helper on the list box: arrows select, a heading or a
  container passes, collapsing clears the selection and keeps the focus on
  the collapsed row, removed rows are freed, the focus rests on the row,
  its expander collapses it (real key presses only by hand, quickstart
  step 5), Tab leaves the tree, a narrow window hides the sidebar on
  Enter.
- [x] T042 STOP: `scripts/check.sh`, the GTK tests one per process; the
  maintainer checks quickstart steps 3–5 on the installed build; report,
  suggest the commit and wait.

## Dependencies

- T001 before everything; each STOP (T010, T016, T023, T033, T037) waits
  for the maintainer.
- Portion 1: T002 and T003 in parallel, then T004; T005 before T006–T008;
  T009 independent of the IMAP tasks.
- Portion 2: T011 first; T012 before T013–T015.
- Portion 3: T017 first; T018 and T019 before T020–T022.
- Portion 4: T024 and T025 first; T026 and T027 before T028; T029 with T028;
  tests T030–T032 after their modules.

## Parallel opportunities

- T002 ∥ T003 (two forks); T009 ∥ T005–T008 (Graph against IMAP).
- T015 with T014 once the signatures exist; T022 with T020–T021 likewise.
- T030 ∥ T031 ∥ T032.

## Implementation strategy

Portion 1 delivers nothing visible but is fully testable against the
scripted servers and fixes the library defects first. Portion 2 stands on
its own with the store's tests. Portion 3 keeps the application running on
the old entry point through a small adapter, so it can be reviewed without
UI. Portion 4 is the visible feature; its GTK tests run one per process.
The size is compared with plan.md at every STOP; a portion that will exceed
its estimate by half stops before doing so.

## Deferred, no tasks

Unread counts, whole-folder loads and label-driven memberships (built by
009), the combined Inbox, moves and deletes with the rule for views (the
flag part built by 011, FR-013(d)), folder management, background
refreshes, expansion memory and horizontal scrolling, OBJECTID (spec
FR-013).
