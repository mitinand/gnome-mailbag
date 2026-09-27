# Contract: Folders

The definitions the crates share for folders, kept in `mailbag-domain`, and
the operations the window, the providers and the store agree on. Names are
the code's; meanings are the spec's. Two words are used on purpose:
*folder* names the thing in the list and in the store (`Folder`,
`list_folders`, `replace_folders`, `read_folders`); *mailbox* names a folder
opened for its messages and everything the user sees (`MailboxReader`,
`LoadTarget::Mailbox`, `replace_mailbox`, `read_mailbox`, "Refresh
Mailbox").

## Domain types

- `FolderRole`: `Inbox | Starred | Important | Junk | Trash | Archive |
  Drafts | Sent | AllMail`. `FolderRole::ORDER` is the sidebar's order.
  `is_view()` is true for Starred, Important and All Mail (spec FR-003,
  FR-013(d)). `icon_name()` gives the role's icon: Inbox
  `mailbag-folder-inbox-symbolic` (bundled), Starred `starred-symbolic`,
  Important `mail-mark-important-symbolic`, Junk `mail-mark-junk-symbolic`,
  Trash `user-trash-symbolic`, Drafts `document-edit-symbolic`, Sent
  `mail-send-symbolic`, Archive and All Mail `folder-symbolic`; all but the
  first are in the platform's icon theme (checked on the host and in the
  GNOME 50 runtime); a folder without a role uses `folder-symbolic`.
- `Folder { identity, name, parent, attributes, role, selectable }`: a
  folder as a provider lists it (data-model.md `folder` without `id` and
  `loaded`).
- `FolderRef { account: AccountId, identity: String }`.
- `FolderMembership { uid: Option<u32>, position: u32 }`.
- `Message` gains `labels: Vec<String>`.
- `ServerStep::ListFolders`, `ServerStep::OpenMailbox` (was `OpenInbox`),
  `FailureKind::MailboxChanged` (was `InboxChanged`).

## Loads (`mailbag-providers`)

- `LoadTarget::FolderList | Mailbox(FolderRef)`.
- `LoadsMail::start_load(&self, account: &AccountId, provider: MailProvider,
  target: LoadTarget, report: Box<dyn FnOnce(LoadResult)>) -> Box<dyn
  CancelsLoadOnDrop>`: one load at a time as today; the result is
  `Stored { incomplete }` (always `None` for a folder list),
  `EmptyFolderList` (a completed folder list without any folder: nothing
  was written, spec FR-001), `Failed`, or `Cancelled`.
- A folder-list load writes through `Store::replace_folders`; a mailbox
  load through `Store::replace_mailbox`. Neither reaches the window with
  data (007 FR-001).

## Store (`mailbag-store`)

- `replace_folders(&self, account, folders: &[Folder], load_cancelled) ->
  Result<StoreWrite, Failure>`; `folders` is never empty (the provider
  layer reports an empty list without writing).
- `replace_mailbox(&self, folder: &FolderRef, messages: &[(Message,
  FolderMembership)], load_cancelled) -> Result<StoreWrite, Failure>`; a folder
  the store does not hold is a `MailNotSaved` failure.
- `StoreWrite { Stored, LoadCancelled }` (today's `InboxWrite`).
- `read_folders(&self, account) -> Result<Vec<StoredFolder>, Failure>`,
  `StoredFolder { folder: Folder, loaded: bool }`, unordered; the sidebar
  sorts by `FolderRole::ORDER` and the locale's collation.
- `read_mailbox(&self, folder: &FolderRef) -> Result<Option<Vec<Message>>,
  Failure>`: `None` when the folder is not loaded or is not in the store.
- `delete_other_accounts` unchanged in meaning.

## Window actions

- `app.refresh-mailbox` ("Refresh Mailbox"): the selected mailbox.
- `app.refresh-account` ("Refresh Account"): the selected account or the
  selected mailbox's account.
- `RetriedOperation::RefreshMailbox | RefreshAccount | ReadStoredMail`; the
  last reads the folder lists and the shown mailbox again (`app.read-stored-mail`).
