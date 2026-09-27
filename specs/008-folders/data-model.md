# Data Model: Folders

The persisted form of the store after this feature (spec FR-004, FR-007),
replacing 007's two tables. Before the first release a change to this model
discards the store at start (007 FR-012). The schema is one SQL text in
`mailbag-store`; its hash is the store's version. All tables are `STRICT`.

## Tables

### `folder` — one mailbox of an account as its server listed it

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER, primary key | Row identity for memberships |
| `account` | TEXT, not null | The Online Accounts ID |
| `identity` | TEXT, not null | The provider's identity: the IMAP or Gmail mailbox name as sent, the Microsoft 365 folder identifier |
| `name` | TEXT, not null | The name shown: decoded from modified UTF-7 when needed, the container prefix dropped on Gmail |
| `parent` | TEXT, null at the root | The parent's `identity` |
| `attributes` | TEXT, not null | Every attribute or well-known name the server listed, space-separated, as sent |
| `role` | TEXT, null, one of `inbox starred important junk trash archive drafts sent all_mail` | The application role |
| `selectable` | INTEGER, 0 or 1 | Whether the folder can be opened |
| `loaded` | INTEGER, 0 or 1 | Whether a mailbox load of it completed (007 FR-006) |

Unique on `(account, identity)`. An account with no rows has no folder
list; an empty completed list is never stored (spec FR-001).

### `message` — one message of an account

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER, primary key | Row identity for memberships |
| `account` | TEXT, not null | The Online Accounts ID |
| `identity` | TEXT, not null | `gmail:<X-GM-MSGID>`, `graph:<immutable id>`, or `imap:<folder identity>/<uid>` |
| `subject`, `sender`, `recipients`, `received`, `seen`, `content_kind`, `content_detail` | as in 007 | The list fields, read state and content |
| `labels` | TEXT, null | Gmail's labels of the message as last received, one per line (a label name may hold a space and never a line break, so the list reads back exactly); data only |

Unique on `(account, identity)`.

### `membership` — a message's place in a folder

| Column | Type | Meaning |
|---|---|---|
| `folder` | INTEGER, references `folder (id)` on delete cascade | |
| `message` | INTEGER, references `message (id)` on delete cascade | |
| `uid` | INTEGER, null on Microsoft 365 | The IMAP UID in this folder, valid with the folder's UIDVALIDITY (not stored until synchronization needs it) |
| `position` | INTEGER, not null | The load's order, newest first |

Primary key `(folder, message)`; an index on `message` serves orphan
checks and deletions.

## Rules

- **Replacing a folder list** (FR-001): in one transaction, delete the
  account's `folder` rows whose identity is not listed (their memberships
  go with them), delete the account's messages left without a membership,
  update the listed rows' name, parent, attributes, role and selectable
  (`loaded` kept), insert new rows with `loaded = 0`.
- **Replacing a mailbox** (FR-004): in one transaction, delete the folder's
  memberships; for each received message, insert the `message` row or
  update its fields, read state, content and labels by `(account,
  identity)`; insert its membership with the UID and position; delete the
  messages that lost their last membership; set `loaded = 1`. A message's
  fields are thus the latest load's whichever folder loaded it; its
  relations in other folders stay until those folders' loads (spec FR-004).
- **Reading a mailbox**: `None` when `loaded = 0`; otherwise the messages
  joined through `membership`, by position.
- **Deleting an account's mail** (007 FR-008): delete its `folder` and
  `message` rows.
- **Not stored**: UIDVALIDITY, counts, expansion state, credentials, server
  replies, failures, load state (007 FR-002, spec FR-013).

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The selection: an account, a mailbox, or nothing | `AccountList` (the window's account state) | Until the user changes it, its row collapses, its folder or account is gone, or the selected account's folders appear for the first time |
| The folder lists of the shown accounts as last read, with the number of the latest read | The window | The run; read again after a complete account update and after each completed Refresh Account |
| The shown mailbox's rows, with the number of the latest read | The window | As 007's shown Inbox |
| Each account's latest refresh outcome, with the target it was for | The window's `Refreshes` | Until the account's next load ends (006 FR-007, 007 FR-005) |
