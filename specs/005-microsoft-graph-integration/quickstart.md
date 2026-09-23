# Microsoft 365 Integration: Acceptance on the Live Account

Applies after portion 4. Automated checks run with `scripts/check.sh`; the
steps below need the maintainer's Microsoft 365 account in GOA with mail
enabled, an HTML-only message and a message with attachments in its Inbox.
Refusals, short pages and stalls are verified by the scripted service in
`mailbag-graph` and `mailbag-providers`, not live.

## Load and read (SC-001, SC-002, SC-007, SC-010)

1. Start the installed application. Select the Microsoft 365 account. Choose
   Refresh Inbox. Expect up to 100 rows, newest first by received time, no
   password prompt, within the wait limit.
2. Open a plain message, the HTML-only message and the one with attachments.
   Expect text in each, including the HTML-only one; no request on opening;
   no attachment download; an attached text file is never shown as the body.
   If the Inbox holds a message whose body is only a picture, open it too:
   the reader shows empty text.
3. Select a Google account and a Generic IMAP account, refresh them, switch
   back. Expect each account to show its own batch.

## Record at debug (SC-004, SC-005, SC-006)

```bash
mailbag --log-level=debug 2> ms365.log
```

Refresh the Microsoft 365 account, quit, then read `ms365.log`:

- One line for the request with its path and no query value beyond the
  fixed ones, one for the answer with status 200, the size, 100 messages and
  whether more were available.
- One line per received message with its immutable identifier, received time
  and read state.
- Note one identifier. In Outlook, move that message to another folder and
  back to the Inbox. Refresh at debug again: the message carries the same
  identifier.
- Search the file for the token: `grep -c 'Bearer' ms365.log` prints 0, and
  the first characters of the token as shown by Online Accounts' debug
  output are absent. Search for a subject and the account's address: none.

## Authorization (SC-003)

1. Remove GNOME's access on the Microsoft account's privacy page ("Apps and
   services"). This stops new tokens; a token already issued stays valid
   until it expires, and GOA keeps handing it out, so refreshes still load
   mail until then. Ask GOA how long the current token lives (the second
   value of `GetAccessToken`, never the token itself).
2. After that time, refresh the Microsoft 365 account in Mailbag. GOA cannot
   renew the token, so expect "Authorization unavailable" and the account
   marked for attention; no password prompt appears. Run this refresh at
   debug as well: the failure line names the cause and carries no token.
3. Sign in again in Online Accounts. Refresh without restarting Mailbag.
   Expect the batch.
4. A token the service refuses (SC-003, SC-006) has no reliable live
   reproduction, because GOA does not hand out a token it failed to renew.
   The service's answer to an invalid token (401,
   `InvalidAuthenticationToken`) was checked live by the probe in research
   §5; the scripted service covers the load and the record, and the window
   test covers the wording. Record this as verified by the probe and by
   tests, not live.

## Window (SC-009)

- Select the Microsoft 365 account: Refresh Inbox is available and the
  "No mail loaded" hint points to it.
- Keyboard: reach Refresh Inbox and the message rows as in 002's acceptance.
