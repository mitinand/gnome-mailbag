# Research: Read and star

Decisions that had alternatives or needed a check, with what the check
showed. Facts are marked checked (a document, a source line, or a probe
run on 2026-10-02 against live servers: Gmail, Microsoft 365 and two
Generic IMAP servers of different vendors), inferred, or unknown. The
probe scripts are not kept; their results are here.

## §1 Pending changes in the store, sent by the cycle

**Decision**: a change is stored as a wanted value per flag on the
message and sent by a cycle (spec FR-001, FR-006).

**Alternatives**: send each change on its own at once and update the
window optimistically, with no stored intent; the cycle then only reads.
Smaller by roughly a third when offline changes are given up, and it
keeps 009's "a cycle reads only". Against it: offline marking is lost
(read on opening would fail on every open without a network, or stay
window-only); an IMAP command needs a connection of its own, since the
worker runs one load at a time and a session runs one command at a time
(sign-in per command ≈ 1.5–2.5 s on Gmail, or a kept connection with its
own state); a running cycle's listing is a snapshot from the cycle's
start, and `relate_known` writes its read state when a message is related
to a filling folder, so a message read in Inbox during All Mail's first
fill flips back to unread until the next refresh; a failed send must be
redone by the user. The maintainer chose the stored change; no chain of
operations with states is built (spec FR-013(e)).

## §2 List first, then send

**Decision**: a cycle sends after storing its listing, before each batch
of missing messages and once before closing (spec FR-007); 009 FR-015(a)
said "before learning changes" and is amended.

**Rationale**: the listing (`UID FETCH 1:* (UID FLAGS …)`) gives every
message's UID in this mailbox next to its identity, so a Gmail message is
addressed without a stored UID and without a search; and it gives the
server's current flags, so a change whose outcome the last cycle did not
learn is settled by comparison (FR-009). Sending before each batch makes
a change made during a minutes-long first fill leave within one batch.

**Alternatives**: send before the listing, as 009 wrote: Gmail would need
the UID on each membership (a column, written on every relation, invalid
after a `UIDVALIDITY` change, so a numbering version stored too; 007
FR-003 forbids addressing by a UID of another numbering) or `UID SEARCH
X-GM-MSGID` per message (checked: works, ≈ 0.2 s each). Both rejected:
the listing already holds the answer.

**Checked**: Gmail's listing carries `X-GM-MSGID` per UID (009); the
fork's `uid_store` exists and the stream reports a `NO`/`BAD` completion
as its last error item (fork `fix/fetch-completion-status`).

## §3 Two nullable columns, not a table

**Decision**: `message.seen_pending` and `message.flagged_pending`, null
when nothing is pending, next to `message.seen` and the new
`message.flagged` (data-model).

**Rationale**: a desired value per property, the newest replacing the
older, is what the feature needs; two columns are exactly that. The effective state is one `COALESCE` in the row read; the
cycle's read gets the pending values with the server values it already
reads; a wish equal to the server value is dropped in the same `UPDATE`.

**Alternatives**: a `pending_change` table (message, flag, wanted,
created): keeps history nobody reads, needs a join in every read and a
delete on settle; rejected. Pending values on the membership: wrong for
Gmail, where a flag belongs to the message under every label (§5).

## §4 Mailboxes opened with SELECT

**Decision**: `SELECT` replaces `EXAMINE` wherever a cycle opens a folder
(spec FR-008; 002's contract amended).

**Checked**: today's reader uses `examine` (`session.rs`,
`examine_mailbox`). On Gmail `EXAMINE` answers `READ-ONLY` with
`PERMANENTFLAGS ()`, and a `UID STORE` there is refused: `NO STORE
attempt on READ-ONLY folder (Failure)`. One Generic IMAP server refuses
with `BAD [CLIENTBUG] UID STORE Can not store in read-only folder`; the
other reports `READ-WRITE` under `EXAMINE` and accepts the command,
against RFC 3501 §6.3.2. `SELECT` answers `READ-WRITE` on all three and
`PERMANENTFLAGS` lists `\Flagged` and `\Seen` on all three. The fork's
`select` returns the same mailbox data as `examine` (`client.rs`).

**Consequences**: a refusal may be `NO` or `BAD`; both are the refusal of
FR-010. 002's contract line "Never SELECT" and "no mail-changing
commands" become "SELECT; the flag commands of 011 FR-007 are the only
mail-changing commands".

## §5 Gmail: flags belong to the message

**Decision**: a Gmail change is stored on the message and sent by the
cycle of whichever label lists it first (spec FR-012).

**Checked** on a live account: `UID STORE +FLAGS.SILENT (\Flagged)` under
Inbox made the message carry `X-GM-LABELS ("\Starred")` at once; All
Mail listed it with `\Flagged` under its own UID; the mailbox with the
`\Flagged` attribute (Starred) listed it; `-FLAGS.SILENT (\Seen)` under
Inbox showed `FLAGS ()` in All Mail. Google's documentation names Starred
among the special folders with the `\Flagged` attribute. Gmail still sends
the untagged `FETCH` line for a `.SILENT` store; the other servers send
none; the fork's stream takes both. A store on a UID the mailbox lacks
answers `OK` and changes nothing (all three servers), so a vanished
message produces no refusal. The special mailboxes have modified-UTF-7
names; they are found by attribute, as 008 does.

## §6 Microsoft 365: PATCH and what the next round reports

**Decision**: `PATCH /me/messages/{immutable id}` with `{"isRead": …}` or
`{"flag": {"flagStatus": "flagged" | "notFlagged"}}`, sent after the
round of changes (spec FR-005, FR-007); `flag` joins the change fields
and a partial entry carrying only `isRead` or `flag` is not "other
fields".

**Checked** against the service: the request needs `Content-Type:
application/json` and the ImmutableId preference; it answers 200 in
0.6–0.8 s with the whole message (≈ 85 KB, body included); the id is
unchanged. The next delta round reports a read-mark change as a partial
entry `[id, isRead]` and a follow-up-flag change as a full entry with
every list field (`bodyPreview, flag, from, id, isRead, receivedDateTime,
subject, toRecipients`). 009 treats a full entry as "fields reported
again" and re-reads the text of a recent message once (009 FR-009); the
cost, one `GET` per starred recent message per round, is accepted for the
third-priority provider and listed as an optional refinement in the
plan. A malformed id answers 400 `ErrorInvalidIdMalformed`; a wrong
`flagStatus` 400; a message the mailbox lacks 404. Delegated scope
`Mail.ReadWrite` is required (documentation); Online Accounts requests
`mail.readwrite` (005 research). A follow-up marked `complete` reads as
not starred: the application has no state for a done follow-up.

**Unknown**: whether a `Prefer: return=minimal` header shrinks the
answer; not tried, listed as optional.

## §7 A refused command

**Decision**: the pending changes the command carried end, the rows show
the server state, and the cycle fails with a failure that names the
change and carries the server's reply, through the refresh's channel
(spec FR-010). Other pending changes of the folder wait for the next
cycle.

**Alternatives**: keep the cycle running and show a lasting notice of a
"partial result" (006's incomplete-list channel): a new kind in 006 with
its own wording for a rare case; rejected. Keep the pending change and
retry on every cycle: a server that refuses `\Flagged` forever would
refuse on every refresh; rejected. Keep the pending change through a
refusal the server marks temporary (IMAP `UNAVAILABLE`, Microsoft 365
throttling), which the challenge suggested since both classifications
exist in the code: the maintainer decided against it on 2026-10-03, a
server error being a server error whose outcome cannot be guaranteed;
listed as optional in the plan.

## §8 No change starts a cycle; Refresh does

**Decision**: a change is stored and shown; cycles start as 009 says,
today by Refresh Mailbox; a change made while a cycle of its folder runs
is sent before the cycle's next batch or before it closes; any other
change waits for the folder's next cycle (spec FR-006).

**History**: the sizing proposed that a change made while no load runs
starts a cycle of the shown folder, so that a star reaches the phone
within seconds. The spec's challenge walked through reading: every
message kept open a second would start a cycle, with the sidebar's
spinner and the refresh actions disabled for its length (about 4 s on a
synchronized Gmail folder: connect and sign-in ≈ 0.8 s, `SELECT` ≈ 0.3 s,
the listing ≈ 1 s, `STORE` ≈ 0.3 s, measured on 2026-10-02), and offline
a failed-refresh banner after every message read. The maintainer chose
Refresh only on 2026-10-03, for reads and explicit changes alike: the
simplest rule, no new code in the window, and background synchronization
later sends without the user. A cycle started by a change is listed as
optional in the plan.

**Alternatives also weighed**: a set of folders due for a cycle after the
running load (≈ 15 lines), optional; forbidding actions in other folders
during a load (a button state and an exception for read on opening),
rejected.

## §9 The star toggle as a stateful action

**Decision**: `message.star` is a stateful boolean action on an action
group inserted on the reader page; the existing `GtkToggleButton` with
`action-name="message.star"` shows the state and toggles it;
`message.mark-unread`, `message.mark-read` are plain actions; the reader
header's `app.mark-scope-read` / `app.mark-scope-unread` call the same
handlers.

**Inferred** from GTK's `GtkActionable` documentation: a toggle button
bound to a boolean-state action without a parameter reflects the state
in `active` and changes the state on activation. Verified by the first
GUI test of portion 5; if it does not hold, the toggle's `toggled` signal
drives the handler directly (≈ 5 lines).

## §10 Read on opening reuses 010's timer

**Decision**: the one-second timer of 010 FR-009 stays; when it fires it
asks for a pending change to read instead of putting the identity into
the window's read-in-window set, which is removed (spec FR-003).

**Rationale**: no new timer; the window-only state existed only because
nothing stored the change. Mark as Unread drops the timer when it is still
pending and, once the message counts read, writes the pending change to
unread; the timer is armed only by opening, so the message stays unread
until opened anew (spec FR-002).

## §11 Every write of a server value ends an equal pending value

**Decision**: the store's flag writes (`set_flag_states`, the upsert of a
full record, `relate_known`) set the pending column to `NULL` when the
value written equals it, and leave a differing pending value untouched;
the sending step then sends every non-null pending value (data-model;
spec FR-001, FR-009).

**Rationale**: the plan as first written let a batch never touch a
pending column and had the sending step skip a pending value equal to the
server value. The challenge found the defect: a star sent and accepted
with the connection lost after the OK, or set by another client first,
left a stale pending value forever; when another client later unstarred
the message, the stale wish differed again and the cycle re-applied it.
With the rule at the write, a pending value is always a change the
server lacks, the sending step needs no comparison and no re-read of the
whole folder before every batch (a folder of 100 000 rows, read up to a
thousand times in a first fill), and FR-009's "a server state equal to
the wish ends the pending change" is one SQL `CASE` in the writes that
already exist.

**Alternatives**: end the equal pending values in the sending step by a
separate write; a second owner of the same rule, rejected.

## §12 The window re-reads the rows after a write

**Decision**: after `write_pending_flag` succeeds, the window reads the
shown folder's rows again, the same read as after a stored batch; the
difference update sets the dot and the star, and the envelope sets the
star action's state from the row. The window never sets a row's state by
itself (007 FR-001).

**Rationale**: the plan first updated the row object and the toggle
directly after the write and retired 010's read-in-window set. The
challenge found the race the set had guarded against: a row read started
before the write (after a stored batch) answers afterwards with the old
row, and the difference update sets every row from the rows it got, so
the star would go out until the next read. The window's reads are
numbered and an older read's answer is dropped (`finish_read`), so a read
started after the write always wins. Cost: one folder read on the pool
per change, about 100 ms for 100 000 rows; the dot and the star change
when it answers. Mark as Unread drops the pending read timer before the
write, so the timer cannot fire during it and store "read" over the
user's "unread".

**Alternatives**: read one row instead of the folder (≈ 30 lines, a
second path to update a row); listed as optional for folders where the
delay shows.

## §13 The cycle's event says what happened

**Decision**: `LoadEvent::BatchStored` becomes `StoreChanged`: the cycle
changed the folder's stored state, a batch or a dropped pending change,
and the window reads again. `drop_pending` sends it; `settle` changes no
effective state and sends nothing.

**Rationale**: the window re-reads the shown folder only on that event
and after a load that ended stored (`finish_load`); a refused command
fails the cycle, so without the event the reverted row stayed on screen
until the folder was reselected. The challenge proposed sending
`BatchStored` from the drop; the maintainer objected to the name, since
no batch was stored, and the event is renamed for its meaning.

## §14 The external review: a command in flight

An outside review of the documents on 2026-10-03 found six points; the
maintainer decided them the same day.

1. **A wish made while a command is in flight was lost.** The first rules
   ended a wish equal to the server value at the moment it was written,
   and an accepted command cleared the pending value outright. Sequence:
   star (pending 1, server 0); the cycle sends; the user unstars while the
   answer is out (server still 0, so the wish was dropped as "equal"); the
   OK settles server 1 and clears nothing: the unstar is gone, also after
   a restart. Now a wish is stored as made; an accepted command ends only
   a pending value equal to the sent value, a refused one only a pending
   value equal to the refused value; a pending value equal to the stored
   server value is ended by the next sending step without a command. The
   same sequence then ends with pending 0 after the OK and a second
   command that unstars (spec FR-001, FR-007, FR-010).
2. **A Microsoft 365 partial entry re-applied a stale flag.** T007 took
   the flag a partial entry did not name from the cycle's starting
   snapshot; a message reported in two pages of one round (its read state
   on the first, its star alone on the second; the service documents that
   an item may appear more than once) had its read state reverted by the
   second page. A report now writes only the flags it names
   (`FlagChanges` with options, `COALESCE` in the store).
3. **Mark as Unread in the first second.** The plan checked "already in
   the wanted state" before dropping the read timer; an open unread
   message is still unread, so the handler returned and the timer later
   marked it read. The timer is dropped first. When the timer's write has
   already started, a second write on GIO's pool could land before it, so
   the window's writes run one at a time through a small queue.
4. **A lost Microsoft 365 answer.** The service documents that a change
   may reach a delta answer with a delay, so the next round may not settle
   a pending change whose request was applied; the request is then sent
   again, which is harmless since it sets a value. The spec says so and
   keeps "never again without the listing's evidence" for IMAP.
5. **A 5xx answer** (504 gateway timeout, 503) does not say the change was
   refused: the service could not complete the request. It is FR-009's
   unknown outcome: the cycle fails, the pending change stays. The
   maintainer chose this on 2026-10-03, apart from the temporary 4xx
   refusals he chose to drop.
6. **One command for every pending message** could exceed a server's
   command line (Dovecot's default is 64 KiB); a hundred UIDs per command.

The test budget rose to 850 for the race tests (a held completion on the
scripted IMAP server; two rapid writes in the window).
