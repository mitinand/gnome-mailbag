# Contract: what crates must agree on about the record

What a person types and sees is in the [spec](../spec.md); the reasons for the
mechanism are in [research](../research.md). This contract fixes only what
several crates have to agree on: the command line, the names of fields and the
rules for code that writes events.

## Command line

| Start | Result | Exit status |
|---|---|---|
| No `--log-level` | No subscriber, no lines | as today |
| `--log-level=error\|warning\|info\|debug` | Logging on for this run | as today |
| `--log-level=` anything else | `Unknown log level "<value>". Use error, warning, info or debug.` on the standard error stream; no window | 1 |
| `--log-level=…` while Mailbag runs | `Mailbag is already running, so logging was not turned on. Quit Mailbag and start it again with this option.`; the running instance is not activated | 1 |

`--help` lists the option with the four levels.

## Event fields

Names are shared so that lines read alike. A field is written where it says
something; no event has to carry every field of its row.

| Field | Meaning | Lowest level |
|---|---|---|
| `messages`, `rows`, `parts`, `commands`, `accounts`, `alerts`, `unsupported` | Counts | info |
| `cause` | The failure value the UI explains, written as it is, such as `TimedOut(SignIn)` | error |
| `step` | Which part of a read of the account list failed | error |
| `code` | Server response code, such as `AUTHENTICATIONFAILED` | error |
| `capabilities`, `method`, `encryption`, `tls` | Server capability list, sign-in method, encryption mode, TLS version | info |
| `reason` | Why an account is not shown or a load was cancelled | info |
| `problem` | An account's problem: attention needed, Mail service unavailable | warning |
| `account` | An account's Online Accounts identifier, such as `account_1726920000_0` ([research §5](../research.md#5-naming-an-account)) | error |
| `provider` | An account's provider type: `imap`, `google`, `microsoft365`, `other` | info |
| `signal` | Which Online Accounts signal announced a change | debug |
| `uid`, `uids`, `uid_validity` | Message and Inbox identity | debug |
| `host`, `port` | Where the connection goes | debug |
| `section`, `content_type`, `charset`, `format`, `delsp`, `disposition`, `transfer_encoding`, `size` | One part of a part tree | debug |
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
   for a read of the account list. `mailbag-imap` and `mailbag-content` return
   their failures as they do today and write info and debug lines only.
   `goa-adapter`'s access to settings and passwords returns its result and
   writes no line; the load writes it. A result that is published again, as the
   account observer does during Retry, is not a new failure and writes no
   second error line.
2. The level follows the [spec](../spec.md#requirements): no info line about a
   single message or part. One event is written once, at one level: a detail
   either has an event of its own that is useful by itself, or is not written.
3. The message of an event is fixed text. Everything that comes from mail, a
   server or the system is a string field, never part of the message and
   never passed with the `%` sigil, because the formatter escapes string
   fields only ([research §9](../research.md#9-values-that-reach-a-line)).
4. A value that costs work to build, such as a part tree line, is built only
   when debug is enabled.
5. An account is named by its identifier's text, `AccountId::as_str`, in the
   field `account`; never by `AccountId`'s `Debug`
   ([001 account contract](../../001-goa-account-observation/contracts/accounts.md#validation-and-diagnostics)).
   The lines of a load carry no account: one load runs at a time, and the
   controller's own lines name it (spec FR-013).
6. Which message a line is about comes from a `message` span with `uid`, which
   `mailbag::inbox_load` opens around each call into `mailbag-content` for one
   message and `mailbag-imap` opens around reading one message's structure;
   those functions do not know which message they work on. The span is opened
   only when debug is enabled, and lines inside it do not repeat the UID. The
   folder is the load's Inbox.
7. Server text becomes a field only in `mailbag-imap`, after the sign-in name
   of any length is replaced. The reply itself goes on to the load and the UI
   unchanged.
8. Nothing is logged through the `log` crate, and no library's own log output
   is enabled ([research §1](../research.md#1-one-owner-of-the-logging-domain)).
