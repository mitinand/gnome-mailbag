# IMAP Acquisition Contract

One selected account is loaded asynchronously on a worker's GLib MainContext.
GTK and GOA observation remain on the main context. The worker owns all GIO
streams and IMAP sessions. There is no IDLE, polling or general reconnect policy.
Dependency choices and prototype evidence are in [research](../research.md).
`mailbag-imap` implements the transport, session and acquisition sections below;
`mailbag-content` implements text-section selection and decoding; the load
sequence in `mailbag` joins them and publishes the batch
([crate layout](../research.md#9-crate-layout)).

## Execution, waiting and cancellation

Create one worker when mail access is first needed and reuse it for the selected
account's attempts. It runs a GLib context and asynchronous futures, not a blocking
job in a pool. Send owned access data, cancellation and completion through
runtime-independent channels. Never move live GIO streams to the UI thread.

Adapt GIO's asynchronous read/write stream to futures-io with
`glib::thread_guard::ThreadGuard`, following the prototype. The wrapper satisfies
async-imap's Send requirement; it does not authorize cross-thread use. Construct,
poll, close and drop the wrapped objects on their owner thread. Keep unsafe_code
forbidden; do not copy the prototype's comparison framework for two IMAP clients.

Use `gio::SocketClient` with `timeout = 30` seconds. Its socket timeout covers
connection, TLS and I/O inactivity. GOA uses its own finite D-Bus call timeout.
No application byte budget, per-part limit, progress watchdog, total-duration
deadline or additional timeout thread. The library response ceiling remains in
effect. Continuing input may make an attempt take longer than 30 seconds.

Cancellation happens only on confirmed exclusion and on quit; switching accounts
does not cancel a load. On the worker, drop the pending acquisition future and
close the retained underlying connection, including on failure; never reuse an
interrupted session. The load counts as running until the connection has closed.
Quit cancels work without blocking GTK on a thread join. No command queue.

## Secure session

| Step | Behavior |
|---|---|
| Address | `gio::NetworkAddress::parse` on GOA's host string, preserving an explicit port. Default 993 for implicit TLS and 143 for STARTTLS. Use the parsed hostname as TLS identity. |
| Transport | `gio::SocketClient`; leave normal GIO platform behavior in place. No application proxy resolver/settings integration. |
| TLS | `gio::TlsClientConnection` with the default certificate database. Never connect an accept-certificate handler; ignore ImapAcceptSslErrors. |
| Implicit TLS | Finish the verified handshake before reading the IMAP greeting. |
| STARTTLS | Read the greeting, reject PREAUTH, get capabilities, require STARTTLS and wait for tagged OK. Discard the plaintext client's parser buffers and capabilities, then wrap the same socket in TLS. |
| After STARTTLS | Create a fresh async-imap client over the verified TLS stream. Do not expect a second greeting. Read capabilities again through TLS. |
| Sign-in | Prefer AUTHENTICATE PLAIN if advertised; otherwise LOGIN only without LOGINDISABLED. A rejected attempt does not trigger another authentication method. |
| After sign-in | Read capabilities again. iCloud did not advertise IDLE until authenticated; do not retain the pre-login set as the session's capabilities. |
| Inbox | EXAMINE INBOX; obtain UIDVALIDITY and EXISTS. Never SELECT. |
| Finish | Close the connection after the batch. No retained idle connection and no mail-changing CLOSE/EXPUNGE/STORE/COPY/MOVE commands. |

A missing/rejected STARTTLS command, handshake error or invalid certificate ends
the attempt before password transmission. No cleartext fallback, second insecure
connection or trust exception. Bytes buffered before STARTTLS must never be
interpreted as authenticated TLS replies. PREAUTH before TLS is a failure, not
permission to bypass encryption. PREAUTH received after verified implicit TLS
may proceed as authenticated without sending a password.

AUTHENTICATE PLAIN supplies raw `NUL + login + NUL + password` to the library's
authenticator; async-imap owns base64 framing. Do not add an application base64
dependency or resend credentials on an unexpected additional challenge.
The fallback LOGIN command interpolates strings in the chosen library.
Non-ASCII passwords are reliable only through PLAIN; this limitation must remain
explicit in compatibility reporting. Do not claim universal LOGIN support or
mislabel a local command-encoding failure as server rejection.

If no supported password method is available, explain that sign-in method support
is missing. OAuth and other SASL methods are outside 002.

## Metadata and the selected window

After EXAMINE, let N be EXISTS. N = 0 yields a confirmed empty batch. Otherwise
calculate explicit sequence bounds max(1, N-99) through N and issue two commands:

```text
FETCH low:high (UID FLAGS INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM TO SUBJECT)])
UID FETCH <uids> (UID BODYSTRUCTURE)
```

The first command establishes the rows. The second uses the UIDs returned by the
first, so both describe the same messages even if the Inbox changes in between.

Here low/high stand for calculated numbers. Do not SEARCH ALL, request older
history or use an open-ended * range. The stable result is min(N, 100) messages,
sorted by descending UID. INTERNALDATE is display data, not the sort key.
Handle normal unsolicited mailbox responses needed by these commands, without
building incremental synchronization.

`mailbag-content` parses the returned From/To/Subject header block with mail-parser,
including encoded words in subjects and display names. ENVELOPE is not requested.
Malformed or missing display text gets replacement characters or neutral labels;
it must not cause another message to disappear.

BODYSTRUCTURE, including its extension data, determines dispositions and text
sections. The protocol library owns its grammar and depth limits;
`mailbag-imap` projects the structure and `mailbag-content` owns only section
selection. Rows never depend on BODYSTRUCTURE: a message whose structure is
missing, unusable or unparseable keeps its row and gets a content explanation.
A structure response that the protocol parser cannot parse uses the isolation
path below. If the row command itself cannot be parsed, the metadata step fails.

### Isolating an unreadable structure

One BODYSTRUCTURE that the parser rejects, for example one nested deeper than the
fork's limit, prevents the whole structure response from being parsed. Any
sender can produce such a message. The fallback is a bounded part of this load,
not a general retry policy:

1. On a structure parsing failure, close that session. Do not continue reading
   its parser buffer. The rows already received stay in the candidate.
2. Open a fresh secure session, authenticate and EXAMINE again. If UIDVALIDITY
   changed, stop with an Inbox-changed explanation.
3. Fetch `UID BODYSTRUCTURE` separately for each row's UID. A parsed response
   supplies that message's structure. A parser failure isolated to that UID gives
   the message an unreadable-structure content explanation.
4. After an individual parse failure, close the unusable session and reopen
   securely before continuing with the remaining UIDs. Verify UIDVALIDITY each
   time. Each UID gets only one isolation attempt; at most 100 are examined.
5. Fetch selected text for messages with usable structures through the normal
   grouped path. Publish a completed batch in which every row is present.

Do not use this fallback for transport failures, timeouts, authentication failures,
NO/BAD, truncated literals or the library response ceiling. Any such failure
stops the attempt, and no batch is published. There is no recursive
fallback or repeated attempt for an already isolated UID.

The pinned async-imap parser reports some syntax errors as `io::ErrorKind::Other`
and leaves the offending buffer intact. Its buffer-limit error also uses Other.
Mark errors produced by the GIO bridge with a private error wrapper so their
origin survives conversion to std::io::Error. In the mapper for this pinned
revision, distinguish the library's fixed ceiling error from its decoder errors;
do not search or print raw response text to classify a failure. Do not treat every
Io error as an unreadable structure or add a wire parser. Exercise these distinct origins
in integration tests. This session-replacement rule follows
the pinned [ImapStream source](https://github.com/mitinand/async-imap/blob/c4378d17cbf34938def1ff33f9dd7df6a064f2b7/src/imap_stream.rs).

An isolated structure failure affects only that message's content. A
disconnected transfer never becomes a completed batch.

## Grouped text acquisition

Choose all text parts before issuing body commands. Group messages by the
**complete request shape**: ordered section identifiers and each section's header
form. Root HEADER versus part.MIME matters even if both bodies use section 1.

For each group, issue one UID FETCH for its UID set, UID itself and these fields:

| Selected body | Requested fields |
|---|---|
| Single-part root | `BODY.PEEK[HEADER]` and `BODY.PEEK[1]` |
| Multipart leaf at section S | `BODY.PEEK[S.MIME]` and `BODY.PEEK[S]` |

A group with several leaves requests every corresponding header/body pair.
Do not send a command per message when the request shapes match. This removes
the measured per-message round-trip cost without concurrent IMAP commands.
The normal path has one row command, one structure command and one text
command per distinct request shape.

Correlate returned fields by UID and section, regardless of response order.
Decode each complete returned MIME entity and release raw buffers as it is
processed. No whole-message/body shortcut (`BODY[]`, `BODY[TEXT]` or a multipart
container payload). Metadata and selected part headers are allowed.

An empty literal is valid content. A completed tagged OK with no response for a
requested UID means that message disappeared; omit it without fabricating an
empty row. A returned UID missing a requested header/body section, NIL instead
of required bytes or an incomplete literal is a load failure.

If every UID from a nonempty window disappears, report that Inbox changed and
allow Refresh. Do not infer
empty Inbox from missing results. Do not refill gaps with older mail; new arrivals
wait for the next manual refresh.

## Selecting text sections

Follow the prototype's selection rules, without a second MIME parser or a
library-neutral provider framework. The load sequence converts `mailbag-imap`'s
BODYSTRUCTURE projection into `mailbag-content`'s part description; selection
walks it once and returns part paths, which map directly to IMAP section
numbers. Explicit
attachment disposition excludes a part and its descendants. Disposition comes
from extension data; absent disposition is not itself a reason to reject all text.

| MIME form | Selection |
|---|---|
| text/plain | Include wherever it occurs, including later in multipart/mixed, unless excluded as an attachment. A name parameter without explicit inline marks an attached text file. |
| multipart/mixed and other ordinary multipart subtypes | Walk children in order; join selected text with two newlines. Several inline text/plain siblings all contribute. |
| multipart/alternative | Select the last branch containing supported plain text. Do not download other alternatives. |
| multipart/signed | Inspect only the first part. Do not fetch the signature or claim verification. |
| multipart/encrypted | Explanation; no encrypted payload. |
| application/pkcs7-mime or x-pkcs7-mime | Unsupported S/MIME explanation; do not download/decrypt it or claim signature verification. |
| multipart/related | Inspect only the first child, as in the prototype. No Content-ID root resolver in 002. |
| message/rfc822 | Skip the entire nested message. |
| text/html, images and other leaves | Skip payloads. HTML-only mail gets its ordinary unsupported-view explanation. |

A supported plain branch may coexist with excluded resources in a mixed message.
Inline PGP-looking text is still text; no heuristic decryption detection. No
preview generation or HTML-to-text fallback.

## Decoding and publication

Combine each original MIME header and body with the required blank separator
and give that entity to mail-parser with full_encoding. For root text use HEADER;
for a multipart leaf use its .MIME header. Accept text only from a plain-text
part with `PartType::Text`. Never call `body_text()` or `body_html()`.

Let mail-parser handle base64, quoted-printable and charset conversion. Invalid
bytes are replaced rather than hiding the body. Unknown charset or unknown
Content-Transfer-Encoding yields a message-specific explanation. Do not reinstate
strict transfer validators or a “non-ASCII without charset” rejection. Unsupported
content or a structurally unusable MIME entity still gets an explanation.

If a selected part cannot be decoded, explain the unavailable body for that
message rather than showing its other selected parts as complete. Other messages
remain usable. NUL is replaced before GTK APIs. No replacement-count telemetry
is required.

Publish only after metadata and all required text commands finish, with complete
text or a content explanation for every represented message. The reader later
clips only its display; received text is not changed. Opening sends no network
request.

## ALERT and diagnostics

Retain ALERT text encountered during the attempt only to explain a simultaneous
or subsequent load failure. This covers greeting, authentication, untagged and
tagged replies that the fork exposes. An OK with ALERT can continue. Successful
attempts do not produce a standalone ALERT notification.

Do not build a notification service, history, sync popover or extra error for
dependent steps that never ran. Map errors to safe step/cause information.
Compile log levels out in native and Flatpak builds; never log raw commands,
mail headers/bodies, credentials or library Debug/Display errors. UI server text
is inert and is not copied into diagnostics.

## Evidence for selective acquisition

For SC-002, “not downloaded” covers **every unselected payload**: attachments,
attached text, HTML alternatives, inline images, signatures and nested messages.
BODYSTRUCTURE and selected header fields may describe them. Assert exact requested
sections and their absence in the scripted server, read-only EXAMINE/BODY.PEEK,
unchanged flags and zero requests on opening.

Use the Rust/GIO server and MIME fixtures described in [quickstart](../quickstart.md).
Do not port the prototype's real-mail printing switches or raw debug transcripts.
