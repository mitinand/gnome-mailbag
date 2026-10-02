# Data model: Read and star

What this feature adds to the stored form of a message
([007 data-model](../007-mail-storage/data-model.md),
[009 data-model](../009-synchronization/data-model.md)) and what it keeps
in memory only. The schema's text is its version: the change discards an
existing store at start (007 FR-012), pending changes included.

## Tables

### `message` — three new columns

| Column | Type | Meaning |
|---|---|---|
| `flagged` | `INTEGER NOT NULL CHECK (flagged IN (0, 1))` | The star as the server last reported it (`\Flagged`; Microsoft 365 `flag.flagStatus = flagged`). Written like `seen`: by a cycle's batch and by an accepted command. |
| `seen_pending` | `INTEGER CHECK (seen_pending IN (0, 1))`, null | The read state the user wants and the server does not have yet; null when nothing is pending. |
| `flagged_pending` | `INTEGER CHECK (flagged_pending IN (0, 1))`, null | The star the user wants and the server does not have yet; null when nothing is pending. |

`seen` keeps its meaning: the read state as the server last reported it.

## Rules

- **Writing a pending change** (`write_pending_flag`, spec FR-001): one
  `UPDATE` of the message by `(account, identity)`: the pending column
  becomes the wanted value, or `NULL` when the server column already
  holds it. The newest wish replaces the older. Committed before the
  window shows the change.
- **Reading a folder's rows** (`read_listed_rows`, spec FR-001): `seen`
  is `COALESCE(seen_pending, seen)` and `flagged` is
  `COALESCE(flagged_pending, flagged)`: the effective state. The window
  never reads the server columns alone, and reads them again after every
  write of its own and every `StoreChanged` event.
- **Reading a folder for a cycle** (`read_folder_sync`): per identity the
  server `seen` and `flagged`, so the listing's changed flags are found
  as today.
- **Writing a server value** (`store_batch`'s `set_flag_states`, the
  upsert of a full record and `relate_known`; spec FR-001, FR-009): the
  server column takes the value, and the pending column of that flag
  becomes `NULL` when it equals the value written (`seen_pending = CASE
  WHEN seen_pending = ?new THEN NULL ELSE seen_pending END`); a differing
  pending value is untouched. So a non-null pending value is always a
  change the server lacks.
- **Reading the pending changes** (`read_pending_changes`, spec FR-007):
  the folder's messages with a non-null pending column, by
  `membership.folder`, as `(identity, flag, wanted)`; read before each
  sending step.
- **Settling** (`settle_flags`, spec FR-007): for the identities of one
  accepted command, server column := the sent value and pending column
  := `NULL`, in one transaction. The effective state does not change, so
  no row read is due.
- **Dropping** (`drop_pending_flags`, spec FR-010): for the identities of
  a refused command, pending column := `NULL`; the cycle tells the window
  the store changed and the next row read shows the server state.
- **Removal**: a message that leaves its last folder is deleted with its
  pending values (009 data-model); nothing is sent for it.

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The listing's `identity → UID, flags` of the folder | The running IMAP cycle | One cycle; the addresses every sending step uses |
| The star action's state and the row object's `starred` and `unread` | The window | While the message is open or listed; set from the stored rows after each read, never by the window itself |
