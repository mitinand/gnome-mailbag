# Contract: Synchronization

The definitions the crates share for a cycle, kept in `mailbag-domain`, and
the operations the window, the providers and the store agree on. Names are
the code's; meanings are the spec's. A *cycle* and a *batch* are the
spec's terms; the code says `FolderBatch` for a batch and keeps the
user's word, *mailbox*, where it names the refreshed folder
(`LoadTarget::Mailbox`, "Refresh Mailbox").

## Domain types (`mailbag-domain`)

- `FolderState { server_position: Option<String>, fill_place:
  Option<String>, synchronized: bool }`:
  what a folder remembers between cycles (data-model.md `folder`). A
  Generic IMAP message's identity is `imap:<folder>/<UIDVALIDITY>/<UID>`.
- `FolderBatch { removed: Vec<String>, read_states: Vec<(String, bool)>,
  known_arrived: Vec<(String, bool)>, arrived: Vec<Message>, state:
  Option<FolderState> }`: one whole part of a cycle's result. `removed`
  holds identities proven gone from the folder (spec FR-004);
  `read_states` the new `seen` of messages the folder holds;
  `known_arrived` messages the folder did not hold but the account did, with
  their listed `seen`, related without fetching (research §4); `arrived`
  full records to insert or update (messages the account did not hold, and
  stored messages whose fields the service reported again), each with its
  content or `NotDownloaded`; `state` is present in the batch that
  completes the cycle, in the first batch of a cycle that has messages
  to fetch (marking the folder not completed), and on Microsoft 365 in
  each page that is not a reading's last (not completed: a first fill's
  page with its fill place, a round's or full reading's page with the
  round's start position) and in a continued first fill's last page
  (its delta link, not completed, before the one more round) (spec FR-008).
- `MessageListRow { identity, fields: DisplayFields, received_unix, seen }`:
  a message as the list shows it, without its content.
- `ReceivedContent::NotDownloaded`: the reader says the text was not
  downloaded (spec FR-009).
- `FailureKind::MailboxChanged` stays only for a UIDVALIDITY that changed
  during a reconnect: after an unreadable structure, or after Gmail's
  session was renewed (research §10, §13; its other three producers go); `IncompleteList::MoreAvailable` and its
  wording go with their last producer.

## Loads (`mailbag-providers`)

- `LoadsMail::start_load(&self, account, provider, target: LoadTarget,
  on_event: Box<dyn FnMut(LoadEvent)>) -> Box<dyn CancelsLoadOnDrop>`:
  `LoadEvent::BatchStored` any number of times, then exactly one
  `LoadEvent::Finished(LoadResult)`, on the calling GLib context. One load
  runs at a time as today.
- `LoadTarget::Mailbox(folder)` runs one cycle of the folder;
  `LoadTarget::FolderList` is unchanged (008).
- `LoadResult::Stored { incomplete }`: the cycle completed, or it ended
  incomplete (`IncompleteList::ServerRefused`): a refused listing removes
  nothing; a batch's row FETCH that ended with NO after a complete
  listing keeps the removals the listing proved and stored. `Failed` and
  `Cancelled` as today; the batches stored before stay (spec FR-010).
- A cycle writes only through `Store::store_batch`; nothing reaches the
  window with data (007 FR-001).
- Renewing access (research §13) stays inside `mailbag-providers`: the
  cycle asks through a channel that `MailLoader` answers on GTK's context
  with the adapter's `request_imap_access` or `request_graph_access`, and
  continues only with a different token. `LoadsMail` changes only for
  `on_event`.

## Store (`mailbag-store`)

- `read_folder_sync(&self, folder: &FolderRef) -> Result<FolderSync,
  Failure>` with `FolderSync { state: FolderState, stored: HashMap<String,
  bool> }` (identity → `seen`); a folder the store does not hold is a
  `MailNotSaved` failure, as for writes.
- `store_batch(&self, folder: &FolderRef, batch: &FolderBatch,
  load_cancelled: impl FnOnce() -> bool) -> Result<StoreWrite, Failure>`:
  one transaction in the order of data-model.md; `StoreWrite::LoadCancelled`
  writes nothing.
- `stored_identities(&self, account: &AccountId, identities: &[String]) ->
  Result<HashSet<String>, Failure>`: which of a batch's identities the
  account already holds.
- `identities_in_other_folders(&self, folder: &FolderRef, identities:
  &[String]) -> Result<HashSet<String>, Failure>`: which of a Microsoft 365
  page's identities another folder of the account holds, so the cycle reads
  such a message again before applying an entry that may be older than that
  folder's state (research §5; added in portion 4).
- `read_folder_rows(&self, folder: &FolderRef) ->
  Result<Option<Vec<MessageListRow>>, Failure>`: newest first; `None` for
  "no mail loaded".
- `read_message_content(&self, account: &AccountId, identity: &str) ->
  Result<Option<ReceivedContent>, Failure>`: `None` when the message is no
  longer stored.
- `replace_mailbox` and `read_mailbox` go; `replace_folders`,
  `read_folders` and `delete_other_accounts` are unchanged in meaning.

## Protocol crates

- `mailbag-imap`: `MailboxReader::open` and `uid_validity()` as today;
  `ImapError` tells that the server ended the session with BYE (research
  §13);
  `MailboxReader::list_messages(row_items) -> Result<FolderListing,
  ImapError>` with `FolderListing { messages: Vec<ListedUid>, refusal:
  Option<ServerReply> }` and `ListedUid { uid, seen, gmail_message_id:
  Option<u64> }` (research §2); `fetch_rows_by_uid(&[u32], row_items) ->
  MessageList` replaces the sequence-number `fetch_rows` and keeps its
  refusal; `fetch_structures` skips a group that all vanished instead of
  failing; `fetch_text` unchanged.
- `mailbag-graph`: `read_message_changes(service_url, token, start:
  ChangesFrom) -> Result<ChangePage, GraphError>` with `ChangesFrom::
  FirstReading(folder_id) | Link(String)` and `ChangePage { changes:
  Vec<MessageChange>, next: NextPage }`, `NextPage::More(next_link) |
  Done(delta_link)`, `MessageChange::Removed(id) | Listed(GraphMessage) |
  Changed { id, is_read: Option<bool>, other_fields: bool }` (an entry
  that carries only what changed, research §5); `read_message` answers a
  404 as `None`; `GraphFailure::PositionRejected` for a 410 or
  `syncStateNotFound` (research §5); `read_message_text(service_url, token,
  id)` for a round of changes; `read_texts_received_between(service_url,
  token, folder_id, from, to) -> Result<Vec<(String, Option<String>)>,
  GraphError>` for a first fill's page; `read_message(service_url, token,
  id) -> Result<Option<(GraphMessage, folder_id)>, GraphError>` with the
  message's current folder (`parentFolderId`), `None` for a 404.
  `list_mailbox_messages` goes.

## Window

- `MessageItem`: the list's row object, with the properties the row
  template binds: `identity`, `sender`, `subject`, `date-text`, `unread`,
  and `read-state-text` ("Read" or "Unread") for the row's accessible
  description (research §9). `message-row.ui` is a `GtkListItem` template,
  its preview hidden in the template; the window builds the
  `GtkBuilderListItemFactory` from the form's bytes, as it loads every form,
  and sets it on the list view that `mailbag.ui` declares.
- `MailUi::show_rows(account, rows: Rc<[MessageListRow]>)` updates the list
  by the difference with the rows shown (research §9): read states in
  place, arrivals and removals in one splice; it keeps the open message,
  whose row is the selected one, while it is listed, and reads its content
  again.
  A batch of any folder of the shown folder's account makes the window
  read the shown folder again.
- Opening a message reads its content with `read_message_content` on GIO's
  pool; the reader shows the text, the reason for none, or "not
  downloaded". A read that fails is shown in the reader's status page with
  Retry reading the stored mail again (007 FR-013's operation).
