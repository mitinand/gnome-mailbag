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
  becomes the wanted value, always, since a command for that flag may be
  in flight and about to change the server column (research §14). The
  newest wish replaces the older. Committed before the window shows the
  change.
- **Reading a folder's rows** (`read_listed_rows`, spec FR-001): `seen`
  is `COALESCE(seen_pending, seen)` and `flagged` is
  `COALESCE(flagged_pending, flagged)`: the effective state. The window
  never reads the server columns alone, and reads them again after every
  write of its own and every `StoreChanged` event.
- **Reading a folder for a cycle** (`read_folder_sync`): per identity the
  server `seen` and `flagged`, so the listing's changed flags are found
  as today.
- **Writing a server value** (`store_batch`'s `set_flag_states`, the
  upsert of a full record and `relate_known`; spec FR-001, FR-009): only
  the flags the server reported are written (a Microsoft 365 partial
  entry names one: `seen = COALESCE(?seen, seen)`); each server column
  written takes the value, and the pending column of that flag becomes
  `NULL` when it equals the value written (`seen_pending = CASE WHEN ?seen
  IS NOT NULL AND seen_pending = ?seen THEN NULL ELSE seen_pending END`);
  a flag not reported and a differing pending value are untouched.
- **Reading the pending changes** (`read_pending_changes`, spec FR-007):
  the folder's messages with a non-null pending column, by
  `membership.folder`, as `(identity, flag, wanted, server value)`; read
  before each sending step, which ends a wanted value equal to the server
  value without a command (a settle with that value).
- **Settling** (`settle_flags`, spec FR-007): for the identities of one
  accepted command, server column := the sent value and pending column
  := `NULL` where it equals the sent value, in one transaction; a newer
  wish for another value stays (research §14). The effective state does
  not change, so no row read is due.
- **Dropping** (`drop_pending_flags`, spec FR-010): for the identities of
  a refused command, pending column := `NULL` where it equals the refused
  value; the cycle tells the window the store changed and the next row
  read shows the effective state.
- **Removal**: a message that leaves its last folder is deleted with its
  pending values (009 data-model); nothing is sent for it.

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The listing's `identity → UID, flags` of the folder | The running IMAP cycle | One cycle; the addresses every sending step uses |
| The star action's state and the row object's `starred` and `unread` | The window | While the message is open or listed; set from the stored rows after each read, never by the window itself |
| The window's queued pending writes | The window | Until each is written, one at a time, in the order of the user's actions |
