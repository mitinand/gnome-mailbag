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
| After sign-in | 002 uses no capability after sign-in, so none is requested. When a later feature needs one (iCloud advertised IDLE only after authentication), read capabilities again then; never reuse the pre-login set. |
| Inbox | EXAMINE INBOX; obtain UIDVALIDITY and EXISTS. Never SELECT. |
| Finish | Close the connection after the batch. No retained idle connection and no mail-changing CLOSE/EXPUNGE/STORE/COPY/MOVE commands. |

A missing/rejected STARTTLS command, handshake error or invalid certificate ends
the attempt before password transmission. No cleartext fallback, second insecure
connection or trust exception. Bytes buffered before STARTTLS must never be
interpreted as authenticated TLS replies. PREAUTH before TLS is a failure, not
permission to bypass encryption. PREAUTH after verified implicit TLS is not
supported either: async-imap starts a session only through sign-in, and
password accounts in Online Accounts do not need it.

AUTHENTICATE PLAIN supplies raw `NUL + login + NUL + password` to the library's
authenticator; async-imap owns base64 framing. Do not add an application base64
dependency or resend credentials on an unexpected additional challenge.
The fallback LOGIN command sends each argument as a quoted string when IMAP
allows one, and otherwise as a literal, so non-ASCII logins and passwords work
with LOGIN as well as with PLAIN (async-imap fork, research §3).

If no supported password method is available, explain that sign-in method support
is missing. OAuth and other SASL methods are outside 002.

## Metadata and the selected window

After the matching successful EXAMINE completion, let N be EXISTS. A connection
closed before that completion is an opening failure, not confirmation of an
empty Inbox. N = 0 yields a confirmed empty batch. Otherwise calculate explicit
sequence bounds max(1, N-99) through N and issue two commands:

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

A server may split one message's fields between multiple FETCH responses in any
order ([RFC 2683 §3.4.4](https://www.rfc-editor.org/rfc/rfc2683.html#section-3.4.4)).
For the row command, collect fields by message sequence number until the command
ends, then establish each row's UID and check its data. A response without FLAGS
leaves the collected flags unchanged; the last response containing FLAGS wins,
including `FLAGS ()`. The fork's `Fetch::has_flags()` distinguishes those cases.
For UID FETCH, collect structures and text sections by UID instead: EXPUNGE can
change sequence numbers during a UID command, and its requested FETCH responses
include UID ([RFC 3501 §6.4.8 and §7.4.1](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.8)).
Check for missing sections only after collecting all responses to that command.

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

### One message's problem stays with that message

A server can end a FETCH with a tagged NO after answering for the other
messages, for example when another client expunged a message meanwhile or a
message is damaged on the server ("Some messages could not be FETCHed"). The
row command addresses messages by sequence number, so such races happen there
too. Collect the responses one by one and keep everything received before the
completion; the fork reports a NO or BAD completion of FETCH, and a connection
closed before the completion, as an error after the responses.

| Command | A requested message without data, after a tagged OK | ... after a tagged NO |
|---|---|---|
| Rows | Absent. No row at all although EXISTS was not zero: the Inbox changed. | Absent, and the rows that arrived are kept with the server's text, which reports the list as incomplete. No row at all: the metadata step fails with that text. |
| Structures | It disappeared; omit its row. | Its row stays with an unreadable-structure explanation. |
| Text | It disappeared; omit its row. | Its row stays with a text-not-received explanation. |

Keeping the rows of a refused command is not the same as hiding the refusal:
the reason travels with the batch, because a message missing from the list
leaves nothing else to explain it, unlike a missing structure or text, which
their own row explains.

Plain text marked `format=flowed` is unflowed before it is shown
([RFC 3676](https://www.rfc-editor.org/rfc/rfc3676.html)): soft line breaks,
which are lines ending in a space, join the paragraph they belong to within the
same quoting depth, `delsp=yes` drops that space while joining, one stuffed
leading space is removed, and the `-- ` signature separator ends a paragraph.
Senders wrap flowed text for a narrow terminal, so showing it as it arrives
leaves ragged columns, and with `delsp=yes` words break apart.

A returned message whose requested section is NIL or missing also gets the
text-not-received explanation. The other messages load normally. Only a network
error, timeout, BAD, BYE, a literal cut off by a closed connection and the
library's buffer limit end the whole load; an unparseable structure response
uses the isolation path.

### Isolating an unreadable structure

One BODYSTRUCTURE that the parser rejects, for example one nested deeper than the
fork's limit, prevents the whole structure response from being parsed. Any
sender can produce such a message. The fallback is a bounded part of this load,
not a general retry policy:

1. On a structure parsing failure, close that session. Do not continue reading
   its parser buffer. The rows already received stay in the candidate.
2. Open a fresh secure session, authenticate and EXAMINE again. If UIDVALIDITY
   changed, stop with an Inbox-changed explanation.
3. Keep structures already parsed, then fetch `UID BODYSTRUCTURE` separately
   for each remaining row's UID. A reply containing only FLAGS does not supply
   a structure and must not exclude its UID from isolation. Discard provisional
   entries without structures before these individual requests, so their results
   also determine whether a message disappeared. A parser failure isolated to
   that UID gives the message an unreadable-structure content explanation.
4. After an individual parse failure, close the unusable session immediately.
   InboxReader remembers that it needs a new session and reopens securely before
   the next command, including a text command after the last isolated UID.
   Verify UIDVALIDITY each time; a failed reconnection ends the load. If no
   command remains, do not reconnect. Each UID gets only one isolation attempt;
   at most 100 are examined.
5. Fetch selected text for messages with usable structures through the normal
   grouped path. Publish a completed batch in which every row is present.

Do not use this fallback for transport failures, timeouts, authentication failures,
BAD, truncated literals or the library's buffer limit. Any such failure stops
the attempt, and no batch is published. A NO completion is handled per message
as described above. There is no recursive fallback or repeated attempt for an
already isolated UID.

The pinned async-imap parser reports some syntax errors as `io::ErrorKind::Other`
and leaves the offending buffer intact. Mark errors produced by the GIO bridge
with a private error wrapper so their origin survives conversion to
std::io::Error. The fork reports its buffer limit as the typed error
`ResponseTooLarge`; recognize it by type. Do not search or print raw response
text to classify a failure, treat every Io error as an unreadable structure or
add a wire parser. Exercise these distinct origins in integration tests. This
session-replacement rule follows the pinned
[ImapStream source](https://github.com/mitinand/async-imap/blob/89badf82c3af2173c6d839481be7aa5825d3ba42/src/imap_stream.rs).

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

An empty literal is valid content. A message without the requested text follows
the table in "One message's problem stays with that message": omitted after a
tagged OK, text not received after a NO or with NIL or a missing section. An
incomplete literal is a load failure.

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
| text/plain | Include wherever it occurs, including later in multipart/mixed, unless excluded as an attachment. A name parameter without explicit inline marks an attached text file, in every form RFC 2231 allows: `name`, the extended `name*` and continuations such as `name*0*`, because a server need not fold them back into one parameter. |
| multipart/mixed and other ordinary multipart subtypes | Walk children in order; join selected text with two newlines. Several inline text/plain siblings all contribute. |
| multipart/alternative | Select the last branch containing supported plain text. Do not download other alternatives. |
| multipart/signed | Inspect only the first part. Do not fetch the signature or claim verification. |
| multipart/encrypted | Explanation; no encrypted payload. |
| application/pkcs7-mime or x-pkcs7-mime | Unsupported S/MIME explanation; do not download/decrypt it or claim signature verification. |
| multipart/related | Inspect the root part: the child whose Content-ID matches the `start` parameter, or the first child when there is no `start`, the server reported no Content-ID for that child, or nothing matches ([RFC 2387](https://www.rfc-editor.org/rfc/rfc2387.html#section-3.2)). IMAP reports no Content-ID for a multipart child, so a `start` naming one falls back to the first child. |
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
or subsequent load failure. This covers the greeting, untagged replies and
tagged completions, including a rejected sign-in, which the fork exposes
through `Client::unsolicited_responses()`. Collect notices before returning from
CAPABILITY, including failure or no supported sign-in method. Preserve ALERTs
from EXAMINE's untagged replies and tagged completion. An OK with ALERT can
continue. Successful attempts do not produce a standalone ALERT notification.

An untagged NO or BAD is a warning under
[RFC 3501 section 7.1.2](https://tools.ietf.org/html/rfc3501#section-7.1.2), so
it keeps its ALERT text without failing the command; the tagged completion
decides. An untagged response during the sign-in exchange, such as an ALERT
before the server asks for the credentials, does not end that exchange either.

Also keep the server's reason for the failure that ends a load: the text of the
NO or BAD completion of the failed command, a BYE greeting, or a BYE received
during the session, for example at a server's connection limit. Keep its
RFC 5530 response code, such as `AUTHENTICATIONFAILED` or `UNAVAILABLE`, when the
server sent one. The fork keeps the code and text of NO and BAD in its error;
imap-proto does not parse RFC 5530 codes, so the fork reads them from the start
of the text. This is inert server text for the failure explanation only.

Do not build a notification service, history, sync popover or extra error for
dependent steps that never ran. Map errors to safe step/cause information.
Compile the `log` crate's levels, which the IMAP library uses, out in native and
Flatpak builds. Server status text and ALERT text reach diagnostics only at
debug and only through the replacement of the sign-in name that
[003](../../003-logging/contracts/record.md) defines. Raw commands, mail
headers and bodies, credentials and library Debug/Display errors are never
logged, except GIO's text for a failed TLS handshake at debug, which holds
fixed phrases of the TLS library and no server data.

## Evidence for selective acquisition

For SC-002, “not downloaded” covers **every unselected payload**: attachments,
attached text, HTML alternatives, inline images, signatures and nested messages.
BODYSTRUCTURE and selected header fields may describe them. Assert exact requested
sections and their absence in the scripted server, read-only EXAMINE/BODY.PEEK,
unchanged flags and zero requests on opening.

Use the Rust/GIO server and MIME fixtures described in [quickstart](../quickstart.md).
Do not port the prototype's real-mail printing switches or raw debug transcripts.
