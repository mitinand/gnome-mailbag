# Log events: working file of the implementation

This file plans the lines this feature adds to Mailbag as it is today:
account observation (001), the Inbox load and text decoding (002) and the
application itself. It is a working file, not a requirement: the
[spec](spec.md) defines the levels and privacy limits, and this file applies
them for the implementation. Once portion 4 is implemented the code is the
source of truth, and this file is not kept in step with later changes. Later
features write their lines under the spec's rules without a list like this
(FR-017).

## How to read the list

One row is one thing the application does or one way it can end. The decision
is a level, or **not logged** with the reason. Fields name what the line
carries besides the time, the level, the place it was written, and the account
identifier FR-013 requires.

No line carries a duration
([record contract](contracts/record.md#durations)); a step's or an
operation's duration is the difference between the times of two lines.

"Reported to the load" means the component returns the failure and writes no
error line itself: FR-004 allows one error line per failed operation, written
by whoever gives the operation up. For an Inbox load that is the load; for a
read of the account list it is the account observer in `goa-adapter`.

The list describes what the code does today. It does not ask for new behavior:
where a row and the code disagree, the row is wrong.

## Application

| What happens | Decision | Fields and notes |
|---|---|---|
| Start with logging on | Mailbag's first line, at any level | Version, native or Flatpak, runtime or OS version, GTK and libadwaita versions, level (FR-015) |
| Start with the option while Mailbag is running | Not a record line | The second start prints the explanation on its own terminal (spec, Edge Cases) |
| Quit | info | Shows that Mailbag meant to stop |
| Routine UI actions: selecting an account, navigating, resizing, opening menus | Not logged | One decision for all of them: no mail access and no failure; selecting never loads (002 FR-003) |
| The user opens a message | debug | Account, UID. Lets the person match the screen to the record. No network work happens |
| Online Accounts was opened in Settings at the user's request | info | A second request while one is pending shares it and writes nothing |
| Settings could not be opened | error | `cause`: the name of the launch failure (unavailable, access denied, timeout, invalid reply), the value the UI explains |

## Account observation

A read of the account list is reported where it completes, in `goa-adapter`,
by one line. `mailbag` logs what changes for
the user. An account is named by its Online Accounts identifier.

`mailbag` keeps a row for each account it shows, and a row's problems, so
those lines are written when something changes. It keeps nothing about an
account it does not show and decides again at every complete read, so the
line about such an account is repeated at every complete read.

| What happens | Decision | Fields and notes |
|---|---|---|
| The account list was read | info, in `goa-adapter` | `accounts`. Each account's provider type is on the lines about it below |
| Online Accounts signals a change | debug, in `goa-adapter` | Which signal. It starts a read, or one more read after the running one. The result, if anything changed, is logged by the rows below |
| The user asks for Retry Check | info | |
| Reading the account list failed | error, in `goa-adapter`, once per real read | `step` (the part that failed: connecting to the session bus, reading the accounts, an account's identifier) and `cause`: the name of the failure value (unavailable, access denied, timeout, invalid reply), the value the UI explains. Rows stay and are marked unconfirmed; `mailbag` writes nothing more. A result published again during Retry is not a new failure and writes nothing |
| A successful read follows a failed one | Not logged separately | The failed read's error line stands before this read's line. The rows' unconfirmed marks clear without a line of their own |
| An account gets a row | info | Account, provider type; never the address or display name. The observer does not know the host; it is written when a load obtains the settings |
| An account's row is removed because the account was removed from Online Accounts | info, once | Account, provider type, reason. The user sees a notice |
| An account is not shown: its Mail is disabled, its provider is not supported, or it has no Mail service and no row yet | info, at every complete read | Account, provider type, reason. When Mail is disabled for an account that had a row, the row is removed with a notice in the same read |
| An account with a row needs attention in Online Accounts, or loses its Mail service | warning, when the problem appears | Account, which problem. The row shows it as a persistent problem. info when the problem is gone |
| Received mail of an account that is no longer shown is discarded | info | Account, number of messages. Only a received batch holds mail; a failed or running load holds none (a running load is cancelled, see Inbox load). A failed read of the account list discards nothing |
| Account display name, address, icon | Not logged | Personal details. The Online Accounts identifier names the account instead (FR-013) |

## Inbox load

`mailbag::inbox::InboxController` writes starts, accepted outcomes,
cancellations and discarded results inside the load's span. `inbox_load`
writes per-message observations on the worker. One operation is one accepted
Refresh Inbox. Ownership and cancellation ordering are defined in the
[record contract](contracts/record.md#load-outcomes-and-cancellation).

| What happens | Decision | Fields and notes |
|---|---|---|
| Refresh Inbox starts a load | info | Account |
| Refresh Inbox is unavailable | Not logged | A state of the menu, not an event |
| The load finished and its result was accepted | info | Messages received, of which with content this version does not support (HTML-only, encrypted, S/MIME). Messages that disappeared are the difference to the rows of "message list loaded" |
| Messages disappeared during the load | debug, in `inbox_load` or `mailbag-imap` where observed | Their UIDs. Another client removing mail is normal; no count is carried for the log |
| Some messages have content that could not be read | warning | Count of M. Covers the outcomes the code already tells apart: unknown character set, unknown transfer encoding, undecodable part, structure that could not be read, text the server did not return. Details are debug lines of `mailbag-imap` and `mailbag-content` |
| The server refused to finish the message list | warning | Response code. The rows received are on the message list line of `mailbag-imap`, and the reply text is its debug line |
| The load failed and the failure was accepted | error | `step` and `cause` as the names of the failure values the UI explains, the server's response code if any, the number of alerts if any. The reply text and alerts are debug lines of `mailbag-imap` |
| Every message of the window disappeared, or the Inbox was replaced | error | Cause "Inbox changed"; only while the result remains applicable |
| The load was cancelled by exclusion of its account or by closing the window | info, in `discard_excluded` or `cancel_load` | Reason, before dropping an active handle; no waiting for the worker |
| The Quit action while a load runs | Not logged as a cancellation | Quit ends Mailbag without cancelling the load, so the record shows the load's start and then the quit line. The shutdown order is not changed for the record |
| A result arrived for an account excluded meanwhile and was dropped | info, at the controller's result applicability check | Account; no completion, warning or error for the discarded result |
| The worker acknowledges cancellation, or a completed load's handle is dropped | Not logged | Cancellation was already recorded by its owner; handle cleanup is not a second outcome |
| The mail worker stopped without a result | error | Cause "worker stopped"; only while the result remains applicable |

## Account settings and password

Written in `mailbag::inbox_load`, in the callback that receives the answer of
Online Accounts, inside the load's span. `goa-adapter` writes nothing here: its
request completes in callbacks where the load's span is not active.

| What happens | Decision | Fields and notes |
|---|---|---|
| Settings and password were received from Online Accounts | info | Encryption mode (TLS from the start, or STARTTLS). Host and port are on the connection's debug line in `mailbag-imap` |
| Settings were not returned; neither encryption mode is set; the password was not returned; Online Accounts did not answer in time | Reported to the controller | The load's error line names the step and cause. "No encryption" also says that no password was requested |
| The request was cancelled | Not logged again | The controller already recorded the cancellation and its reason at info |
| Sign-in name, password | Not logged | FR-009 |

## IMAP

Written in `mailbag-imap`, inside a load's span. Every failure below is
reported to the load; this crate writes info and debug lines only. Server text
is written here, where the sign-in name is known, after every occurrence of
it, whatever its length, is replaced with `<login>` (FR-011). A line about one
message names it by its UID; the folder is the load's Inbox, which the debug
line "Inbox opened" names.

| What happens | Decision | Fields and notes |
|---|---|---|
| Connected | info | debug adds host, port and the address reached. Written for every real connection, also when a load reconnects |
| The load reconnects after a structure it could not read | info | Explains a long load and the extra connection lines; each reconnection has its own line |
| The connection was secured | info | Mode, TLS version |
| The certificate failed validation; STARTTLS is not offered or failed | Reported to the load | The cause the load already receives. A failed TLS handshake adds a debug line with GIO's text for it, such as "An unexpected TLS packet was received" when the encryption chosen in Online Accounts does not match the port, and the certificate checks that failed; never the certificate's names |
| The server's capabilities | info | The capability list as Mailbag already receives it before sign-in; no request is sent for the log. It describes the server software, not the mailbox |
| Signed in | info | Method |
| Sign-in refused; no supported sign-in method | Reported to the load | Response code at error; reply text at debug, sign-in name replaced |
| The server sent an alert | info that it arrived; debug its text, sign-in name replaced | Written as each alert arrives, whether the load succeeds or fails. Alerts are server-written sentences for the user; the load's error line also gives the number attached to its failure |
| The Inbox was opened read-only | info | Number of messages. debug adds folder name, UIDVALIDITY, UIDNEXT |
| The message list was loaded | info | Rows. debug adds the UID range |
| A row's list fields (raw From, To, Subject lines) | Not logged | Header values (FR-009) |
| Part structures were loaded | info | Messages |
| One message's part tree | debug | UID, then one line per part: section, content type, `charset`, `format`, `delsp`, disposition, transfer encoding, size. Written where the server's description is read. File names, other parameters, the part's description, content identifiers and an attached message's envelope are left out (FR-009) |
| One message's structure could not be read | debug | UID, and whether the server refused or the description could not be parsed. The description itself is not written. Counted in the load's warning |
| Text was loaded | info | Messages, commands sent |
| One command's group of messages completed or failed | debug | Sections requested, UIDs, and that it failed when it did |
| The server did not return one message's text; a message disappeared | debug | UID, which of the two |
| Received part headers and bodies | Not logged | Mail content (FR-009) |
| A step timed out; the server closed the connection | Reported to the load | Step; the closing reply's text at debug, sign-in name replaced |
| The IMAP library's own protocol trace | Not logged | Contains credentials and mail; stays compiled out (FR-012) |

## Message content

Written in `mailbag-content`, inside a load's span and a message's span. It
writes debug lines only; the load turns the outcomes into its counts. The UID
comes from the message's span; the folder is the load's Inbox. Decoding lines
carry no section: parts are decoded in the order of the selected sections,
and the part tree gives each part's character set and transfer encoding.

| What happens | Decision | Fields and notes |
|---|---|---|
| Text parts were selected for a message | debug | Selected sections. Each decision on the way has its own line where it is made: which alternative, the last with plain text, was chosen; which root of a related set, and whether its `start` named a part |
| A text part is left out as a file: it has a file name and no inline disposition | debug | UID, section; never the name. An attachment disposition is already on the part tree |
| No text was selected: no plain text, encrypted, S/MIME | debug | UID, `explanation`. Counted at info as unsupported by this version |
| A part was decoded | debug | UID, character set, transfer encoding, whether `format=flowed` was applied, characters out |
| A part could not be decoded: unknown character set, unknown transfer encoding, undecodable entity | debug | UID, `cause`: the explanation the reader shows, with the declared name of an unknown character set or encoding. Counted in the load's warning |
| A list header is present, but no value came out or the value has replacement characters | debug | UID, header name. No cause is claimed: the decoder does not report one. An absent header is normal and writes nothing. Not counted in the load's warning; the row stays usable |
| Decoded text and decoded list fields | Not logged | Mail content (FR-009) |
