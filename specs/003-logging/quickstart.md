# Quickstart: checking Logging

How to see that the feature works. Automated checks run in
`scripts/check.sh`; the manual ones need a Generic IMAP account in GNOME
Online Accounts, a test server that can succeed and refuse a load, and the
installed Flatpak build. The final README trial also needs a person who did
not participate in the design. This file holds reusable procedures only.
Record results, participants and unverified items in the handoff message or
an artifact outside the repository; do not append validation results here.

## Automated

| Criterion | Check |
|---|---|
| SC-001 | A test that no subscriber is installed without the option. Real stdout, stderr and newly created files are checked for both a successful and a failed load below. The same absence is how "negligible cost when off" (FR-016) is checked; there is no benchmark |
| SC-002 | Loads against the scripted IMAP server with marker strings in its fixtures (password, sign-in name, Online Accounts identifier, folder, host, subject, address, file name, body text, an attached message's subject, a refusal that repeats a two-character sign-in name), at info and at debug; the file name may appear at neither level; the buffer is searched for each marker |
| SC-003 / FR-011 | One fixture per defect (unknown character set, unknown transfer encoding, undecodable part, unreadable structure, text not returned, list header with no value); assert folder, UID, step and known cause, header name and shape. For parts with a file name, in Content-Type, in Content-Disposition alone and in continued form, assert that `file_name_params` names the parameters and that no name value appears |
| SC-004 | Each applicable failed load of the 002 scenarios: one ERROR whose `step` and `cause` name the failure the UI explains, with total duration. Exclusion and quitting during access or transfer: one cancellation INFO with reason and duration before any completion callback, no WARN/ERROR or duplicate acknowledgement. Discarded late batches/failures, including after re-addition, do not produce completion/WARN/ERROR. Refused list and unreadable content: one WARN each |
| SC-005 / FR-008 | Equal INFO counts for 1 and 100 ordinary messages; reconnection adds its connection sequence. The final line of a finished, failed and cancelled load, of a successful and a failed account read and of a successful and a failed Settings launch has a numeric nonnegative `duration_ms`; no other line has one |
| SC-006 | Every line of a load has the load's label and identifier, including the lines written in main-thread callbacks (settings received, result), while an account update arrives during the load |
| SC-008 | A writer that fails every write: the load finishes and nothing panics. The unread pipe is a manual check below |
| Escaping | A folder name and a server sentence with line breaks, quotes and a NUL, passed as string fields; one line per event. `scripts/check.sh` rejects `%` inside event macros |
| Option | Unknown level: message and status 1. Parsing of the four levels |
| Account label | `account_label` numbers accounts by first appearance, repeats the number for a known account, gives the same numbers for the same set of accounts, and never contains the identifier's text, generated or arbitrary |

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

For SC-001, take a file inventory before each application run and compare it
after quitting: the working directory and Mailbag's application-specific
locations under the user's data, config, cache and state directories. Keep
the inventories and stream captures in `logging_check_dir`, outside the
repository; these shell-created files are not files created by Mailbag.

After the first inventory, start without the option, refresh the test Inbox
successfully and quit:

```bash
./target/debug/mailbag > "$logging_check_dir/success.stdout" 2> "$logging_check_dir/success.stderr"
```

Compare the inventory. Then make the test server refuse sign-in, take a new
inventory, and run without the option again:

```bash
./target/debug/mailbag > "$logging_check_dir/failure.stdout" 2> "$logging_check_dir/failure.stderr"
```

Refresh, confirm that the UI shows the failure, and quit. Inspect all four
capture files: zero Mailbag lines in stdout and stderr on both runs. GTK and
GLib warnings may be present unchanged. The before/after inventories must
show no new file created by Mailbag. If either the failure scenario or the
file-creation check could not be verified, leave that part of SC-001
unverified. Restore the test server's successful response for the next runs.

```bash
cargo run --locked -- --log-level=info
```

Expected: the first line with versions and `level info`; account list read;
after Refresh Inbox the steps with their counts and the load's total duration; no folder, host, UID
or address anywhere.

```bash
cargo run --locked -- --log-level=debug 2> "$logging_check_dir/debug.log"
```

Open two messages. Expected in `debug.log`: a line with each opened
message's UID, part trees one line per part, host and port, no subject,
address, sign-in name or attachment file name.

With the test server holding an active load, quit once through the Quit
action and in a separate run by closing the window. Repeat while Online
Accounts access is pending where the fixture permits it. Expected: one
`quitting` cancellation line with load context and duration before the quit
line, no warning or error for cancellation, and no wait for a worker reply.
These event checks belong to portion 3; portion 1 checks only the first line and the quit line.

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
finishes without a visible delay. Then leave `less` unscrolled and refresh
several times: once the pipe is full, Mailbag waits; scrolling `less` lets it
continue, no line is missing and no load has failed. This wait is the
accepted behavior for a stream nobody reads (spec, Clarifications).

## Manual, installed Flatpak: implementer checks

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

## Independent README trial (SC-007)

The maintainer arranges a participant who did not take part in the design.
Give them the installed application and the README only, with a test account
ready. Have them obtain the record, reproduce a problem, find the output
file and repeat the command while Mailbag is already running. Observe whether
the README suffices, including its privacy advice, without explaining the
procedure from this directory.

Record their result separately from the implementer's Flatpak checks,
outside the repository. If no such participant is available, report the
independent trial as unverified and keep SC-007 and T041 incomplete. The
implementer's own run does not stand in for it.
