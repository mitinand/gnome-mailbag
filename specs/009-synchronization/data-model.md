# Data Model: Synchronization

The persisted form of the store after this feature (spec FR-001, FR-008,
FR-009), changing 008's three tables. Before the first release a change to
this model discards the store at start (007 FR-012), so every folder fills
again once. The schema stays one SQL text in `mailbag-store` whose hash is
the store's version; all tables are `STRICT`.

## Tables

### `folder` — changed columns

| Column | Type | Meaning |
|---|---|---|
| `server_position` | TEXT, null | Microsoft 365 only: the `@odata.deltaLink` the next round of changes starts from, once a first reading completed; null otherwise |
| `fill_place` | TEXT, null | Microsoft 365 only: the `@odata.nextLink` an unfinished first fill continues from; null otherwise. Kept apart from `server_position` so that neither link's meaning depends on `synchronized` (external review, 2026-09-29) |
| `synchronized` | INTEGER, 0 or 1 | Whether the folder's latest cycle completed; replaces 008's `loaded`. The first batch of an IMAP cycle that has messages to fetch, and each Microsoft 365 page that is not a reading's last, set it to 0; the completing batch sets it to 1 |

The other columns are 008's. Replacing a folder list (008 FR-001) keeps
these two columns of a folder it keeps. No numbering version is stored:
it is part of a Generic IMAP message's identity (below).

### `message` — changed identity and content codes

A Generic IMAP message's `identity` becomes
`imap:<folder identity>/<UIDVALIDITY>/<UID>` (spec FR-005); Gmail's and
Microsoft 365's are 008's. `content_kind` gains `not_downloaded`: a message
whose text a cycle did not download under spec FR-009. The other columns
are 008's.

### `membership` — changed columns

`position` goes: a folder's rows are ordered by the message's `received`,
newest first, then by `message.id`, newest first. The primary key
`(folder, message)` and the index on `message` stay.

No index is added for the order: the read starts from the folder's
memberships (the primary key's prefix) and sorts them.

## Rules

- **Reading a folder for a cycle**: the folder's `server_position`,
  `fill_place` and `synchronized`, and the identity and `seen` of every message it holds;
  one read at the cycle's start.
- **Storing a batch** (spec FR-008), in one transaction, after the load's
  cancellation check under the store's lock:
  1. delete the folder's memberships of the removed identities;
  2. delete the account's messages left without a membership;
  3. set `seen` of the listed read-state changes by `(account, identity)`;
  4. for each full record (an arrival, or a message whose fields the
     service reported again), insert the `message` row or update its list
     fields and read state by `(account, identity)`; its content replaces
     the stored one unless the record's content is `not_downloaded` and a
     content is stored (another folder's cycle downloaded it);
  5. insert the arrival's membership in the folder if missing, and the
     memberships of the batch's messages the account already held,
     setting their `seen` as listed;
  6. when the batch carries a folder state, write the two state
     columns.
  A folder the store does not hold fails the write, as 008's loads do.
- **Reading a folder's rows**: `None` when `synchronized = 0` and the folder
  holds no membership ("no mail loaded", 007 FR-006); otherwise the
  identity, list fields and `seen` of its messages in the order above,
  without `content_detail`.
- **Finding stored messages of a batch**: which of up to a hundred
  identities the account already holds, by `(account, identity)`.
- **Reading a message's content**: `content_kind` and `content_detail` by
  `(account, identity)`, when the message is opened.
- **Deleting an account's mail** (007 FR-008): unchanged.
- **Not stored**: IMAP UIDs (a Generic IMAP identity holds its UID, Gmail
  matches by `X-GM-MSGID`; read and star adds them, spec FR-015(a)), the
  listed identities of a Microsoft 365 re-reading (held in memory for one
  cycle), pending changes, counts.

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The folder's stored identities with `seen`, and the server's listing | The running cycle | One cycle |
| The identities a Microsoft 365 full reading listed | The running cycle | One cycle; a stopped re-reading starts over |
| The shown folder's rows, the number of the latest read, and whether another read is due | The window | As 008's shown mailbox; a batch of the shown folder marks a read due |
| The list model's items and the open message's identity | The message list | While the folder is shown |
