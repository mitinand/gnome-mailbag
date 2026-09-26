# Gmail Integration: Acceptance on the Live Account

Applies after portion 4. Automated checks run with `scripts/check.sh`; the
steps below need the maintainer's Google account in GOA with mail enabled and
one Inbox message carrying a user label whose name is not in Latin letters.

## Load and read (SC-001, SC-002, SC-007)

1. Start the installed application. Select the Google account. Choose Refresh
   Inbox. Expect up to 100 rows, newest first, no password prompt.
2. Open a plain-text message, an HTML-only message and one with attachments.
   Expect the same behaviour as for a Generic IMAP account.
3. Select a Generic IMAP account, refresh it, switch back. Expect each account
   to show its own batch.

## Record at debug (SC-004, SC-005, SC-006)

```bash
mailbag --log-level=debug 2> ~/gmail.log
```

Refresh the Google account, quit, then read `~/gmail.log`:

- A line with the list Gmail announced after sign-in, the lines for ENABLE
  (the enabled list and the command's result), and one with the server's
  `name`, `vendor` and `version` and nothing else from the ID reply.
- One line per received message with its Gmail message identifier and its
  labels; the non-Latin label is readable text.
- Take one identifier, convert it to hexadecimal
  (`printf '%x\n' <decimal>`), open the message in the Gmail web interface
  and compare with the identifier in the page address. They match.
- Search the file for the token: `grep -c 'ya29' ~/gmail.log` prints 0. Search
  for the address and the mail address of the account: none.

## Authorization (SC-003)

1. On the Google account's third-party access page
   (myaccount.google.com → Security → Third-party apps), remove GNOME's
   access.
2. Refresh the Google account in Mailbag. Expect "The mail server rejected
   sign-in" with Gmail's reason and the sentence "Check this account's
   sign-in in Online Accounts." Within the hour after revocation GOA may still
   hand out its cached token; the refusal then comes from Gmail, which is the
   expected outcome. No password prompt appears.
3. Re-authorize the account in Online Accounts. Refresh again without
   restarting Mailbag. Expect the batch.
4. "GOA cannot provide the token" has no reliable live reproduction: GOA
   answers from its cache for most of an hour. The goa-adapter test with the
   fake bus covers it (GetAccessToken answered with an error, then a hang);
   the window test covers the wording. Record this as verified by tests, not
   live.

## Window (SC-008)

- Select the Microsoft 365 account: Refresh Inbox stays disabled and the
  status says Mailbag cannot load mail for it yet.
- Keyboard: reach Refresh Inbox and the message rows as in 002's acceptance.
