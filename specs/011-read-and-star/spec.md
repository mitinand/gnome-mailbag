# Feature Specification: Read and star

**Feature**: `011-read-and-star`
**Created**: 2026-10-02
**Status**: Approved on 2026-10-03 (tasks T001), after the challenge and
the consistency analysis with their decisions applied. Sized on
2026-10-02, before writing (budget: at
most 600 production lines and 700 test lines, the test budget raised to
800 and then 850 on 2026-10-03; no thread, timer or queue of its own, no new
dependency, no change to the IMAP library forks; three new columns on the
stored message); the decisions taken there, and the facts checked against
live servers the same day, are recorded under Clarifications and
Assumptions. Challenged on 2026-10-02 (the requirements, then the plan's
mechanisms, in fresh sessions), analysed for consistency and reviewed
once more from outside on 2026-10-03; the decisions are under
Clarifications. FR-002 and FR-004 amended on 2026-10-03 at the review of
the window: the row's star stands under the date and stars or unstars
its message (Clarifications, the window's review).
**Input**: Marking a message read or unread and starring or unstarring it
are the first changes the user makes that last: they survive a refresh and
a restart, reach the server, and show up in every other client of the
account. A message the user keeps open for a second counts as read for
good. The window shows the user's change at once and keeps showing it
until the server has it; the next refresh carries it there; what the
server reports never hides a change the user made and the server does not
have yet. The same change means the
same thing on Generic IMAP, Gmail and Microsoft 365.

**Scope**: This is the complete read-and-star specification for Mailbag,
written for the target application: many accounts, many folders each, a
message that several Gmail labels list, mail from any server the
application supports. It owns the four changes (read, unread, star,
unstar), their meaning on each provider, the actions in the window that
make them, read on opening, how a change is kept until the server has it,
how and when it is sent, and what happens when the server refuses it or
the connection breaks around it.

It does not own which messages a folder holds and how a cycle learns the
server's state ([009](../009-synchronization/spec.md)); the list, the
row and the read-on-opening second ([010](../010-message-list/spec.md),
whose FR-009 this feature makes durable); moving and deleting, which use
the same keeping and sending rules for another kind of change; how many
messages an action takes at once (one, the open message, until a
selection exists); or when cycles run without the user (background
synchronization). What waits for a layer that does not exist yet is
marked deferred in FR-013 and gets no plan decisions, tasks or code
until that layer exists.

A *pending change* is the value the user wants for one flag of one
message (read or unread; starred or not) that the server does not have
yet. The *server state* is the flag as the server last reported it. The
*effective state* is what the window shows: the server state with the
pending change, if any, applied over it. A *cycle* and a *batch* are
009's.

## User Scenarios & Testing

### User Story 1 — A change lasts and reaches the server (Priority: P1)

The user stars a message, or marks it unread from the message menu. The
star lights and the dot returns at once, and they stay that way: after a
refresh, after a restart, after the folder is selected again. The next
refresh carries the change to the server, and the user's phone shows the
same star. A change the user makes while the network is down is kept and
goes out with the next refresh that reaches the server.

**Why this priority**: the feature is the first that changes mail; a
change that a refresh undoes would be worse than none.

**Independent Test**: a scripted server records the commands it
receives; the window's rows, the stored state and the server's record
are compared after each action, after a refresh and after a restart.

**Acceptance Scenarios**:

1. **Given** an unstarred message is open, **When** the user presses the
   star, **Then** the star lights and the row's star mark appears as soon
   as the change is stored, and at the next refresh the scripted server
   receives one command setting the flag on exactly that message.
2. **Given** the user starred a message, **When** the folder is refreshed
   or selected again, or the application is restarted, **Then** the
   message is still starred in the row and the reader.
3. **Given** a read message is open, **When** the user chooses Mark as
   Unread, **Then** the dot returns, the message stays open, the server
   receives the change, and the message is listed as unread by the next
   cycle's reading.
4. **Given** the network is down, **When** the user stars a message and
   refreshes, **Then** the star lights, the refresh fails as a refresh
   does today, and the next refresh that reaches the server sends the
   change.
5. **Given** the user starred a message and the server has it, **When**
   another client unstars it, **Then** the next cycle shows it unstarred:
   the server's state is the truth once it has the user's change.

---

### User Story 2 — Reading marks read, for good (Priority: P1)

The user opens a message and reads it. After about a second its dot goes
out, as the list already does, and now the change is stored and sent with
the next refresh: the message is read after a refresh, after a restart
and on the phone. Reading starts no refresh of its own: the window stays
quiet while the user reads.
A message the user glances at and leaves within the second stays unread.
Mark as Unread on the open message makes it unread again and keeps it
open; it is not marked read again until it is opened anew.

**Why this priority**: reading is the most frequent action; it is where
the window's state and the store's parted in 010.

**Independent Test**: scripted folders with unread messages; the stored
state and the scripted server's commands after opening, after waiting,
after Mark as Unread, after a restart.

**Acceptance Scenarios**:

1. **Given** an unread message, **When** the user opens it and keeps it
   open for a second, **Then** the dot goes out, the stored message is
   read, no load starts, and the next refresh sends the change.
2. **Given** an unread message, **When** the user opens another message
   within the second, **Then** the first is still unread in the store and
   nothing is sent for it.
3. **Given** a message marked read by opening, **When** the application
   restarts before a refresh, **Then** the message is listed as read.
4. **Given** the open message counts as read, **When** the user chooses
   Mark as Unread, **Then** the message is unread and stays open, and it
   is not marked read again while it stays open.
5. **Given** a read message, **When** the user opens it, **Then** nothing
   is stored or sent.

---

### User Story 3 — A change during a long refresh (Priority: P2)

A large folder fills for the first time, batch after batch, and the user
reads the messages already listed and stars some. Every change shows at
once and is sent while the fill goes on, between its batches, on the same
connection; none waits for the fill to end. A change the user makes in
another folder meanwhile is kept and sent by that folder's next refresh.

**Why this priority**: first fills take minutes; without this, the phone
lags behind by the length of the fill.

**Independent Test**: a scripted folder of a few hundred messages fills
in batches; the window stars a listed message during the fill; the
scripted server's record shows the flag command between two batch
reads.

**Acceptance Scenarios**:

1. **Given** a first fill of the shown folder is running, **When** the
   user stars a listed message, **Then** the star shows at once and the
   scripted server receives the command before the next batch is read.
2. **Given** a load of folder A runs, **When** the user marks a message
   read in folder B, **Then** the change shows at once, nothing is sent
   while A loads, and B's next refresh sends it.
3. **Given** a running cycle has read its last batch, **When** the user
   stars a message before the cycle closes, **Then** the cycle sends the
   change before closing.

---

### User Story 4 — The same change on every provider (Priority: P2)

Starring means the same on every account: on Gmail the message gets the
star and appears under Starred; on Microsoft 365 it is flagged for
follow-up; on any other IMAP server it carries the standard flag. A Gmail
message that several labels list is one message: read in Inbox, it is
read under every label and in All Mail, and the change is sent once.

**Why this priority**: constitution VII; the same action must not mean
different things by account.

**Independent Test**: scripted servers of each kind; a starred message is
found under the provider's own notion of starred; a scripted Gmail
message under two labels is changed in one and checked in the other.

**Acceptance Scenarios**:

1. **Given** a Gmail message listed under Inbox and under a user label,
   **When** the user marks it read in Inbox, **Then** it is read in both
   folders at once, the Inbox cycle sends one command, and the label's
   next cycle finds the server already agreeing and sends nothing.
2. **Given** a Microsoft 365 message, **When** the user stars it,
   **Then** the service marks it flagged for follow-up, and unstarring
   clears the flag.
3. **Given** a message in a Gmail Starred folder, **When** the user
   unstars it, **Then** it is unstarred at once and leaves the Starred
   folder when its next cycle proves it gone (009 FR-004).

---

### User Story 5 — The server refuses, the connection breaks (Priority: P3)

The server refuses a change: the change is undone in the window, the
refresh fails with the server's words, as a failed refresh does. The
connection breaks right after a change was sent: the user sees no
difference; the next cycle finds the server either already has the
change, and keeps it, or lacks it, and sends it once more.

**Why this priority**: rare, but the store must never hold a claim the
server rejected (constitution III).

**Independent Test**: a scripted server that refuses the flag command,
and one that closes the connection after accepting it; the window's row,
the stored state, the banner and the server's record are compared.

**Acceptance Scenarios**:

1. **Given** the scripted server refuses the flag command, **When** the
   cycle sends a star, **Then** the row shows the message unstarred, the
   refresh fails with a notice that carries the server's reply, and the
   next refresh sends nothing for that message.
2. **Given** the scripted server closes the connection after accepting
   the command, **When** the next cycle lists the folder, **Then** it
   finds the flag set, ends the pending change without sending, and the
   server's record holds one command for it.
3. **Given** the scripted server closes the connection before the command
   arrives, **When** the next cycle lists the folder, **Then** it finds
   the flag unset and sends the change once.

---

### Edge Cases

- The user stars and unstars before any cycle runs: the pending change is
  the newest wish; the next cycle finds it equal to the server state and
  ends it without a command (FR-007).
- The user changes the flag again while a command for it is in flight:
  the newer wish stays, since an accepted or refused command ends only a
  pending change equal to its own value; the next sending step sends the
  newer wish (FR-007, FR-010).
- Another client changes the flag the other way before the user's change
  is sent: the user's change is sent and wins; afterwards the server's
  state is the truth (FR-007, FR-009).
- The message leaves the server before its change is sent: on Generic
  IMAP the listing proves it gone and the message goes with its pending
  change; on Gmail it stays under its other labels and that label's cycle
  sends the change; a message left in no folder is deleted with it (009
  FR-004 and its data model).
- Mark as Unread within the second after opening: the second's timer is
  dropped first, nothing is marked read (FR-003); when the timer's write
  has already started, the unread wish is written after it.
- A Microsoft 365 round reports one message in two pages, the first with
  its read state, the second with its star alone: each writes only what
  it reports; the read state stays (FR-001).
- The unread filter is on: the open message stays listed as 010 FR-008
  says, whether its read state is the server's or pending.
- The server does not keep the flag permanently: the next listing shows
  it gone, and the message shows unstarred; no requirement handles it
  beyond FR-009 (Assumptions).
- The store is discarded at start before the first release: pending
  changes not yet sent are lost with it (007 FR-012, accepted).
- A command for a message the server no longer has: on IMAP the server
  answers OK and does nothing; the change settles, and the next listing
  removes the message; on Microsoft 365 the refusal is FR-010's: the
  change ends, the refresh fails with the service's words, and the next
  round removes the message.

## Clarifications

### Session 2026-10-02 (sizing)

- Q: Must the user's changes be part of the synchronization cycle, or can
  they be sent on their own with the window updated optimistically? → A:
  Both were weighed. Sending on its own keeps the cycle read-only and is
  smaller when offline changes are given up; it needs a second connection
  per IMAP command (or a sign-in each), loses offline marking, and races
  a running cycle's listing (a message read in Inbox during All Mail's
  first fill flips back to unread until the next refresh). The maintainer
  chose the pending change kept in the store and sent by the cycle
  (FR-001, FR-006, FR-007); no chain of operations with states is built
  (FR-013).
- Q: Does a cycle send before or after its listing? → A: After. The
  listing gives every message's address on the server (its UID in this
  mailbox) and its flags, so Gmail needs no stored UID and no search, and
  a change whose outcome is unknown is settled by the same listing
  (FR-007, FR-009). 009 FR-015(a) said "before learning changes" and is
  amended.
- Q: When does a running cycle send? → A: After storing its listing,
  before each batch of missing messages, and once before closing: one
  step of the cycle's loop, so a change made during a first fill goes
  out within one batch (FR-007).
- Q: What if the server refuses? → A: The pending change is dropped, the
  row shows the server's state, and the cycle fails with the server's
  reason through the failed refresh's channel (FR-010). Keeping the cycle
  going with a new partial-result notice was rejected as a new kind in
  006 for a rare case.
- Q: Is a cycle started for a folder where a change was made while
  another load ran? → A: No. The change is kept and sent by that folder's
  next cycle, which the user starts; nothing is queued, nothing is
  forbidden (FR-006). (At the sizing a change made while no load ran was
  to start a cycle of the shown folder; the challenge changed that,
  below.) Background synchronization
  runs every folder's cycles later. Forbidding actions in other folders
  during a load was floated and not taken: it would add a button state
  and an exception for read on opening.
- Q: Where is the star in the row? → A: In the first line, before the
  date (FR-004); a star under the dot in the status column was shown and
  found odd. Form change approved by the maintainer. (Superseded at the
  window's review, below.)
- Q: Which controls act? → A: The envelope's star toggle, Mark as Unread
  in the message menu, and Mark as Read and Mark as Unread in the reader
  header's menu, all on the open message (FR-002). No keyboard shortcuts
  now (FR-013).
- Q: Does read on opening get its own rule? → A: No. 010 FR-009's second
  stays; when it passes, the pending change is written (FR-003).
- Q: How are messages addressed on the server? → A: By the identity the
  listing gives: a Generic IMAP identity holds the UID and the folder's
  numbering version; a Gmail identity is matched to the UID the listing
  shows next to it; a Microsoft 365 identity is the immutable id the
  service accepts in a request. No UID is stored on a membership (007
  FR-003 amended).

### Session 2026-10-02 (challenge of the spec and the plan)

- Q: Does a change, read on opening included, start a cycle of its
  folder? → A: No (the maintainer's decision, agreeing with the review).
  A cycle per read would show the sidebar's spinner and disable Refresh
  for seconds after every message read, and offline a failed-refresh
  banner per message. Cycles start as 009 says, today by Refresh
  Mailbox; a change made while a cycle of its folder runs is sent before
  the cycle's next batch or before it closes; otherwise it waits for the
  next refresh (FR-006). Background synchronization later sends without
  the user.
- Q: Does a refusal the server marks temporary (IMAP `UNAVAILABLE`,
  Microsoft 365 throttling) keep the pending change? → A: No (the
  maintainer's decision against the review's suggestion): a server error
  is a server error, and the outcome cannot be guaranteed; every refusal
  ends the change (FR-010).
- Q: FR-001 forbade a cycle to touch a pending change while FR-009 ended
  one the server already had: which holds? → A: One rule: the write of
  a server state ends a pending change equal to it, by the listing's
  write and by an accepted command's write alike; a change the server
  lacks is never touched by a cycle (FR-001, FR-009). Without it a stale
  wish would have re-applied a change another client had undone.
- Q: Does a group of equal changes go in one command on every provider?
  → A: On IMAP only; Microsoft 365 has no multi-message update, each
  message is one request (FR-007).
- Q: How does the window learn a stored change? → A: It reads the stored
  rows again, as after every stored batch; the window never sets a row's
  state by itself (007 FR-001). 010's window-only record of messages
  counted read is retired with FR-003. Reading one row instead of the
  folder was weighed and left optional (plan).
- Q: Test budget. → A: Raised to 800 lines after the plan's challenge
  measured the repository's GUI tests at 36–80 lines each and the
  scripted servers' new commands at about 85.

### Session 2026-10-03 (consistency analysis)

- Q: What does the user see when the store cannot write a change (a full
  disk, a damaged store)? → A: Nothing was specified. Proposed: a toast
  "Message not changed", the channel 006 FR-006 gives an operation
  outside a load, widened to a change to stored mail that could not be
  written; the row stays as it was and the user may repeat the action.
  Alternatives: the list's banner through the refresh outcomes (a new
  state for a non-load failure, about three times the code); a record
  line alone, which constitution III forbids. The maintainer chose the
  toast on 2026-10-03 (FR-011).
- Q: How does the envelope's toggle show "starred"? → A: Its icon: the
  filled star while the message is starred, the outline otherwise, set by
  the code as the row's texts are; the pressed look alone was judged too
  faint (FR-002).
- Q: Which earlier passages did the amendments miss? → A: 009 FR-001 and
  FR-005 name only the read state; 007 FR-002 lists what is stored per
  message; 009's cycle flowchart sends before the listing; 009's contract
  says a cycle writes only through the batch write; 009's data model says
  read and star adds UIDs; 002's contract names EXAMINE in three more
  places. All added to the Amendments and to T002.

### Session 2026-10-03 (external review)

- Q: A wish equal to the server state was dropped at once; what if a
  command for the opposite value is in flight? → A: The newer wish was
  lost once the command's acceptance wrote the server value. Now a wish is
  stored as made, an accepted or refused command ends only a pending
  change equal to its own value, and a wish equal to the stored server
  state ends when a cycle next compares them (FR-001, FR-007, FR-010).
- Q: A Microsoft 365 partial entry names one flag; where does the other
  come from? → A: From nowhere: a report writes only what it names. Taking
  it from the cycle's starting snapshot re-applied a stale value when a
  message came in two pages of one round (FR-001).
- Q: Mark as Unread within the second checked the state before dropping
  the timer. → A: The timer is dropped first; and the window's writes run
  one at a time, so a wish made while the timer's write runs lands after
  it (FR-003).
- Q: The next Microsoft 365 round may not report a change yet. → A: A
  pending change the round does not report is sent again; setting a value
  twice is harmless. The promise "never sent again blindly" holds for
  IMAP, where the listing is the evidence (FR-009).
- Q: Is a 5xx answer a refusal? → A: No: the service could not complete
  the request, the outcome is unknown, the pending change stays (FR-009;
  the maintainer's decision, distinct from the temporary refusals he
  chose to drop, which are 4xx answers that did not apply the change).
- Q: One command for every pending message? → A: A hundred messages per
  command (FR-007); a few thousand six-digit UIDs would exceed a server's
  command line.
- Q: Test budget. → A: Raised to 850 for the two race tests.

### Session 2026-10-03 (the window's review)

- Q: Where is the row's star, and can the row change it? → A: Under the
  date, at the end of the second line, in a place every row keeps so that
  nothing moves when it shows. While the pointer is over the row, an
  outline star shows there; a click on it stars the row's message, a
  click on the filled star unstars it, and neither click opens the
  message (FR-002, FR-004). Without a pointer, as on a touch screen, only
  the filled star shows, so the row can unstar but not star; the
  envelope's star stays the full path. The maintainer's choice after
  seeing the first line's star in the live window, with the place kept
  rather than the star sliding in.

## Requirements

### Functional Requirements

**The change**

- **FR-001 — A pending change, kept apart**: Each change the user makes
  (read, unread, star, unstar) MUST be stored as the wanted value of that
  flag on that message, apart from the server state, before the window
  shows it; the window then shows the effective state: the server state
  with the pending change applied over it. A newer wish for the same flag
  replaces the older, whatever the server state at that moment. A pending
  change survives a refresh, a reselection and a restart, and ends only
  when the server has it (FR-007, FR-009), when the server refuses it
  (FR-010), or when the message leaves the store. A cycle writes the
  server state it was told, a report that names one flag writing that
  flag alone: a pending change equal to the value written ends with that
  write, and one the server does not have yet is never changed or dropped
  by a cycle (009 FR-002, amended).
- **FR-002 — Actions in the window**: The open message MUST be starred
  and unstarred by the star toggle in the reader's envelope, which shows
  the effective state with the filled star icon while the message is
  starred, and marked unread by Mark as Unread in the message menu; the reader header's menu offers Mark as Read and Mark as Unread
  for the open message. A row's star (FR-004) stars and unstars that
  row's message, open or not, without opening it. Each action stores its change; the row and the
  reader then show it from the store (FR-001). Mark as Unread leaves the message open
  and unread; it is not counted read again until it is opened anew. A
  message that is already in the wanted state is left as it is. The
  actions are available in every folder, the Starred, Important and All
  Mail views included (008 FR-013(d)).
- **FR-003 — Read on opening is durable**: When the second of 010 FR-009
  passes for an unread message, the window MUST store a pending change
  to read for it, with FR-001's effect; the dot goes out once it is
  stored. Opening another message within the second, or Mark as Unread
  within it, drops the timer and stores nothing. Opening a read message
  stores nothing. The window's own record of messages counted read (010
  FR-009) is retired: the row's read state is the stored effective state.
  The window writes its changes one at a time, in the order of the user's
  actions, so a wish made while an earlier write is still running lands
  after it.
- **FR-004 — The star in the row**: A row MUST show a filled star at the
  end of its second line, under the date, while the message's effective
  state is starred, changing in place; every row keeps that place, so the
  subject never moves when a star shows. While the pointer is over a row
  whose message is not starred, an outline star shows in that place. A
  click on the star, outline or filled, asks for the opposite state of
  that row's message (FR-002) and does not open the message. The row's
  accessible description says "Starred" with its read state. (Amends 010
  FR-002 and FR-011(b); the form change was approved on 2026-10-02 and
  amended on 2026-10-03.)

**Sending**

- **FR-005 — What each change means on the server**: On Generic IMAP and
  Gmail, read is the standard flag `\Seen` and starred is `\Flagged`, set
  and cleared in a mailbox opened for writing; on Gmail these flags
  belong to the message under every label, and `\Flagged` is the Starred
  label (Google's documentation; checked on 2026-10-02). On Microsoft
  365, read is the message's read mark and starred is its follow-up flag
  set to flagged; cleared is not flagged; a follow-up marked complete
  reads as not starred. No other flag or label is set or cleared.
- **FR-006 — When changes are sent**: Pending changes are sent only by a
  cycle of a folder that lists the message (009 FR-002), and cycles start
  as 009 says: today by Refresh Mailbox, later by background
  synchronization. No change starts a cycle of its own. A change made
  while a cycle of its folder runs is sent before the cycle's next batch
  or before it closes (FR-007); any other change waits for the folder's
  next cycle. A failed refresh is not retried on its own for a pending
  change (006 keeps retries the user's).
- **FR-007 — How a cycle sends**: After storing its listing (on Microsoft
  365, after its round of changes), before each batch of missing
  messages, and once before closing, a cycle MUST send the folder's
  pending changes that differ from the stored server state; one equal to
  it ends without a command. Each message is addressed as the listing
  identifies it: on IMAP by the UID the listing shows for the message's
  identity in this mailbox, under this opening's numbering version; on
  Microsoft 365 by the message's identity. On IMAP equal changes to
  several messages go in one command, a hundred messages per command at
  most (servers bound a command line); on Microsoft 365 each message is
  one request. When the server accepts a command, the server state
  becomes the sent value and a pending change equal to it ends, in one
  transaction, so the window shows no difference; a wish made meanwhile
  for another value stays and goes with the next sending step. On IMAP,
  a pending message the listing
  does not show is left for the cycle of a folder that lists it. A cycle
  otherwise changes nothing on the server (009 FR-001, amended).
- **FR-008 — Mailboxes opened for writing**: An IMAP folder MUST be opened
  for writing (`SELECT`) wherever a cycle may send; a mailbox the server
  opens read-only refuses the command, and FR-010 applies. (Amends 002
  contracts/imap-reading.md, which required `EXAMINE`.)

**Failures**

- **FR-009 — An unknown outcome is settled by reading**: When the
  connection breaks after a command was sent, or the service answers that
  it could not complete the request (a 5xx status), the cycle fails as
  any broken cycle (009 FR-011) and the pending change stays. On IMAP the
  next cycle's listing writes the server state: a pending change the
  server has ends with that write (FR-001); one it lacks is sent
  (FR-007); a change is never sent again without the listing's evidence.
  On Microsoft 365 the service may report a change with a delay, so a
  pending change the next round does not report is sent again; the
  request sets a value, so a repeated request is harmless.
- **FR-010 — A refused change**: When the server refuses a command (an
  IMAP `NO` or `BAD`; a Microsoft 365 4xx other than the rejected token
  009 FR-011 handles, a temporary refusal included; a 5xx is FR-009's
  unknown outcome), the pending changes equal to the value that command
  carried MUST end, a newer wish for another value stays, the rows show
  the effective state, and the cycle fails under
  006 with a failure that names the change and carries the server's
  reply; the other pending changes of the folder wait for the next cycle.
  The failure is the refresh's: its channel, Retry and details are 006's;
  006 needs no amendment for it, this is one more failure declared under
  its FR-001.
- **FR-011 — Store and privacy**: The stored message keeps its server
  read state and star and the wanted value of each, when one is pending,
  written and read together with the message (007 FR-002, FR-011); a
  discarded store drops them (007 FR-012); records follow 003. A change
  the store cannot write (a full disk, a damaged store) changes nothing
  on screen and is a failed user action under 006: shown once, as a
  toast titled "Message not changed" with the advice to try again, no
  button; the record line holds the store's reason (006 FR-006, amended).

**Gmail**

- **FR-012 — One message under many labels**: A Gmail message is one
  stored message however many labels list it (009 FR-006); its pending
  change shows under every label at once and is sent once, by the cycle
  of whichever label lists it first; the cycles of the other labels find
  the server agreeing and send nothing (FR-007).

**Deferred**

- **FR-013 — Deferred, with the layer each waits for**: (a) *Selection and
  batch actions*: changes to several messages at once, and the keyboard
  shortcuts for the actions, with the selection the message list will
  gain. (b) *Moving and deleting*: a change of place, kept and sent under
  FR-001 and FR-006 with its own rules for the destination; label
  membership on Gmail. (c) *Unread counts*: the folder's count following
  the effective state. (d) *Background synchronization*: cycles that send
  pending changes without the user, for every folder; a change in a
  folder that is not loading then no longer waits for the user. (e)
  *Undo*, and any record of changes as a chain of operations with states
  and reconciliation: not built; a pending change is a wanted value per
  flag.

### Key Entities

- **Pending change**: the wanted value of one flag (read, starred) of one
  message that the server does not have yet; at most one per flag, the
  newest wish; ends by sending, by the server's listing agreeing, by a
  refusal or with the message.
- **Server state**: a message's read state and star as the server last
  reported them; written only by cycles and by an accepted command.
- **Effective state**: the server state with the pending changes applied
  over it; what the row and the reader show.
- **Command**: one request to the server carrying one change for one or
  several messages of a folder.

## Success Criteria

### Measurable Outcomes

- **SC-001**: In scripted scenarios on each kind of server, each of the
  four changes made in the window reaches the server as exactly one
  command for exactly that message, at the next refresh; the row and the
  reader show the change before any command is sent and unchanged after
  it; no load starts on the change itself.
- **SC-002**: A change made with the scripted server unreachable shows in
  the row, survives a restart of the application, and reaches the server
  with the first successful cycle afterwards; the server's record holds
  one command for it.
- **SC-003**: During a scripted first fill of 300 messages in batches of
  100, a star set after the first batch is received by the server before
  the second batch is read; a star set after the last batch is received
  before the connection closes.
- **SC-004**: With a scripted server that closes the connection after
  accepting a command, the next cycle ends the pending change without a
  second command; with one that closes before the command, the next cycle
  sends it once; with a scripted service answering 504, the pending
  change stays and the next cycle sends it again. A change made while a
  command for the same flag is in flight survives the command's
  acceptance and is sent by the next sending step.
- **SC-005**: With a scripted server that refuses the command, the row
  shows the server's state within the cycle, a notice carries the
  server's reply, and the next cycle sends nothing for that message.
- **SC-006**: A scripted Gmail message listed under two labels, marked
  read under one, is read under both at once; one command is sent, and
  the other label's cycle sends none.
- **SC-007**: Opening an unread message and waiting a second marks it
  read in the store and, at the next refresh, on the scripted server;
  opening and leaving
  within half a second, or Mark as Unread within the second, leaves it
  unread with no command; after Mark as Unread the message stays open
  and is not marked read again.
- **SC-008**: On the installed build with an account of each provider,
  each of the four changes made in the window is visible in the
  provider's web client after the next refresh, and a change made in the
  web client is shown in the window after the next refresh.

## Assumptions

- Gmail keeps `\Seen` and `\Flagged` on the message under every label,
  and `\Flagged` is the Starred label: checked on 2026-10-02 against a
  live account (a flag set under Inbox was listed under All Mail and the
  Starred mailbox at once). Google's documentation lists Starred among
  the special folders with the `\Flagged` attribute.
- A mailbox opened with `EXAMINE` refuses a flag command on Gmail and on
  one Generic IMAP server probed (`NO` and `BAD` respectively); another
  Generic IMAP server accepted it against the standard. `SELECT` is used
  everywhere (FR-008); the standard's difference between the two does
  not otherwise matter to a cycle.
- Microsoft 365 accepts the change by the immutable identity in about
  0.6–0.8 s and answers with the whole message, about 85 KB; a change of
  the read mark comes back in the next round as a partial entry, a
  change of the follow-up flag as a full entry with every list field,
  which 009's rule treats as fields reported again and re-reads the text
  of a recent message once. Checked on 2026-10-02; the extra read is
  accepted for the third-priority provider.
- One load runs at a time in the application (008 FR-012) and only
  Refresh starts one, so every change waits for the user's refresh
  (FR-006); background synchronization lifts this.
- Servers that announce `\Flagged` among their permanent flags keep the
  star; all probed servers do. A server that keeps it for the session
  only shows the star gone at the next listing, which is the truth.
- The reader header's Mark as Read and Mark as Unread were designed for a
  wider scope (a conversation); until conversations exist they act on
  the open message.
- IMAP servers bound a command line (Dovecot's default is 64 KiB); a
  hundred UIDs per command stays far below any such bound.

## Amendments to earlier specifications

To be applied with this feature, in the owning documents:

- 009 FR-015(a): built; its lifecycle is FR-001, FR-006, FR-007, FR-009
  and FR-010 here, with one change: a cycle sends after its listing,
  before each batch and before closing, not "before learning changes",
  since the listing gives the address and settles unknown outcomes; the
  "How a cycle runs" flowchart moves its sending node after the listing's
  states and loses "deferred". 009 FR-001 ("a cycle reads only: it never
  changes anything on the server"): a cycle sends pending changes under
  FR-007 and otherwise reads; FR-001 ("and their read state") and FR-005
  ("its number and read state"): the star beside the read state. 009
  FR-002(a): stored mail also changes by the user's pending change, kept
  apart from the server state, and the window learns of it from its own
  action. 009 data-model ("not stored: … read and star adds them" for
  UIDs, "pending changes"): UIDs stay unstored, the stored message
  carries its pending wanted values, and the cycle's read, the batch's
  steps and the row read name both flags, the equal-value rule and the
  effective values. 009 contracts/synchronization.md: a cycle writes
  through the batch write, the settle and the drop, and reads the pending
  changes; the flag shapes and `StoreChanged`.
- 007 FR-014(c): built. 007 FR-002 (what is stored per message): the
  star as the server last reported it and, when one is pending, the
  wanted read state and star. 007 FR-003 ("the UID on each membership
  stays deferred to read and star"): not needed; a message is addressed
  on its server by what the cycle's listing shows for its identity. 007
  FR-012 stands: a discarded store drops pending changes until release
  readiness.
- 010 FR-009: the second now stores a pending change and the change is
  sent (FR-003 here), and the window's own record of messages counted
  read is retired; FR-011(b): built; FR-002 ("no star in the row"): the
  star mark of FR-004; Key Entities, Row: the read state and the star are
  the effective state.
- 002 contracts/imap-reading.md ("EXAMINE INBOX; never SELECT"; the
  EXAMINE completion, the isolation step and the ALERT paragraph; the
  read-only session): mailboxes are opened with `SELECT`, and the only
  changes a session makes are the flag commands of FR-007.
- 008 FR-013(d): flag changes in the view folders are built here; the
  rule for moves and deletes there stands.
- 006 FR-006: the toast row also carries a change to stored mail that
  could not be written (FR-011).
