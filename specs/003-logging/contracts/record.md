# Contract: the record and how crates write it

What a person types and sees is in the [spec](../spec.md). This contract fixes
the parts that several crates must agree on.

## Command line

| Start | Result | Exit status |
|---|---|---|
| No `--log-level` | No subscriber, no lines | as today |
| `--log-level=error\|warning\|info\|debug` | Logging on for this run | as today |
| `--log-level=` anything else | `Unknown log level "<value>". Use error, warning, info or debug.` on the standard error stream; no window | 1 |
| `--log-level=…` while Mailbag runs | `Mailbag is already running, so logging was not turned on. Quit Mailbag and start it again with this option.`; the running instance is not activated | 1 |

`--help` lists the option with the four levels.

## Line

The one-line formatter of `tracing-subscriber` lays the line out: time,
level, the enclosing spans with their fields, the module that wrote the
event, the message, the event's fields.

```text
<time> <LEVEL> load{account="…"}:message{uid=4711}: mailbag_content: <message> <field>=<value> …
```

- `<time>`: local time with milliseconds and the UTC offset, from GLib.
- `<LEVEL>`: `ERROR`, `WARN`, `INFO`, `DEBUG`. The option's value `warning`
  selects `WARN`.
- The module path tells where the line was written; there is no separate
  table of components.
- Colors are off.
- Lines go straight to the standard error stream, as GLib's own warnings do.
  There is no queue and no writer thread
  ([research §3](../research.md#3-writing-to-the-standard-error-stream)).

The layout is the library's and is for people. Tests assert on fields and
markers, not on columns or punctuation.

## Context fields

A span carries what every line inside it must show. Events inside the span do
not repeat these fields.

| Span | Fields | Opened by |
|---|---|---|
| `load` | `account` (Online Accounts identifier) | `mailbag::inbox::InboxController`, when `begin_load` accepts a load; kept in `RunningLoad`. `WindowUi` starts the loader and reports its result inside this span. `MailLoader` captures the span at entry, enters it in the Online Accounts completion callback and passes a clone with `LoadRequest`; the mail worker attaches it to the load's future. The controller enters the same span for cancellation and for deciding whether to accept or discard a result |
| `message` | `uid` | `mailbag::inbox_load`, inside the load span, around each of its three calls into `mailbag-content` for one message: selecting its text parts, decoding its received text, and decoding its list headers. `mailbag-content`'s functions do not know which message they work on. `mailbag-imap` opens the same span around reading one message's structure. The folder is the load's Inbox. Opened only when debug is enabled |

A span covers only code that runs inside it. `goa-adapter`'s request for
settings and the password completes in GIO callbacks outside the span, so
the adapter writes no line about it; `mailbag::inbox_load` writes that line
in its completion callback, where it has the span and the received settings.

An account is named by its
Online Accounts identifier, such as `account_1726920000_0`
([research §5](../research.md#5-naming-an-account)). A
read of the account list is reported by a single line where it completes, in
`goa-adapter`, and has no span and no identifier: the application learns of a
read only after it has ended, so a span opened there could not cover it. Lines about one account
outside a load (shown, not shown, a problem) carry `account` as an event
field; the lines that observe an account add `provider`.

## Load outcomes and cancellation

`InboxController` owns the load's start and final lines because it owns
whether a result is still applicable. `inbox_load` owns the Online Accounts
step and the worker's work, not the decision to present a result.

- `discard_excluded` writes one info cancellation line with reason
  `account excluded`, before dropping an active cancellation handle.
- `cancel_load` writes it with reason `quitting`, before dropping an active
  handle. Closing the window takes this path before the quit line; the Quit
  action does not cancel a running load, and the shutdown order is not
  changed for the record. Neither waits for a callback or the worker.
- A cancellation acknowledgement writes no second final line. If a received
  batch or failure arrives after exclusion, `finish_load` writes info that the
  result was discarded at the existing `show_result` decision; it writes no
  success, warning or error for that result. Use the existing applicability
  check.
- A result accepted for presentation produces its completion and warning
  lines or its single error line. `step` and `cause` are the names of the
  existing failure values, such as `step=SignIn cause=TimedOut` or
  `cause=NoEncryption`: the same values the UI turns into its explanation.
  The UI's wording stays in `window_ui.rs` and is not shared or moved.

These lines use the span of the existing `RunningLoad`.
Dropping a handle after normal completion is not a cancellation event, and
repeated exclusion or shutdown callbacks do not repeat a cancellation line.

## Durations

No line carries a duration. Every line has its time with milliseconds, and
the lines of one operation follow each other, so the duration of a step, a
failed step's included, or of a whole load is the difference between two
lines. No timing lines are written, and errors passed between crates gain no
timing fields.

## Event fields

Names are shared so that lines read alike. A field is present only when the
working file [log events](../log-events.md) plans it for that event.

| Field | Meaning | Lowest level |
|---|---|---|
| `messages`, `rows`, `parts`, `commands`, `accounts`, `alerts`, `unsupported` | Counts | info |
| `step`, `cause` | The failing step and cause, as the names of the failure values the UI explains | error |
| `code` | Server response code, such as `AUTHENTICATIONFAILED` | error |
| `capabilities`, `method`, `encryption`, `tls` | Server capability list, sign-in method, encryption mode, TLS version | info |
| `reason` | Why an account is not shown or a load was cancelled | info |
| `problem` | An account's problem: attention needed, Mail service unavailable | warning |
| `account` | An account's Online Accounts identifier, as an event field outside a load | error |
| `provider` | An account's provider type: `imap`, `google`, `microsoft365`, `other` | info |
| `signal` | Which Online Accounts signal announced a change | debug |
| `folder`, `uid`, `uids`, `uid_validity`, `uid_next` | Message identity | debug |
| `host`, `port`, `address` | Where the connection went | debug |
| `section`, `content_type`, `charset`, `format`, `delsp`, `disposition`, `transfer_encoding`, `size` | One part of a part tree | debug |
| `header` | A present list header that did not decode | debug |
| `sections`, `alternative`, `start_matched`, `explanation` | Text parts selected; which alternative was chosen; whether a related set's `start` named a part; why no text part was selected | debug |
| `flowed`, `characters_out` | How one part was decoded | debug |
| `server_text`, `alert` | Server status text and alert text, sign-in name replaced | debug |
| `certificate_errors`, `tls_error` | Names of the certificate checks that failed; GIO's text for a failed TLS handshake | debug |

Fields that must never exist, at any level: a password or token, the sign-in
name, an address, a display name, a subject, any other header value, a file
name, decoded or raw text of a part, attachment content, an attached
message's envelope, a part's description or content identifier, and part
parameters other than the three listed.

## Rules for code that writes events

1. One error line per failed operation, written by the owner that gives the
   operation up: `mailbag::inbox::InboxController` for a load, `goa-adapter`
   for a read of the account list, `mailbag` for opening Settings.
   `mailbag-imap` and `mailbag-content` return their failures as they do
   today and write info and debug lines only. `goa-adapter`'s access to
   settings and passwords returns its result and writes no line; the load
   writes it (Context fields). A result that is published again, as the
   account observer does during Retry, is not a new failure and writes no
   second error line.
2. The level follows the [spec](../spec.md#requirements): no info line about a
   single message or part.
3. The message of an event is fixed text. Everything that comes from mail, a
   server or the system is a string field, never part of the message and
   never passed with the `%` sigil, because the formatter escapes string
   fields only ([research §9](../research.md#9-values-that-reach-a-line)).
4. A value that costs work to build, such as a part tree line, is built only when
   debug is enabled.
5. An account is named by its identifier's text, `AccountId::as_str`, in the
   field `account`; never by `AccountId`'s `Debug`
   ([001 account contract](../../001-goa-account-observation/contracts/accounts.md#validation-and-diagnostics)).
6. Server text becomes a field only in `mailbag-imap`, after the sign-in name
   of any length is replaced. The reply itself goes on to the load and the UI
   unchanged.
7. Nothing is logged through the `log` crate, and no library's own log output
   is enabled ([research §1](../research.md#1-one-owner-of-the-logging-domain)).

## First line

The first line ignores the chosen level, because a record is unusable without
it. It is written before the subscriber is installed and begins like every
other line, with the time from the same source and a fixed level word:

`<time>  INFO mailbag: Mailbag <version>, <native|Flatpak> build, <runtime or OS>, GTK <version>, libadwaita <version>, level <level>`

## Changes to 002 documents

Applied together with the first code that writes server text.

- `002/contracts/imap-reading.md`, section "ALERT and diagnostics": replace
  "UI server text is inert and is not copied into diagnostics" with: "Server
  status text and ALERT text reach diagnostics only at debug and only through
  the replacement of the sign-in name that
  [003](../../003-logging/contracts/record.md) defines. Raw commands, mail
  headers and bodies, credentials and library Debug/Display errors are never
  logged, except GIO's text for a failed TLS handshake at debug, which holds
  fixed phrases of the TLS library and no server data." The sentence "Compile log levels out in native and Flatpak builds"
  is narrowed to the `log` crate, which the IMAP library uses.
- `002/data-model.md`, LoadFailure: replace "Server text is for plain-text UI
  presentation only, never diagnostics" with "Server text is for plain-text UI
  presentation and, with the sign-in name replaced, for debug lines (003)."
- The doc comment on `ImapError`'s `Debug` implementation is reworded to
  match; the implementation itself keeps leaving the text out.
