# Data Model: Folders

**Amended** on 2026-09-29 by
[009's data model](../009-synchronization/data-model.md): `folder.loaded`
becomes `synchronized` beside new `server_position` and `fill_place`, `membership.position`
goes, a Generic IMAP identity carries its UIDVALIDITY, and batches replace
"Replacing a mailbox" and "Reading a mailbox" below.

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
| `name` | TEXT, not null | The server's name for display: under a listed parent the part after the parent's name and the delimiter, otherwise the whole name (so Gmail's system labels lose the container's prefix); decoded from modified UTF-7 when needed; INBOX kept as sent, the window shows it as "Inbox" |
| `parent` | TEXT, null at the root | The parent's `identity` |
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

Unique on `(account, identity)`.

### `membership` — a message's place in a folder

| Column | Type | Meaning |
|---|---|---|
| `folder` | INTEGER, references `folder (id)` on delete cascade | |
| `message` | INTEGER, references `message (id)` on delete cascade | |
| `position` | INTEGER, not null | The load's order, newest first |

Primary key `(folder, message)`; an index on `message` serves orphan
checks and deletions.

## Rules

- **Replacing a folder list** (FR-001): in one transaction, delete the
  account's `folder` rows whose identity is not listed (their memberships
  go with them), delete the account's messages left without a membership,
  update the listed rows' name, parent, role and selectable
  (`loaded` kept), insert new rows with `loaded = 0`.
- **Replacing a mailbox** (FR-004): in one transaction, delete the folder's
  memberships; for each received message, insert the `message` row or
  update its fields, read state and content by `(account, identity)`;
  insert its membership with its position, the order of the load's list;
  delete the messages that lost their last membership; set `loaded = 1`. A message's
  fields are thus the latest load's whichever folder loaded it; its
  relations in other folders stay until those folders' loads (spec FR-004).
- **Reading a mailbox**: `None` when `loaded = 0`; otherwise the messages
  joined through `membership`, by position.
- **Deleting an account's mail** (007 FR-008): delete its `folder` and
  `message` rows.
- **Not stored**: UIDVALIDITY and UIDs, the server's attributes and Gmail's
  labels until the feature that reads them (spec Clarifications,
  simplification review), counts, expansion state, credentials, server
  replies, failures, load state (007 FR-002, spec FR-013).

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The selection: an account, a mailbox, or nothing | `AccountList` (the window's account state) | Until the user changes it, its row collapses, its folder or account is gone, or the selected account's folders appear for the first time |
| The folder lists of the shown accounts as last read, with the number of the latest read | The window | The run; read again after a complete account update and after each completed Refresh Account |
| The shown mailbox's rows, with the number of the latest read | The window | As 007's shown Inbox |
| Each account's latest refresh outcome, with the target it was for | The window's `Refreshes` | Until the account's next load ends (006 FR-007, 007 FR-005) |
