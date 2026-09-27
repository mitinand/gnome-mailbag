# Quickstart: Folders

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
store (`~/.var/app/io.github.mitinand.Mailbag/data/mailbag/mail.sqlite`) is
discarded at the first start after the structure changed (007 FR-012).

| Step | How | Expected |
|---|---|---|
| 1. Folder lists (US1, US2) | Select each account; choose Refresh Account | The account becomes a heading; the system folders first in the spec's order with their icons and their server names; the user's folders below as a tree in the locale's order; nothing selected |
| 2. Names (FR-005) | On an account whose folders have non-Latin names | Readable names, not `&…-` sequences |
| 3. Gmail (US4) | Refresh Account on a Gmail account | No container row; the system labels at the top; a message in two loaded labels is in both folders once |
| 4. A mailbox (US1) | Select a folder; Refresh Mailbox; open a message; select another folder and come back | "No mail loaded" before the refresh; the newest messages after; the other folder unchanged |
| 5. Nesting (US3) | Expand a folder with children; collapse the parent of the shown folder | Children indented one step per level; collapsing clears the selection and the list asks to select a mailbox |
| 6. The server changes (US5) | In the web interface: create, rename and delete a folder; Refresh Account | The new folder appears, the deleted one is gone, the renamed one is a new unloaded folder (Microsoft 365: the same folder with its mail); a shown deleted folder clears the selection |
| 7. Failures (US7) | Network off; Refresh Account, then Refresh Mailbox | The failure page or the banner by 006, wording that speaks of the mailbox and the folder list; Retry repeats the right action; stored folders and rows unchanged |
| 8. Restart offline (US1) | Quit; network off; start | The same folders and rows; no load |
| 9. The record (003) | Start with `--log-level=debug`, Refresh Account and Refresh Mailbox | Folder names at debug only; no subject, sender, text or credential |

Cases the automated tests cover with scripted servers: a cut or refused
LIST, a name with a quote and a backslash, two folders with one role, a
folder with two marks, a missing well-known folder on Microsoft 365, a
second listing page, an empty folder list.
