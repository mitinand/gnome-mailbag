# Quickstart: Read and star

How the feature is checked. Automated checks run with `scripts/check.sh`;
the installed-build checks are the maintainer's, on a Flatpak build
(`README.md`), with an account of each provider and that provider's web
client open beside the window.

## Automated

- `mailbag-store`: rows read the effective state; a pending write stores
  the wish even when it equals the server value; a batch's flag write
  leaves the pending values, equal or not, and leaves a flag the report
  did not name; the pending query lists only the folder's non-null
  values; a settle or a drop ends only
  a pending value equal to the command's; a restart over the same store
  reads the pending state again (SC-002's store half).
- `mailbag-imap`: the folder is opened with `SELECT`; `\Flagged` is read
  in the listing and the rows; `store_flags` sends one `UID STORE` for a
  UID set with the silent form, drains a `FETCH` line, returns the reply
  of a `NO` and of a `BAD`, and fails on a lost connection; the scripted
  server keeps the flags a store changed and can hold a completion until
  the test releases it.
- `mailbag-graph`: `update_message_flags` sends `PATCH` with the JSON
  body, the content type and the ImmutableId preference; a 200, a 400, a
  404 and a 429; a partial entry with `flag` only and a full entry; the
  scripted service records the method and the body.
- `mailbag-providers` (scripted servers): each change reaches the server
  as one command for exactly its messages, at the next cycle (SC-001); a
  change made while a command is in flight survives its acceptance and is
  sent next; 250 pending changes go in three commands; a 504 keeps the
  pending change for the next cycle; a message reported in two pages of
  one round keeps both flags; a wish the IMAP listing already shows ends
  without a command, while Microsoft 365 sends every pending change and
  no report ends one; a read mark sent under Starred after the star was
  taken off stays pending; a refused listing after the commands ends the
  cycle incomplete (research §15);
  a star during a first fill of 300 messages is received before the
  second batch's rows are read, and one after the last batch before the
  connection closes (SC-003); a connection closed after the command and
  one closed before it (SC-004); a refused command: pending dropped, the
  `StoreChanged` event sent, the cycle failed with the reply, nothing
  sent next time (SC-005); a Gmail message under two labels: one
  command, the second cycle sends none (SC-006); Microsoft 365 sends
  after the round and the next round's partial and full entries leave
  the effective state as it is.
- `mailbag` (GUI tests, one per process): the star toggle lights and the
  row's star appears after the re-read, and no load starts; Mark as
  Unread keeps the message open, the dot returns and the timer does not
  mark it read again; read on opening after the second writes the
  pending change, not within half a second (SC-007); two rapid changes
  are written in the user's order; a refused change shows the row as the
  server has it and the failed refresh's banner.

## Installed build

1. Open a message, wait a second: its dot goes out; select another folder
   and come back: it stays read; nothing loads meanwhile. Refresh: the
   web client shows it read (SC-007, SC-008).
2. Star the open message: the star lights in the reader and in the row.
   Refresh: the web client shows the star (Gmail: under Starred;
   Microsoft 365: flagged for follow-up). Unstar it and refresh: both
   clear (SC-008).
3. Choose Mark as Unread on the open message: the dot returns, the
   message stays open and does not become read again while it stays open;
   refresh: the web client shows it unread. Open it anew: it counts read
   after a second.
4. Turn the network off, star a message and mark another unread: both
   show at once; refresh: the usual failure banner. Turn the network on
   and refresh: both reach the web client (SC-002).
5. Quit with a change made offline, start again: the change is still
   shown; refresh: it reaches the server.
6. Gmail: mark a message read under one label it shares with another;
   select the other label: it is read there too. Unstar a message while
   viewing the Starred folder, refresh: it leaves the folder.
7. In the web client, star a message and mark another unread; refresh:
   the window shows both.
8. Refresh a large folder for the first time and, while it fills, open
   and star listed messages: the web client shows the changes while the
   fill is still running (SC-003).
9. Change a message in the window and, before refreshing, change it the
   other way in the web client; refresh: the window's change wins, and
   the web client follows it.
10. Star a message, refresh, and unstar it while the refresh runs: after
    the refresh the message is unstarred in the window and, after one
    more refresh, in the web client; the record shows the second command.
