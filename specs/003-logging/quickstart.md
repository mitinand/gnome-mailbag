# Quickstart: checking Logging

How to see that the feature works. Automated checks run in
`scripts/check.sh`; the manual ones need a Generic IMAP account in GNOME
Online Accounts and, for the last section, the installed Flatpak build. None
of this has been run yet; results are recorded when each portion is reviewed.

## Automated

| Criterion | Check |
|---|---|
| SC-001 | A test that no subscriber is installed and no writer thread exists without the option. The proof on real streams is the first manual run below |
| SC-002 | Loads against the scripted IMAP server with marker strings in its fixtures (password, sign-in name, folder, host, subject, address, file name, body text, an attached message's subject, a refusal that repeats a two-character sign-in name), at info and at debug; the file name may appear at neither level; the buffer is searched for each marker |
| SC-003 | One fixture per defect (unknown character set, unknown transfer encoding, undecodable part, unreadable structure, text not returned, list header with no value); assert the fields of the debug lines: folder, UID, step and cause where the code knows one, header name and shape for the header case |
| SC-004 | Each failing load of the 002 scenarios: exactly one ERROR line, its `step` and `cause` equal to the UI's explanation; cancelled loads: no WARN or ERROR; refused list and unreadable content: one WARN each |
| SC-005 | Count INFO lines for Inboxes of 1 and 100 ordinary messages; equal. A scenario with one unreadable structure adds only connection lines |
| SC-006 | Every line inside a load has the load's label and identifier while an account update arrives during the load |
| SC-008 | A writer that blocks until released: the load finishes, then the buffer contains the lost-lines line with the right number; a writer that fails every write: nothing panics, the count grows |
| Escaping | A folder name and a server sentence with line breaks, quotes and a NUL, passed as string fields; one line per event. `scripts/check.sh` rejects `%` inside event macros |
| Option | Unknown level: message and status 1. Parsing of the four levels |

`scripts/check.sh` additionally fails when `tracing-log` appears in the
dependency tree or when `mailbag-imap`'s `log` dependency loses
`max_level_off`.

## Manual, native build

```bash
cargo run --locked
```

Refresh an Inbox, quit. Expected: no Mailbag lines in the terminal.

```bash
cargo run --locked -- --log-level=info
```

Expected: the first line with versions and `level info`; account list read;
after Refresh Inbox the steps with durations and counts; no folder, host, UID
or address anywhere.

```bash
cargo run --locked -- --log-level=debug 2> mailbag.log
```

Open two messages. Expected in `mailbag.log`: a line with each opened
message's UID, part trees one line per part, host and port, no subject,
address, sign-in name or attachment file name.

```bash
cargo run --locked -- --log-level=debg
```

Expected: the message naming the four levels, exit status 1, no window.

With Mailbag running, start it again with `--log-level=debug`. Expected: the
"already running" message, status 1, the running window unchanged.

```bash
cargo run --locked -- --log-level=debug 2>&1 | less
```

Do not scroll `less`. Refresh several times. Expected: the window stays
usable and loads finish. Scroll to the end: a lost-lines line appears if the
queue overflowed.

## Manual, installed Flatpak (SC-007)

Follow the README section as written, as a person who has not seen this
directory:

```bash
flatpak run io.github.mitinand.Mailbag --log-level=debug 2> mailbag.log
```

Expected: `mailbag.log` appears in the current host directory; its first
Mailbag line says `Flatpak build` and names the GNOME runtime; the manifest's
permissions are unchanged from 002.
