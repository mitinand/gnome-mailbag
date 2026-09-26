# Error Handling: Acceptance

Applies after portion 4. Automated checks run with `scripts/check.sh`;
they cover every declaration and the channels of a failed load, a short
list and a message's content (SC-001 to SC-004, SC-006); the Settings
toast has no window test (plan.md, "Post-implementation"). The steps below need the installed build and the
maintainer's accounts in GNOME Online Accounts; they cover what tests
cannot: the real widgets, the keyboard and the screen reader (SC-005).

## A failed load (US1)

1. Make a Generic IMAP account's stored password wrong: Online Accounts
   checks the sign-in when an account is added and offers no password
   change, so revoke the account's app password at the provider. Start
   Mailbag, select the account, refresh. Expect the status
   page with the warning icon, a short title such as "Sign-in rejected", a
   plain explanation, the advice to check the sign-in in Online Accounts, the
   buttons Online Accounts and Details. No password or address anywhere on the
   page.
2. Open Details. Expect the dialog: the title in the header bar, the two
   paragraphs, "Reply from the mail server" with the server's words and
   `<login>` in place of the user name if the server repeated it,
   "Technical details" with `Failure: ServerRejectedSignIn` and the server code,
   the Online Accounts button. Press the copy button, paste into a text
   editor: the same text in the same order.
3. Restore the account (a new app password needs the account added
   again). Turn the network off, refresh. Expect a short
   title such as "Server unreachable" with Retry and Details; the dialog
   shows the technical details. Turn the network on, press Retry: the list
   loads.

## A short list and a content problem (US3, US5)

4. With the scripted server only (tests): a refused list shows the banner
   "Some messages not loaded" with Details; a page cut short by the Graph
   service shows "Not all messages loaded". Live, the banner cannot be
   provoked on demand; the tests are the evidence.
5. Open an encrypted message and one with an unknown character set (the
   002 fixtures, or real ones in the Inbox). Expect the envelope, then the
   reader's status page with the warning icon and the explanation; the
   list keeps every row.

## Keyboard and screen reader (FR-011, SC-005)

6. On the failed-load status page, press Tab from the account list: the
   focus reaches the action button, then Details; Enter opens the dialog;
   in the dialog Tab reaches the copy button, the blocks' text and the
   action; Escape closes it.
7. With Orca running, select the account with the failure: the status
   page's title and description are read; open the dialog: the title and
   each paragraph are read.

## The record (SC-003, FR-014)

```bash
mailbag --log-level=debug 2> ~/failures.log
```

8. Repeat step 1 and quit. Expect one error line naming the account and
   `cause=ServerRejectedSignIn` (the domain kind since portion 6), the server's reply at debug with `<login>`, and
   no password, user name or address anywhere: `grep -c '<user name>'
   ~/failures.log` prints 0.
9. A panic on the worker cannot be provoked live; the test with a
   panicking load is the evidence that the status page says "Refresh
   stopped" with the panic's message and place under Technical details and
   that the next refresh works.
