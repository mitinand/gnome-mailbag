# Microsoft 365 Integration: Research

**Date**: 2026-09-23. Facts checked in documentation, in source or by the
read-only probe against the maintainer's personal Microsoft 365 account in
GOA (a throwaway Rust program built inside the GNOME 50 SDK, outside the
repository). Each section ends with the decision it supports.

## 1. What GOA gives for a Microsoft 365 account

Checked in `goamsgraphprovider.c` (GNOME Online Accounts 3.58.1, the version
installed on the development machine) and on the live bus:

- The provider type is `ms_graph`; its features are Mail, Calendar, Contacts
  and Files. The Mail interface is a bare skeleton with `EmailAddress` and
  `Name`: no IMAP host, `ImapSupported` false, `ImapHost` empty. The existing
  IMAP access request would fail on it with `NoEncryption`.
- The account object exports `org.gnome.OnlineAccounts.OAuth2Based` and no
  `PasswordBased`. `GetAccessToken()` returns `(access_token s, expires_in i)`;
  on the live account the token was 1504 characters long with about 56
  minutes left. GOA renews a token that is close to expiry before returning
  it (004 research §1).
- The scopes GOA requests include `mail.readwrite`, `mail.readwrite.shared`,
  `mail.send`, `mailboxsettings.read` and `user.read`, all for Microsoft
  Graph: GOA itself calls `https://graph.microsoft.com/v1.0/me` with the token
  to learn the account's address. The token is therefore for Graph and allows
  reading mail.
- GOA's client identifier is compiled into Fedora's build (the meson default;
  the same string appears in `libgoa-backend-1.0.so`). A personal account
  needs nothing beyond signing in; the "Use Organization Account" switch
  reveals a Client ID (optional) and a Tenant ID (required). Every GNOME
  application using GOA shares that client identifier, which is what the
  service's limits count.

**Decision**: a second, token-only access request in goa-adapter:
`request_graph_access` finds the account's object, requires `OAuth2Based`
and calls `GetAccessToken`. It reads no Mail setting, because there is none
to read. The error and request types of the IMAP request are renamed to
`AccessError` and `AccessRequest` and shared: the same D-Bus calls fail in the
same ways. `expires_in` is discarded as in 004.

## 2. Reaching the service: the web library

Candidates for HTTPS from Rust on this platform:

| Library | Fit |
|---|---|
| libsoup 3 through the `soup3` crate (gtk-rs, 0.9.0, March 2026, depends on glib/gio 0.22 like the workspace) | Runs on the GLib main loop the mail worker already owns; uses GIO's TLS backend, certificate database and proxy resolver, the same as `mailbag-imap`; the GNOME 50 runtime ships libsoup 3.6.6 |
| reqwest | Needs a tokio runtime beside the GLib executor; its own TLS stack and root store in the Flatpak |
| ureq | Blocking, so a request could not be cancelled by dropping a future; rustls with its own root store |
| isahc | curl-based, runtime-agnostic, but its last release is from 2022 |

Checked by building a probe in the GNOME 50 SDK sandbox (the host has no
`libsoup3-devel`; the SDK has the headers and the Rust extension):

- The crate's library name is `soup`, not `soup3`.
- A `soup::Session` created on a thread with its own `MainContext` and
  driven by `block_on` completes `send_and_read_future`; the `Authorization`
  and `Prefer` headers arrive as sent; `@odata.nextLink` can be followed by
  passing the URL as it is.
- Dropping the future in flight cancels the request; the thread continues.
- The default session refuses an expired certificate ("Unacceptable TLS
  certificate") with no handler connected; TLS completes before the request
  is sent. Checked in libsoup's source: the `accept-certificate` signal must
  be answered with TRUE for a bad certificate to pass.
- The session's `timeout` property defaults to 0, which means no limit
  (`soup-session.c`); `idle-timeout` defaults to 60 s.
- `ServerMessage::pause` and `unpause` sit behind the crate's `v3_2` feature;
  the scripted service does not need them, a bare TCP listener that never
  answers serves the stall test.

**Decision**: `soup3` with default features; one session per load, created
inside `list_inbox_messages` on the worker's context, with `timeout` set to
the 30 s wait limit of 002 and a `Mailbag/<version>` user agent; tests reach
the same steps with a shorter limit through a second entry point, the way
`InboxReader::open_with_short_socket_timeout` works. No session type is
kept: a load makes one request (plan challenge). The host setup gains
`libsoup3-devel` in `setup.sh` and the README. Rejected: reqwest and ureq
for the second runtime and the second TLS stack; isahc for its maintenance
state.

## 3. JSON

`serde_json` reading into `serde_json::Value` and walking the fields by
name; no `serde` derive, so no procedural macro is compiled, matching the
choice 003 made for `tracing`. The answer has one shape with seven fields;
walking it by hand is about 40 lines. Rejected: `serde` derive for the build
cost of one shape; json-glib, which has no maintained Rust binding.

## 4. The list request and its answer

From Microsoft's pages, with the probe's confirmation where the pages leave
room:

- `GET /me/mailFolders/inbox/messages` addresses the Inbox by its well-known
  name, which works regardless of the mailbox's language
  ([mailFolder](https://learn.microsoft.com/en-us/graph/api/resources/mailfolder?view=graph-rest-1.0)).
- `$top` accepts 1 to 1000 and defaults to 10; `$select` limits the
  properties; `$orderby` is supported
  ([list messages](https://learn.microsoft.com/en-us/graph/api/user-list-messages?view=graph-rest-1.0)).
  Probe: `$top=100&$orderby=receivedDateTime desc` returned exactly 100
  messages newest first, out of an Inbox of 3452, with `@odata.nextLink`
  present because more exist.
- `Prefer: outlook.body-content-type="text"` returns `body` as text. The
  list page also says the operation "returns message bodies in only HTML
  format"; the probe settled it: on the list, 100 of 100 bodies came with
  `contentType` `text`. The answer's `Preference-Applied` header echoed only
  `IdType=ImmutableId`, so the body's own field, not the header, is where
  another form would show; the code examines neither (decision below).
- `Prefer: IdType="ImmutableId"` per request gives identifiers that survive
  folder moves ([immutable identifiers](https://learn.microsoft.com/en-us/graph/outlook-immutable-id)).
  Probe: immutable identifiers are 68 characters, default ones 136; the two
  differ for the same message; `GET /me/messages/{immutable id}` works. Both
  preferences travel in one `Prefer` header, comma-separated (RFC 7240).
- Sizes and times on the live account: metadata only 57 KB in 0.8 s; with
  text bodies 1.2 MB in 1.3 s, bodies from 42 to 56 901 characters. The first
  request of a cold run once took 9 s, later ones under a second. A tiny
  request to `/me` beforehand took 0.7 s, so the cost is connection setup,
  not the query.
- `receivedDateTime` is ISO 8601 in UTC ("2018-09-09T03:15:08Z");
  `glib::DateTime::from_iso8601` reads it and `to_unix` gives the seconds the
  batch already uses.
- `from` and `toRecipients` are `emailAddress { name, address }`; `subject`
  and `isRead` are plain values. `glib::DateTime::from_iso8601` is available
  without a feature flag in glib 0.22.9.
- Field shapes over the newest 100 live messages: no `from`, `subject`,
  `receivedDateTime`, `isRead` or `body` missing or empty; no empty name or
  address; 9 messages with an empty `toRecipients` list.

**Decisions**: one request with `$top=100`, `$orderby=receivedDateTime
desc`, `$select=id,subject,from,toRecipients,receivedDateTime,isRead,body`
and both preferences. The answer reader requires the object, `value` and
each entry's `id` and `isRead`; the other fields are read when present and
of the documented type and left out otherwise, so a message without a
subject shows the window's existing "No subject" and one without a body the
existing "text not returned" explanation; `body.contentType` is not
examined, because the service documents the preference, the probe confirmed
it, and the spec's challenge chose to build nothing for a body in another
form (plan challenge: one odd message must not fail the load).
`@odata.nextLink` present sets `more_available`; nothing follows it (spec,
Clarifications). The display fields use mailbag-content's rule for names
(the name, else the address, joined by a comma), exposed as
`display_names`, so that one owner formats senders for every provider
(constitution IV); an empty recipient list becomes no To row, as today.

## 5. Failures

- Errors are a JSON object `error { code, message, innerError }`; `code` is
  the machine-readable value to depend on, `message` is developer text that
  can change and "shouldn't be displayed to the user directly"
  ([error responses](https://learn.microsoft.com/en-us/graph/errors)).
  Probe: an invalid token gives 401 with `InvalidAuthenticationToken` and a
  `WWW-Authenticate` header.
- 429 and 503 carry `Retry-After`; the Outlook service allows 10 000 requests
  per 10 minutes and four concurrent per application identity and mailbox
  ([throttling](https://learn.microsoft.com/en-us/graph/throttling-limits#outlook-service-limits)).
  The identity is GOA's, shared with Evolution and the file browser on the
  same machine. The spec's challenge decided that a refusal shows status and
  code and the wait is not read.
- 403 and 404 after a valid sign-in mean a mailbox the service cannot serve
  or a missing licence; they are refusals with a code, not sign-in failures.
- Transport: GIO reports a refused connection, a failed certificate and a
  timeout as `glib::Error`s; `IOErrorEnum::TimedOut` is the wait limit.

**Decisions**: `GraphFailure { ConnectionFailed, TimedOut, Refused { status,
code }, InvalidReply }` with the platform's or the service's text kept for
the explanation and the debug line; the window adds the 004 sign-in sentence
for status 401 only. The error line at error level carries status and code
(003 FR-011: the text at debug). No retry anywhere.

**Amended by [006](../006-error-handling/spec.md) on 2026-09-25**: the
service's message may appear in the failure dialog as a remote text, under
"Message from the mail service", never in the explanation; the explanation
and the sign-in advice come from the failure's declaration.

## 6. Evidence for later features (no code here)

Recorded from the probe so that storage, folders and error presentation start
from facts:

- Change tracking: `GET /me/mailFolders/inbox/messages/delta` with
  `$select=id,isRead,receivedDateTime`, `$filter=receivedDateTime ge <14
  days ago>`, `$orderby=receivedDateTime desc` and `Prefer:
  odata.maxpagesize=100` finished its first round in one page of 33 messages
  and returned an `@odata.deltaLink` of 406 characters. Without the filter,
  the first round pages through the whole folder at 100 per page. An
  immediate second round returned no entries and a new deltaLink. After the
  maintainer moved one message to Deleted Items and toggled another's read
  state in Outlook, the next round held exactly two entries: `{"@odata.type",
  "@removed": {"reason": "deleted"}, "id"}` for the moved message, under the
  same immutable identifier, and `{"@odata.type", "id", "isRead": true}` for
  the other. An update carries only the changed property and the identifier,
  and reports the server's current state: the folder counts confirmed that
  the message was read again (Outlook's reading pane had re-marked it), so
  delta is a state report, not an action log.
- The folder resource gives `totalItemCount` and `unreadItemCount` in one
  request (3452 and 28 for the Inbox; 162 and 1 for Deleted Items after the
  move).
- `GET /me/messages/{id}/$value` returns the message regenerated as MIME
  (316 KB for one message with attachments, in 0.15 s); it accepts the
  immutable identifier.
- Personal Microsoft accounts and organization accounts both go through the
  same GOA provider; the live account is a personal one. An organization
  that blocks the shared client identifier fails in Online Accounts.

## 7. Crate layout

`mailbag-graph` is the third provider's protocol layer, next to
`mailbag-imap`, as 002 research §9 foresaw ("Graph never reaches
`mailbag-imap`: it shares no code, only a contract"). It depends on glib,
gio, soup3 and serde_json and on nothing of the workspace; it knows no batch,
no window and no provider rule beyond the request it builds. The load
sequence that joins it with goa-adapter and mailbag-content lives in
`mailbag-providers`, as `gmail.rs` and `imap.rs` do. The table in 002
research §9 gains a row for it.

Why a crate: the same reason as for `mailbag-imap`, the compiler enforces
that no widget and no content rule reaches the service layer, and a new
dependency cannot add a forbidden edge quietly (`scripts/check.sh`).

## 8. Rejected and deferred

| Item | Why not now |
|---|---|
| Page loop over `@odata.nextLink` | Probe: 100 in one page; the incomplete notice covers a short page (spec challenge) |
| A session type held across requests (`GraphService`) | One request per load; the session lives inside the function (plan challenge) |
| Failing the load when `body.contentType` is not `text` | Would turn one odd message into an empty list; the answer reader reads what is there (plan challenge) |
| `Retry-After` in the explanation | Error presentation decides how waits are shown |
| Separate secure-connection failure kind | TLS completes before the request; the library refuses a bad certificate by default; one connection failure with the platform's reason |
| Request identifier of a refused request | Only for a support case with Microsoft, which GOA's shared identity does not get |
| Attachment indication | Nothing reads it before the attachments feature |
| MIME through `$value` and mailbag-content's part selection | 101 requests and full messages per batch; the service renders text itself |
| Folder listing, delta code, HTML body, search, notifications, conversation identifier | Deferred in the spec with their features |
| A `Mailbox` type in mailbag-content or a mailbag-graph dependency on it | The service layer must not depend on content; the pair travels as plain strings and the provider applies the rule |
