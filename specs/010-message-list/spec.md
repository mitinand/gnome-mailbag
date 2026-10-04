# Feature Specification: Message list

**Feature**: `010-message-list`
**Created**: 2026-09-30
**Status**: Approved on 2026-09-30 (tasks T001). FR-001, FR-010, SC-009,
SC-010 and the passages restating FR-009 amended on 2026-10-05 at the
consistency analysis of Read and star: a row taken out in the window stays
out while the folder stays shown, since every stored change now reads the
rows again; the read state is stored and sent. FR-002, FR-009, FR-011(b) and the Row entity amended on 2026-10-03 by
[Read and star](../011-read-and-star/spec.md): the row shows a star under the date, the
second's read state is stored and sent, the window's own record of
messages counted read is retired, and a row's read state and star are
the store's effective values. Sized at the feature-start on 2026-09-30 (budget: at most
700 production lines and 750 test lines; one timer for the removal
animation; no thread, queue, new dependency or change to the IMAP library
forks); the decisions taken there are recorded under Clarifications. The
same day the maintainer added read on opening and Move to Trash from the
row as window-only behaviour (FR-009, FR-010): one more timer, and the
budget confirmed as at most 750 production and 850 test lines; the test
budget raised to 920 lines on 2026-10-01, and the size accepted at
about 895 production and 1 025 test lines the same day. Challenged
on 2026-09-30 (the spec, then the plan's mechanisms, in fresh sessions);
the decisions are under Clarifications.
**Input**: The list pane shows a folder's messages so that a glance tells
what each one is about: who wrote it, when, the subject and the first
words of its text, for every message whatever form its content came in,
with a dot marking the unread ones. Reading by removal moves on to the
next unread message. New and removed messages slide into and out of the
list instead of jumping. A filter narrows the list to the unread
messages. An opened message counts as read after a second, and the row's
trash button takes the message out of the list; the read state is stored
and sent since Read and star, and the removal is the window's alone until
moving and deleting.

**Scope**: This is the complete message list specification for Mailbag,
written for the target application: many accounts, many folders each,
folders of a hundred thousand messages, mail from any server the
application supports. It owns how the stored messages of one folder are
shown: the list and its order, the row and every field it shows, the
preview text made for every message, the wording of dates, what the list
does when messages arrive or leave while it is shown, which message opens
after the open one leaves the list, the unread filter, and the place of
the delete control in the row.

It does not own which messages a folder holds or when they arrive and
leave ([009](../009-synchronization/spec.md)), which content is kept
([009](../009-synchronization/spec.md) FR-009, later the content cache),
the reader ([002](../002-imap-integration/spec.md),
[006](../006-error-handling/spec.md)), the folder list
([008](../008-folders/spec.md)), nor any action on a message: marking as read and starring are
[Read and star](../011-read-and-star/spec.md), moving and deleting a later
feature: they make durable and send what the window does here (FR-009,
FR-010) and use the rules set here (FR-006 to FR-008). What waits for a layer that does not exist yet
is marked deferred in FR-011 and gets no plan decisions, tasks or code
until that layer exists.

## User Scenarios & Testing

### User Story 1 — A glance tells what each message is about (Priority: P1)

The user opens a folder. Each row shows the sender, a date that reads
like a calendar, the subject, and the first words of the message's text
in two dimmed lines. The text is the message's real words: a newsletter
that came as a web page shows its greeting and first sentence, not its
styling or hidden markup; a plain reply shows its first line; a message
that came in a foreign character set shows its letters. An unread message
carries a small dot before its first line. Every message in the folder has
a preview, however old it is and whatever form its content came in, with
one exception: a message whose text cannot be read at all (encrypted, an
unknown character set, a structure the server could not describe) shows
no preview, and the reader explains why when it is opened.

**Why this priority**: the list is where the user decides what to read;
without a preview every decision costs an opening.

**Independent Test**: fill a scripted folder with messages in every
supported form and compare each row's preview with the message's first
words.

**Acceptance Scenarios**:

1. **Given** a message whose content is a web page with styles, scripts, a
   hidden opening line and images, **When** the folder is shown, **Then**
   its row's preview is the page's visible words, starting with its
   greeting, without any style, script, address or image text.
2. **Given** a message that came as plain text in quoted-printable
   encoding and a Cyrillic character set, **When** the folder is shown,
   **Then** its preview shows the decoded Cyrillic words.
3. **Given** a message received a year ago in a folder whose text is not
   kept, **When** the folder is shown after a refresh, **Then** the row has
   its preview like every other row, and opening the message says that its
   text was not downloaded.
4. **Given** an encrypted message, **When** the folder is shown, **Then**
   its row has sender, date and subject and an empty preview, and the
   reader explains the encryption when the message is opened.
5. **Given** an unread message and a read one, **When** the folder is
      shown, **Then** the unread one shows the dot before its first line and
   the read one shows none; a screen reader speaks "Unread" or "Read" for
   the row, with "starred" when the message is starred (011 FR-004).

---

### User Story 2 — Reading by removal moves on to the next unread (Priority: P1)

The user reads mail and removes what is done with; each removal opens the
message the user most likely wants next. The list is ordered newest first,
so "below" is older. When the open message leaves the list, the message
that opens next is chosen among its two neighbours: the row above and the
row below. The unread neighbour wins over a read one; when both or neither
is unread, the row below opens; at the top of the list the row below
opens, at the bottom the row above; when the list is left empty, nothing
is open.

The rule is for the user's own removals: today the row's trash button,
later moving and deleting, which make the removal durable. A message a
refresh finds gone from the server is not the user's choice: the reader
then shows that no message is selected, as synchronization already says.
A message the user has kept open for about a second counts as read: its
dot goes out, and it is a read neighbour for the rule. The read state is
stored and sent since Read and star (FR-009); the removal lives in the
window only until moving and deleting: nothing is stored or sent, and
showing the folder anew lists the message again (FR-010). In the
scenarios below the open message leaves by the row's trash button.

**Why this priority**: the order of reading is the maintainer's stated
goal for the list; a wrong choice sends the user back to the list after
every removal.

**Independent Test**: scripted folders where the open message leaves in
each neighbour situation; the message that opens next is compared with
the rule.

**Acceptance Scenarios**:

1. **Given** the open message is the top row, **When** it leaves the list,
   **Then** the row below it opens.
2. **Given** the open message is the bottom row, **When** it leaves,
   **Then** the row above it opens.
3. **Given** the rows above and below the open message are both read,
   **When** it leaves, **Then** the row below opens.
4. **Given** the row above is unread and the row below is read, **When**
   the open message leaves, **Then** the row above opens.
5. **Given** the row above is read and the row below is unread, **When**
   the open message leaves, **Then** the row below opens.
6. **Given** both neighbours are unread, **When** the open message leaves,
   **Then** the row below opens.
7. **Given** the open message is the only row, **When** it leaves, **Then**
   the list is empty and the reader shows that no message is selected.
8. **Given** the message that opens next, **When** it opens this way,
   **Then** its row is highlighted and the reader shows it exactly as if
   the user had opened it; nothing about it changes on the server or in
   the store.
9. **Given** an unread message, **When** the user opens it and keeps it
   open for a second, **Then** its dot goes out; **When** the user opens
   another message within that second, **Then** the first keeps its dot.
10. **Given** a message the user opened, **When** the user presses its
    row's trash button, **Then** its row slides shut and leaves, the
    message the rule names opens, and the scripted server receives no
    command.
11. **Given** messages read by opening and removed by the trash button,
    **When** the folder is refreshed, **Then** the read ones stay read,
    stored and sent (011 FR-003, FR-006), and the removed ones stay out;
    **When** the folder is shown anew after another, **Then** the removed
    ones are listed again as the store holds them, until moving and
    deleting stores and sends the removal (*amended 2026-10-05*).

---

### User Story 3 — The list moves instead of jumping (Priority: P2)

A refresh of a folder the user is looking at brings a new message and
takes a removed one away. The new row slides open in its place and the
rows below it move down; the removed row slides shut and the rows below
move up. The user's place in the list is kept: rows in view stay where
they are, and when the list is at its top, a new message appears at the
top in view. Nothing animates when a folder is shown anew, when the unread
filter changes what is listed, or when the system's animations are off; a
first fill adds its batches below the rows shown, so only rows in view
move.

**Why this priority**: a list that jumps loses the user's place and hides
what changed; second to the content of the rows.

**Independent Test**: a scripted refresh adds and removes rows of a shown
folder; the rows' heights are sampled over time and the scroll position
before and after compared.

**Acceptance Scenarios**:

1. **Given** a refreshed folder shown at its top, **When** a refresh
   stores a new message, **Then** its row grows open at the top over about
   a quarter of a second and stays in view.
2. **Given** a refreshed folder scrolled down, **When** a refresh stores a
   new message above the rows in view, **Then** the rows in view do not
   move.
3. **Given** a refreshed folder, **When** a refresh finds a shown message
   gone, **Then** its row shrinks shut over about a quarter of a second and
   the rows below move up with it; no row is drawn half open afterwards.
4. **Given** a folder shown anew, **When** its stored rows are read,
   **Then** they appear at once, without animation.
5. **Given** animations are turned off in the system settings, **When** a
   message arrives or leaves, **Then** the list changes at once.

---

### User Story 4 — Only the unread messages (Priority: P2)

The user turns on the unread filter in the list's header. The list shows
the folder's unread messages only, in the same order; the open message
stays open and listed while it is being read, even once it counts as read,
until the user opens another message or turns the filter off. The filter
stays on when the user moves to another folder or account, until it is
turned off or the window closes. When no unread message is left, the list
says so.

**Why this priority**: the filter is the other half of reading by removal;
without it a folder of thousands of read messages hides the unread ones.

**Independent Test**: a scripted folder with read and unread messages;
the rows listed with the filter on and off, and after read-state changes,
are compared with the rule.

**Acceptance Scenarios**:

1. **Given** a folder with read and unread messages, **When** the filter is
   turned on, **Then** only the unread rows are listed, in the folder's
   order, and the list's title is unchanged; a read message that is open
   at that moment stays listed and open.
2. **Given** the filter is on and a message is open, **When** it counts
   as read after a second (FR-009) or a refresh reports it read, **Then**
   it stays listed and open; **When** the user then opens another message,
   **Then** the read one leaves the list at once.
3. **Given** the filter is on, **When** a refresh reports another listed
   message read, **Then** its row leaves the list as a removed row does.
4. **Given** the filter is on, **When** a refresh stores a new unread
   message, **Then** its row appears; a new read message does not.
5. **Given** the filter is on and no message of the folder is unread,
   **When** the folder is shown, **Then** the list's place says "No unread
   messages", with "Every message in this folder is read."
6. **Given** the filter is on, **When** the user selects another folder,
   **Then** that folder is shown with the filter still on.
7. **Given** the filter is on and the open message leaves the list,
   **When** the next message is chosen, **Then** the neighbours are taken
   from the filtered list (FR-007).

---

### User Story 5 — Dates read like a calendar (Priority: P3)

A message received today shows its time; yesterday's says "Yesterday";
one from earlier this week shows the weekday; one from earlier this year
shows the day and month; an older one shows the date. The full date and
time stay in the reader.

**Why this priority**: a small reading aid; the row works with any date
form.

**Independent Test**: scripted received dates relative to a fixed "now",
compared with the rule in the user's locale.

**Acceptance Scenarios**:

1. **Given** messages received today, yesterday, four days ago, two months
   ago and two years ago, **When** the folder is shown, **Then** their
   dates read as the time, "Yesterday", the weekday's name, the day with
   the month's name, and the locale's short date.
2. **Given** a message whose server reported no usable date, **When** the
   folder is shown, **Then** its date is empty and the row is placed after
   the dated rows.

---

### Edge Cases

- A message whose only text part holds nothing readable (a web page made
  of images alone, a plain part of empty lines): the preview is empty; the
  row keeps its other fields (FR-003).
- A message whose web-page text starts with a very long invisible head
  (styles and scripts beyond the piece read for the preview): the plain
  part is used when there is one; otherwise the preview is empty
  (Assumptions).
- Two messages with the same received date: their order is the same at
  every showing (FR-001).
- A refresh stores several batches in one run: every arrival and removal
  among the rows shown animates, batch by batch (FR-006).
- A refresh removes the open message: the reader shows that no message is
  selected (009 FR-013); FR-007 is for the user's own removals.
- The folder shown is switched during an animation: the new folder is
  shown at once; the old rows are gone.
- The unread filter is on and the folder was never refreshed: the list
  says no mail is loaded, as without the filter (007 FR-006).
- The trash button takes out the last row shown: the list says "No
  unread messages" with the filter on, else "Mailbox is empty", until the
  next reading lists the rows again (decided at the implementation on
  2026-10-01).
- The trash button is pressed within the second after opening: the
  message leaves unread; the read timer is dropped with it.
- A message removed by the trash button returns when the folder is shown
  anew after another folder, at once as a folder shown anew does
  (FR-006); a refresh or a change of another row, which read the rows
  again since Read and star, keeps it out (FR-010, *amended 2026-10-05*);
  the stored folder is the truth until the removal is durable
  (FR-011(a)).

## Clarifications

### Session 2026-09-30 (feature-start)

- Q: What does the feature own? → A: The list, the row, previews, dates,
  order, animations, the next message after the open one leaves, the
  unread filter, the delete control's place in the row. Not which messages
  a folder holds, which texts are kept, the reader, or any action.
- Q: Does every message get a preview, whatever its age? → A: Yes. A
  preview is made from a small piece of the message's own text when the
  message is stored, so old messages whose text is not kept have one too
  (FR-003). The cost is a read of a piece of every message during a first
  fill; the maintainer accepted it against a list where old rows have no
  third line.
- Q: Which text is the preview made from when a message has both a plain
  and a web-page form? → A: The web page. Plain forms of newsletters are
  often empty or filler; the visible words of the page are what the user
  would see. The plain form is used when the page yields no words.
- Q: Which message opens after the open one leaves the list? → A: The
  maintainer's rule, aimed at the next unread message: the unread
  neighbour when exactly one of the two neighbours is unread; otherwise
  the row below; the only neighbour at the top or bottom; nothing when
  the list is empty (FR-007). Closing the reader instead was rejected: it
  sends the user back to the list after every removal.
- Q: When does the list animate? → A: Arrivals and removals among the
  rows shown, one row at a time; a folder shown anew and a filter change
  happen at once (FR-006). An animation needs one timer in the
  application, accepted at the feature-start. (The condition "only a
  folder whose latest refresh completed", proposed here, was dropped at
  the challenge, below.)
- Q: Where is the delete control in the row? → A: At the end of the row,
  over its content, shown while the pointer is over the row, so it takes
  no width from the preview (FR-002; moved after the date on 2026-10-02). Showing it while the row has the
  keyboard focus waits for the keyboard way to press it (moving and
  deleting, decided at the challenge below).
- Q: How is a message opened? → A: As decided for synchronization: one
  click or Enter opens it, the open message's row is highlighted, the
  arrow keys move the keyboard focus without opening (FR-001). Opening a
  message by moving the selection was considered and not taken: every
  arrow press would open a message and, later, mark it read.
- Q: Does the list act on messages before the actions that store and
  send exist? → A: Yes, in the window only (the maintainer's decision):
  an opened message counts as read after a second, and the row's trash
  button removes the row, without a store or server change; the next
  reading of the stored folder shows the stored state again (FR-009,
  FR-010). Against it: constitution III, since the window then shows a
  state the store does not hold, and a refresh brings read messages back
  unread and removed ones back into the list. Accepted for the
  pre-release build because the list's own rules (FR-006 to FR-008) can
    then be exercised by hand, and nothing is thrown away: read and star
  and moving and deleting add the durable part and the sending (FR-011).
  (Amended 2026-10-05: Read and star stored the read state and made every
  stored change read the rows again, so a removed row returned within a
  second of the next read on opening; the removal now stays until the
  folder is shown anew, FR-010.)

### Session 2026-09-30 (specification challenge)

- Q: Does FR-007 apply when a refresh removes the open message? → A: No.
  The next message would open without the user's choice and, a second
  later, count as read (FR-009), which read and star will one day send to
  the server. A refresh that removes the open message closes the reader,
  as 009 FR-013 says; FR-007 is for the user's own removals.
- Q: Must a first fill be told apart from a refresh so that it does not
  animate? → A: No. The window would need to remember the folder's
  state when the load started, since a refresh's first batch marks the
  folder not completed (009 FR-008), and the store would answer one more
  flag. Without the condition a first fill adds its batches below the rows
  shown and only rows in view move; a folder shown anew and a filter
  change still happen at once (FR-006).
- Q: Are placeholders in square brackets removed from a preview? → A: No.
  A page's image text does not reach the words, and the rule would remove
  the sender's own words such as "[Ticket 4711]" (FR-003(d)).
- Q: Does the row cut the preview at 160 characters? → A: No; its two
  lines cut it, and the stored 400 characters are the only bound
  (FR-002, FR-003).
- Q: Is the trash button shown while the row has the keyboard focus? →
  A: Not yet: it could not be pressed from the keyboard, so showing it
  would promise nothing; both come with moving and deleting (FR-011(a)).
- Q: What does the list do in a narrow window after the trash button? →
  A: The next message opens without moving to the reader's page, so the
  user stays in the list they were working in (FR-010).
- Q: What is the first fill's cost with previews for every message? → A:
  Not yet measured: the documents' figures give about 18 minutes of
  structure requests and up to 1.6 gigabytes of pieces for a folder of
  100 000 messages on the slowest probed server. The time is measured on
  the installed build at the feature's live check and written into the
  Assumptions; the decision stands.
- Q: Does the time in the row follow the user's settings? → A: It
  follows the locale (the maintainer's decision after a fixed 24-hour
  form was proposed): the 12- or 24-hour form, the names and the short
  date come from the locale (FR-004). The desktop's separate Time Format
  setting is not read: a sandboxed application reaches it only through
  the desktop portal, about 55 lines for one key, which the maintainer
  chose not to spend; a user who switched that setting away from the
  locale's form sees the locale's form.

## Requirements

### Functional Requirements

**The list**

- **FR-001 — The list of a folder**: The list pane MUST show the stored
  messages of the selected folder, newest first by received date;
  messages with equal dates keep one order at every showing, and messages
  without a usable date come last. The pane's title is the folder's name
  with the account below it. The list MUST be usable for a folder of
  100 000 messages: rows exist for what is visible, and showing or
  scrolling such a folder never freezes the window (constitution V). One
  click on a row or Enter on the focused row opens its message; the open
  message's row is highlighted; the arrow keys move the keyboard focus
  without opening. A folder shown anew shows its rows at once and opens
  nothing; a click on the row's star stars or unstars the message instead
  of opening it (011 FR-004).
- **FR-002 — The row**: Each row MUST show, in three lines: first, the
  sender's name (the address when the sender gave no name; "Unknown
  sender" when there is none) in the heading style and, at the end of the
  line, the received date worded by FR-004; second, the subject ("No
  subject" when there is none); third, the preview (FR-003) in up to two
  dimmed lines of smaller text, cut with an ellipsis. Before the first
  line stands the unread mark: a small dot in the accent colour, shown
  while the message is unread and hidden once it is read, changing in
  place when the read state changes. The dot is decorative: the row's
  accessible description says "Unread" or "Read" and, since 011, "starred"
  with it (011 FR-004). Long values are cut
  with an ellipsis, never wrapped, except the preview's two lines. At the
  end of the first line, so that it takes no width from the preview, a
  small trash icon with the tooltip "Move to Trash" slides in
  after the date on the first line while the pointer is over the row,
  moving the date aside, and slides away when it leaves; it covers no
  text and turns red while the pointer is over it (changed from a round
  button over both preview lines on 2026-10-02, the maintainer's choice
  after the live check); pressing it is FR-010. Nothing else is shown in the row: no
  attachment, thread or account marker (FR-011). *Amended 2026-10-03 by
  [Read and star](../011-read-and-star/spec.md)*: a star stands at the end of the second
  line, under the date, while the message is starred, and an outline star
  shows there while the pointer is over the row; a click on it stars or
  unstars the message (011 FR-004).
- **FR-003 — A preview for every message**: Every stored message MUST have
  a preview: the first words of its text as the user would read them,
  made once, with the batch that stores the message and off the window's
  thread (constitution V), and kept with it, so the list needs no server
  while it is shown or scrolled. The preview is made from the message's
  own content by these rules. (a) The source is the message's readable
  text part that is not an attachment: its web-page form when it has one,
  else its plain-text form; only a bounded beginning of that part is read
  (Assumptions), except that a recent message's page is read whole in the
  same request as its text. Where the mail service provides a
  text-only preview of the message itself, that text is the source. (b)
  The text is decoded as its character set and transfer encoding declare;
  bytes that cannot be decoded become replacement marks, never a wrong
  letter. (c) A web page yields its visible words only: styles, scripts,
  the page head, markup, image and address text are removed, the words of
  links are kept, and words that adjoining blocks would run together are
  kept apart. (d) The words are normalised: line breaks and every run of
  white space become one space, invisible control and formatting
  characters are removed, letters keep their accents, and the result is
  cut to its first 400 characters. (e) When the web page yields
  no words, the plain form is used the same way; when no source yields
  words, or the message's text cannot be read at all (encrypted, secured
  with S/MIME, an unknown character set or encoding, a structure the
  server could not describe, a text the server did not return), the
  preview is empty and the reader explains on opening (006 User Story 5).
  A preview never holds a header, an address list, an
  attachment's content or markup, and is never made up from the subject
  or the sender: a message without readable text has an empty preview
  (constitution III). When a message's content is stored again (a draft
  edited elsewhere, 009 FR-009), its preview is made again.
- **FR-004 — Date wording**: The row's date MUST read, by the computer's
  clock and the user's locale at the time the row is shown: for a
  message received today, its time without seconds, in the 12- or
  24-hour form of the locale; yesterday, "Yesterday"; two to six days ago,
  the weekday's name; earlier in the current year, the day
  and the month's name in the locale's order; earlier, the locale's short
  date. Names, the time's form and the short date come from the locale.
  A message without a usable received date shows no date. The reader keeps the full date and
  time. The wording follows the day boundary of the local time zone.

**Changes while the list is shown**

- **FR-005 — The user's place is kept**: While a refresh stores its
  batches, the list MUST change only by what each batch changed: arrived
  rows appear in their place, removed rows leave, read state changes in
  place; every other row keeps its place and its state. Rows in view stay
  where they are when rows arrive or leave above them; when the list is at
  its top, arrivals appear at the top in view. The open message stays open
  while it is listed (009 FR-013).
- **FR-006 — Animations**: A row that arrives among the rows shown MUST
  grow open in its place and a row that leaves MUST shrink shut before it
  disappears, each over about a quarter of a second, the rows after it
  moving with it; rows in view keep their place as FR-005 says. No
  animation runs when a folder is shown anew, when the unread filter
  changes what is listed, or when the system's animation setting is off:
  those changes happen at once. A first fill adds its batches below the
  rows shown, so only rows in view move. A row that leaves under an
  animation is gone from the list when the animation ends; opening it
  meanwhile is not possible.
- **FR-007 — The next message after the open one leaves**: When the user
  takes the open message out of the list as shown (today the row's trash
  button, FR-010; later moving and deleting), the list MUST
  open another message at once, chosen among the two neighbours the
  leaving row had at that moment, the row above (newer) and the row below
  (older): (a) only the row below exists, at the top of the list: the row
  below; (b) only the row above exists, at the bottom: the row above; (c)
  both exist and exactly one is unread: the unread one; (d) both exist and
  both are read, or both are unread: the row below; (e) neither exists:
  nothing opens, and the reader shows that no message is selected. The
  chosen row is highlighted and scrolled into view, and the reader shows
  its message as if the user had opened it; the store and the server are
  unchanged. Every feature that removes a message at the user's request
  uses this rule. A message a refresh finds gone from the server closes
  the reader instead (009 FR-013): the window never opens a message the
  user did not choose because of a server change.
- **FR-008 — The unread filter**: A toggle in the list pane's header,
  "Show unread only", MUST narrow the list to the folder's unread
  messages, in FR-001's order, and widen it again when turned off; it is
  off when the window opens and keeps its state across folders and
  accounts until the window closes. With the filter on: a message that
  arrives unread is listed, one that arrives read is not; a listed message
  that a refresh reports read leaves the list as a removed row; the open
  message is listed while it is open whether or not it is unread, so that
  reading never takes the message away from under the reader, and it
  leaves the list, without animation, when the user opens another message
  or leaves the folder; the neighbours of FR-007 are taken from the
  filtered list. When the filter leaves no row,
  the list's place says "No unread messages" with the explanation "Every
  message in this folder is read."; a folder never refreshed says what it
  says without the filter (007 FR-006). Marking as read on opening is
  FR-009, stored and sent since Read and star; the open message is kept
  the same way.

**Acting from the list**

- **FR-009 — Read on opening**: About one second after a message is
  opened, by the user or by FR-007, it MUST count as read: its dot goes
  out, it is a read neighbour for FR-007 and a read message for FR-008.
  Opening another message before the second has passed leaves the first
  unread. Until read and star is built, this is the window's state alone:
  nothing is stored or sent, and the next reading of the stored folder (a
  refresh, selecting the folder again) shows the stored read state again.
  Read and star makes the change durable and sends it (FR-011(b)).
  *Amended 2026-10-03 by [Read and star](../011-read-and-star/spec.md)*: when the second
  passes, the window stores a pending change to read (011 FR-003), the
  dot goes out once the stored rows are read again, and the next refresh
  sends it; the window's own record of messages counted read is retired.
- **FR-010 — Move to Trash from the row**: Pressing the row's trash button
  MUST take the message out of the list at once, animated as a leaving row
  (FR-006), and open the next message when it was the open one (FR-007).
  In a narrow window, where the list and the reader are separate pages,
  the list stays in view: the next message opens without moving to the
  reader's page. Until moving and deleting is built, this is the window's
  state alone:
    the message stays in the store and on the server, nothing is sent, the
  row stays out while the folder stays shown, whatever reads its rows
  again, and showing the folder anew after another lists it again
  (*amended 2026-10-05*: since Read and star every stored change reads
  the rows again, so a removed row otherwise returned within a second of
  the next read on opening). Moving and deleting makes the removal
  durable, sends it and gives the action its keyboard way (FR-011(a)).

**Deferred**

- **FR-011 — Deferred, with the layer each waits for**: (a) *Moving and
  deleting*: the removal FR-010 shows becomes durable and is sent; a
  keyboard way to reach the action, and the trash button shown while the
  row has the keyboard focus; a message the user moves or deletes leaves
  the list under FR-006 and FR-007. (b) *Read and star*: the read
  state FR-009 sets becomes durable and is sent; a star mark in the row.
  *Built by [Read and star](../011-read-and-star/spec.md) (011 FR-003, FR-004).*
  (c) *Conversations*: a row for a conversation with its message count.
  (d) *Attachments*: an attachment mark in the row. (e) *Combined Inbox*
  and *Search*: lists over more than one folder, with the account named
  in the row, under the same row and rules. (f) *Content cache*: keeping
  the source text of previews longer or shorter changes nothing here; a
  preview is kept as long as its message.

### Key Entities

- **Row**: one stored message as the list shows it: sender, date wording,
  subject, preview, read state and star (the store's effective values
  since 011; before, the stored read state or the window's under FR-009),
  and whether it is the open message.
- **Preview**: up to 400 characters of a message's first readable words,
  kept with the message from the moment it is stored; empty when the
  message has no readable text.
- **Unread filter**: whether the list is narrowed to unread messages; one
  state for the window.
- **List position**: the rows in view and the open message, kept across
  changes.

## Success Criteria

### Measurable Outcomes

- **SC-001**: For a scripted set of messages in every supported form
  (plain text; a web page only; both forms; base64 and quoted-printable
  encodings; UTF-8, Cyrillic and Western character sets; attachments before
  the text; a nested message; a web page with styles, scripts, a hidden
  opening line, images and links), every row's preview starts with the
  message's first readable words and holds no markup, style rule, script,
  address or header text; a message of images alone and an encrypted
  message show an empty preview.
- **SC-002**: After a completed refresh of a scripted folder of 10 000
  messages, every row has its preview.
- **SC-003**: In scripted scenarios for each case of FR-007 (top, bottom,
  both neighbours read, one unread above, one unread below, both unread,
  the only row, and each with the unread filter on), the message the rule
  names is open afterwards, highlighted and in view, and the store is
  unchanged.
- **SC-004**: With the unread filter on, a scripted folder of 1 000 read
  and 10 unread messages lists exactly the 10; the open message stays
  listed after a scripted refresh reports it read and leaves when another
  message is opened; a new unread message appears and a new read one does
  not.
- **SC-005**: In the automated checks, a row that arrives in a shown
  folder is hidden when inserted and shown after the next frames, a row
  that leaves is closed and gone from the list after the animation, and a
  folder shown anew changes at once. On the installed build a row grows
  or shrinks over about a quarter of a second, no other row is drawn at a
  partial height, with animations off the change is immediate, and the
  rows in view before an arrival above them are at the same positions
  after it.
- **SC-006**: A folder of 100 000 stored messages, each with a preview,
  is listed and scrolled from top to bottom without the window freezing.
- **SC-007**: For scripted received dates against a fixed clock (today,
  yesterday, 2 to 6 days ago, 7 days ago, earlier this year, last year,
  no date), every row's date reads as FR-004 says in the user's locale,
  with today's time in the locale's 12- or 24-hour form.
- **SC-009**: In a scripted folder, an opened unread message loses its dot
    between 0.8 and 1.5 seconds after opening; a message opened and left
  within half a second keeps it; the read state is stored after the
  second and the next scripted refresh sends it (011 FR-003, FR-006;
  *amended 2026-10-03 by Read and star*; before, nothing was stored or
  sent).
- **SC-010**: In a scripted folder, pressing a row's trash button removes
    the row as SC-005 measures and opens the message FR-007 names; the
  store and the scripted server are unchanged; after a refresh the row
  stays out, and the folder shown anew lists it again (*amended
  2026-10-05*).
- **SC-008**: On the installed build with an account of each provider,
  the largest folder's rows show previews that match the messages'
  opening words as another client shows them, and the first batch of a
  first fill is listed with previews within 5 seconds of the start on the
  scripted server.

## Assumptions

- The beginning of a text part, 64 kilobytes, is enough for the words a row
  shows in practice; a web page whose invisible head is longer yields no
  words from that piece and falls back as FR-003(e) says. (Measured on
  2026-10-01 on 200 messages of a real Inbox: 16 kilobytes, the planning
  value, left 5 previews empty and 63 too short for the row's two lines;
  64 kilobytes left none empty and 6 short, the whole part 5.)
- Reading that piece for every message makes a first fill of a large
  folder take minutes rather than seconds on slow servers; the maintainer
  accepted this for previews on every row. Measured on the installed build
  at the live check (2026-10-02), each Inbox filled from nothing: 763
  Gmail messages in 56 s, 3 486 Microsoft 365 messages in 81 s and
  5 976 Generic IMAP messages in 12 min 27 s, the first batch listed after
  about 10 s on Gmail and that server, which spends about 75 ms opening each
  message whatever is read of it; making a fill faster belongs to
  synchronization (009). The batch
  stores its rows only with their previews, so an interrupted fill leaves
  complete rows. For a message older than 30 days with both forms, the
  plain form's piece is read together with the page's, so the fallback of
  FR-003(e) needs no second request.
- Microsoft 365 provides a text-only preview of each message with its
  list fields; it is used as the source text and normalised like any
  other.
- "Yesterday", "No unread messages", "Unread" and the other words of this
  feature are English until translations are wired (release readiness);
  weekday and month names and the short date come from the locale
  already, and so does the time's 12- or 24-hour form; the desktop's
  separate Time Format setting is not read (Clarifications).
- The system's animation setting is the one GNOME offers in Accessibility;
  the toolkit honours it for the transitions used.
- The window-only removal (FR-010) is a pre-release stage: the build is
  used by the maintainer to exercise the list, and a folder shown anew
  shows the stored truth; the read state is stored since Read and star
  (FR-009).
- Before the first release the store's changed structure discards it at
  start (007 FR-012); this feature adds the preview to the stored message,
  so every folder fills again once.

## Amendments to earlier specifications

To be applied with this feature, in the owning documents:

- 009 FR-003 and FR-009: a batch carries, for every message it stores, the
  preview FR-003 requires, made from the beginning of the message's text
  part (its web-page form first), read with the batch; the 30-day text
  rule is unchanged. FR-013 ("rows are ordered newest first by received
  date until the message list decides the order"): the order is FR-001
  here; the kept position and open message are FR-005. FR-015(d): the
  message list is built.
- 007 FR-014(e): previews leave the content cache's deferred list; the
  stored message gains its preview (FR-003).
- 002 FR-004 ("do not … generate list previews"): previews are generated
  by the message list under FR-003, from the message's own parts; the
  reader is unchanged. 002 contracts/ui.md, the list and reader binding:
  the row shows the preview, the date wording and the trash button as
  FR-002 says; "preview … remain hidden" is replaced.
