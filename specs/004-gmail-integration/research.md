# Gmail Integration: Research

**Date**: 2026-09-22–23. Facts checked in documentation, in source or by the
read-only probe against the maintainer's Google account in GOA
(`~/Projects/mailbag-imap-prototypes/gmail-probe`, outside the repository).
Each section ends with the decision it supports.

## 1. What GOA gives for a Google account

Checked in `goagoogleprovider.c` (GNOME Online Accounts, master) and on the
live bus:

- The Mail interface is built with `imap-host imap.gmail.com`,
  `imap-use-ssl TRUE`, `imap-user-name` = the account's mail address, when the
  keyfile has `MailEnabled`. `ImapUseTls` is false. So a Google account always
  means implicit TLS on port 993, which is also what Google documents.
- The account object exports `org.gnome.OnlineAccounts.OAuth2Based` and no
  `PasswordBased`. Generic IMAP accounts export `PasswordBased` and no
  `OAuth2Based`; the Microsoft 365 account exports `OAuth2Based` with
  `ImapSupported` false.
- `OAuth2Based.GetAccessToken()` returns `(access_token s, expires_in i)`.
  GOA returns its cached token while more than about ten minutes of life
  remain and renews it otherwise (`goa_oauth2_provider_get_access_token_sync`);
  a failed renewal is `GOA_ERROR_NOT_AUTHORIZED` or `GOA_ERROR_FAILED`.
  `expires_in` is 0 when unknown. The scope GOA requests includes
  `https://mail.google.com/`, which is the scope Gmail's IMAP server demands.
- Probe: `GetAccessToken` answered at once with a token valid for another
  1669 seconds.

**Decision**: the credential kind is chosen by the interface the account
object exports, not by provider type: OAuth2Based → token, else PasswordBased
→ password, else the settings are incomplete. GOA's own data model says which
credential an account has; no provider knowledge enters goa-adapter beyond
recognition. `expires_in` is read for the signature and discarded: a load
lasts seconds and GOA already renewed anything close to expiry.

## 2. Signing in with the token

Google's [XOAUTH2 page](https://developers.google.com/workspace/gmail/imap/xoauth2-protocol):
the initial client response is `base64("user=" user "^Aauth=Bearer " token
"^A^A")`; on failure the server sends a `+` continuation carrying base64 JSON
`{"status":"400|401","schemes":"Bearer","scope":"https://mail.google.com/"}`
and the client must answer with an empty line, after which the tagged `NO`
arrives. The page shows the SASL-IR form only.

Probe, with the async-imap fork at 3c4cdde:

- `AUTHENTICATE XOAUTH2` without an initial response: Gmail answers an empty
  `+`, the fork's `Authenticator` supplies the credentials, sign-in succeeds.
  No SASL-IR support is needed.
- A wrong token: challenges received `["", "{\"status\":\"400\",...}"]`, the
  fork's handshake loop sent the empty reply, and the result is
  `Error::No(StatusResponse { code: AUTHENTICATIONFAILED, text: "Invalid
  credentials (Failure)" })`, the same shape 002 already maps to
  `Failed(SignIn)` with the server's reply.
- Pre-authentication capabilities: `IMAP4rev1 UNSELECT IDLE NAMESPACE QUOTA
  ID XLIST CHILDREN X-GM-EXT-1 XYZZY SASL-IR AUTH=XOAUTH2 AUTH=PLAIN
  AUTH=PLAIN-CLIENTTOKEN AUTH=OAUTHBEARER`. After sign-in Gmail sends an
  untagged `CAPABILITY` with `UIDPLUS COMPRESS=DEFLATE ENABLE MOVE CONDSTORE
  ESEARCH UTF8=ACCEPT LIST-EXTENDED LIST-STATUS LITERAL- SPECIAL-USE
  APPENDLIMIT=35651584`; the fork forwards it on the unsolicited channel. No
  `QRESYNC`, no `LITERAL+`.

**Decision**: `Credential::AccessToken` signs in with XOAUTH2 through the
existing `authenticate` path and a second `Authenticator`; the response is
sent once and later challenges get the empty reply, exactly like
`PlainCredentials`. `AUTH=XOAUTH2` absent from the pre-authentication list is
`NoSignInMethod`. OAUTHBEARER is not used: Google documents XOAUTH2 for IMAP
and the fork needs no change for it. The untagged capability list after
sign-in is logged at debug from `ServerNotices` so the record shows what
Gmail offers.

## 3. Gmail's attributes on the row FETCH

Google's [extensions page](https://developers.google.com/workspace/gmail/imap/imap-extensions):
`X-GM-MSGID` and `X-GM-THRID` are 64-bit unsigned integers; `X-GM-LABELS`
is a parenthesized list of labels "encoded in UTF-7 as appropriate", system
labels written with a leading backslash (`\Inbox`, `\Sent`, `\Drafts`,
`\Junk`, `\Trash`, `\Flagged`, `\All`, `\Important`); the web interface shows
the message identifier in hexadecimal.

Checked in the forks: imap-proto `parser/gmail.rs` parses all three
attributes in FETCH responses and is wired into `msg_att`; async-imap's
`Fetch` exposes `gmail_msg_id()` and `gmail_labels()`, but no accessor for
`X-GM-THRID`, and `Fetch.response` is private.

Probe on the Inbox window of 20 messages: every row carried UID, message
identifier, thread identifier and a label list, and the typed FETCH parsed
them all.

**Decisions**: `fetch_rows(RowItems::WithGmailAttributes)` adds `X-GM-MSGID
X-GM-LABELS` to the row FETCH and fills `MessageRow.gmail`. The thread
identifier is deferred to conversations (spec, Deferred): reading it needs a
fork accessor, and nothing in this feature reads it.

## 4. Readable names: `ENABLE UTF8=ACCEPT`

Probe:

- By default Gmail sends localized folder names in modified UTF-7, for
  example `[Gmail]/&BBIEQQRP- &BD8EPgRHBEIEMA-` for "[Gmail]/Вся почта", and
  user labels the same way inside `X-GM-LABELS`.
- `ENABLE UTF8=ACCEPT` after EXAMINE: `BAD ENABLE not allowed now.` Before any
  EXAMINE, or after UNSELECT: `* ENABLED UTF8=ACCEPT` then `OK`. RFC 5161
  allows ENABLE only in the authenticated state.
- After it, LIST names and `X-GM-LABELS` come as UTF-8 quoted strings
  ("ЯрлыкВерхний/ЯрлыкВложенный"); imap-proto parses them (the fork carries
  the UTF-8 text backport); EXAMINE with a UTF-8 name works.
- No dependency decodes modified UTF-7: not imap-proto, async-imap or
  mail-parser.

**Decision**: offer `UTF8=ACCEPT` once, after sign-in and before EXAMINE,
through `InboxReader::open(account, OpenOptions { readable_names, .. })`; the
Gmail load sets it, the Generic IMAP load does not. The tagged result is
logged at debug; a refusal leaves names as sent. The untagged `ENABLED` line
needs no parser of its own: imap-proto parses it into the same response as
`CAPABILITY`, and the announced-list line in `ServerNotices` records it.
Rejected: a UTF-7 decoder, own or a crate, because the server offers UTF-8
itself.

## 5. Evidence for later features (no code here)

Recorded so that storage, folders and conversations start from facts:

- `X-GM-LABELS` omits the label of the opened folder: in the Inbox the list
  was `()` for messages that only had `\Inbox`; in All Mail the same messages
  showed `\Inbox`. Full membership is visible from All Mail. Not documented by
  Google; observed 2026-09-22.
- `UID SEARCH X-GM-MSGID n` in All Mail found each of the three newest Inbox
  messages, with different UIDs (821 for Inbox 774) and a different
  UIDVALIDITY (All Mail 12, Inbox 1). Identity across folders is the message
  identifier, as the spec's FR-004 states.
- CONDSTORE: EXAMINE INBOX (CONDSTORE) reported HIGHESTMODSEQ 90283 while the
  largest MODSEQ of any Inbox message was 90205; `CHANGEDSINCE 90204` returned
  exactly the newest message. HIGHESTMODSEQ is account-wide; it serves as the
  next "since" value but must not be compared with message MODSEQs. No
  QRESYNC.
- Folder names are localized per account language; special folders are
  recognized by `NameAttribute::All/Trash/Sent/Flagged/Junk/Drafts`, and
  `\Important` arrives as an extension attribute. `INBOX` keeps its standard
  name.
- Nested user labels arrive as one name with `/`.

## 6. Client identification and the record

Google asks clients to send `ID`. Probe: the reply is `name GImap, vendor
Google, Inc., version gmail.imap-server_…, support-url …, remote-host
<client's public IP>, connection-token <opaque>`. `Session::id()` exists in
the fork and works after EXAMINE (RFC 2971 allows ID in any state).

**Decisions**: `identify_client` sends `name` and `version` after EXAMINE and
logs only the reply's `name`, `vendor` and `version` at debug, never
`remote-host` or `connection-token` (003 FR-009: no personal data). New debug
lines in this feature: the capability list after sign-in, the ENABLE result,
the ID reply fields, and per row the message identifier and labels. The token
never reaches a line: it lives in `Credential`, which has no Debug or Display,
and the fork's trace is compiled off. Label names are folder-like names and
stay at debug (003 FR-010).

## 7. Gmail's limits and lifetimes

From Google's pages: IMAP sessions last up to about 24 hours, OAuth sessions
about the token's validity (usually one hour), after which Gmail closes the
connection with a message that the session expired; 15 clients at a time per
account ("Too many simultaneous connections"); 2500 MB per day download and
500 MB upload over IMAP, with a suspension of about one hour, up to 24 hours;
the "Folder size limits" setting defaults to no limit, with options from
1,000 to 10,000 messages (secondary sources; Google's page does not state the
default).

**Decision**: no mechanism. A load takes seconds on one connection that is
dropped at the end; each refusal is a NO or BYE with text that 002 already
shows. After the user revokes access at Google, GOA may hand out its cached
token until it is near expiry; the refresh then fails as a rejected sign-in
until GOA renews. `EnsureCredentials` would not make GOA notice earlier:
checked in `goaoauth2provider.c`, for an OAuth account it calls the same token
function as `GetAccessToken` without forcing a refresh, so it returns the
cached token without an error until that token is near expiry; its only
extra effect is that a failed refresh sets AttentionNeeded, which the daemon
also does by itself when the network changes (`goadaemon.c`). It stays
optional in the plan and is not built.

## 8. Crate layout: `mailbag-providers`

The maintainer chose (2026-09-22) to create the provider layer now rather
than keep two load sequences in the UI crate or wait for Graph. What moves is
what `mailbag` holds only "until a scheduling layer exists" (002 research
§9): the mail worker, `MailLoader` and `LoadsInbox`, the batch types and the
load sequence, with their tests. What stays in `mailbag`: `InboxController`
(UI state per account), the GTK models and all wording.

Why a crate and not a module: the crate is the boundary the compiler enforces;
the load sequence was the one place 002 could not guard. Why now: the second
provider is where a provider-specific type first needs a home that is neither
the protocol crate (which must not know providers) nor the window. Why one
crate for all providers: Graph will join it with libsoup; the contract between
providers is derived from the implementations living side by side.

Test support: `tests/support/record.rs` and `bus.rs` are shared by `#[path]`
today; the new crate includes them the same way, so nothing is copied.

## 9. Sign-in explanation

`window_ui.rs` adds "You can change this account's password in Online
Accounts." when the server blamed the credentials. For a Google account there
is no password to change; the remedy is to re-authorize the account. A
Gmail-specific failure kind would carry provider knowledge through three
crates. **Decision**: one sentence for every provider, "Check this account's
sign-in in Online Accounts.", and a new `ImapAccessError::AccessToken` wording
"Unable to get this account's authorization from Online Accounts. No server
sign-in was attempted." No other UI change.

## 10. Rejected and deferred

| Item | Why not now |
|---|---|
| SASL-IR for XOAUTH2 | One round trip saved; the fork has no support and Gmail accepts the plain form |
| OAUTHBEARER | Not documented by Google for IMAP; XOAUTH2 works |
| UTF-7 decoder | UTF-8 mode replaces it |
| X-GM-THRID | Fork accessor needed; no reader until conversations |
| All Mail, labels as folders, X-GM-RAW, CONDSTORE sync, reconnect after expiry | Deferred in the spec with their features |
| Provider trait or shared contract document | Two implementations exist now; the contract is written when Graph tests it (roadmap rule) |
