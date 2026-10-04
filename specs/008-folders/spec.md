# Feature Specification: Folders

**Feature**: `008-folders`
**Created**: 2026-09-26
**Status**: Implemented and accepted on the installed build on 2026-09-27
([plan](plan.md), Post-implementation). Approved on 2026-09-27. Sized and
challenged on 2026-09-26,
clarified on 2026-09-27; the decisions are recorded under Clarifications.
Amended on 2026-09-27 before the window was built: an empty folder list no
longer hides the account (FR-001), and folder lists that cannot be read
share the stored mail's Retry (FR-008). Amended at the simplification
review on 2026-09-27: the server's attributes, Gmail's labels and IMAP UIDs
are not stored until a feature reads them (FR-002 to FR-004, FR-007), and
the interface calls the folder list the mailbox list (FR-011). Amended at
the acceptance on the installed build, 2026-09-27: a mailbox marked as not
existing is a container (FR-006), rows have no tooltip (FR-006), a heading
and a container do not react to the pointer (FR-009), the expander only
expands (FR-010), and the reserved name INBOX is shown as "Inbox" (FR-005).
Amended at the final checks, 2026-09-27: where nested and system folders
sit is stated (FR-005, FR-009), and showing the folders under a personal
namespace prefix beside the Inbox is deferred (FR-013(i)). Amended after an
external review, 2026-09-28: a Microsoft 365 message may keep its old
folder's relation until that folder's next load (FR-004), and the tree's
keyboard focus and Tab are decided (Assumptions). Amended after a spike on
2026-09-28: the tree is a list box over the platform's tree model, the
arrow keys select, and the space between accounts sits above their rows
(FR-006, FR-010, Assumptions); checked on the installed build the same
day. FR-010 amended on 2026-09-29 by
[Synchronization](../009-synchronization/spec.md): the reader stays open
while its message is listed. FR-004, FR-007, FR-012, FR-013(b), Key
Entities, Assumptions and SC-002 amended the same day by Synchronization:
Refresh Mailbox runs a cycle over the whole folder.
**Input**: Support for several mailboxes per account: their discovery, role
recognition, nesting, storage and display in the sidebar. Deleting mailboxes
is not built. Once mailboxes are shown, the account itself can no longer be
selected. Unread counts and the combined Inbox are separate features.

**Scope**: This is the complete folder specification for Mailbag, written for
the target application with many accounts and many folders each, on any
server that follows the standards the application supports: mailboxes as
IMAP defines them (RFC 3501, RFC 9051), the special-use mailbox attributes
of RFC 6154 and RFC 8457, Gmail's IMAP extensions, and Microsoft Graph's
mail folders. It owns what a folder is to the application (its identity on
its server, its name, its place in the hierarchy, whether it can be opened,
its roles), how a message belongs to folders, how the folder list is
obtained and kept, and how the user moves between folders. It does not own
how deep a folder is fetched: a load keeps delivering the newest 100
messages of one folder and replaces what the store holds for that folder
([007](../007-mail-storage/spec.md) FR-004); fetching a whole folder belongs
to synchronization. Whatever waits for a layer that does not exist yet is
marked deferred in FR-013 and gets no plan decisions, tasks or code until
that layer exists.

The user-visible word for a folder is *mailbox*, IMAP's own term: "Refresh
Mailbox", "Select a mailbox". This document says *folder* for the concept.

## User Scenarios & Testing

### User Story 1 — The account's folders appear and open (Priority: P1)

A new account is shown in the sidebar as an empty row that can be selected.
Selecting it shows exactly what an unloaded folder shows, the "no mail
loaded" page as it is; Refresh Account, the action in the main menu,
obtains the folder list. After Refresh Account the account
becomes a heading over its folders, nothing is selected, and the user
selects a folder: the list says that no mail is loaded for it. Refresh
Mailbox loads the newest messages of that folder, exactly as the Inbox is
loaded today, and they stay stored: after a restart without a network, the
folders and the stored messages of each loaded folder are there.
**Independent Test**: a scripted server of each provider with several
folders; Refresh Account, check the sidebar; select a folder, Refresh
Mailbox, check its rows; restart with the server stopped and compare.

**Acceptance Scenarios**:

1. **Given** an account whose folder list was never obtained, **when** the
   user selects it, **then** the list area shows the "no mail loaded" page
   exactly as for an unloaded folder; Refresh Mailbox is unavailable.
2. **Given** the account is selected, **when** the user chooses Refresh
   Account and the load completes, **then** the account's folders appear
   under the account, the account can no longer be selected, and nothing is
   selected until the user selects a folder.
3. **Given** a folder whose messages were never loaded, **when** the user
   selects it, **then** the list says that no mail is loaded, never that the
   folder is empty (007 FR-006).
4. **Given** a folder is selected, **when** the user chooses Refresh Mailbox
   and the load completes, **then** the folder's newest messages are shown
   and opening one shows its text; every other folder's set of messages is
   untouched, while a message two folders hold shows its latest state in
   both.
5. **Given** folders and messages were loaded, **when** Mailbag restarts with
   the server unreachable, **then** the sidebar shows the same folders and
   each loaded folder shows the same rows, and no load starts.

### User Story 2 — System folders first, with their icons (Priority: P1)

Under each account the folders the server marks as system folders come first
in a fixed order, each with its icon: Inbox, Starred, Important, Junk,
Trash, Archive, Drafts, Sent, All Mail. They keep the names their server
gives them (the reserved name INBOX shown as "Inbox", FR-005). The user's
own folders follow, in the order the system locale's collation gives them,
whatever script their names use, with a plain folder icon. A folder the
server does not mark is a plain folder, even when its name is "Drafts":
roles come from the server only.
**Independent Test**: scripted folder lists with role marks of each provider's
kind, including a server that marks only some system folders and one whose
system folders have non-Latin names; check order, icons and names.

**Acceptance Scenarios**:

1. **Given** a server that marks Sent, Trash, Drafts and Junk, **when** the
   folders are shown, **then** those four follow the Inbox in the fixed
   order with their icons, under their server names, before every other
   folder.
2. **Given** a server that marks only Sent and Trash and also has unmarked
   folders named "Drafts", "Junk" and "Archive", **when** the folders are
   shown, **then** Drafts, Junk and Archive are plain folders in
   alphabetical place with the plain icon.
3. **Given** a Microsoft 365 account whose folders are named in the
   mailbox's language, **when** the folders are shown, **then** the system
   folders are recognized by the service's well-known names and shown under
   their own names with their icons.
4. **Given** a server that marks a mailbox as the collection of all,
   flagged or important messages, **when** the folders are shown, **then**
   it is the All Mail, Starred or Important folder, in its place with its
   icon.

### User Story 3 — Nested folders (Priority: P2)

Folders inside folders are shown as a tree: a parent has an expander, its
children are indented one step per level, without a limit on depth. A
container the server does not allow to be opened can be expanded but not
selected. Collapsing the parent of the shown folder, or its account, clears
the selection and the list, which then asks the user to select a mailbox.
**Independent Test**: scripted folder lists with three levels of nesting and
a container that cannot be opened; check the tree, selection and the list
after collapsing.

**Acceptance Scenarios**:

1. **Given** a folder with children, **when** the user expands it, **then**
   its children appear under it; selecting the parent itself shows the
   parent's own messages.
2. **Given** a container that cannot be opened, **when** the user activates
   it, **then** nothing is selected and no load starts; expanding it shows
   its children.
3. **Given** the shown folder's parent or account is collapsed, **when** the
   user looks at the window, **then** nothing is selected and the list asks
   the user to select a mailbox.
4. **Given** three levels of nesting, **when** the user expands them, **then**
   each level is indented one step more than its parent.

### User Story 4 — Gmail labels are folders (Priority: P1)

A Gmail account shows its labels as folders, the system labels first with
their roles, without the container the server lists them under. A message
that carries two labels is stored once and shown in each label's folder that
has been loaded; a folder shows exactly what its own load listed and never a
message that another label's load happened to bring.
**Independent Test**: a scripted Gmail server whose two labels share a
message; load both labels, check both folders and the stored message;
load one label only and check that the other folder shows nothing.

**Acceptance Scenarios**:

1. **Given** a Gmail account, **when** its folders are shown, **then** the
   system labels appear under the account with their roles, the container
   itself is not shown, and the user's labels follow as a tree.
2. **Given** a message carries labels A and B and both were loaded, **when**
   the user opens folder A and folder B, **then** the message is in each,
   once, with the same text, and it is stored once.
3. **Given** a message carries labels A and B and only A was loaded, **when**
   the user selects B, **then** B says that no mail is loaded.

### User Story 5 — The folder list follows the server (Priority: P2)

The user creates, renames or deletes folders in another client or in the
web interface. After the next Refresh Account, the sidebar shows the new
list: a new folder appears; a deleted one is gone together with the
messages Mailbag had stored for it alone. On the server a rename loses
nothing; in Mailbag's copy it depends on the provider: Generic IMAP and
Gmail name a mailbox by its name alone, so the renamed folder appears as a
new folder whose mail is not loaded until the next Refresh Mailbox, while
Microsoft 365 keeps the folder's identifier, so the stored mail stays under
the new name at once. If the shown folder is gone, nothing is selected.
**Independent Test**: two scripted folder lists of one account that differ by
an added, a removed and a renamed folder; check the sidebar, the stored
messages of each folder and the selection after the second Refresh Account.

**Acceptance Scenarios**:

1. **Given** a folder was deleted on the server, **when** Refresh Account
   completes, **then** the folder is gone from the sidebar with the messages
   stored for it alone; after a restart it is still gone.
2. **Given** the shown folder was deleted on the server, **when** Refresh
   Account completes, **then** nothing is selected and the list asks the user
   to select a mailbox; when instead the user chooses Refresh Mailbox on the
   deleted folder, the load fails with the server's refusal and the folder
   stays listed with its rows until Refresh Account.
3. **Given** a folder was renamed on the server, **when** Refresh Account
   completes, **then** on Generic IMAP and Gmail the old name is gone from
   the sidebar and the new name is a folder whose mail is not loaded yet;
   Refresh Mailbox on it loads the mail the server still holds. On
   Microsoft 365 the folder keeps its stored messages under the new name
   without a further load.
4. **Given** a folder's role marks changed on the server, **when** Refresh
   Account completes, **then** its icon and place follow the new roles.

### User Story 6 — The account is a heading (Priority: P2)

Once its folders are known, the account row is a heading over them: it
cannot be selected and shows nothing of its own in the list. It keeps what
it shows today: the provider's icon, the account's name and the button that
explains a problem with the account. It can be collapsed to hide its folders.
**Independent Test**: activate the account row with and without a folder
selected; open its problem button; collapse and expand it.

**Acceptance Scenarios**:

1. **Given** a folder is selected, **when** the user activates its account
   row, **then** the selection does not change and no load starts.
2. **Given** an account has a problem, **when** the user opens its button,
   **then** the explanation and its actions are those of today.
3. **Given** the user collapses an account, **when** the user looks at the
   sidebar, **then** its folders are hidden; if the shown folder was among
   them, nothing is selected.

### User Story 7 — A load that fails (Priority: P2)

Refresh Account can fail: the server does not give the folder list, gives
only part of it, or a later page or child listing fails. Refresh Mailbox can
fail before the messages arrive: the server refuses to open the folder.
Such a load changes nothing: neither the stored folder list nor any stored
messages. It is shown as any failed load is (006 FR-006): the failure page
in the list's place when nothing is shown, the banner over the shown
folder's rows otherwise, with Details, and Retry repeats the same action.
The wording names the mailbox and the step, never the Inbox.
**Independent Test**: scripted servers that refuse the folder list, cut it
short, or refuse to open the selected folder; check the sidebar, the stored
rows and the failure shown after each.

**Acceptance Scenarios**:

1. **Given** folders are stored and a folder is shown, **when** the server
   refuses the folder list or ends it before completing it, **then** the
   sidebar and the stored rows are unchanged and the banner over the rows
   names the failed folder list; Retry repeats Refresh Account.
2. **Given** an empty account is selected, **when** its folder list fails,
   **then** the failure page takes the list's place with Details and Retry.
3. **Given** a folder is shown, **when** the server refuses to open it,
   **then** nothing stored changes, the banner names the failure, and Retry
   repeats Refresh Mailbox.
4. **Given** a Microsoft 365 folder list needs several pages, **when** any
   page fails, **then** the whole Refresh Account fails and nothing stored
   changes.

### Edge Cases

| Situation | Required visible result | Basis |
|---|---|---|
| A server marks only some of its system folders (RFC 6154 makes every attribute optional) | The marked ones are system folders; the others are plain folders under their names | FR-003 |
| A server sends folder names in modified UTF-7 instead of UTF-8 (RFC 3501 §5.1.3; RFC 6855 is optional) | The sidebar shows the readable names; opening the folder still works; a name that cannot be decoded is shown as sent | FR-005 |
| A server would accept the request for UTF-8 names but does not announce the capability | The request is not sent and names are decoded; a server that announces the capability, answers OK and still sends modified UTF-7 is out of scope | FR-005 |
| A folder carries two role marks | Its role is the first mark the server lists; only the role is stored | FR-003 |
| Two folders carry one role mark | Both keep the role, ordered by name; no folder is hidden | FR-003 |
| A folder's name contains a double quote or a backslash | Listed, opened and shown like any other | FR-002 |
| A folder's parent is not in the server's list (RFC 9051 §6.3.9.7) | The folder sits directly under the account | FR-006 |
| The server keeps the user's folders under the Inbox, its personal namespace having the prefix `INBOX.` (RFC 2342) | The tree follows the server: the other folders sit under the Inbox, the system folders first among them by their marks; collapsing the Inbox hides them; showing them beside the Inbox is deferred | FR-006, FR-009, FR-013(i) |
| A Microsoft 365 folder list is longer than one page | Every page is read before anything is stored | FR-001 |
| The service has no folder for a well-known name (no Archive was ever created) | No folder gets that role; the load succeeds | FR-003 |
| The folder list arrived but the folder does not open | A failed Refresh Mailbox; nothing stored changes, the folder stays listed | FR-011 |
| The server refuses or cuts short the folder list | A failed Refresh Account; nothing stored changes | FR-001, FR-011 |
| The shown folder is gone from a completed folder list | Nothing is selected; the list asks the user to select a mailbox | FR-010 |
| Refresh Account completes with no mailbox at all | Nothing stored and nothing shown changes; the record says that no mailbox was found | FR-001 |
| The stored folder lists cannot be read | The failure page with Details in the list's place, as for stored mail that cannot be read; its Retry reads the folder lists and the shown mailbox again; the sidebar keeps what it showed | FR-008 |
| The server lists only containers that cannot be opened (RFC 9051 §6.3.5 allows it) | The containers are shown; the account stays a selectable row, so Refresh Account stays available | FR-009 |
| A Microsoft 365 listing repeats a folder on two pages or marks one as removed | The folder counts once; a removed entry is left out | FR-001 |

## Clarifications

### Session 2026-09-26 (sizing and pre-specification challenge)

- Q: Do folders come before synchronization, which the roadmap had first? →
  A: Yes (order A). Loads keep the newest-100 window per folder;
  synchronization later fetches whole folders and reads real folder rows.
  Its condition: synchronization works on the opened folder; keeping every
  folder fresh on a schedule is background work.
- Q: Does selecting a folder load it? → A: No, as for accounts in 007.
  Loading on selection is revisited with synchronization, which decides how
  folders are kept fresh.
- Q: Where do roles come from? → A: From the server only: the mailbox
  attributes of RFC 6154 and RFC 8457 and the reserved name INBOX on IMAP
  and Gmail; the well-known folder names of Microsoft Graph. No table of
  folder names in any language: a name table returns with moves and
  deletes, where a role chooses a destination (FR-013(d)).
- Q: Two folders with one role, or one folder with two? → A: Both keep the
  role; one folder's role is the first mark the server lists. No winner is
  chosen and no ambiguity is recorded: until moves and deletes, a role only
  chooses an icon and a place.
- Q: Unread counts? → A: The next feature (FR-013(a)); server counts
  obtained with the folder list are the lasting mechanism until every folder
  is kept local.
- Q: The combined Inbox? → A: Its own feature after synchronization
  (FR-013(c)).
- Q: Are readable folder names guaranteed? → A: No: RFC 6855 is optional,
  and a server may answer OK to the request and enable nothing. Names are
  decoded when the server does not announce UTF-8 support; the server's own
  name remains the folder's identity.
- Q: A folder list the server cut short? → A: A failed load. The IMAP library
  in use reports such a list as complete; it is corrected before this
  feature relies on it.
- Applied without a question: the rule to hide an emptied container was
  dropped (no situation under the standards that a plain container rule
  does not cover); a folder whose parent is missing sits at the root instead
  of under the nearest listed ancestor; a Microsoft 365 folder list is read
  with the service's change-tracking listing, which returns the whole tree,
  with page continuation; the memory of what the user expanded across loads
  was dropped (the tree is shown expanded; collapsing is the user's within
  the run); the failure wording that names the Inbox is generalized to the
  mailbox; the account row's problem button and its explanation, built in
  code today, move into a form when the row changes.

### Session 2026-09-27 (clarification)

- Q: Does a message loaded from one Gmail label appear in another label's
  folder that was never loaded, because its labels say so? → A: No. A
  folder shows exactly what its own load listed; labels are stored with the
  message but do not fill a folder that was not loaded, which would look
  loaded while holding a chance subset. Synchronization fills every label
  from All Mail (FR-013(b)). This is how Geary and Thunderbird behave.
- Q: In which order are folders shown? → A: Roles first in one fixed order
  for every server: Inbox, Starred, Important, Junk, Trash, Archive, Drafts,
  Sent, All Mail; then the user's folders as a tree, in the order the
  system locale's collation gives them: the Unicode Collation Algorithm as
  the platform implements it, which orders names of any script consistently
  (Latin, Cyrillic and others together, case and accents secondary), never
  the order of code points. The order serves reading: the folders that wait
  for attention first, storage last. A setting to change the order and to
  hide counts of Junk and Trash is a later candidate, not this feature.
- Q: How are deeply nested folders shown in a sidebar of fixed width? → A:
  With the platform's tree list as it is: no depth limit, one expander width
  of indentation per level, ellipsized names with the full name in the
  tooltip. The standards set no depth limit, so the store and the tree take
  any depth; a capped indentation or horizontal scrolling stays a later
  option (FR-013(g)). *Changed at the acceptance: rows have no tooltip
  (FR-006).*
- Q: Does a selected account without a folder list get a page of its own? →
  A: No, and no text of its own either. It shows the existing "no mail
  loaded" page exactly as an unloaded folder does; only the general rename
  of "Refresh Inbox" (FR-011) touches that page. The state disappears once
  folder lists are obtained without the user (FR-013(e)); nothing is built
  for it.
- Q: What happens when Refresh Account completes and the server lists no
  mailbox at all (IMAP allows it: RFC 9051 §5.1 lets a user have no INBOX,
  and a LIST may answer with nothing)? → A: Nothing is stored; the account is
  hidden from the sidebar for the rest of the run, with the toast 001 uses
  for a hidden account and a record line; after the next start the account
  is shown again with the folder list it had stored before, or as one whose
  folders are not loaded when it had none. The case is too rare to carry
  through every rule of the sidebar. *Changed on 2026-09-27 before the
  window was built, see below.*
- Q: Can a renamed IMAP folder be recognized, so that its stored mail
  stays? → A: Only with a server-given identifier. For Generic IMAP, RFC
  9051 keeps a renamed mailbox's UIDs but neither promises that its
  UIDVALIDITY stays nor makes UIDVALIDITY unique across mailboxes, so a
  client that matched folders by it would guess. For Gmail, Google's own
  documentation is the source of truth: it says labels are renamed with the
  standard RENAME command and refers to RFC 3501 for it, and says nothing
  about UIDVALIDITY or a label identity across a rename; Gmail was observed
  to keep UIDVALIDITY and UIDs, which is not documented and not relied on.
  The standard's answer is RFC 8474: a server advertising OBJECTID gives
  every mailbox a MAILBOXID that survives a rename. Until such a server is
  supported (FR-013(h)), a renamed IMAP or Gmail folder is a new folder and
  Refresh Mailbox loads it again.
- Q: What does Refresh Account refresh? → A: The folder list of one account:
  the selected empty account or the account of the selected folder. It is a
  maintenance action rather than a daily one; refreshing every account is
  background work (FR-013(e)).
- Where a failed Refresh Account is shown follows 006 FR-006 without a new
  decision: the failure page when nothing is shown, the banner over the
  shown folder's rows otherwise.
- Plan challenge (2026-09-27): one latest outcome per account with its
  target, as 007 FR-005 already says, instead of one per target; an
  account's folder subtree is rebuilt after Refresh Account (collapsed
  subfolders reopen, which the run-only expansion rule allows); the folder
  lists are read in one numbered read; sorting happens in the sidebar, not
  in the store. `FolderRole::is_view` stays, as the recorded reminder for
  the actions feature.
- Decided with the clarification: the folder
  list is a separate action, Refresh Account, and the Inbox refresh becomes
  Refresh Mailbox; the application's roles are listed explicitly and every
  attribute the server sent is kept per folder as data, the mapping to
  roles being the application's logic; a message belongs to folders through
  a relation that carries the attributes of that relation (the IMAP UID, the
  position), so a Gmail message with several labels is one stored message
  in several folders and a plain IMAP or Microsoft 365 message has exactly
  one; the container Gmail lists its system labels under is not a folder;
  a folder is shown under the name its server gives it, with no
  substitution for the Inbox; collapsing the parent or the account of the
  shown folder clears the selection and the list; a shown folder that is
  gone clears them too; an account without a folder list is an empty row
  that can be selected; nesting has no depth limit and each level is
  indented one step; the IMAP library's handling of quoted names is
  corrected so that names with a quote or a backslash work; mailboxes marked
  as the collection of all, flagged or important messages are views by the
  standard's own description, recognized as roles now so that the actions
  feature can treat them as such (FR-013(d)).

### Session 2026-09-27 (review after the consistency analysis)

- Folder lists that cannot be read from the store are shown as any
  unreadable stored list (007 FR-013): the failure page with Retry, not a
  silent "not loaded" (FR-008).
- An account whose server lists only containers stays selectable, so the
  list can be refreshed (FR-009); the standard allows such a list.
- What a folder load keeps independent is the folder's set of messages;
  a message's fields are shared and follow the latest load; a relation in
  another folder stays until that folder's load (FR-004).
- Gmail labels are stored one per line, since a label name may contain a
  space ("Muy Importante" in Google's documentation).
- Microsoft 365's listing may repeat a folder or mark one removed even in
  the first round; both are handled (FR-001).
- The special-use return option is used where the server offers it, since
  RFC 6154 only lets a server include the attributes in a plain LIST
  (FR-003).
- `UTF8=ONLY` counts as `UTF8=ACCEPT` (RFC 6855 §6) (FR-005).

### Session 2026-09-27 (before the window was built)

- Q: Does an empty completed folder list still hide the account for the
  run? → A: No. No supported provider lists no mailbox (Gmail and
  Microsoft 365 always have an Inbox), so the hiding, its toast and its
  state have no case to serve; nothing is stored, nothing shown changes and
  the record says so (FR-001).
- Q: Does an unreadable folder list need a Retry of its own? → A: No. It is
  shown as any stored mail that cannot be read, and the one Retry reads the
  folder lists and the shown mailbox again (FR-008).

### Session 2026-09-27 (simplification review)

- Q: Are the server's attributes, Gmail's labels and the IMAP UIDs stored
  now, for the features that will read them? → A: No. Nothing reads them
  yet; a stored UID is not valid without the folder's UIDVALIDITY, which
  synchronization adds; and before the first release any change to the
  store's structure discards the store (007 FR-012), so values stored now
  would be gone before their reader exists, while adding them later needs
  no conversion. The feature that reads them stores them (FR-013(b)). This
  replaces the clarification's "every attribute is kept" and the UID and
  labels kept with the relation and the message.
- Q: Which words name the folder list in the interface? → A: *Mailbox
  list*, since the user-visible word for a folder is mailbox: "Loading
  mailbox list", "Mailbox list not received".

### Session 2026-09-27 (acceptance on the installed build)

- Q: Does a server give the Inbox a readable name? → A: No. LIST names it
  `INBOX`, on Gmail as on a Generic IMAP server (checked live on
  2026-09-27); only Gmail's XLIST, deprecated since 2013, lists it under a
  localized name, and XLIST is not part of the standards the application
  supports. The reserved name INBOX is a word
  of the protocol (RFC 9051 §5.1), not a name anyone chose, so it is shown
  as "Inbox", as other mail clients do; Microsoft 365 names its Inbox
  itself. This replaces the clarification's "no substitution for the
  Inbox" (FR-005).

### Session 2026-09-28 (the sidebar's list, after a spike)

- Q: Which list shows the tree? → A: A `GtkListBox` bound to the tree model,
  its row an `AdwActionRow` with a `GtkTreeExpander` as its first prefix. A
  spike compared it with the `GtkListView` tree built first: in the list box
  the keyboard focus rests on the row, which the platform outlines, the
  action row sits where the platform places it, so it prints no warning,
  and the space between accounts goes above a row rather than into it; the
  list box selects the row the arrow keys reach, which a mail sidebar may
  do, since selecting reads stored rows and never loads (FR-010). The price
  is that every row is built at once, about 0.35 s for 1 000 folders in the
  spike, and that the row passes the expander's keys on.

## Requirements

### Functional Requirements

**Discovery**

- **FR-001 — Refresh Account obtains the folder list**: Refresh Account MUST
  obtain the complete folder list of one account from its server and, when
  it completes, replace the account's stored folder list in one step: a
  folder no longer listed is deleted with the memberships and the messages
  that belonged to it alone, a new one is added without messages, a listed
  one keeps its stored messages and is updated. A folder list the server
  refused, cut short or that could not be completed (a further page failed)
  MUST make the action fail and change nothing stored. A completed folder
  list without any folder stores nothing and changes nothing shown; the
  record says that no mailbox was found. Refresh Mailbox loads
  the selected folder's newest messages as today (002 FR-002, 004 FR-003,
  005 FR-003) and does not touch the folder list. One load runs at a time;
  both actions are unavailable while one runs (007).
- **FR-002 — What a folder is**: A folder belongs to one account and has: the
  identity its provider gives it (a Generic IMAP or Gmail mailbox name as
  the server sends it; a Microsoft 365 folder identifier), by which it is
  opened and matched between loads; its application role (FR-003); its
  name for the user (FR-005); its parent, if
  any (FR-006); whether it can be opened; and whether a load of it completed
  (007 FR-006). A mailbox name with any character IMAP allows, a double
  quote and a backslash included, MUST be listed, opened and shown
  correctly. On the IMAP providers a renamed folder is, to Mailbag, a new
  folder whose mail is not loaded yet, and the old name is gone from the
  sidebar, because IMAP names a mailbox by its name alone; the server keeps
  the mail, and the next Refresh Mailbox loads it. On Microsoft 365 the
  identifier survives a rename or a move, so the folder keeps its stored
  messages at once; an IMAP server that offers RFC 8474 object identifiers
  could do the same (FR-013(h)).
- **FR-003 — Server roles and application roles**: The server's roles are
  what it states: the mailbox attributes of a LIST reply on IMAP and Gmail,
  asked for with the special-use return option where the server offers it
  (RFC 6154 lets a server leave them out of a plain LIST), the well-known
  folder names of Microsoft Graph. The providers read them to give each
  folder its application role and, on IMAP, to tell whether it can be
  opened; they are not stored until a feature reads them (Clarifications,
  simplification review). The application's roles are: Inbox, Starred, Important, Junk,
  Trash, Archive, Drafts, Sent, All Mail. The mapping is the application's:
  on IMAP and Gmail, `\Flagged` → Starred, `\Important` → Important,
  `\Junk` → Junk, `\Trash` → Trash, `\Archive` → Archive, `\Drafts` →
  Drafts, `\Sent` → Sent, `\All` → All Mail, and the reserved name INBOX or
  the attribute `\Inbox` → Inbox; on Microsoft 365, the well-known names
  `inbox`, `junkemail`, `deleteditems`, `archive`, `drafts`, `sentitems` →
  Inbox, Junk, Trash, Archive, Drafts, Sent. A well-known name the service
  has no folder for means that no folder has that role. Attributes the
  application does not map (`\Memos`, `\Scheduled`, `\Snoozed`, structural
  attributes) give no role. A folder's name never gives it a role. A folder
  with several role marks gets the role of the first mark the server lists;
  several folders may hold the same role and all keep it. Starred,
  Important and All Mail are views by the standard's own description
  (RFC 6154, RFC 8457): mailboxes that collect messages from other
  mailboxes; what actions they allow is decided by the actions feature
  (FR-013(d)).
- **FR-004 — Message identity and membership**: A message is stored once per
  account under the identity its provider gives it: Gmail's message
  identifier (004 FR-004), Microsoft 365's immutable identifier (005 FR-004);
  a Generic IMAP message has no identity beyond its place in a folder and is
  the message of that one place. A message belongs to folders through a
  relation that carries the message's position in the folder's list; the
  IMAP UID, valid only with the folder's UIDVALIDITY, joins the relation
  with synchronization (FR-013(b)). A Generic IMAP message has exactly one
  such relation; a Microsoft 365 message is in one folder on the server,
  and the store may keep its old folder's relation until that folder's next
  load (below); a Gmail message has one per label whose load listed it. A folder
  shows exactly the messages its own loads listed, each once; nothing puts a
  message into a folder that was not loaded. A load of a folder replaces
  that folder's relations in one step, keeps a message the load listed
  that another folder already holds, and deletes a message left without any
  relation. A message's fields, read state and content are those of
  the latest load that listed it, whichever folder that was, so a folder
  shows a message's latest state even when its own load is older. A
  relation another folder holds stays until that folder's next load: on
  the providers where a message is in one server folder, a message moved
  on the server may still be listed in its old folder until that folder is
  refreshed; synchronization removes the lag (FR-013(b)). Gmail reports a
  message's labels with it (004 FR-005); synchronization stores them and
  turns them into relations (FR-013(b)). No message is ever
  matched to another by guessing from its date, size or headers.
  *Amended by 009 (FR-005, FR-006, FR-013)*: a Generic IMAP message's
  identity is `imap:<folder>/<UIDVALIDITY>/<UID>`, so a renumbered folder's
  messages are new messages; a folder's relations change by its cycles'
  batches, not by one replacement; a relation carries no position, since
  rows are ordered by received date; a moved Microsoft 365 message stays
  listed in its old folder until that folder's next cycle, since a folder's
  removals are proven by its own reading (009 FR-004); Gmail labels are not
  stored: each label folder is synchronized as a folder, and a message its
  account already holds is related without fetching.
- **FR-005 — Names**: A folder is shown under the name its server gives it,
  except the reserved IMAP name INBOX, in any case, which is shown as
  "Inbox" (Clarifications, acceptance). Under a listed parent a folder is
  shown by the part of its name after the parent's name and the hierarchy
  delimiter; a folder directly under the account keeps its whole name.
  When the server announces neither `UTF8=ACCEPT` nor
  `UTF8=ONLY` (RFC 6855), names are decoded from modified UTF-7 for
  display; a name
  that cannot be decoded is shown as sent. The shown name is never used to
  open or match the folder; its identity is (FR-002). A Gmail system label
  is shown under its own name, without the container's prefix.
- **FR-006 — Nesting**: Folders form a tree as their server describes it (the
  hierarchy delimiter of IMAP, the parent folder of Microsoft 365), without
  a limit on depth anywhere: not in the store, not in the tree. The tree is
  the platform's tree model shown in a list box, each row with the
  platform's expander: each level is indented by one expander width, and a
  name too long for the sidebar is shortened with an ellipsis.
  A folder whose parent is not listed sits directly under the account. A
  folder that cannot be opened is shown as a container: it can be expanded
  and never selected. A mailbox the server marks `\Noselect`, or
  `\NonExistent`, which implies it (RFC 5258 §3), cannot be opened; Gmail
  marks its container either way, depending on how the list was asked for.
  On Gmail, the container the server lists its system labels under is not
  shown; its children are folders of the account.

**Storage**

- **FR-007 — Stored folders and their mail**: The store holds each account's
  folder list as the latest completed Refresh Account left it and, for each
  folder, the messages and relations its latest completed Refresh Mailbox
  left. This replaces 007 FR-003's "built now" part with 007's target model
  except the folder state that synchronization needs (UIDVALIDITY and the
  like, FR-013(b)): the folder with its provider identity, the message with
  its provider identity, and membership as a relation carrying the
  position; the UID, the server's attributes and Gmail's labels come with
  the feature that reads them (FR-013(b)). 007's
  rules on privacy, wholeness, the window's thread, a store that cannot be
  used and an account's departure apply unchanged; an account's departure
  removes its folders with its mail. *Amended by 009 FR-001 and FR-008*:
  a folder's messages and relations are what its cycles stored; the folder
  state is the saved server position, the place an unfinished first fill
  continues from, and whether its latest cycle completed; a relation carries no position.
- **FR-008 — The window reads folders from the store**: The sidebar's folders
  and a selected folder's rows MUST be read from the store, never from a
  load's result directly (007 FR-001): at start, when an account appears,
  and after each completed load of the account. Folder lists that cannot be
  read are a failure of the read, shown as 007 FR-013 shows an unreadable
  stored list: the failure page in the list's place with Details, whose
  Retry reads the stored mail again, the folder lists and the shown mailbox
  together; the sidebar keeps what it showed; the failure is written to the
  record.

**Navigation**

- **FR-009 — The sidebar**: An account without a stored folder list is one
  row that can be selected; selecting it shows the existing "no mail loaded"
  page (007 FR-006) exactly as for an unloaded folder, with no page, text or
  advice of its own: the target application obtains folder lists without
  the user (FR-013(e)), so this state is temporary. An account whose folder
  list holds at least one folder that can be opened is a heading that cannot
  be selected and can be collapsed; an account whose list holds only
  containers stays a selectable row, so Refresh Account stays available; it keeps its
  provider icon, its name and its problem button (001). Under it the
  folders form the tree of FR-006; at every level the folders with a role
  come first in the fixed order Inbox, Starred, Important, Junk, Trash,
  Archive, Drafts, Sent, All Mail, each with its role's icon, then the other
  folders, ordered by the system locale's collation
  (the Unicode Collation Algorithm as the platform implements it, so names
  in any script sort consistently and never by code point), with a plain
  folder icon. A heading and a container do not react to the pointer as a
  row that can be selected does.
- **FR-010 — Selection and refresh**: Selecting a folder shows its stored
  rows and never loads (007). Refresh Mailbox loads the selected folder;
  Refresh Account loads the folder list of the selected account or of the
  selected folder's account. A row's expander only expands or collapses
  it; it never selects the row. The arrow keys, and Tab entering the tree,
  select the folder they reach, as a click does; a heading or a container
  they pass changes nothing. Collapsing a row with the pointer keeps the
  keyboard focus on that row. On a narrow window a click or Enter shows the
  selected folder's list; the arrow keys keep the sidebar. The list's title names the folder and its
  account. Each account keeps the outcome of its latest load, of a mailbox or of
  its folder list: a mailbox's outcome shows over that mailbox only, a
  folder list's outcome shows whenever a mailbox of the account or the
  account is shown, and the account's next load replaces it (007 FR-005,
  FR-007). The
  reader stays open while its message is listed (amended by 009 FR-013,
  which replaces "the reader closes when the shown folder's rows are
  replaced"). Nothing is
  selected, and the list asks the user to select a mailbox, when: the user
  collapses the shown folder's parent or account; a completed folder list no
  longer holds the shown folder; the account's folders appear for the first
  time.
- **FR-011 — Failures name the mailbox**: A failed folder list and a folder
  that could not be opened are failures of the action that met them,
  declared under 006 (FR-001, FR-012) and shown by 006 FR-006 and 007 FR-005:
  the failure page in the list's place when nothing is shown, the banner
  over the shown folder's rows otherwise, Details in the dialog, Retry
  repeating the same action. The wording that today names the Inbox ("Inbox
  not opened", "Inbox changed", "Loading this Inbox stopped", "Refresh
  Inbox") MUST speak of the mailbox and the two actions instead, and
  obtaining the folder list is a step of its own in the wording; the
  interface calls it the mailbox list. The "no
  mail loaded" page's advice, shown for an unloaded folder and for a
  selected account without a folder list alike, reads "Choose Refresh
  Account or Refresh Mailbox in the main menu." (decided at the plan
  challenge, 2026-09-27). The
  mailbox itself is named by the list's title, which stays visible over the
  banner and over the failure page.
- **FR-012 — Bounded work**: Refresh Account is one command on IMAP and, on
  Microsoft 365, the listing's pages plus one request per well-known name;
  Refresh Mailbox is unchanged. *Amended by 009 FR-012*: Refresh Mailbox
  runs one cycle of the folder. No load runs without the user, and one load
  runs at a time (constitution V).

**Deferred**

- **FR-013 — Deferred, with the layer each waits for**:
  (a) *Unread counts*: server counts obtained with the folder list and the
  badge the folder row already has (the next feature).
  (b) *Synchronization*: a whole folder instead of its newest 100, the
  folder state it needs (UIDVALIDITY with each relation's IMAP UID, and the
  server attributes it reads), and for Gmail All Mail plus Trash and Spam as
  the synchronized folders with a message's labels, then stored, becoming
  its relations to
  every label (007 Clarifications); until then a folder holds what its own
  loads listed. *Built by 009*, with each Gmail label folder synchronized
  as a folder instead of All Mail with labels (009 FR-006), and no IMAP UID
  on the relation (007 FR-003 as amended).
  (c) *Combined Inbox*: one list over every account's Inbox-role folder,
  set apart from the accounts by space, as the accounts are (Assumptions).
  (d) *Moves and deletes*: a single destination per role; roles by folder
  name where the server marks none; and the rule for views on IMAP: in a
  Starred, Important or All Mail folder there is no move and no delete,
  because the standard does not define their effect on the message's real
  folder, while flag changes are allowed; a provider that documents the
  effect (Gmail) may allow more in its own terms. *Flag changes in the
  views built 2026-10-03 by [Read and star](../011-read-and-star/spec.md) (011 FR-002); the
  rule for moves and deletes stands.*
  (e) *Background*: folder lists and folders kept fresh without a user
  action, for every account.
  (f) *Folder management*: creating, renaming, moving, deleting and hiding
  folders from Mailbag, which no feature owns yet.
  (g) *Release readiness*: expansion state across restarts; scrolling for
  trees wider than the sidebar; settings for the folder order.
  (h) *Object identifiers (RFC 8474)*: on a server advertising OBJECTID,
  MAILBOXID as the folder's identity, so a renamed folder keeps its stored
  mail, and EMAILID as the message's identity, so one message in several
  folders is stored once, as on Gmail; waits for a supported server that
  offers the extension.
  (i) *The personal namespace*: on a server whose personal namespace has a
  prefix such as `INBOX.` (RFC 2342, NAMESPACE in RFC 9051 §6.3.10), the
  folders under the prefix shown at the account's level beside the Inbox;
  waits for a decision on how such a tree is shown, taken with a server
  that uses one.

### Key Entities

- **Folder**: One mailbox of one account as the server lists it: provider
  identity, name, parent, whether it can be opened, its application role,
  whether a load completed. Owned by its account; gone
  with it. Shown to the user as a mailbox.
- **Server role**: An attribute the server listed for the folder (an IMAP
  mailbox attribute, a Microsoft Graph well-known name), read from the
  listing to give the application role; not stored.
- **Application role**: What the application makes of the server roles:
  Inbox, Starred, Important, Junk, Trash, Archive, Drafts, Sent, All Mail,
  or none. Chooses an icon and a place in this feature; Starred, Important
  and All Mail are views.
- **Label**: On Gmail, a folder. A message's labels are the folders it
  belongs to; the server lists them with the message. *Amended by 009*:
  not stored; each label folder is synchronized as a folder (009 FR-006).
- **Message**: As in 007, stored once per account under its provider
  identity where the provider gives one.
- **Membership**: The relation between a message and a folder. *Amended by
  009*: it carries no position (rows are ordered by received date), and no
  IMAP UID until read and star (007 FR-003 as amended).
- **Account**: As in 001 and 007; in the sidebar an empty selectable row
  until its folders are known, then a heading over them.

## Success Criteria

### Measurable Outcomes

- **SC-001**: After Refresh Account on a scripted account of each provider,
  the sidebar lists every folder the server listed, with the right nesting,
  the system folders first in the fixed order with their icons, under their
  server names (INBOX shown as "Inbox", FR-005); a restart
  with the server stopped shows the same (US1, US2, US3; FR-001, FR-003,
  FR-005, FR-006, FR-009).
- **SC-002**: Selecting each listed folder shows its stored rows or "no mail
  loaded", never another folder's rows; Refresh Mailbox stores its newest
  messages and leaves every other folder's rows unchanged (US1; FR-007,
  FR-008, FR-010). *Amended by 009*: Refresh Mailbox stores the folder's
  messages; other folders' rows change only where they hold a message the
  cycle updated.
- **SC-003**: Folder lists that mark only some system folders, mark two
  folders with one role, mark one folder with two roles, hold names in
  modified UTF-7, hold a name with a quote and a backslash, hold a container
  that cannot be opened, and lack a well-known folder on Microsoft 365 each
  give the results the Edge Cases require (US2, US3; FR-002, FR-003, FR-005,
  FR-006).
- **SC-004**: On a scripted Gmail account a message with two loaded labels
  is stored once and shown in both folders, and a label that was not loaded
  shows nothing (US4; FR-004).
- **SC-005**: A second Refresh Account whose list adds, removes and renames
  folders leaves the sidebar equal to the new list, the removed folder's
  own messages deleted, the renamed folder's messages kept on Microsoft 365
  and not on the IMAP providers, and nothing selected when the shown folder
  was removed (US5; FR-001, FR-002, FR-010).
- **SC-006**: A refused folder list, a cut-short folder list, a failed
  listing page and a folder the server refuses to open each leave the stored
  folders and messages exactly as before and show the failure where 006
  FR-006 says, with wording that speaks of the mailbox (US7; FR-001, FR-011).
- **SC-007**: Activating an account heading changes nothing and starts
  nothing; collapsing the account or the parent of the shown folder clears
  the selection and the list (US3, US6; FR-009, FR-010).
- **SC-008**: On the installed build, Refresh Account on an account of each
  provider shows its folders as this specification requires, with readable
  non-Latin names (FR-003, FR-005).

## Assumptions

- Servers follow RFC 3501/9051 for LIST, mailbox names and UIDs; a server
  that lists a virtual mailbox without a special-use attribute is, to the
  application, an ordinary mailbox.
- Microsoft 365 hidden folders stay hidden; folders whose children the
  service does not list are shown without them.
- A Gmail label's folder is opened like any mailbox and its newest 100
  messages are those of the label; All Mail is a folder like the others in
  this feature. *Amended by 009*: a cycle reads the label's folder whole.
- The store's structure changes with this feature; under 007 FR-012 an
  existing store is discarded at start and refilled by refreshing.
- The approved sidebar form already holds the tree row with its expander,
  icon, title and hidden badge; this feature binds it. The form changes
  are: the main menu's "Refresh Inbox"
  becomes "Refresh Mailbox" and gains "Refresh Account"; the account row's
  problem button and its explanation move from code into a form. Decided
  after a spike on 2026-09-28 (Clarifications): the tree is a `GtkListBox`
  with single selection and `tab-behavior` `item`, bound to the tree model;
  its row is the platform's action row with the expander, which takes no
  focus, as its first prefix, then the icon, which sits 1 px higher than
  the platform places it, closer to the middle of the name. A click on a
  row selects it and the expander arrow only expands (FR-010). Space, not a
  line, sets every account but the first apart from the one above it: the
  platform's spacer at one and a half times its height (18 px), above the
  account's row and outside it. The keyboard focus rests on the row, which
  the platform outlines; the arrow keys move between rows and select, Tab
  leaves the tree after the current row, and the row passes the keys that
  expand and collapse it (`+`, `-`, `*`) to its expander.

## Amendments to earlier specifications

Applied with this feature, in the owning documents:

- 007 FR-002, FR-003 and FR-014(b): "its Inbox" becomes "its folders"; the
  target model of folder, identity and membership is built now (FR-004,
  FR-007); discovery, roles, nesting and navigation are no longer deferred
  there.
- 002 FR-002/FR-003 and FR-012, 004 FR-003 and FR-005, 005 FR-003 and
  FR-006: a load reads the named folder rather than the Inbox; the refresh
  entry point is Refresh Mailbox; labels as membership are built as FR-004
  says, with the label-driven filling left to synchronization.
- 006: the failure wording named in FR-011, and obtaining the folder list
  as a step of a load.
