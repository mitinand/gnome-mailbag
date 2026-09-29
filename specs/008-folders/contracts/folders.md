# Contract: Folders

**Amended** on 2026-09-29 by
[Synchronization](../../009-synchronization/contracts/synchronization.md):
`replace_mailbox` and `read_mailbox` give way to `store_batch`,
`read_folder_rows` and `read_message_content`.

The definitions the crates share for folders, kept in `mailbag-domain`, and
the operations the window, the providers and the store agree on. Names are
the code's; meanings are the spec's. Two words are used on purpose:
*folder* names the thing in the list and in the store (`Folder`,
`list_folders`, `replace_folders`, `read_folders`); *mailbox* names a folder
opened for its messages and everything the user sees (`MailboxReader`,
`LoadTarget::Mailbox`, `replace_mailbox`, `read_mailbox`, "Refresh
Mailbox"). The IMAP crate keeps the protocol's word for its list:
`list_mailboxes`, `MailboxList`, `MailboxName`, `ImapStep::ListMailboxes`.

## Domain types

- `FolderRole`: `Inbox | Starred | Important | Junk | Trash | Archive |
  Drafts | Sent | AllMail`. `FolderRole::ORDER` is the sidebar's order.
  `is_view()` is true for Starred, Important and All Mail (spec FR-003,
  FR-013(d)). The role's stored code belongs to the store, its icon to the
  sidebar (below).
- `Folder { identity, name, parent, role, selectable }`: a folder as a
  provider lists it (data-model.md `folder` without `id` and `loaded`).
- `FolderRef { account: AccountId, identity: String }`.
- `ServerStep::ListFolders`, `ServerStep::OpenMailbox` (was `OpenInbox`),
  `FailureKind::MailboxChanged` (was `InboxChanged`).

## Loads (`mailbag-providers`)

- `LoadTarget::FolderList | Mailbox(FolderRef)`.
- `LoadsMail::start_load(&self, account: &AccountId, provider: MailProvider,
  target: LoadTarget, report: Box<dyn FnOnce(LoadResult)>) -> Box<dyn
  CancelsLoadOnDrop>`: one load at a time as today; the result is
  `Stored { incomplete }` (always `None` for a folder list; a completed
  folder list without any folder writes nothing, spec FR-001), `Failed`, or
  `Cancelled`.
- A folder-list load writes through `Store::replace_folders`; a mailbox
  load through `Store::replace_mailbox`. Neither reaches the window with
  data (007 FR-001).

## Store (`mailbag-store`)

- `replace_folders(&self, account, folders: &[Folder], load_cancelled) ->
  Result<StoreWrite, Failure>`; `folders` is never empty (the provider
  layer writes nothing for an empty list).
- `replace_mailbox(&self, folder: &FolderRef, messages: &[Message],
  load_cancelled) -> Result<StoreWrite, Failure>`: the messages in the
  load's order, which is their position; a folder the store does not hold
  is a `MailNotSaved` failure.
- `StoreWrite { Stored, LoadCancelled }` (today's `InboxWrite`).
- `read_folders(&self, account) -> Result<Vec<Folder>, Failure>`,
  unordered; the sidebar sorts by `FolderRole::ORDER` and the locale's
  collation.
- A role's stored code (`inbox` … `all_mail`, data-model.md) is the
  store's, beside the content codes.
- `read_mailbox(&self, folder: &FolderRef) -> Result<Option<Vec<Message>>,
  Failure>`: `None` when the folder is not loaded or is not in the store.
- `delete_other_accounts` unchanged in meaning.

## Window

- The sidebar gives each role its icon: Inbox
  `mailbag-folder-inbox-symbolic`, Starred `mailbag-folder-starred-symbolic`,
  Important `mailbag-folder-important-symbolic`, Archive
  `mailbag-folder-archive-symbolic` and Sent `mailbag-folder-sent-symbolic`
  (bundled, from the icon-development-kit's `inbox`, `star`,
  `mail-important`, `box` and `paper-plane`), Junk
  `mail-mark-junk-symbolic`, Trash `user-trash-symbolic`, Drafts
  `document-edit-symbolic`, All Mail `folder-symbolic`; the
  others are in the platform's icon theme (checked on the host and in the
  GNOME 50 runtime); a folder without a role uses `folder-symbolic`.

- `app.refresh-mailbox` ("Refresh Mailbox"): the selected mailbox.
- `app.refresh-account` ("Refresh Account"): the selected account or the
  selected mailbox's account.
- `RetriedOperation::RefreshMailbox | RefreshAccount | ReadStoredMail`; the
  last reads the folder lists and the shown mailbox again (`app.read-stored-mail`).
