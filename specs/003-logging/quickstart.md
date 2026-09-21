# Quickstart: checking Logging

How to see that the feature works. Automated checks run in
`scripts/check.sh`; the manual ones need a Generic IMAP account in GNOME
Online Accounts, a test server that can succeed and refuse a load, and the
installed Flatpak build. This file holds reusable procedures only.
Record results and unverified items in the handoff message or
an artifact outside the repository; do not append validation results here.

## Automated

| Criterion | Check |
|---|---|
| SC-001 | A test that no subscriber is installed without the option; one start without it below shows empty streams. The same absence is how "negligible cost when off" (FR-016) is checked; there is no benchmark |
| SC-002 | Loads against the scripted IMAP server with marker strings in its fixtures (password, sign-in name, folder, host, subject, address, file name, body text, an attached message's subject, a refusal that repeats a two-character sign-in name), at info and at debug; the file name may appear at neither level; the buffer is searched for each marker |
| SC-004 | Each applicable failed load of the 002 scenarios: one ERROR whose `cause` is the failure value the UI explains. Exclusion and closing the window during access or transfer: one cancellation INFO with its reason, no WARN/ERROR or duplicate acknowledgement. Discarded late batches/failures do not produce completion/WARN/ERROR. Refused list and unreadable content: one WARN each |
| SC-008 | A writer that fails every write: the load finishes and nothing panics. The unread pipe is a manual check below |
| Escaping | An account identifier and a server sentence with line breaks, quotes and a NUL, passed as string fields; one line per event. Clippy, run by `scripts/check.sh`, rejects `%` inside event macros |
| Option | Unknown level: message and status 1. Parsing of the four levels |
| Connection | A connection that cannot be made still leaves a debug line with the host and port |

`scripts/check.sh` additionally fails when `tracing-log` appears in the
dependency tree or when `mailbag-imap`'s `log` dependency loses
`max_level_off`.

## Manual, native build

Run from the repository root. Build before capturing the application so
Cargo's own output is not confused with Mailbag's streams:

```bash
cargo build --locked
logging_check_dir=$(mktemp -d)
```

For SC-001, start without the option and quit, keeping the streams outside
the repository:

```bash
./target/debug/mailbag > "$logging_check_dir/stdout" 2> "$logging_check_dir/stderr"
```

Expected: no Mailbag line in either file. GTK and GLib warnings may be present
unchanged.

```bash
cargo run --locked -- --log-level=info
```

Expected: the first line with versions and `level info`; account list read;
after Refresh Inbox the steps with their counts; no host, folder or UID
anywhere.

```bash
cargo run --locked -- --log-level=debug 2> "$logging_check_dir/debug.log"
```

Open two messages. Expected in `debug.log`: a line with each opened
message's UID, part trees one line per part, the host and port of the
connection, no subject, address, sign-in name or attachment file name. With a
wrong host in Online Accounts, the `connecting` line still names that host and
the load fails with one ERROR line.

With the test server holding an active load, close the window. Expected: one
`quitting` cancellation line with the account before the quit line, no warning
or error for cancellation, and no wait for a worker reply. In a separate run
quit through the Quit action instead: the load's start and then the quit line,
with no cancellation line.

```bash
cargo run --locked -- --log-level=debg
```

Expected: the message naming the four levels, exit status 1, no window.

With Mailbag running, start it again with `--log-level=debug`. Expected: the
"already running" message, status 1, the running window unchanged.

```bash
cargo run --locked -- --log-level=debug 2>&1 | less
```

Scroll `less` to its end and keep it there. Refresh. Expected: the load
finishes without a visible delay (SC-008).

## Manual, installed Flatpak (SC-007)

Use a temporary host directory outside the repository and follow the README's
Flatpak command:

```bash
flatpak run io.github.mitinand.Mailbag --log-level=debug 2> mailbag.log
```

Expected: `mailbag.log` appears in the current host directory; its first
Mailbag line says `Flatpak build` and names the GNOME runtime. Reproduce a
load and check its lines. While Mailbag is still running, repeat the command:
the captured explanation says logging was not turned on, the exit status is
1, and the running window is not activated. Verify with `flatpak info
--show-permissions io.github.mitinand.Mailbag` that permissions are unchanged
from 002. GTK and GLib warnings may share the file unchanged.
