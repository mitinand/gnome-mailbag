# Quickstart: Mail Storage

How to check the feature. Automated tests cover the scripted servers; the
steps below need the installed Flatpak build and the maintainer's accounts.

## Automated

```bash
scripts/check.sh
```

The graphical tests run one per process (they need a GTK session):

```bash
cargo test -p mailbag <test name> -- --ignored --exact
```

## Installed build

Build and install the branch with `scripts/build-flatpak.sh --install`. The store is
`~/.var/app/io.github.mitinand.Mailbag/data/mailbag/mail.sqlite`; the
runtime's own `sqlite3` reads it:

```bash
flatpak run --command=sqlite3 io.github.mitinand.Mailbag ~/.var/app/io.github.mitinand.Mailbag/data/mailbag/mail.sqlite "SELECT account, count(*) FROM message GROUP BY account"
```

| Step | How | Expected |
|---|---|---|
| 1. Stored mail without a network (US1) | Refresh each account's Inbox; quit; turn the network off; start; select each account; open a few messages, one without plain text among them | The same rows, read states and texts as before quitting; no load starts |
| 2. A refresh replaces the Inbox (US2) | Network on; send yourself a message; refresh | The rows stay with the spinner during the load; the new message appears; the reader closes |
| 3. A failed refresh keeps the mail (US3) | Network off; refresh | The rows stay; the banner says the server is unreachable; its dialog has Retry; after a restart the rows are there without a banner |
| 4. An account leaves (US4) | Turn Mail off for one account in Online Accounts; then query the store | The account disappears, as 001 describes; the query shows no row for it; turning Mail on and refreshing brings its mail back |
| 5. A damaged store (US5) | Quit; overwrite the store: `head -c 8192 /dev/urandom > <store path>`; start with `--log-level=info` | One warning line says the store was discarded as not a store; every account says that no mail is loaded until refreshed |
| 6. Privacy (FR-009) | `stat -c %a ~/.var/app/io.github.mitinand.Mailbag/data/mailbag` | `700` |
| 7. The record (SC-008) | Start with `--log-level=debug`, refresh, quit | No subject, sender, text, password or token in the record |

A changed structure (US5, the other reason) is checked by the store's tests:
a developer build with another schema text discards the store the same way.
