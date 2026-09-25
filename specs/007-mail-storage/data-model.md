# Data Model: Mail Storage

The persisted form of the store as built now (spec FR-002, FR-003). The target
model with folders, membership and provider identities is in the spec's
FR-003 and is not built. Before the first release a change to this model
discards the store at start (FR-012; [research §4](research.md)).

## Tables

The schema is one SQL text in `mailbag-store`; its hash is the store's
version. Both tables are `STRICT`.

### `inbox` — an account's Inbox as the latest completed load left it

| Column | Type | Meaning |
|---|---|---|
| `account` | TEXT, primary key | The Online Accounts ID |

A row means that a load of this account completed; with no message rows the
Inbox is empty (FR-006). No row means that nothing was loaded.

### `message` — one message of a stored Inbox

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER, primary key | Insertion order, which is the load's order, newest first |
| `account` | TEXT, not null, references `inbox (account)` on delete cascade | The Inbox it belongs to |
| `identity` | TEXT, not null | What the load calls the message, for the record only: `uid:<n>`, `gmail:<X-GM-MSGID>` or `graph:<immutable id>` |
| `subject`, `sender`, `recipients` | TEXT, null when absent | The decoded list fields (`DisplayFields`) |
| `received` | INTEGER, null when unknown | The received date, seconds since the Unix epoch |
| `seen` | INTEGER, 0 or 1 | The read state as the server last reported it |
| `content_kind` | TEXT, one of the codes below | What the reader shows |
| `content_detail` | TEXT, null unless the code needs it | The text, or the unknown character set's or encoding's name |

An index on `(account, id)` serves reading an Inbox in order.

### Content codes

| `content_kind` | `content_detail` | `ReceivedContent` |
|---|---|---|
| `text` | the text, complete | `Text(text)` |
| `plain_text_missing` | — | `Explained(NoPlainText { has_html: false })` |
| `html_only` | — | `Explained(NoPlainText { has_html: true })` |
| `encrypted` | — | `Explained(Encrypted)` |
| `smime` | — | `Explained(SecuredWithSMime)` |
| `unknown_charset` | the name | `Explained(UnknownCharset(name))` |
| `unknown_encoding` | the name | `Explained(UnknownEncoding(name))` |
| `undecodable` | — | `Explained(Undecodable)` |
| `structure_unreadable` | — | `StructureUnreadable` |
| `text_not_returned` | — | `TextNotReturned` |

The codes are a `CHECK` constraint in the schema text, so a new code changes
the version (research §4).

## Rules

- **Replacing an Inbox** (FR-004): in one transaction, delete the account's
  `inbox` row (its messages go with it), insert it again, insert the load's
  messages in their order, commit. A failed or refused write changes nothing.
- **Deleting an account's mail** (FR-008): delete its `inbox` row.
- **Settings of the connection**: `journal_mode=WAL`, `synchronous=NORMAL`,
  `foreign_keys=ON` (research §5).
- **Not stored**: credentials, server replies, failures, load state, the
  account's name or address, UIDVALIDITY, Gmail labels, thread identifiers,
  HTML, attachments (FR-002, FR-014).

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The accounts of the latest complete Online Accounts answer with Mail on | `Store` | The run; before the first answer there is none (research §6) |
| Each account's latest refresh outcome: stored, with or without an incomplete list, or failed | The window's `InboxController` | The run (006 FR-007) |
| The shown account's stored Inbox | The window | Until another account is selected or a load of it completes |
