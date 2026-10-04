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
| `flagged` | `INTEGER NOT NULL CHECK (flagged IN (0, 1))` | The star as the server last reported it (`\Flagged`; Microsoft 365 `flag.flagStatus = flagged`). Written like `seen`: by a cycle's batch and when a cycle ends a pending change. |
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
  upsert of a full record and `relate_known`; spec FR-001, FR-009): the
  flags the server reported are written, both of them since 2026-10-05
  (before, a Microsoft 365 partial entry named one and `seen =
  COALESCE(?seen, seen)` kept the other; such an entry is now read from
  the message, 009 FR-007); the pending columns are untouched, since a
  report alone does not end a pending change (research §15).
- **Reading the pending changes** (`read_pending_changes`, spec FR-007):
  the folder's messages with a non-null pending column, by
  `membership.folder`, as `(identity, flag, wanted)`; read before each
  sending step.
- **Settling** (`settle_flags`, spec FR-007): for the identities whose
  value the cycle saw the server hold (the IMAP listing at the cycle's
  start for a change not yet sent, the flags read right after the
  command for one it sent, a Microsoft 365 request accepted), server
  column := that value and
  pending column := `NULL` where it equals it, in one transaction; a
  newer wish for another value stays (research §14, §15). The effective
  state does not change, so no row read is due.
- **Dropping** (`drop_pending_flags`, spec FR-010): for the identities of
  a refused command, pending column := `NULL` where it equals the refused
  value; the cycle tells the window the store changed and the next row
  read shows the effective state.
- **Removal**: a message that leaves its last folder is deleted with its
  pending values (009 data-model); nothing is sent for it.

## In memory only

| Value | Owner | Lifetime |
|---|---|---|
| The listing's `identity → UID, flags` of the folder | The running IMAP cycle | One cycle; the addresses every sending step uses, and the server values only the first sending step compares with, since the listing is old by the later ones (research §15) |
| The value last sent per `(identity, flag)` | The running IMAP cycle | One cycle; a wish equal to it is not sent again; since 2026-10-04 (later) the flags read right after the command end what they show, and this value keeps an unconfirmed wish from being sent again within the cycle and tells the cycle that commands went out, so 009's state pass runs once more (research §15) |
| The star action's state and the row object's `starred` and `unread` | The window | While the message is open or listed; set from the stored rows after each read, never by the window itself (research §15.4) |
| The window's queued pending writes | The window | Until each is written, one at a time, in the order of the user's actions |
