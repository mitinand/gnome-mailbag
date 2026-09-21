# Log events: what Mailbag does today

This is the list [FR-017](spec.md#requirements) requires. It covers account
observation (001), the Inbox load and text decoding (002) and the application
itself. The levels and privacy limits are defined in the [spec](spec.md);
this list applies them.

## How to read and extend the list

One row is one thing the application does or one way it can end. The decision
is a level, or **not logged** with the reason. Fields name what the line
carries besides the time, the level, the place it was written, and the account
label and operation identifier FR-013 requires.

"Reported to the load" means the component returns the failure and writes no
error line itself: FR-004 allows one error line per failed operation, written
by whoever gives the operation up. For an Inbox load that is the load; for a
read of the account list it is the account observer in `goa-adapter`.

The list describes what the code does today. It does not ask for new behavior:
where a row and the code disagree, the row is wrong.

A later feature records the same decisions for its events in its own spec, as
a section or as a separate file with these columns. When it changes something
listed here, it amends this file.

## Application

| What happens | Decision | Fields and notes |
|---|---|---|
| Start with logging on | Mailbag's first line, at any level | Version, native or Flatpak, runtime or OS version, GTK and libadwaita versions, level (FR-015) |
| Start with the option while Mailbag is running | Not a record line | The second start prints the explanation on its own terminal (spec, Edge Cases) |
| Quit | info | Shows that Mailbag meant to stop. Its absence alone does not prove a crash, because lines can be lost |
| Lines were lost | Written when writing resumes, at any level, like the first line | Number of lost lines, no reason given: an unread pipe and a full disk look the same (FR-016) |
| Routine UI actions: selecting an account, navigating, resizing, opening menus | Not logged | One decision for all of them: no mail access and no failure; selecting never loads (002 FR-003) |
| The user opens a message | debug | Folder, UID. Lets the person match the screen to the record. No network work happens |
| The user asks to open Online Accounts in Settings | info | Duration |
| Settings could not be opened | error | Cause: unavailable, access denied, timeout, invalid reply. Same as the UI's explanation |

## Account observation

A read of the account list is reported where it completes, in `goa-adapter`,
by one line without an operation identifier. `mailbag` logs what changes for
the user.

| What happens | Decision | Fields and notes |
|---|---|---|
| The account list was read | info, in `goa-adapter` | Number of accounts by provider type, duration |
| The list is read again after a change signal from Online Accounts | debug | Which kind of signal. The result, if anything changed, is logged by the rows below |
| The user asks for Retry Check | info | |
| Reading the account list failed | error, in `goa-adapter`, once per real read | The read that failed and the cause: unavailable, access denied, timeout, invalid reply. Same as the UI's explanation. A result published again during Retry is not a new failure and writes nothing |
| An account appeared | info | Label, provider type; never the address or display name. The observer does not know the host; it is written when a load obtains the settings |
| An account was removed, its Mail was disabled, or it is not supported | info | Label, reason |
| An account needs attention in Online Accounts, or its Mail service is unavailable | warning | Label, which of the two. The UI shows it as a persistent problem. info when the problem is gone |
| Accounts are hidden because their Mail service is unavailable | info | Number of accounts |
| A failed read of the account list is followed by a successful one | info, in `goa-adapter` | Tells the reader that the account list is confirmed again |
| Received mail of an excluded account is discarded | info | Label, number of messages discarded. A failure of the account service discards nothing today |
| Account display name, address, icon | Not logged | Personal details (FR-009, 001 FR-013) |

## Inbox load

Written in `mailbag::inbox_load`. One operation is one load, from Refresh
Inbox to its result. The load writes its single error line.

| What happens | Decision | Fields and notes |
|---|---|---|
| Refresh Inbox starts a load | info | Label, new operation identifier |
| Refresh Inbox is unavailable | Not logged | A state of the menu, not an event |
| The load finished | info | Messages received, of which with content this version does not support (HTML-only, encrypted, S/MIME), messages that disappeared during the load, total duration |
| Messages disappeared during the load | debug | Their UIDs. The count is in the line above; another client removing mail is normal |
| Some messages have content that could not be read | warning | Count of M. Covers the outcomes the code already tells apart: unknown character set, unknown transfer encoding, undecodable part, structure that could not be read, text the server did not return. Details are debug lines of `mailbag-imap` and `mailbag-content` |
| The server refused to finish the message list | warning | Rows received, response code. The reply text is a debug line of `mailbag-imap` |
| The load failed | error | Step and cause as the UI shows them, the wait limit in seconds for a timeout, the server's response code if any. The reply text and alerts are debug lines of `mailbag-imap` |
| Every message of the window disappeared, or the Inbox was replaced | error | Cause "Inbox changed" |
| The load was cancelled by exclusion of its account or by quitting | info | Which of the two |
| A result arrived for an account excluded meanwhile and was dropped | info | Label |
| The mail worker stopped without a result | error | Cause "worker stopped" |

## Account settings and password

Written in `goa-adapter`, inside a load's operation.

| What happens | Decision | Fields and notes |
|---|---|---|
| Settings and password were received from Online Accounts | info | Duration, encryption mode (TLS from the start, or STARTTLS). debug adds host and port |
| Settings were not returned; neither encryption mode is set; the password was not returned; Online Accounts did not answer in time | Reported to the load | The load's error line names the step and cause. "No encryption" also says that no password was requested |
| The request was cancelled | Reported to the load | The load logs the cancellation at info |
| Sign-in name, password | Not logged | FR-009 |

## IMAP

Written in `mailbag-imap`, inside a load's operation. Every failure below is
reported to the load; this crate writes info and debug lines only. Server text
is written here, where the sign-in name is known, after every occurrence of
it, whatever its length, is replaced with `<login>` (FR-011).

| What happens | Decision | Fields and notes |
|---|---|---|
| Connected | info | Duration. debug adds host, port and the address reached. Written for every real connection, also when a load reconnects |
| The load reconnects after a structure it could not read | info | Which attempt. Explains a long load and the extra connection lines |
| The connection was secured | info | Mode, TLS version, duration |
| The certificate failed validation; STARTTLS is not offered or failed | Reported to the load | The cause the load already receives. Which certificate check failed is added only if the plan finds it available without new error plumbing; never the certificate's names |
| The server's capabilities | info | The capability list as Mailbag already receives it before sign-in; no request is sent for the log. It describes the server software, not the mailbox |
| Signed in | info | Method, duration |
| Sign-in refused; no supported sign-in method | Reported to the load | Response code at error; reply text at debug, sign-in name replaced |
| The server sent alerts | info: their number; debug: their text, sign-in name replaced | Alerts are server-written sentences for the user |
| The Inbox was opened read-only | info | Number of messages. debug adds folder name, UIDVALIDITY, UIDNEXT |
| The message list was loaded | info | Rows, duration. debug adds the UID range |
| A row's list fields (raw From, To, Subject lines) | Not logged | Header values (FR-009) |
| Part structures were loaded | info | Messages, duration |
| One message's part tree | debug | Folder, UID, then one line per part: section, content type, `charset`, `format`, `delsp`, disposition, transfer encoding, size, and which parameters carry a file name. Written where the server's description is read. The file name, other parameters, the part's description, content identifiers and an attached message's envelope are left out (FR-009) |
| One message's structure could not be read | debug | Folder, UID, and whether the server refused or the description could not be parsed. The description itself is not written. Counted in the load's warning |
| Text was loaded | info | Messages, commands sent, bytes received, duration |
| One command's group of messages | debug | Sections requested, UIDs, bytes, duration |
| The server did not return one message's text; a message disappeared | debug | Folder, UID, which of the two |
| Received part headers and bodies | Not logged | Mail content (FR-009) |
| A step timed out; the server closed the connection | Reported to the load | Step, wait limit; the closing reply's text at debug, sign-in name replaced |
| The connection was closed after the load | debug | |
| The IMAP library's own protocol trace | Not logged | Contains credentials and mail; stays compiled out (FR-012) |

## Message content

Written in `mailbag-content`, inside a load's operation and a message's span.
It writes debug lines only; the load turns the outcomes into its counts.

| What happens | Decision | Fields and notes |
|---|---|---|
| Text parts were selected for a message | debug | Selected sections and the rule that chose them: single part; the last alternative that has plain text; the root of a related set, with whether its `start` named a part. Folder and UID come from the message's span |
| A part carries a file name in its Content-Type parameters | debug | Section, the shape of the raw value and its extension when the raw value shows one; never the name (FR-009) |
| No text was selected: no plain text, encrypted, S/MIME | debug | Folder, UID, which. Counted at info as unsupported by this version |
| A part was decoded | debug | Folder, UID, section, character set, transfer encoding, whether `format=flowed` was applied, bytes in, characters out, duration |
| A part could not be decoded: unknown character set, unknown transfer encoding, undecodable entity | debug | Folder, UID, section, the declared name, the failing stage. Counted in the load's warning |
| A list header is absent from the message | debug | Folder, UID, header name. Normal |
| A list header is present, but no value came out or the value has replacement characters | debug | Folder, UID, header name and the shape of the raw value (FR-009). No cause is claimed: the decoder does not report one. Not counted in the load's warning; the row stays usable |
| Decoded text and decoded list fields | Not logged | Mail content (FR-009) |
