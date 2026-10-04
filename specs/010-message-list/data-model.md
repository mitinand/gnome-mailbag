# Data model: Message list

What this feature adds to the stored form of a message
([007 data-model](../007-mail-storage/data-model.md),
[009 data-model](../009-synchronization/data-model.md)) and what it keeps
in memory only. The schema's text is its version: the change discards an
existing store at start (007 FR-012).

## Tables

### `message` — one new column

| Column | Type | Meaning |
|---|---|---|
| `preview` | `TEXT NOT NULL` | The first readable words of the message, up to 400 characters, as spec FR-003 makes them; empty when the message has no readable text or the server did not return the part. |

Written with every arrived record (`store_arrived`): the record's value
replaces the stored one, since a fetched row always carries a fresh
preview. Read with the rows (`read_listed_rows`). A read-state change or
a message related without fetching (009 FR-005) leaves it as it is.

## Rules

- A preview is text the sender wrote, cut and normalised; it is never a
  header, an address, markup or a made-up summary (spec FR-003, FR-010).
- The preview follows the content: when a message's record is stored
  again with a new text (009 FR-009), the new preview comes with it.
- Privacy: previews are mail content and follow 003's rules; none is
  written to a record line.

## In memory only

- **Row object** (`MessageItem`, window): the stored row and its texts
  made on demand (sender, subject, date wording, the preview), the read
  state and the star as the store's effective values (since 2026-10-03 by
  [Read and star](../011-read-and-star/spec.md); before, the stored read
  state until a read on opening changed it, spec FR-009), and the
  animation state (`shown`, `transition-ms`, spec FR-006). Every read of
    the stored rows sets the read state and the star.
- **Removed in window** (`removed_in_window`, window): the identities the
  trash button took out, kept while the folder stays shown and emptied
  when another folder is shown (spec FR-010, since 2026-10-05).
- **Shown mailbox** (window): the folder's stored rows as the latest read
  found them and which folder they belong to (a change of folder shows
  the rows at once, research §10); whether the unread filter is on (one
  state for the window, spec FR-008).
- **Read in window** (window): the identities counted read by spec FR-009
  since the latest read of the stored rows; emptied by every new read
  (research §11). *Retired on 2026-10-03 by
  [Read and star](../011-read-and-star/spec.md)*: the second's read is a
  stored pending change, read back with the rows.
- **Pending read** (window): the one-second timeout of the open message,
  dropped when another message opens or the message leaves, and, since
  [Read and star](../011-read-and-star/spec.md), by Mark as Unread.
- **Pending change** (window): the one timeout that applies the list's
  latest state once the rows that leave are closed (research §10); a
  closed row is never opened and never a neighbour for spec FR-007.
