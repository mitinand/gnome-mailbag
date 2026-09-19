# IMAP Integration: In-Memory Data

These are proposed Rust roles, not a persisted schema or a future database API.
AccountList retains account eligibility and selection; InboxController owns
each account's received mail for this run and the single load. WindowUi makes
the combined page decision.

## Data and ownership

| Role | Contents and lifetime |
|---|---|
| ImapAccess | Account ID, host, login, TLS mode and password from goa-adapter. Owned by the current attempt; released after success, failure or cancellation. No Debug output or persistent cache. |
| AccountInbox | For one account in this run: loading, a ReceivedBatch or a LoadFailure; absent means not loaded. Discarded on confirmed exclusion and at exit. |
| ReceivedBatch | AccountId, Inbox UIDVALIDITY and up to 100 ReceivedMessages. Replaced only by that account's next refresh. |
| ReceivedMessage | UID, decoded subject/from/to display fields, INTERNALDATE, observed `\Seen` and ReceivedContent. No previews, attachment bytes or remote action state. |
| ReceivedContent | Complete decoded plain text or a message-specific explanation: no supported plain text, encrypted content, unusable or unreadable structure or unsupported encoding. Invalid bytes alone do not replace the body with an error. |
| OpenedMessage | UID in the selected account's batch, or none. Cleared by a refresh and by selecting another account. No body-fetch or server-revalidation state. |
| ActiveLoad | AccountId, current step and cancellation handle; at most one. Lives until its connection has closed. No byte counter, progress clock or application watchdog. |
| LoadFailure | Safe failing step/cause and any ALERT text received for that attempt. Server text is for plain-text UI presentation only, never diagnostics. |

UID identifies a message within an Inbox version; UIDVALIDITY identifies that
version. Sort rows by descending UID, which follows Inbox addition order, rather
than INTERNALDATE. A row points to its message by UID, never by an old position.
These identities do not establish persistent reconciliation across refreshes.

The batch is built on the worker and published only when all required commands
complete. Every message returned by the row command keeps its row; an unreadable
part structure changes only that message's ReceivedContent.

Unsupported content is a represented message, not an interrupted transfer.
Network failures, incomplete literals and other unfinished acquisition publish
no batch; the account shows its LoadFailure. Discard raw MIME bytes after
decoding; do not retain a second raw copy.

MailUi shows at most the first 65,536 UTF-8 bytes of decoded body text, cut at a
character boundary, without an explanation; the stored text is not truncated.
Header and ALERT presentation stays plain text.

## Operation transitions

There are two lifecycle states, Idle and Loading. One worker with a GLib context
runs at most one mail acquisition.

| Event | State change and publication |
|---|---|
| Select an account | Show its AccountInbox, or that nothing has been loaded. Never start a load. |
| Refresh Inbox while Idle | Replace the selected account's AccountInbox with loading, clear the opened message and enter Loading. |
| Refresh Inbox while Loading | Unavailable; the menu action is disabled. |
| Select another account while Loading | Only the visible account changes; the load continues for its own account. |
| Successful completion | Store the batch for the load's account and enter Idle. It is visible if that account is selected. |
| Failed completion | Store the LoadFailure for the load's account and enter Idle. |
| Access request reports an unavailable account | An ordinary step failure; only the F01 observer can confirm exclusion. |
| Confirmed F01 exclusion | Discard that account's AccountInbox and cancel its running load. |
| GOA observation failure or recovery | No mail change; F01 controls the visible page. |
| Quit | Cancel the load and close the connection on its worker; never join the worker on GTK's thread. |

A completion is stored only under the AccountId the load was started for, and
only while that account's AccountInbox is still loading. Switching accounts
therefore cannot show it for another account, and exclusion, which discards the
AccountInbox, cannot be undone by a late result. A cancelled load stays Loading
until its connection has closed, so Refresh remains unavailable until then. No
generation registry or pending-operation queue is needed.

## Boundaries and failures

Waiting policy belongs to [the acquisition contract](contracts/imap-reading.md):
finite GOA calls and GSocketClient's timeout, with no aggregate deadline or
application download budget. The library response ceiling is not a total memory
guarantee. The worker keeps substantial parsing/decoding off GTK's context.

A failure names settings, password, connection, secure connection, server sign-in,
Inbox access, message metadata or text acquisition. Timeout qualifies the actual
waiting step. Content explanations belong to a message, not LoadFailure.

Server/library errors must be mapped before reaching diagnostics: their raw
formatting can expose response bytes. Only deliberately selected ALERT text can
reach the UI. Replace embedded NUL before GTK string APIs. Opening uses the
received read flag and text without changing either on the server.

Missing UIDs and unreadable structures are handled by
[the acquisition contract](contracts/imap-reading.md). Neither they nor a
disconnected transfer is evidence of an empty Inbox.
