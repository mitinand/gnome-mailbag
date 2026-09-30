# Quickstart: Synchronization

How to check the feature. Automated tests cover the scripted servers; the
steps below need the installed Flatpak build and accounts of each provider.

## Automated

```bash
scripts/check.sh
```

The graphical tests run one per process:

```bash
cargo test -p mailbag <test name> -- --ignored --exact
```

## Installed build

Build and install the branch with `scripts/build-flatpak.sh --install`. The
store is discarded at the first start after the structure changed (007
FR-012), so every folder fills again.

| Step | How | Expected |
|---|---|---|
| 1. First fill (US1) | Refresh Account, select the account's largest folder, Refresh Mailbox; scroll and open messages while it fills | The newest messages appear within seconds, recent ones readable; older ones keep appearing below; the window answers throughout; at the end the list holds the folder's message count |
| 2. Nothing changed (US2, SC-002) | Refresh Mailbox again | Ends within seconds; nothing on screen changes; the open message stays open |
| 3. Changes elsewhere (US2) | In the web interface or another client: mark messages read and unread, delete one, move one to another folder, send one to the account; Refresh Mailbox | The read state follows; the deleted and the moved messages leave; the new one appears at the top; an open message that was not touched stays open |
| 4. Gmail labels (US2, FR-006) | On Gmail: refresh Inbox and All Mail; remove a label from a message in the web interface; refresh that label's folder | A message in both is fetched once; it leaves the label's folder and stays in All Mail |
| 5. Old and recent text (US5) | Network off; open a message of this week and one older than 30 days | The recent one shows its text; the old one says its text was not downloaded; no failure appears |
| 6. Quitting during a fill (SC-009) | Start the first fill of a large folder (for example after step 1's store was discarded); close the window after a few seconds | Mailbag is gone within a second |
| 7. Continuing (US4) | Start Mailbag again; select the folder; Refresh Mailbox | The rows stored before the quit are listed at once; the fill continues and completes; messages stored before are not fetched again (the record's counts show it) |
| 8. Failures (US3) | Network off during a refresh; then network on and refresh | The banner by 006, stored rows unchanged, nothing removed; the banner goes when the next refresh starts, and that refresh completes |
| 9. Accounts (US6) | During a fill, turn the account's Mail off in Online Accounts; turn it on again and refresh | The fill stops and the account's mail is gone; the next refresh fills the folder from nothing |
| 10. The record (003) | Start with `--log-level=debug` and refresh a folder | Counts of listed, removed, changed and arrived messages; folder names at debug only; no subject, sender, text or credential |

Cases the automated tests cover with scripted servers: a listing cut short,
refused or ended by a lost connection; a removal reported during the
listing; a numbering reset on Generic IMAP and on Gmail; a message gone
between the listing and its details; an empty folder; Microsoft 365
repeated and reordered entries, a removed entry, a read-state change for an
unknown message, a rejected position and a rejected place of a first fill,
a removal met with another entry for a message in one page; a refusal the
server marks temporary of a batch's structures or texts; a text the service
no longer returns keeping the stored one (a store test); cancellation
reported within a second while the server stops answering.
