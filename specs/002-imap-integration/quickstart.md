# IMAP Integration: Validation Quickstart

This guide applies after implementation. New scripts, test filters and fixture
interfaces below are **planned**, not already passing tests. The maintainer's
2026-09-16–17 prototype results are accepted design evidence; do not rerun those
experiments merely to confirm the stack choice. Mailbag still needs checks for
its own integration.

This documentation revision does not build or run Mailbag, change host trust
or accounts, or contact a real mailbox. Before implementation, verify dependency
versions and fork revisions against [research](research.md#2-imap-and-content-dependencies).

## Development and packaging

Use the existing [setup guide](../../README.md). After portion 1, the workflow is:

```bash
./scripts/setup.sh
./scripts/generate-cargo-sources.sh --check
./scripts/check.sh
./scripts/build-flatpak.sh
```

Regenerate cargo-sources.json after an intentional Cargo.lock change and review
both together. The check must reject a stale manifest without silently replacing
it. Check that the two fork revisions are present, the required crate features
are enabled and alternate runtimes/TLS stacks are absent.

Source preparation may download dependencies; compilation inside the Flatpak
build sandbox runs without network access and with Cargo offline. Inspect the
installed notices and missing-notice failure described in
[the packaging contract](contracts/packaging.md#portion-1-acceptance).
No cargo vendor step, root vendor input or release source bundle is required.

## Rust/GIO fixtures

Portion 3 adds a scripted IMAP server to `mailbag-imap`, available to its own
tests and, through the `test-support` feature, to `mailbag`. It runs on GIO with a GLib context and loopback
sockets; it is not a Python server or production backend. Port the relevant
prototype scenarios and the 20 synthetic MIME samples (`tests/fixtures/mime/`), then add Mailbag-specific
batch/grouping/publication cases.

Generate disposable certificates with `tools/make-certs.sh` using OpenSSL.
This script writes test files under target/test-certs and never installs a CA.
Automated fixture setup generates the needed files when absent; missing tools
fail with an actionable message rather than skipping security tests. Add the
OpenSSL tool prerequisite to developer/CI test setup. Private tests may use a
test trust database; no production CA-path option or bypass is added.

After the modules are implemented:

```bash
cargo test --locked -p goa-adapter
cargo test --locked -p mailbag-imap
cargo test --locked -p mailbag-content
cargo test --locked -p mailbag inbox
./scripts/check.sh
git diff --check
```

Ensure each filter runs real tests. Use shorter injected socket timeouts and
outer test deadlines where necessary; there is no injected application size
budget. Bind dynamic loopback ports for automated tests. Server assertions store
command names, UIDs, section identifiers and credential-transmission counts,
never raw credentials or personal mail.

The prototype has 19 scenarios, including two IDLE scenarios. IDLE is outside
002: retain that evidence in research, without adding an IDLE implementation to
make all 19 names appear in Mailbag. Port the other relevant cases: implicit TLS,
STARTTLS and its absence/injection, PREAUTH before TLS, sign-in rejection with
UTF-8 response text, ALERT, UTF-8 greeting, stall, huge literal, cancellation,
deep structures and certificate failures.

Use the selected fork's parser-depth and compatibility coverage. A huge literal
must end as a library response failure without allocating its declared payload.
Deep BODYSTRUCTURE tests must show that the application process survives.
Do not introduce another application depth/size policy or a vendored codec patch.

## Automated acceptance map

| Criterion | Evidence |
|---|---|
| SC-001 — Batch | Stable 0/1/100/101-message Inboxes yield 0/1/100/100 unique rows. Descending UID order follows addition. |
| SC-002 — Acquisition/content | EXAMINE and exact selected BODY.PEEK sections; no mutation commands or changed flags. HTML, inline images, signatures, text attachments and nested messages are never fetched as payload. Opening sends nothing. |
| SC-003 — Refresh | Selection never loads. Refresh clears rows and reader, then loads; a failure leaves the list empty and names its step; refreshing again recovers. A message with an unreadable structure keeps its row and shows an explanation. A list the server refused after answering for part of it keeps those rows and names the reason in a toast, and is never shown as complete. |
| SC-004 — Privacy/storage | Synthetic markers never enter diagnostics; command tracing stays compiled out in debug/release. Permanent: no application password files. This stage: no application mail files or restoration after restart. |
| SC-005 — Ownership | Switch accounts during GOA access, connect and text transfer: the result is stored only for its own account. No overlapping acquisitions. Confirmed exclusion during a load discards the account's mail, and the late result does not restore it. |
| SC-006 — Access/responsiveness | F01 accessibility remains intact; Refresh Inbox and rows work by keyboard. Stalled loading permits navigation/quit; failures identify their step and differ from unsupported content. |
| SC-007 — Security/permissions | Both verified TLS modes and the certificate, STARTTLS and false/false cases on the host build, where the disposable CA can be trusted; no certificate override and no password in a failing case. In the installed application: a load from a real server, whose certificate the runtime's own authorities verify, and only the permitted network addition. |

Additional fixtures exercise the changed integration directly:

- Request grouping: one row command and one structure command; one text command per complete
  section/header request set. A single-part root and multipart leaf at section 1
  must not share the wrong header request.
- Unreadable BODYSTRUCTURE: the structure response fails to parse; isolation
  gives only the bad UID an explanation and keeps its row; later good UIDs are
  still received. Test several bad messages, including every message bad.
- Error origins: transport errors, timeout, incomplete literal and library
  response ceiling never enter the structure-isolation path. No failed parser
  session is reused. UIDVALIDITY change on fallback reconnect stops the attempt.
- Per-message results: a message without a response after a tagged OK
  disappeared and is omitted; a structure or text missing after a tagged NO, and
  NIL or a missing section in a returned response, keep the row with an
  explanation while the other messages load; a broken transfer fails the load.
- Authentication: PLAIN including non-ASCII credentials; LOGIN fallback with
  non-ASCII credentials sent as literals; LOGINDISABLED; no alternate method
  after rejection; the server's text and RFC 5530 code of a rejection, such as
  AUTHENTICATIONFAILED or UNAVAILABLE.
- MIME: UTF-8/Windows-1251/KOI8-R, Asian encoding through full_encoding, base64/QP,
  invalid bytes with replacement characters, unknown charset/transfer encoding,
  encoded Subject/From/To, multiple mixed plain parts, a later plain part without
  disposition, name-without-inline, signed, encrypted, related with and without
  a start parameter, flowed text with and without delsp, HTML-only and nested
  message/rfc822.
- ALERT: plain text included in a failing attempt's explanation, including
  authentication/tagged failures; no extra notification/history for success.
- Timeout/cancellation: stalled versus slowly progressing input, cancellation
  while a read is pending, connection closure and no queued duplicate Refresh.

## Installed-app fixture and host trust

Provide an ignored Rust test entry point for running the same GIO server during
manual acceptance, without adding an application server binary. Planned use:

```bash
./tools/make-certs.sh
MAILBAG_IMAP_SCENARIO=basic-101 cargo test --locked -p mailbag-imap serve_fixture -- --ignored --nocapture
```

The entry point prints its loopback endpoint and scenario and runs until stopped.
Use ports 1993/1143 for this manual mode, or report an explicit bind failure.
It accepts disposable test credentials in memory and supports the authentication
needed by GOA and the command subset used by Mailbag. It must not print passwords,
literal contents or real account data. Automated tests invoke the server directly.

`MAILBAG_IMAP_SCENARIO` chooses `basic-<message count>` or `long-text`, whose
three bodies of 65,535, 65,536 and 65,537 UTF-8 bytes test the display
boundary. `MAILBAG_IMAP_CERTIFICATE` selects `localhost`, `unknown-ca`,
`wrong-host` or `expired`, and `MAILBAG_IMAP_STARTTLS` selects `offered`,
`not-offered`, `rejected`, `inject` or `preauth`. While it runs, the server
prints its connections, sign-ins carrying credentials and command names, so a
refused connection shows that it sent no password and no plaintext sign-in.

Two ignored tests connect to that running server through the host's own trust
store, each by its own filter so that no other test replaces the trust database:

```bash
MAILBAG_IMAP_EXPECT=rejected cargo test --locked -p mailbag-imap host_trust -- --ignored --nocapture
MAILBAG_TEST_ACCOUNT_ID=account_… MAILBAG_IMAP_EXPECT=success \
  cargo test --locked -p mailbag online_accounts -- --ignored --nocapture
```

The first checks the transport alone; the second runs the whole chain, taking
the settings and password from the disposable Online Accounts account, and
accepts `success`, `rejected` or `no-encryption`. They replace repeating the
certificate, STARTTLS and false/false cases by hand in the interface; the
installed application still has to load, open and refresh mail once.

For SC-007, use a disposable CA trusted by the host and a matching localhost
certificate, then configure a Generic IMAP account in Online Accounts. Leave
SMTP unused. The test certificate script creates trusted-case, unknown-CA,
wrong-host and expired variants. Installing/removing host trust is a separate
maintainer setup action; it is not performed by the tests or this plan.

On Fedora, after reviewing the generated test CA, the maintainer can install it:

```bash
sudo cp target/test-certs/ca.pem /etc/pki/ca-trust/source/anchors/mailbag-imap-002.pem
sudo update-ca-trust
```

Use only the disposable account for state inspection; do not remove or alter
personal accounts. The prototype's real iCloud/GOA access is existing evidence,
not proof that this host has no Generic IMAP account. Record actual GOA, GLib,
GnuTLS and Flatpak versions for Mailbag's installed acceptance.

After acceptance, remove the disposable account and only its test anchor:

```bash
sudo rm /etc/pki/ca-trust/source/anchors/mailbag-imap-002.pem
sudo update-ca-trust
```

## Visible integration and final acceptance

After portion 5, in the normal graphical session:

```bash
cargo test --locked -p mailbag mail_ui_transitions -- --ignored --test-threads=1
cargo run --locked
./scripts/build-flatpak.sh --install
flatpak info --user --show-permissions io.github.mitinand.Mailbag
flatpak run io.github.mitinand.Mailbag
```

Each graphical test runs in its own process, because GTK is initialized once
per process; run `mail_ui_transitions` and F01's `account_ui_transitions` by
their own filters rather than together.

The graphical filter checks existing row/reader binding, page
priority, selection without loading, refresh clearing and loading, a failed load
and a repeated refresh, disabled busy Refresh and spinner visibility only while a
load runs. No synchronization popover or success/warning control.

For SC-006, check F01 accessibility for regressions and the new keyboard paths:
activate Refresh Inbox; select/open message rows; confirm the row exposes its
read/unread state. Navigate and quit during a stalled request. Do not turn this
into a repeat of the full input-method/narrow-width matrix. Preserve the approved
adaptive forms.

Test decoded bodies of 65,535/65,536/65,537 UTF-8 bytes, including a long line and
complex Unicode. The reader stays usable and shows at most the first 64 KiB, cut
at a character boundary, without an explanation. Verify headers/ALERT text are
inert. If the threshold makes the reader unusable in Mailbag, bring back the
result for a decision.

Complete both TLS modes against the host-trusted CA, then unknown CA, wrong
hostname and expiry, including GOA's certificate-exception setting, on the host
build: Flatpak gives the sandbox the runtime's own certificate authorities and
reserves `/etc`, so a disposable CA installed on the host never reaches the
installed application (see the scope note in [spec.md](spec.md#assumptions)).
Mailbag must transmit zero passwords in failing TLS cases. Reset the fixture's
credential counters after GOA account setup: GOA itself can contact the server.
The installed application shows instead that a real server with a publicly
trusted certificate loads, which exercises the same transport.

Test false/false settings through the private GOA fixture: the load stops with
the encryption-setting explanation, the password is never requested and the
server sees no connection. Test STARTTLS absence/rejection/handshake failure,
injected pre-TLS bytes and PREAUTH before TLS: no plaintext sign-in or downgrade.

Inspect installed permissions against F01: network is the only addition. Stop
the server after loading, then open/reopen supported received messages without
requests. Quit and restart with the server stopped; no mail is restored.
Inspect app state separately from GOA's secret store for the two SC-004 guarantees.

Report actual commands, results, version/revision identities and remaining gaps
at the relevant portion's review pause. Keep private logs/screenshots outside
the repository and remove personal data. These future checks are not satisfied
by the earlier prototype run or by document consistency checks.
