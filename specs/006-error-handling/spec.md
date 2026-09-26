# Feature Specification: Error Handling

**Feature**: `006-error-handling`
**Created**: 2026-09-24
**Status**: Implemented on `claude/errors` and accepted live by the
maintainer 2026-09-25 (plan.md, "Post-implementation"); where the wording
is written in code corrected 2026-09-26, and failures handed to the
application as domain values decided the same day and built in portion 6
(research §1). The decisions taken
at sizing, at the specification challenge and on the prototype are
recorded under Clarifications.
**Input**: One lasting model for how Mailbag reports failures, from the
component that meets them to what the user sees. Today each failure is
presented the way its feature happened to choose: a status page for a failed
load, a short notice for an incomplete list, an explanation inside the reader
for one message, an icon at the account row for Online Accounts problems.
Refreshing is the only action offered anywhere, the server's text is the
only detail, and the wording lives in a contract of feature 002. Before mail
is stored locally and refreshed in the background, the model must be decided
once: which properties a failure declares, which channel shows it, what the
user can do, and what a person can copy into an issue report.

**Scope**: This is the complete error-handling specification for Mailbag, not
a first layer. Like the [logging specification](../003-logging/spec.md), it
holds principles, channels, properties and rules, never a list of failures:
which failures exist, their properties and their wording live in code and
are reviewed there (FR-012). Its rules already hold
for the target application, with local storage and background
synchronization; what waits for a layer that does not exist yet is marked
deferred in FR-013 and gets no plan decisions, tasks or code until that layer
exists.

## User Scenarios & Testing

### User Story 1 — A failed load with nothing to show explains itself (Priority: P1)

The user refreshes an account's Inbox and the load fails, for example
because the mail server cannot be reached, it rejects the sign-in, Online
Accounts has no encryption configured for the account, or the mail service
refuses the request; the failures that exist are declared in code (FR-012),
this is not their list. There are no messages to show: today every refresh
starts from an empty list, and with local storage this is the account whose
store is still empty. A status page takes the list's place: what failed,
what happened and what to do next, one action button when something can be
done (Retry when repeating may succeed, Online Accounts when a setting or a
credential is the cause) and a Details button. When nothing the user does
would change the outcome, no action button is offered; Details stays.
**Independent Test**: drive each failure value the providers can report
through the window with the scripted loader and check, by what carries the
failure and its declared action, which channel shows it and which buttons
it offers.

**Acceptance Scenarios**:

1. **Given** the mail server does not answer within the wait limit, **when**
   the load ends, **then** the status page names the step that stopped
   responding and offers Retry; the action says what to do, so there is no
   advice.
2. **Given** the server rejected the sign-in and blamed the credentials,
   **when** the load ends, **then** the status page says the server rejected
   sign-in, advises checking the account's sign-in in Online Accounts and
   refreshing afterwards, and offers Online Accounts; the server's own words
   are in the failure dialog. It does not claim that the password is wrong.
3. **Given** the server offers no sign-in method Mailbag supports, **when**
   the load ends, **then** the status page says so, offers no action button,
   only Details, and no credential was sent.
4. **Given** a failure is shown for one account, **when** the user selects
   another account, **then** that account shows its own state, and the
   failure is there again when the first account is selected again, until its
   next load replaces it.

### User Story 2 — A failed refresh while messages are on screen (Priority: P1, built with local storage)

The account's messages are on screen and a refresh fails. The messages
stay: a banner above them names the failure in a few words, and its button
opens the failure dialog with the explanation, the advice, the action and
the technical text. This is the common case once messages are stored
locally; until local storage exists (007) a refresh starts from an empty
list, so the story is deferred with FR-013 and gets no tasks before then.
**Independent Test**: a stored Inbox and a scripted server that rejects the
sign-in; check the rows, the banner and its dialog, then a successful
refresh.

**Acceptance Scenarios**:

1. **Given** messages are on screen and the server rejects the sign-in,
   **when** the refresh ends, **then** the rows stay, the banner above them
   says the sign-in was rejected, and its dialog offers Online Accounts.
2. **Given** the banner is shown, **when** a later refresh succeeds, **then**
   the banner is gone and the rows are the fresh ones.
3. **Given** the banner is shown, **when** the user selects another account
   and comes back, **then** the banner is there again with the same rows.

### User Story 3 — An incomplete list stays visibly incomplete (Priority: P1)

Some messages could not be loaded: the server refused to finish the list, or
the mail service offered more than one request holds. The rows that arrived
stay in the list, and a banner (a bar with one line of text and at most one
button, above the list) says that messages are missing for as long as those
rows are on screen. Today the notice is a toast (a short pop-up that
disappears by itself), after which the list looks complete. **Independent
Test**: a load whose list the server refused to finish and one whose
service offered a further page, through the scripted loader; check the
banner after the load and its absence after the next complete load.

**Acceptance Scenarios**:

1. **Given** the server refused to finish the message list, **when** the load
   ends, **then** the rows that arrived are shown, the banner above them says
   that some messages are missing, and its button opens the failure dialog,
   whose technical details hold the server's reason.
2. **Given** the banner is shown, **when** the user refreshes and the load
   delivers a complete list, **then** the banner is gone.
3. **Given** the banner is shown, **when** the user selects another account
   and comes back, **then** the banner is there again with the same rows.

### User Story 4 — A person reports a problem with its technical details (Priority: P2)

The user who meets a failure is not a developer. From a status page or a
banner they open the failure dialog, read what happened, what to do and the
technical text Mailbag has about the failure, and copy the whole of it into
an issue report with one button. The technical text is what the maintainer
needs and nothing that should not leave the machine: the failure named as
the record names it, the server's status and code, the server's own words
with the user's sign-in name replaced. **Independent Test**: a rejected
sign-in whose server text repeats the sign-in name; the copied text holds
the marker in its place and none of the fixture's private markers.

**Acceptance Scenarios**:

1. **Given** the status page shows a failed load with a server reason,
   **when** the user opens Details, **then** the failure dialog shows, in
   this order, the explanation in plain words, the advice, the server's
   reply under "Reply from the mail server", the technical details with the
   failure in the record's terms and the server's code, and the action
   button; the blocks are selectable, and the copy button puts the whole
   dialog text on the clipboard in the same order.
2. **Given** the server's text repeats the account's sign-in name, **when**
   the failure is shown, **then** the name reads `<login>` in the dialog and
   on the clipboard, and the status page shows no server text at all.
3. **Given** the list is short because the mail service offered more than
   one request holds, **when** the banner is shown, **then** its button opens
   the dialog with the explanation alone: no action, since nothing the user
   does helps, and no technical lines.

### User Story 5 — One message's problem stays with that message (Priority: P3)

A message's text cannot be shown, for example because it has only HTML,
it is encrypted, its character set is unknown, or the server did not return
its text. Its row
stays in the list, the reader opens with the envelope, and a status page
stands in the body's place: the warning icon, the title, the explanation
and, when something helps, the advice or the action button. No banner and
no dialog: a message's problem has nothing technical to add. 002 wrote the
explanation in place of the text; the status page replaces that, because
the body becomes a web view in the HTML reader (011) and is no place for
Mailbag's own words, and because it is GNOME's form for content that
cannot be shown.
**Independent Test**: the 002 content fixtures through the window; each
explained message keeps its row, and opening it shows the status page under
the envelope.

**Acceptance Scenarios**:

1. **Given** the server did not return one message's text, **when** the user
   opens it, **then** the status page under the envelope says so and offers
   Retry, which refreshes the whole Inbox and closes the reader; until then
   the list and the other messages are untouched.
2. **Given** one message's structure cannot be read, **when** the load ends,
   **then** the load succeeds with that message's row present and no notice
   for the list or the account.

### Edge Cases

| Situation | Required visible result | Basis |
|---|---|---|
| The server sends an ALERT during a failed step | The alert's text is the first block of the failure dialog, under "Alert from the mail server", as inert text; the copied text includes it | RFC 3501 §7.1 asks for the alert to be presented to the user; 002 carried this |
| A failure has no action | The status page shows title, explanation, advice and Details, with no action button; a banner keeps its button, and the dialog behind it has no action button | A server without a supported sign-in method, a message Mailbag cannot show and a mail service's "more available" page have nothing to do |
| Two failures of the same account in a row | The newer replaces the older; no history | One writer per channel (002); a history is out of scope |

## Clarifications

### Session 2026-09-24 (specification challenge)

- Q: Where is the sign-in name replaced with `<login>`? → A: Once, where the
  failure is built, for every channel. The name is known only where the
  server is spoken to; a second, unmasked text for the page would need a new
  field in a provider error type or the name in the window. 003 FR-011 is
  amended (Assumptions).
- Q: Is scope a declared property? → A: No. What carries a failure already
  fixes its scope and its channel: a failed load belongs to its account, a
  short list to the list, a message's content to the message. FR-002 stays
  as the rule that scope never widens or narrows.
- Q: Does a toast carry advice? → A: Yes, title and advice in one line, as
  today's notice about Settings that did not open. No button, no details.
- Q: Does this spec forbid tests that check wording? → A: No. A test that
  each failed step names itself checks behavior the user relies on; test
  style is a matter of code review, not of this spec.
- Applied without a question: a message's failure has no title; a failed
  load always has Details; the account problems of 001 are the named
  exception; SC-001 speaks of failure values, not of scripted servers; US1
  scenario 1 follows FR-004; cancellation is stated once (FR-010, SC-004).

### Session 2026-09-24 (failure-gallery prototype review)

The maintainer reviewed a throwaway GTK prototype of every channel, built
from the approved forms, and decided:

- The failure dialog carries everything behind a channel's one button:
  explanation and advice as paragraphs, the technical text under its own
  heading, the action as a button, a copy button in the header bar, no
  Close button (the header bar closes). It follows Workbench's Dialog demo,
  not the alert dialog, whose centered text
  wraps badly for two paragraphs.
- The dialog's order is a rule: the explanation in plain words for a
  novice, the advice, then what the remote side said in words, each text
  as its own block with its source named, then the technical identifiers,
  then the action button. Nothing appears twice. The mail service's error
  message and the system's text (GIO) are remote texts like an IMAP reply
  or alert and get the same block; the alert comes first.
- The copy button copies the whole dialog text, not only the technical part.
- Titles are short enough for a banner's one line at the list pane's
  narrowest width ("Sign-in rejected", "Server unreachable"); with stored
  mail on screen (007) they will be what the banner shows, so a generic
  "could not refresh" line is not used.
- Every banner has its button, so banners align the same way; a banner
  without a button centers its title and looked like an exception.
- A message's content problem is shown by a status page in the body's
  place, under the envelope, with the warning icon: GNOME's form for
  content that cannot be shown. The prototype offered a banner over an
  empty body as well; the maintainer chose the status page, since nothing
  technical is behind such a failure and the body becomes a web view in
  011. The reader's banner stays for 011's remote images. A message's
  failure therefore has a title again.
- A status page that shows a failure carries the warning icon instead of
  the mail icon, on the list and in the reader alike. On the list it is a
  page of its own beside the account page, declared in the form with the
  warning icon, its action button and its Details button, so no icon or
  button is switched between an account page and a failure (decided on
  2026-09-25, as the reader's page already was); its explanation and
  advice go into its description, escaped, so that the icon-title-text
  spacing equals the reader's empty page. This amends the 002 rule that put
  the explanation into a separate plain-text label.
- The failure dialog's spacing, decided on the prototype: explanation to
  advice 12, advice to the first block 24, block to block 24, last block
  to the action button 24; in a block, heading to card 6 and the card's
  inner margin 12; the column 24 from the dialog's sides. All on
  libadwaita's own scale (6, 12, 24), read from its stylesheet. A
  preferences page was tried for the spacing and rejected: it is a
  settings widget.
- Every widget that a form can declare lives in a form (AGENTS.md, UI
  layout). The prototype's forms are the drafts of this feature's layout
  changes, listed under Assumptions.
- Q: What does the user see for a failure nobody described, such as a
  service status 500? → A: The general declaration of its step: "Request
  failed", a plain explanation, Retry, the status and code in the technical
  lines, the service's message in its block. Every open-ended value has
  such a general arm; every enumerated failure is matched exhaustively, so
  a new variant without a declaration does not compile. A panic on the
  worker is the same kind of case (FR-014); a panic on the main thread
  cannot be shown.
- Defects: a panic on a worker becomes a failure with the panic's message
  and place in Details (FR-014); a panic on the main thread ends the
  application and gets no mechanism now.
- UI decisions follow a Workbench Library demo when one fits, a standing
  project rule the maintainer named during this review.
- Wording is impersonal, as GNOME's own applications write ("Could not …",
  "Unable to …"); the third person ("Mailbag could not …") came from 002
  and is dropped.

## Requirements

### Functional Requirements

**What a failure declares**

- **FR-001 — Properties, declared once**: Every failure that reaches the
  user MUST be declared once, with: a *title* that names what failed, short
  enough for a banner's one line at the list pane's narrowest width; an
  *explanation* of what happened; at most one *action*; *advice* or none;
  *remote text* or none: what the server, the service or the system said in
  words, each text with its source; *details* or none. A channel shows these
  properties and nothing else about the failure, so the same failure reads
  the same wherever it is shown. There is no failure without a declaration:
  a value the code does not tell apart,
  such as a status, a server code or the platform's text, falls into the
  general declaration of its step, which names the step, offers Retry and
  carries the value in the technical lines; where a failure is an
  enumeration, the compiler refuses a declaration that forgets a variant.
- **FR-002 — Scope**: The scope is the smallest unit the failure affects.
  It is not declared: it follows from what carries the failure, a failed
  load, a short list, one message's content, an operation outside mail. It
  is one of four: *one message* (its text or structure), *the list* (fewer
  messages are shown than the Inbox offered), *the account* (nothing of its
  mail could be loaded, or a setting or credential of the account is wrong),
  *the application* (something outside any account's mail, such as Settings
  that did not open). A failure MUST NOT widen its scope:
  one message's problem never fails the list, a list short of messages never
  fails the account, and an account's failure never hides another account.
  It MUST NOT narrow it either: a load that delivered nothing is an account
  failure, never an empty Inbox; a list short of messages is shown as
  incomplete, never as complete (constitution III).
- **FR-003 — One action**: A failure offers at most one action, and only
  when the action can change the outcome. Two actions exist: *Retry*, which
  runs the failed operation again, and *Online Accounts*, which opens the
  system's Online Accounts settings. A failure whose outcome neither can
  change offers none, such as a server that offers no sign-in method Mailbag
  supports, or a message Mailbag cannot show. Refreshing stays available
  from the menu in every state, so Retry is a convenience, never the only
  way. A third action needs a failure that neither existing action serves,
  named in the feature that introduces it.
- **FR-004 — Advice**: Advice is one sentence saying what to do next, in the
  imperative. It is present only when there is a next step: after the action,
  when the action alone does not finish the repair ("then choose Refresh
  Inbox"), or instead of one, when the step happens outside Mailbag. A
  failure with nothing to do has no advice. Advice MUST NOT claim more than
  the failure proves (FR-009).
- **FR-005 — The failure dialog**: Every channel's button opens the failure
  dialog, which presents the failure in this order and nothing else:

  1. The *explanation*: what happened, in plain words that a person who
     knows nothing of mail protocols understands. No code, status, protocol
     term or words of the server enter it.
  2. The *advice*: what to do next, when there is a next step (FR-004).
  3. What the remote side said in words, each text as a block of its own
     under a heading that names the source: "Alert from the mail server",
     "Reply from the mail server", "Message from the mail service", "From
     the system". An alert the server marks as such (RFC 3501) comes
     first. The mail service's message is such a text like any other; it is
     shown here, labelled, not in the explanation.
  4. The *technical details*, under their own heading: identifiers only,
     for the maintainer and an issue report: the failure value that the
     record's error line names (003 FR-004), the server's or the service's
     status and code, the step that failed, and, when the failure carries
     them, the host and the sign-in method.
  5. The *action* as a button, when there is one (FR-003).

  The blocks of 3 and 4 share one form and are selectable. The dialog has
  a header bar with the title, the close button and a copy button that
  puts the whole dialog text on the clipboard in the same order, one
  snippet for an issue report. A failed load always has technical details,
  at least the failure value; a short list has blocks 3 and 4 only when
  the failure carries more than its explanation; a message's content
  problem opens no dialog at all (FR-006).
  Nothing that the record may not hold enters the dialog (003 FR-009);
  server and service text carries `<login>` in place of the account's
  sign-in name from the moment the failure is built, once, for every
  channel, so the dialog and the record show the same text. Nothing shown
  in one part of the dialog is repeated in another.

**Channels**

- **FR-006 — One channel per failure**: A failure appears in exactly one
  channel, chosen by what carries it and by what is on screen:

  | What carries the failure | Channel | Shows | Stays |
  |---|---|---|---|
  | One message: its content cannot be shown | A status page in the body's place, under the envelope; the row stays | Warning icon, title, explanation, advice; the action button when declared | While the message is open |
  | The list: fewer messages arrived than the Inbox offered | The banner above the list; the rows that arrived stay | Title; one button opening the failure dialog | While that list is on screen |
  | The account: its load failed, nothing to show | The status page in place of the list | Warning icon, title, explanation, advice; the action button when declared, and a Details button opening the failure dialog | Until the account's next load replaces it |
  | The account: a problem Online Accounts reports | The account row's problem icon, as [001](../001-goa-account-observation/spec.md) defines it | As 001 | As 001 |
  | The application: an operation outside any account's mail | A toast | Title and advice in one line, nothing else; a failed Settings launch writes no record line (003 FR-004) | The platform's default |

  A banner is one line and one button: the title, and a button that opens
  the failure dialog, which carries everything else, the action included.
  Every failure has at least an explanation, so every banner has its button.
  A status page that shows a failure carries the warning icon; the mail
  icon stays for the empty states (no account, nothing loaded, an empty
  Inbox). Its explanation and advice are its description, so the spacing
  between icon, title and text is the platform's, the same as on the
  reader's empty page; only the buttons are its child. A toast carries no
  button and no details; what it announces is over, and the record has the
  rest. The account
  page priority of 001 stands: while an account page covers the list and the
  reader, no mail failure is shown.
- **FR-007 — No other channel**: No dialog interrupts work to announce a
  failure, no desktop notification is sent, no failure is stored, and no
  history of notices is kept. Details is a view opened by the user from a
  channel, not a channel.
- **FR-008 — A notice lives with its cause**: A notice appears when the
  failure arrives and goes when the next result of the same operation
  replaces it: a complete load clears the banner and the status page, a
  reopened message shows its own state. Selecting another account shows that
  account's state; the notice returns with the account. A notice never
  outlives what it is about and is never dismissed by time alone, except the
  toast.

**Wording**

- **FR-009 — Wording**: The title names what failed in Mailbag's words, not
  the server's, in a few words ("Server unreachable", "Sign-in rejected").
  The explanation is one or two sentences about what happened, in plain
  words a novice understands; no code, status, protocol term or words of
  the server enter it, those are the dialog's blocks (FR-005). The
  voice is impersonal, as everywhere in Mailbag (AGENTS.md, UI wording):
  "Could not reach the mail server", "This message cannot be decrypted",
  "no password was sent"; the application never names itself in a
  sentence. No text
  claims a cause the failure does not prove: a rejected sign-in is "the
  server rejected sign-in", never "wrong password", unless the server's code
  says so. Text from a server or a message is inert: never markup, never
  interpreted, cut at the display limit and with NUL replaced, as
  [002](../002-imap-integration/contracts/ui.md) rules. All application
  wording is English; translation waits for release readiness and for the
  approved amendment the constitution requires (Public Repository
  Language). When it comes, the title, the explanation, the advice, the
  block headings and the button labels are translated; the remote side's
  texts are never
  translated; the technical details stay in English, as identifiers that
  match the record's error line and read the same in every report.
- **FR-010 — Cancellation is not a failure**: A load or request that ended
  because the user or Mailbag stopped it declares nothing and shows nothing.

**Accessibility**

- **FR-011 — Reachable and announced**: Every action button, every banner
  button, the Details button, and the failure dialog's text, copy button and
  close button are reachable by keyboard and named for assistive technology.
  The text of a banner and of the status page is exposed as text. The account
  row rules of 001 stand.

**Every source, every feature**

- **FR-012 — Failures of every feature**: These rules apply to every failure
  Mailbag has today, whatever its source: the mail server or the network,
  Online Accounts (settings, password, token, timeout), decoding a message
  (character set, encoding, encryption, S/MIME), and Mailbag itself (a mail
  worker that stopped, Settings that did not open). Every later feature
  declares its failures under these rules; no specification keeps a list of
  failures or of their wording. The account problems that 001
  shows at the row and on the account page keep 001's rules, its two buttons
  included; they are the one exception. The wording table and the "incomplete
  list in a toast" rule of the 002 UI contract are superseded by this feature.
- **FR-013 — Deferred, with the layer each waits for**: (a) *Local storage
  (007)*: an account failure while the account's stored mail is on screen is
  shown as the banner above that mail with the failure's title, so that
  stale mail is told apart from an incomplete list; the store's own
  failures (it cannot be opened, the disk is full) declare their scope under
  FR-002; two list-scope notices at once are decided there. (b) *Background
  synchronization (016)*: failures of operations the user did not start use
  the same channels without a status page, since the list is already shown;
  how a person learns of a failure that happened while they were away is
  decided there. Until these layers exist, every load is started by the
  user, and the status page is the only account channel.

- **FR-014 — Defects in Mailbag**: A panic on a worker thread (the mail
  worker; later work sent to GIO's thread pool, such as the store's calls)
  is caught on that thread and becomes a failure
  of the operation the worker ran: its Details carry the panic's message and
  its place in the code, its action is Retry, and the worker keeps serving
  the next operation. A panic in Mailbag's own code carries fixed text; a
  panic inside a library may carry part of the text it was handling, and
  the dialog shows it as received, where it is visible before it is copied.
  A panic on the main thread ends the application: the
  platform aborts when a panic reaches GTK, so no notice is possible. Its
  text stays on the standard error stream, where the record and the system
  journal collect it; no crash file is written and nothing is reported at
  the next start.

## Success Criteria

### Measurable Outcomes

- **SC-001**: For every failure value the providers can report, a test of
  its declaration finds the action, the advice, the remote texts and the
  technical details FR-001–FR-005 require; for a failed load, a short
  list and a message's content a test through the window finds the
  channel FR-006 assigns to it, with the buttons the declaration names
  (FR-012); for a Settings launch the launcher's test finds the failure
  reported once, and the toast shows that one line.
- **SC-002**: After a load whose list is short, the banner is on screen with
  the rows; after the next complete load of the same account it is gone;
  after switching accounts and back it is there again (FR-006, FR-008).
- **SC-003**: The failure dialog's text and the clipboard of a rejected
  sign-in whose server text repeats a two-character sign-in name contain
  `<login>` and none of the fixture's private markers of 003 SC-002, and the
  status page shows no server text (FR-005, FR-009).
- **SC-004**: A cancelled load shows nothing in any channel, and a load that
  delivered nothing never shows an empty Inbox (FR-002, FR-010).
- **SC-005**: On the installed build, Tab from the list reaches the banner's
  button and from the status page its buttons, Enter activates them, and a
  screen reader reads the banner's and the status page's text (FR-011).
- **SC-006**: A load whose worker panics on purpose in a test ends as a
  failure whose Details name the panic's message and its place; the next
  load runs on the same worker (FR-014).

## Assumptions

- "Lasting" means the mechanism, not the failures: nothing about a failure
  is written to disk, and a restart forgets it.
- Neither scope nor lifetime is a declared property: both follow from what
  carries the failure and from what is on screen (FR-002, FR-006, FR-008).
  The sizing named "scope and lifetime" as the inputs of the channel rule;
  the challenge showed that the failure's carrier already fixes both, so
  declaring them would be a second way to say the same thing.
- "Repeating helps" is expressed by the Retry action. Whether background
  synchronization (016) tries again on its own is decided in its
  specification.
- The layout in `crates/mailbag/resources/ui/` changes in five places,
  approved on the prototype 2026-09-24 and amended in review 2026-09-25: a
  banner above the message list (`mailbag.ui`); a failure page of its own
  in the list stack, with the warning icon and its action and Details
  buttons as its child (`mailbag.ui`); 001's two account-page buttons,
  Retry Check and Online Accounts, declared as the account page's child
  (`mailbag.ui`), replacing the buttons and the explanation label that 001
  and 002 build in code; a status page with its action button in the
  reader's body slot (`message-content.ui`); and two new forms, the failure
  dialog and its block. No other widget is built in code.
- 002 is amended by FR-012: its wording table and its toast rule for an
  incomplete list are superseded, the wording moves into the code that
  declares each failure, the reader's explanation in place of the text
  becomes a status page in the body's place (FR-006), and the status page's
  explanation goes into its description, escaped, instead of a separate
  plain-text label. 005 is amended: the mail service's
  developer message about a refusal, kept out of the page by
  [research §5](../005-microsoft-graph-integration/research.md), may appear
  in Details, which are for the maintainer. 003 is amended: its FR-011 has
  the server's reply travel on to the UI unchanged and the sign-in name
  replaced only for the record; from this feature the name is replaced once,
  where the failure is built, and the same text reaches the record and
  Details ([003 research §6](../003-logging/research.md) likewise). 001 is
  unchanged: the row problem icon and the toasts for hidden accounts keep
  their rules.
- Whether the mail service's developer message about a refusal can carry the
  mailbox address is not checked. Mailbag sends the service only a token, so
  the message cannot echo a sign-in name. No mechanism is added; if a probe
  ever shows personal data there, the feature that owns the failure replaces
  it the same way.
- Out of scope: a history of notices, a synchronization status panel,
  automatic retries or waiting before a retry, desktop notifications,
  storing failures, asking Online Accounts to re-check an account's
  credentials, a third action, the application version in Details (the
  About dialog shows it), and a report at the next start about a crash
  (FR-014).
- The [constitution](../../.specify/memory/constitution.md) governs this
  feature.
