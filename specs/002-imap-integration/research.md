# IMAP Integration: Decisions after the Prototypes

**Revised**: 2026-09-18. This document replaces the earlier imap-next/blocking-job
design. Prototype findings below were supplied and verified by the maintainer
on 2026-09-16–17. They are accepted evidence and were not rerun during this
documentation revision. The prototype source was read as an implementation
example; Mailbag integration still needs its own checks.

## 1. Execution and platform services

**Decision:** One worker thread serves the selected account. It runs asynchronous
mail access on its own `glib::MainContext`. GIO owns sockets and TLS. GTK and
existing GOA observation stay on the application's main context.

**Evidence:** The prototypes completed synthetic-server scenarios and real iCloud
access with this combination. Inside Flatpak, p11-kit exposed 395 host trust
entries, while the runtime's file certificate set contained 146 certificates.
The tested file-based rustls/OpenSSL paths did not see host additions. Async GIO
operations require a running GLib context; a Tokio executor does not drive it.

**Rationale:** Use the platform services already shipped with GNOME. The transport
bridge implements futures-io traits over GIO and uses
`glib::thread_guard::ThreadGuard` to meet async-imap's Send bound. This is a
type-bound bridge, not permission to access or drop GIO streams on other threads.
Create, poll, close and destroy the session on its owning worker.

**Alternatives rejected:** No Tokio, async-std, rustls transport, spawn_blocking,
GIO pool job or custom certificate-file store. The permanent choice between a
thread per account and a shared pool is deferred; 002 has only one active account.

GIO retains its normal platform behavior. No Mailbag proxy resolver, proxy
settings UI or dedicated GNOME proxy acceptance is added in 002. Future network
monitoring is not a new mechanism in this feature.

## 2. IMAP and content dependencies

**Decision:** Use the maintained forks of async-imap and imap-proto, and
mail-parser for the entire message-content domain. The starting dependency
declarations are:

```toml
async-imap = { version = "=0.11.3", default-features = false, features = ["runtime-futures"] }
mail-parser = { version = "0.11.9", features = ["full_encoding"] }
log = { version = "0.4", features = ["max_level_off", "release_max_level_off"] }

[patch.crates-io]
async-imap = { git = "https://github.com/mitinand/async-imap", rev = "89badf82c3af2173c6d839481be7aa5825d3ba42" }
imap-proto = { git = "https://github.com/mitinand/imap-proto", rev = "caa2c81038d7674c46fb038f90ccf74034380c18" }
```

Runtime-independent futures traits/channels are glue, not another executor.
Use the existing GLib/GIO versions. Before implementation, check the declared
and resolved package versions, revision identities and feature graph against
this baseline. Record any proposed departure before changing it; this revision
does not claim a newly resolved Cargo.lock.

**Evidence and reason for changing the earlier recommendation:** async-imap has
production use in Delta Chat, and its parser accepted real iCloud replies that
imap-codec rejected. In particular, iCloud sent `* BYE` without human-readable
text. The earlier plan's strict parser was a compatibility cost on a real account.
Both candidates needed fixes; the async-imap forks now address the relevant
earlier objections, including lost ALERTs and runtime coupling.

**Alternatives rejected:** imap-next/imap-codec and its proposed local vendored
patch are removed. Separate application dependencies for base64, quoted_printable
and GIO CharsetConverter are also removed. A transitive decoder dependency used
inside a chosen library is not a second application-owned decoding path.

mail-parser 0.11.9 with full_encoding owns RFC 5322/MIME parsing, encoded words,
base64/QP and charset decoding, including its 41-codepage set and Asian encodings.
Do not call `body_text()` or `body_html()`: these convenience methods can convert
HTML to text. Inspect the parts and their actual type instead.

**Mandatory privacy choice:** Disable log levels at compile time in both build
profiles, even though the fork also redacts passwords. IMAP commands are traced
by the library. Do not print library error objects that can contain a raw response;
map them to a safe step/cause and show only deliberately selected server error
text in the UI.

## 3. Fork inventory and maintenance

| Fork | Pinned revision | Tag and base |
|---|---|---|
| [async-imap](https://github.com/mitinand/async-imap) | `3c4cdde1cd5f4cbf264fe7fa18426c57dce8974d` | `mailbag-2026-09-20`, based on upstream main |
| [imap-proto](https://github.com/mitinand/imap-proto) | `caa2c81038d7674c46fb038f90ccf74034380c18` | `mailbag-2026-09-17-4`, based on release 0.16.7 |

The [async-imap revision](https://github.com/mitinand/async-imap/commit/3c4cdde1cd5f4cbf264fe7fa18426c57dce8974d)
contains:

1. runtime-futures using futures-io without Tokio or async-std.
2. `Handle::wait_until(stop)`, allowing an external timer for IDLE.
3. Preservation of ALERTs during authentication and in tagged replies.
4. `Client::capabilities()` before authentication.
5. Password redaction in trace output.
6. CAPABILITY NO/BAD handling as an error rather than an empty capability set.
7. ALERTs of a rejected sign-in or command: a tagged NO/BAD with an ALERT code
   is forwarded like a successful one, and `Client::unsolicited_responses()`
   reads them after a failed sign-in. Added 2026-09-19 with the maintainer's
   approval, because the previous revision lost the ALERT of a rejected
   sign-in, for example a request for an application-specific password.
8. The code and text of a NO or BAD response in `Error::No` and `Error::Bad`
   (`StatusResponse`), including RFC 5530 codes such as `AUTHENTICATIONFAILED`
   and `UNAVAILABLE`, which imap-proto leaves in the text. The previous error
   held only a debug-formatted string.
9. A NO or BAD completion of FETCH, and a connection closed before the
   completion, reported as an error after the responses received before it.
   The previous revision ended the response stream silently, so a server's
   partial failure or a BYE looked like success.
10. LOGIN arguments sent as literals when a quoted string cannot carry them, such
    as a non-ASCII password.
11. The buffer-limit error as the typed `ResponseTooLarge` instead of a plain
    message, so it is recognized by type.
12. ALERT from the matching tagged completion of the FETCH stream, for OK,
    NO and BAD. The original command result is preserved and the ALERT is
    delivered once. The stream has its own completion handler, separate from
    `check_done_ok_from`.
13. `Fetch::has_flags()` so a missing FLAGS field does not clear the last
    received flags, while `FLAGS ()` does. The parsed attributes already preserve
    this distinction; Mailbag does not need another wire parser.
14. EXAMINE/SELECT succeeds only after its matching tagged OK. A connection
    closed before that completion returns `ConnectionLost`, instead of a
    mailbox with partial or default data that can falsely confirm an empty Inbox.
15. ALERT from untagged mailbox replies and tagged EXAMINE/SELECT completions,
    including NO and BAD, forwarded once without changing the command result.
16. Capability names and system flag names compared without regard to case, as
    [RFC 3501 section 9](https://tools.ietf.org/html/rfc3501#section-9) requires
    of atoms. The previous revision compared them literally, so a server
    answering `starttls` or `logindisabled` looked like one offering neither,
    and a message flagged `\seen` looked unread with a custom keyword.
17. Untagged responses during the AUTHENTICATE exchange passed over instead of
    ending it: an ALERT before the continuation request left client and server
    waiting for each other until the socket timeout, with no sign-in attempt.
18. An untagged NO or BAD during EXAMINE/SELECT treated as the warning
    [RFC 3501 section 7.1.2](https://tools.ietf.org/html/rfc3501#section-7.1.2)
    defines, leaving the outcome to the tagged completion, and an untagged BYE
    reported as the new `Error::Bye` with its code and text. The previous
    revision failed the command on a warning and lost the reason for a BYE.

Items 16 to 18 come from the external review of 2026-09-20 and are reproduced
by Mailbag's own tests against the scripted server.

Items 8–11 were added on 2026-09-19 after the maintainer's review of portion 3:
different IMAP servers must not cost the user the whole Inbox because of one
message, or hide the server's own reason for a failure.

Items 12–13 accompany the FETCH response corrections: split replies must retain
the last explicitly supplied flags, and an ALERT must remain available to
explain a later failure in the same load.

Items 14–15 fix the mailbox parser's own completion and notice handling. Mailbag
also collects pre-authentication CAPABILITY notices before handling its result,
so a command failure or missing sign-in method does not discard an ALERT.

The [imap-proto revision](https://github.com/mitinand/imap-proto/commit/caa2c81038d7674c46fb038f90ccf74034380c18)
contains:

1. An upstream fix for UTF-8/8-bit response text, exercised by a Dovecot greeting
   containing a dash.
2. An upstream fix for unknown FETCH attributes, including EMAILID/THREADID.
3. Limits of 32 BODYSTRUCTURE part levels and 16 extension-list levels.
   The unbounded parser previously overflowed the stack and terminated a process.
4. License texts inside the package directory, absent from the 0.16.7 package.

**Maintenance decision:** Cargo depends on full revisions, never branch heads.
The default branch in both forks is `mailbag`. Do not use GitHub's Sync fork
button or `gh repo sync` without explicitly targeting `--branch main`.
An update means rebuilding the branch from the chosen base, carrying the fixes,
running the fork tests, tagging the result, then updating the application revision
and lock/source manifests. Do not submit upstream PRs or issues for this work.

**Trade-off:** The project owns a small patch set and its regression checks.
This is accepted maintenance, not a claim that the dependencies are unmodified.

## 4. Loading, waiting and recovery

**Decision:** Keep the 100-message window and eager text acquisition, but remove
Mailbag's download-size counter and all per-part size rules. Use the dependency's
existing response ceiling. The prototype's async-imap buffer ceiling is 512 MiB;
this is a library constraint, not a new application setting or aggregate memory
guarantee. Large-literal and parser-depth cases are covered by dependency/transport
regression scenarios.

Use `GSocketClient::timeout`, initially 30 seconds, for connection, TLS and socket
I/O inactivity. GOA calls retain their existing finite D-Bus timeout. There is
no application watchdog, progress-clock state or absolute batch deadline.
A slowly continuing transfer can take longer than the inactivity interval.

**Evidence:** The prototypes exercised stalled reads, huge declared literals,
certificate failures and cancellation. Cancellation drops the loading future
and closes the connection; the interrupted IMAP session is not reused.

**Decision:** Fetch list fields and part structures in two commands, then group
text requests by their complete section/header request set. Per-message text requests cost about
0.23 seconds each in the maintainer's measurement, or roughly 25 seconds for
100 messages. Grouping removes those repeated round trips; it is not an elapsed
time guarantee for every server.

List fields never depend on BODYSTRUCTURE, so a structure the parser rejects
cannot remove a row. If the structure response cannot be parsed, a bounded
per-message fallback isolates the unreadable structure; that message keeps its
row and gets a content explanation, as production clients do. Any sender can
trigger this, because the fork's nesting limit is deterministic. The maintainer
chose this over skipping such messages with a warning count: it needs no
batch-level warning or exception to FR-003 and costs one extra round trip.
Do not turn transport failure into a content explanation or an automatic network
retry loop. The [acquisition contract](contracts/imap-reading.md) defines
session replacement for this fallback.

**Alternatives rejected:** A custom byte budget, broad retry policy, backoff timer,
body fetcher on opening, cross-account cache and incremental synchronization.

## 5. Content policy

**Decision:** Apply the prototype's MIME selection rules, detailed in
[the acquisition contract](contracts/imap-reading.md). Fetch the original MIME
header and selected body section together, then pass that MIME entity to
mail-parser. This includes HEADER for a single-part message and section.MIME
for a multipart leaf.

Decoding is permissive about characters: invalid bytes become replacement
characters and do not hide the whole body. Unknown charset or
Content-Transfer-Encoding gets an explicit content explanation. Unsupported
content types, encryption and absent plain text retain their existing
explanations. Do not restore strict base64/QP validators or a second charset
decoder.

It is not permissive about a transfer encoding that did not deliver the
content. mail-parser answers content it cannot decode with the still-encoded
body and its encoding-problem mark, and it drops the last characters of a
base64 payload that ends inside a group of four without any mark. Both cases
would show an unreadable payload or a silently shortened message as the text,
so both get the undecodable explanation: reading the mark, and counting the
base64 characters of the body, cost far less than a decoder of our own and
report the failure instead of hiding it.

The sample rules choose the last supported alternative, first signed/related
part, all appropriate mixed plain-text parts, and exclude attachments and nested
messages. `text/plain` with a name parameter and no explicit inline disposition
is treated as an attachment, including the RFC 2231 forms `name*` and `name*0*`
that a server may leave unfolded. There is no signature verification or decryption.

**Evidence:** The prototype includes 14 MIME samples. On the maintainer's real
mailbox, 42% of messages had only HTML. This is expected unsupported content in
002, not a load failure or a reason to introduce HTML conversion.

**UI choice:** GtkLabel shows at most the first 64 KiB of decoded body text,
without an explanation: this stage only shows that text was received. The stored
text is complete. This is a display limit only; it does not reinstate a download
budget.

## 6. UI and account ownership

**Decision:** Reuse the existing hidden `sync_button_list` as a loading-only
spinner. No widget replacement, done/warning icon, synchronization popover or
`app.sync-status` behavior. Refresh Inbox is the only way to load: it clears the
account's list and reader, and a failed load leaves the list empty while the
status page names the failed step. Selecting an account shows the mail received
for it in this run and never loads. No toast. The UI exists only to evaluate the
integration, so each temporary mechanism is kept to the minimum.

ALERT support is limited to including received ALERT text, together with the
server's reason for the failure (NO, BAD or BYE text and RFC 5530 code), in a
failure explanation.
It is plain text, not a separate notification, history or success-side UI.
This intentionally does not claim complete standalone ALERT presentation under
RFC 3501. The fork's ability to retain replies does not require a notification
subsystem in this stage.

AccountList/F01 remains the only owner of exclusion. A settings/password request
can return a failed step but cannot clear rows/mail or request an observer refresh
as a second exclusion path. The shared API was approved on 2026-09-19. F01
account errors keep their page priority.

## 7. Packaging and dependency policy

**Decision:** Replace build-time cargo vendor with a committed `cargo-sources.json`
generated from Cargo.lock. Use flatpak-cargo-generator from
[flatpak-builder-tools revision de2225a](https://github.com/flatpak/flatpak-builder-tools/commit/de2225a)
(2026-09-12, MIT). Pin the tool revision in tool-versions.env and install it with
aiohttp/tomlkit through setup.sh. Normalize the generated Cargo config filename
to config.toml. The [packaging contract](contracts/packaging.md) is authoritative
for generation, checks, build inputs and license installation.

**Evidence:** The maintainer built the prototype offline in 28 seconds and ran
all 19 scenarios and real GOA/iCloud access inside the sandbox. This establishes
the chosen packaging path; it does not mark the revised Mailbag package as tested.

**Decision:** The manifest owns CARGO_HOME and CARGO_NET_OFFLINE. Meson inherits
them and receives the vendor directory through an option; it never assigns
CARGO_HOME. Root vendor/ and its ignore entry are removed during implementation.
No vendored-source archive is added to releases.

Distribute ready binaries through the project's own Flatpak repository or bundle.
Users do not install Rust or compile the app. Keep the manifest compatible with
Flathub packaging requirements, but do not target submission now. The maintainer
cited Flathub's 2026-05-29 AI-participation disclosure/reviewer policy as the reason;
this revision does not independently reverify that policy or make a submission.

## 8. License decisions

Keep `cargo deny check licenses` as the license gate. Add BSD-3-Clause and
allow-git entries for the two named forks. Preserve their package license files.

| Package / expression | Chosen treatment |
|---|---|
| hashify: Apache-2.0 OR MIT | The package supplies both texts in a REUSE-style `LICENSES/` directory (`Apache-2.0.txt`, `MIT.txt`); install them as supplied notices. No fallback is needed. |
| stop-token: MIT OR Apache-2.0 | No supplied texts: preserve standard license texts with an explicit provenance explanation and authors from the resolved Cargo.toml; do not claim upstream supplied these files. |
| self_cell: Apache-2.0 OR GPL-2.0-only | Select Apache-2.0; do not select GPL-2.0-only for this GPL-3 project. |
| memchr: Unlicense OR MIT | Select MIT. |
| unicode-ident: (MIT OR Apache-2.0) AND Unicode-3.0 | Preserve the Unicode-3.0 obligation as well as the selected permissive license. |
| encoding_rs: (Apache-2.0 OR MIT) AND BSD-3-Clause | Add BSD-3-Clause to the allowed set and preserve its notice. |

Meson treats files named as license, licence, copying, copyright or notice
files, and every file in a crate's `LICENSES/` directory, as supplied notices.
hashify was first listed as missing texts because the name-based search did not
look inside `LICENSES/`; the maintainer approved recognizing that directory on
2026-09-19. The only Meson exception is keyed by the crate name stop-token. New
packages without license texts fail the build. Fallback notices live in
`third-party-notices/`, not the repository's REUSE `LICENSES/` directory.
Install per-crate notices below `share/licenses/io.github.mitinand.Mailbag`.

## 9. Crate layout

**Decision:** Add two workspace crates. `mailbag-imap` owns the protocol: GIO
transport, the IMAP session and commands, and a typed BODYSTRUCTURE projection
with section paths. `mailbag-content` owns text-part selection and decoding,
including header display fields, through mail-parser. Neither depends on the
other. The worker and the load sequence that joins them stay in `mailbag` until
a scheduling layer exists.

| Crate | May depend on | Must not depend on |
|---|---|---|
| `mailbag` | gtk, libadwaita, glib, gio and the crates below | — |
| `goa-adapter` | glib, gio | gtk, libadwaita |
| `mailbag-imap` | glib, gio, async-imap, imap-proto | gtk, libadwaita, mail-parser, `mailbag-content` |
| `mailbag-content` | mail-parser | gtk, libadwaita, glib, gio, `mailbag-imap` |

No crate depends on `mailbag`. The rules are the dependency lists themselves, so
a forbidden call does not compile; `scripts/check.sh` inspects `cargo tree` so a
new dependency cannot add a forbidden edge quietly. The load sequence in
`mailbag` is the one place the compiler cannot guard: review keeps widgets out
of it.

**Why crates rather than modules:** A crate is the only privacy boundary the
compiler enforces. Inside one crate, every module can reach `pub(crate)` items
and every declared dependency, so a layering held only by discipline drifts.

**Why the protocol has no notion of a provider:** The maintainer's earlier Python
client supported generic IMAP, Gmail and Microsoft Graph. Gmail there was a
boolean passed through the shared IMAP path; it reached transport capability
policy, identity rules, the membership model and SQL. The differences between
Gmail and generic IMAP fall into cheap wire syntax (`X-GM-EXT-1`, `X-GM-MSGID`,
`X-GM-THRID`, `X-GM-LABELS`, `X-GM-RAW`) and semantics that cannot be shared:
labels presented as folders, All Mail as the store, `\Deleted` with EXPUNGE
depending on an account setting, deletion proof across labels and identity that
survives UIDVALIDITY resets
([Gmail IMAP extensions](https://developers.google.com/workspace/gmail/imap/imap-extensions),
[imapsync Gmail notes](https://imapsync.lamiral.info/FAQ.d/FAQ.Gmail.txt)).
`mailbag-imap` therefore reports advertised capabilities and fetches the
attributes it is asked for. A future provider difference is a separate type in
a semantic layer above it, never a flag passed down.

**Why content does not depend on the protocol:** Selection works on a MIME part
description, not an IMAP structure; a downloaded full message or another
provider's MIME content yields the same shape through mail-parser. The load
sequence converts the `mailbag-imap` projection into that description. Both trees
have the same shape, so a selected part's path is its IMAP section number.

**Deferred:** No provider trait, factory or provider flag, and no crates for
storage, scheduling or shared domain types. An interface derived from a single
implementation would encode IMAP's vocabulary (UID, UIDVALIDITY, flags), which
Graph cannot satisfy. Such crates are added when they have code to hold.

## 10. GOA encryption flags

**Decision:** Read the Mail interface flags literally. `ImapUseSsl` selects
implicit TLS; otherwise `ImapUseTls` selects STARTTLS. When both are false the
account has no encryption configured: the settings request fails with that
explanation before the password is requested or a socket is opened (US3-6).

**Evidence:** GOA's account dialog offers only "STARTTLS after connecting" and
"SSL on a dedicated port" and saves exactly one of the two flags as true; a
stored "none" is displayed as SSL (gnome-online-accounts 3.56,
`goaimapsmtpprovider.c`). Both-false settings therefore come from older versions
or edited configuration, not a deliberate choice. GOA substitutes SSL for this
case inside its own credential check (`get_tls_type_from_object`, checked in
3.58.1), but the D-Bus Mail properties export the stored values.

**Alternatives rejected:** Substituting implicit TLS contradicts the stored
setting and depends on a GOA implementation detail. Attempting STARTTLS would
also keep cleartext impossible; refusal was kept because it reports the actual
setting without guessing.

## 11. Evidence retained for future work only

Microsoft 365 through the tested GOA provider exposes Graph, with
`ImapSupported=false` and Graph-only scopes. The accepted future HTTP choice is
libsoup 3, already in the runtime and using the same platform TLS/proxy services.
Do not add a Graph backend or libsoup dependency in 002.

IDLE can legitimately remain silent for up to 29 minutes. The prototype needed
the socket timeout disabled during IDLE, with `Handle::wait_until` and a GLib
timer, then restored for active I/O. iCloud advertised IDLE only after login;
always refresh capabilities after authentication. IDLE watches one selected
mailbox and does not replace later polling/synchronization design. No IDLE,
polling, network monitor or background refresh is implemented in 002.
