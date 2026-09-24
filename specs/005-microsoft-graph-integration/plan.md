# Implementation Plan: Microsoft 365 Integration

**Branch**: `claude/graph` | **Feature**: `005-microsoft-graph-integration`
**Date**: 2026-09-23 | **Spec**: [spec.md](spec.md)
**Status**: Implemented on `claude/graph` and accepted live by the maintainer
2026-09-24; approved 2026-09-23 after the plan challenge, together with the
goa-adapter contract amendment and the one-function change in
mailbag-content; tasks are in [tasks.md](tasks.md); the simplify and
post-implementation reviews are in the last two sections.

## Size

The budget agreed on 2026-09-23 at feature-start, and this plan's estimate
after reading the code. Reassess with the maintainer before exceeding about
1.5 times an estimate.

| Item | Budget (2026-09-23) | This plan |
|---|---|---|
| New modules and production lines | mailbag-graph ~280; providers ~135; goa-adapter ~70; UI ~25. New ~510 | mailbag-graph ~230 plus ~80 in its scripted service (test support); goa-adapter ~75 plus ~15 in the fake GOA; providers ~120; mailbag-content 6 (one existing rule exposed); UI ~35. New ~465 |
| Call sites or existing files touched | ~14 | 19: goa-adapter (`imap_access.rs`, `lib.rs`, new `graph_access.rs`), providers (`lib.rs`, `worker.rs`, `batch.rs`, `load.rs`, new `microsoft365.rs`), mailbag-content (`lib.rs`), mailbag (`Cargo.toml`, `accounts.rs`, `window_ui.rs`, `mail_ui.rs`, `inbox.rs`), the root `Cargo.toml`, `cargo-sources.json`, `scripts/check.sh`, `scripts/setup.sh`, `README.md`. The rename `ImapAccessError` → `AccessError` and `ImapAccessRequest` → `AccessRequest` touches 11 files mechanically |
| New threads, timers, queues | 0 | 0. The wait limit of 002 (30 s) is set on the web library's session, which already owns the clock |
| New state, types, error types | ~8 | 9 types and 2 variants: `GraphAccess` (goa-adapter); `InboxPage`, `GraphMessage`, `Mailbox`, `GraphError`, `GraphFailure` (mailbag-graph); `MessageIdentity`, `IncompleteList`, `LoadKind` (providers; the last is crate-private); `LoadFailure::MicrosoftGraph`, `MailProvider::Microsoft365`. Over the budget by one: `IncompleteList` is the cheapest form of the spec challenge's decision 1; `Mailbox` is a plain name-and-address pair so that the display rule stays in mailbag-content |
| New fields in existing data | identity on the message | `ReceivedMessage.identity` replaces `uid`; `ReceivedBatch.incomplete` replaces `list_refusal` with a wider type |
| Changes to other features' contracts or documents | goa-access contract; 002 research §9; 002 FR-012; 004 FR-009 | The same four, with 004 SC-008 and its Assumption amended beside FR-009, plus mailbag-content exposes its display-name rule (a function, no behaviour change); this is the one exception to "no change to mailbag-content", taken for constitution IV |
| New dependencies | soup3, serde_json | `soup3` 0.9 (+ `soup3-sys`), `serde_json` 1 (+ `serde` without derive, `itoa`, `ryu`, `memchr`). Host builds need `libsoup3-devel` |
| Tests | ~17 | ~24 scenarios plus ~9 small unit cases: mailbag-graph 10 (and 10 unit cases for the answer reader), goa-adapter 6, providers 4, UI 4 (and 1 unit case in mailbag-content). Over the ~17 by about 40 %, under 1.5×: the plan challenge split the 401 and 429 wordings, and the consistency analysis added the empty-page and one-request checks |

## Summary

On Refresh Inbox for a Microsoft 365 account, `mailbag-providers` asks
goa-adapter for the account's access token only (the account carries no
server settings), then asks the new `mailbag-graph` crate for the newest 100
Inbox messages with their text in one web request to Microsoft Graph, and
turns the answer into the same batch the window shows for the other
providers. Each message's identity is the service's immutable identifier; its
text is the body the service rendered as text; its display fields are the
service's structured fields, formatted by the rule mailbag-content already
owns. A refused request fails the load with the service's status and error
code; a page that ends before 100 messages while the service offers more is
shown and reported as incomplete. The window changes only in that Microsoft
365 accounts become refreshable and the failure explanation learns the
service's reason.

`mailbag-graph` speaks HTTPS through libsoup 3 on the existing mail worker
thread and reads JSON with serde_json; it shares no code with `mailbag-imap`
and knows nothing about the window or the batch. `mailbag-providers` gains a
third load sequence next to the two IMAP ones ([research §7](research.md)).

## Minimal version

Everything below is built. Each line names its cost.

| Step | What it does | Cost |
|---|---|---|
| Token from GOA | `request_graph_access(account_id, on_complete)` in goa-adapter: find the account's object, require `OAuth2Based`, call `GetAccessToken`, return `GraphAccess { account_id, access_token }`. No Mail settings are read. `ImapAccessError` and `ImapAccessRequest` become `AccessError` and `AccessRequest`, shared by both requests; the variants stay | ~75 lines, the rename, 6 cases in 5 tests with the fake GOA (~15 fixture lines) |
| The list request | `mailbag-graph`: `list_inbox_messages(service_url, token, 100)` creates the web session for this load with the 30 s wait limit and a `Mailbag/<version>` user agent, sends one GET to `/me/mailFolders/inbox/messages` with `$top`, `$orderby=receivedDateTime desc`, `$select` of the seven fields, `Prefer: IdType="ImmutableId", outlook.body-content-type="text"` and the bearer token, and reads the JSON into `InboxPage { messages, more_available }`. Required in the answer: the object, `value`, each entry's `id` and `isRead`; the other fields are read when present and of the documented type, otherwise left out, and `body.contentType` is not examined. A non-200 answer is `GraphFailure::Refused { status, code }`, a transport error `ConnectionFailed` or `TimedOut`, an answer without the required shape `InvalidReply`. Tests shorten the wait limit through a second entry point, as `mailbag-imap` does | ~230 lines; scripted `soup::Server` for tests ~80; 8 tests |
| The load | `load_microsoft365_inbox(access, service_url)` in providers: list, then one `ReceivedMessage` per answer row with `MessageIdentity::GraphImmutableId`, display fields through mailbag-content's rule, the received time, the read state and `ReceivedContent::Text(body)`, or the existing `TextNotReturned` explanation when the answer holds no body for that message; `more_available` sets `ReceivedBatch.incomplete = Some(IncompleteList::MoreAvailable)`; one debug line per message | ~60 lines, 3 tests |
| The batch's shape | `ReceivedMessage.identity: MessageIdentity { ImapUid(u32), GraphImmutableId(String) }` replaces `uid`; `ReceivedBatch.incomplete: Option<IncompleteList { ServerRefused(ServerReply), MoreAvailable }>` replaces `list_refusal`; `LoadFailure::MicrosoftGraph(GraphError)`; the worker's request carries `LoadKind { GenericImap(ImapAccess), Gmail(ImapAccess), Microsoft365 { access: GraphAccess, service_url } }` instead of an access and a provider that could disagree, and the worker reports a `LoadResult` whose failure is of either kind | ~60 lines, test adaptations |
| Window | Microsoft 365 eligible in `mail_provider`; `graph_failure_status` words the four `GraphFailure` kinds, with the sign-in sentence of 004 when the status is 401; the incomplete-list notice gets its second form, "Not all messages in this account were loaded: the mail service offered more than one request holds."; the Settings wording loses "IMAP" | ~35 lines, 4 tests |
| Packaging | Workspace member; `check.sh` rules for the new crate; `cargo-sources.json` regenerated; `setup.sh` checks `libsoup-3.0` and its dnf line and the README setup list gain `libsoup3-devel`; the README's record description mentions the service's statuses and message identifiers | scripts and docs only |

Not built: a page loop, a retry, a token cache, `Retry-After`, a separate
secure-connection failure kind, the request identifier, the attachment
indication, a MIME path, folder listing, change tracking, a provider trait.

## Optional mechanisms

None is planned. Each would need the situation named beside it to occur.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| Follow `@odata.nextLink` until 100 messages | Acceptance shows the service answering with fewer than 100 while offering more; today the batch is reported as incomplete | ~40 lines, a rule for a message seen twice, 2 tests |
| Pass `Retry-After` on to the user | Users hit the shared allowance often enough that "refused, status 429" is not enough of an explanation | ~15 lines and a wording; belongs to error presentation |
| Ask GOA for a fresh token after a 401 | The service refuses a token that GOA renewed after Mailbag fetched it; a load takes seconds and GOA renews with minutes to spare | ~25 lines, a second GOA call inside the load |
| MIME through `$value` | A later feature needs the original message bytes; the HTML reader can ask the service for the HTML body instead | ~40 lines per message fetch, 100 requests per batch |

## Behaviour for this stage

| Situation | Choice |
|---|---|
| Microsoft 365 account selected, Refresh Inbox | Token from GOA, one request, up to 100 rows newest first with text; identifiers, received times and read states in the record at debug |
| The service answers 401 | `Refused { status: 401, code }`: "The mail service rejected the sign-in" with the code, plus "Check this account's sign-in in Online Accounts." |
| A message in the answer has no, or an empty, subject, sender, recipients, date or body | The row shows the window's existing fallbacks ("No subject", "Unknown sender", no To row, no date) or the existing "text not returned" explanation; the other messages load normally. Live: 9 of 100 messages had an empty recipient list, none lacked anything else |
| The service answers 403, 404, 429, 5xx | `Refused { status, code }`: "The mail service refused the request" with status and code; no retry |
| GOA cannot provide the token | `LoadFailure::OnlineAccounts(AccessToken)`, wording unchanged from 004 |
| The account's object has no `OAuth2Based` or is not listed | `LoadFailure::OnlineAccounts(Settings)`: "Unable to get this account's settings from Online Accounts." |
| Fewer than 100 messages and a further page offered | The batch is shown; the notice says the mail service offered more messages than one request holds |
| The connection fails or the certificate is refused | `ConnectionFailed` with the platform's reason |
| No answer within the wait limit | `TimedOut`: "The mail service stopped responding" |
| The answer is not the documented JSON (no object, no `value`, an entry without `id` or `isRead`) | `InvalidReply`: the load fails; nothing partial is shown |

## Technical Context

| Area | Design |
|---|---|
| Language/platform | Rust 2024, toolchain 1.95, GLib/GIO 0.22.9, libadwaita 0.9.2; unchanged |
| Web access | libsoup 3 through the `soup3` crate 0.9 (lib name `soup`), on the mail worker's own GLib context; one session per load; `send_and_read_future`; the default session refuses an unacceptable certificate; the session timeout is set to 30 s because the library's default is none ([research §2](research.md)). `glib::DateTime::from_iso8601` needs no feature |
| JSON | `serde_json` on `Value`, no derive ([research §3](research.md)) |
| GOA | `org.gnome.OnlineAccounts.OAuth2Based.GetAccessToken` → `(s i)`; a Microsoft 365 account's Mail interface carries only the address ([research §1](research.md)) |
| Service | `https://graph.microsoft.com/v1.0`; the request and its answer in [research §4](research.md); failures in [§5](research.md) |
| Content | mailbag-content's display-name rule, exposed as a function; no MIME |
| Logging | 003 rules; new lines at debug, the failure line at error as today |
| Storage | None; memory only |
| Testing | `cargo test`; fake GOA on the private bus (`tests/support/goa.rs`); scripted web service on `soup::Server` (`mailbag-graph/src/test_server.rs`, feature `test-support`); record capture (`tests/support/record.rs`) |
| Packaging | New workspace crate with two external dependencies: `cargo-sources.json` regenerated; the GNOME 50 runtime ships libsoup 3.6; host builds need `libsoup3-devel` (`setup.sh` and README); `deny.toml` unchanged (MIT and Apache-2.0) |

## Constitution Check

| Principle | Assessment |
|---|---|
| I — Necessary complexity | One request per load, no loop, no retry, no cache. The challenge of the spec removed the page loop, the wait, the secure-connection kind, the request identifier and the attachment indication. The service crate is the third provider's own protocol layer, as 002 research §9 foresaw |
| II — Clear language | `GraphAccess`, `InboxPage`, `MessageIdentity::GraphImmutableId`, `IncompleteList::MoreAvailable`, `GraphFailure::Refused { status, code }` say what they hold |
| III — Truthful failure | A refused request carries the service's status and code; a short page is reported as incomplete; a malformed answer fails the load; nothing is invented for a message without a folder number |
| IV — One owner | GOA's object decides that a token exists; the UI owns eligibility; providers own the load and the batch; mailbag-graph owns the request; mailbag-content owns the display-name rule for every provider, which is why it changes by one function |
| V — Responsive, bounded work | Same worker, one request, 30 s wait limit, batch of 100 |
| VI — Evidence | The probe results of 2026-09-23 are in research; live acceptance follows [quickstart](quickstart.md); refusals and short pages are covered by the scripted service |

No exception is proposed.

## Project Structure

### Documentation

```text
specs/005-microsoft-graph-integration/
├── spec.md
├── plan.md          # this file
├── research.md      # GOA, service and library facts; probe results; decisions
├── quickstart.md    # acceptance on the live account
└── tasks.md         # portions and review pauses
specs/002-imap-integration/contracts/goa-access.md   # amendment proposed by 005
specs/002-imap-integration/research.md               # §9 crate table amended
specs/002-imap-integration/spec.md                    # FR-012 amendment note
specs/004-gmail-integration/spec.md                   # FR-009 amendment note
```

No data model: nothing is persisted. No new contract document: the one shared
interface that changes is the goa-adapter access contract, amended in place.

### Source code

```text
crates/goa-adapter/src/graph_access.rs     # new: request_graph_access, GraphAccess
crates/goa-adapter/src/access_calls.rs     # AccessError, AccessRequest; calls both requests share
crates/goa-adapter/src/imap_access.rs      # request_imap_access over the shared calls
crates/goa-adapter/src/lib.rs              # exports
crates/mailbag-graph/                      # new crate
├── Cargo.toml
└── src/
    ├── lib.rs           # list_inbox_messages, InboxPage, GraphMessage, Mailbox, GraphError, GraphFailure
    ├── reply.rs         # JSON of the answer into GraphMessage; the error body into a code
    ├── test_server.rs   # scripted service on soup::Server (feature test-support)
    └── tests.rs         # request shape, answer, refusals, transport failures, record
crates/mailbag-content/src/lib.rs          # display_names exposed
crates/mailbag-providers/src/
├── lib.rs           # MailProvider::Microsoft365; start_load chooses the access request
├── worker.rs        # LoadKind; run_load dispatches three sequences
├── batch.rs         # MessageIdentity, IncompleteList, LoadFailure::MicrosoftGraph
├── imap_batch.rs    # identity and incomplete on the IMAP batch (was load.rs)
├── microsoft365.rs  # new: load_microsoft365_inbox
└── tests.rs         # Microsoft 365 load against the scripted service
crates/mailbag/src/accounts.rs             # eligibility
crates/mailbag/src/window_ui.rs            # graph_failure_status, notice, Settings wording
crates/mailbag/src/mail_ui.rs              # the identity in the "message opened" line
crates/mailbag/src/inbox.rs                # failure and incomplete lines
tests/support/goa.rs                       # fake Microsoft 365 object
scripts/check.sh                           # rules for mailbag-graph
scripts/setup.sh                           # libsoup-3.0 development files
README.md                                  # libsoup3-devel; record description
```

Dependency rules, extending 002 research §9:

| Crate | May depend on | Must not depend on |
|---|---|---|
| `mailbag-graph` | glib, gio, soup3, serde_json | gtk, libadwaita, mail-parser, `mailbag-imap`, `mailbag-content`, `mailbag-providers`, `mailbag` |
| `mailbag-providers` | as before, plus `mailbag-graph` | gtk, libadwaita, `mailbag` |

`scripts/check.sh` rejects the forbidden packages in `mailbag-graph` and adds
`mailbag-graph` to the lists of `goa-adapter`, `mailbag-imap` and
`mailbag-content`.

## Function map

**goa-adapter, `request_graph_access(account_id, on_complete)`** (new entry
point next to `request_imap_access`):
1. `read_objects` — GetManagedObjects over the observer's connection.
2. `find_account_object` — the object whose `Id` matches; shared with the
   IMAP request. Absent → `Settings`.
3. `require_oauth2` — the object exports `OAuth2Based`; otherwise `Settings`.
4. `read_access_token` — `GetAccessToken`; `expires_in` discarded; a D-Bus
   error → `AccessToken`, a timeout → `Timeout`, cancellation → `Cancelled`.
5. Build `GraphAccess { account_id, access_token }` and complete once.

**mailbag-graph, `list_inbox_messages(service_url, access_token, batch_size)`**:
1. `open_session` — a `soup::Session` for this load with the 30 s wait limit
   and the user agent; tests reach the same steps with a shorter limit.
2. `build_inbox_request` — the URL with `$top`, `$orderby`, `$select`; the
   `Authorization`, `Prefer` and `Accept` headers.
3. `send` — `send_and_read_future`; a transport error maps to
   `ConnectionFailed` or `TimedOut` by GIO's error kind; the debug line
   "request sent" carries the path.
4. `check_status` — 200 continues; anything else reads the error body's
   `error.code` and fails as `Refused { status, code }`, with the developer
   message kept for the debug line only.
5. `read_inbox_page` — `value` into `GraphMessage`s: `id` and `isRead`
   required; `subject`, `from`, `toRecipients`, `receivedDateTime` (as Unix
   seconds) and `body.content` read when present and of the documented type,
   otherwise left out; `@odata.nextLink` present → `more_available`. Only the
   required shape missing is `InvalidReply`.
6. The debug line "answer received": status, bytes, messages, more available.

**mailbag-providers, `load_microsoft365_inbox(access, service_url)`**:
1. `list_inbox_messages(service_url, &access.access_token, 100)`; `start_load`
   passes the constant `MICROSOFT_GRAPH`, tests the scripted service's URL.
2. For each message: `MessageIdentity::GraphImmutableId(id)`,
   `DisplayFields { subject, from: display_names(from), to: display_names(to) }`,
   `internal_date`, `seen`, `ReceivedContent::Text(body)` or
   `Explained(TextNotReturned)` when the answer held no body; one debug line
   with identifier, received time and read state.
3. `ReceivedBatch { account_id, uid_validity: None, messages, incomplete:
   more_available.then_some(IncompleteList::MoreAvailable) }`.

**mailbag-providers, `MailLoader::start_load(account_id, provider, report)`**:
`GenericImap` and `Gmail` request IMAP access as today and become
`LoadKind::GenericImap` or `LoadKind::Gmail`; `Microsoft365` requests Graph
access and becomes `LoadKind::Microsoft365`. The worker's `run_load(kind)`
runs the matching sequence; a failure of either kind becomes `LoadFailure`.

**mailbag, `graph_failure_status(error)`**: title and reason by `GraphFailure`;
`Refused` adds "The mail service said: status, code"; status 401 adds the
sign-in sentence; `incomplete_list_notice` words `ServerRefused` as today and
`MoreAvailable` as "Not all messages in this account were loaded: the mail
service offered more than one request holds."

## Contract amendment (approved 2026-09-23)

The shared goa-adapter access contract gains a second operation. The text is
in [goa-access.md](../002-imap-integration/contracts/goa-access.md) under
"Amendment by 005". In short: `request_graph_access` finds the
account's object, requires `OAuth2Based`, calls `GetAccessToken` and returns
`GraphAccess`; no Mail setting is read; the error and request types are
shared with the IMAP operation under provider-neutral names; everything else,
including "no EnsureCredentials" and the privacy rules, stays.

## Implementation portions

One intended feature PR: **Microsoft 365 integration**. The maintainer
creates commits and the PR. After each portion: run its checks and
`scripts/check.sh`, report what changed, evidence, limitations, a suggested
commit and the intended PR, then **stop for review**. Start the next portion
only on explicit instruction.

| Portion | Reviewable result | Checks and proposed commit |
|---|---|---|
| 1 — Service crate | `mailbag-graph` with `list_inbox_messages`, its types and failures, the scripted service and its tests; workspace member, `check.sh` rules, `cargo-sources.json`, `setup.sh` and README prerequisite. The application's behaviour is unchanged | Ten scenarios: request shape (path, query, bearer, both preferences in one header); two messages with text bodies and fields, one of them without recipients; short page with a further page offered; 401 with code; 429 with code; answer without the required shape; connection refused; no answer within the limit; the token absent from the record; cancellation closes the request; plus unit cases for the answer reader, including an empty `value` as an empty page. Flatpak build. `feat(graph): read a mailbox's Inbox from Microsoft Graph` |
| 2 — Mechanical, no behaviour change | The rename to `AccessError` and `AccessRequest`; `MessageIdentity` with both variants; `IncompleteList` with both variants and the notice's second form, which nothing produces yet; `LoadKind` with `GenericImap` and `Gmail`, replacing the access and provider pair in the worker, and the worker reporting a `LoadResult`; all existing tests adapted | All existing tests pass; the "message opened" line names the identity; `check.sh` clean without any `allow`. `refactor: name each message's identity by its provider's kind` |
| 3 — GOA access | `request_graph_access`, `GraphAccess`, the fake GOA's Microsoft 365 object, the Settings wording without "IMAP" | Six goa-adapter scenarios: token returned; account absent; no `OAuth2Based`; `GetAccessToken` fails; held reply times out; cancel and drop. `feat(goa): provide the access token of a Microsoft 365 account` |
| 4 — Microsoft 365 load and window | `LoadKind::Microsoft365`, `LoadFailure::MicrosoftGraph`, mailbag-content's `display_names`, `microsoft365.rs`, `start_load` for the third provider, eligibility, `graph_failure_status`, debug lines; live acceptance per quickstart | Four providers scenarios: batch from the scripted service with identities, fields, text and no incomplete notice; short page with the notice; refused request as `MicrosoftGraph` with exactly one request sent; token never logged and each message named by its identifier. Three window tests for eligibility and the 401 and 429 wordings. `feat: load the Inbox of Microsoft 365 accounts` |

The service crate comes first because it is the part with the new
dependency and the new protocol; it can be reviewed and built for Flatpak
before anything in the application changes. The mechanical portion holds
only what compiles clean on its own: a crate-private variant that nothing
constructs would fail the lint, so `LoadKind::Microsoft365` and
`LoadFailure::MicrosoftGraph` arrive with their producers in portion 4.

Compare the size with the table above at every pause.

## Plan challenge (2026-09-23)

A fresh reviewer challenged this plan's mechanisms against the code. Applied
without changing scope: `GraphService` removed, the session is created
inside `list_inbox_messages` and tests shorten its wait limit through a
second entry point as `mailbag-imap` does; the answer reader requires only
the object, `value`, `id` and `isRead`, reads the other fields when present
and does not examine `body.contentType`, so one odd message no longer fails
the load (the spec's Clarifications accept markup shown as text); a message
without a body gets the existing "text not returned" explanation; the
portions are recut so that the mechanical one compiles clean and holds no
behaviour, with the two variants that only the load produces moved to portion
4; `LoadFailure::Service` is named `MicrosoftGraph`; the incomplete notice's
second form no longer claims that messages "could not be loaded"; the wrong
glib feature line is gone; `scripts/setup.sh` joins the packaging row. The
estimate fell from ~485 to ~465 new lines and from 10 to 9 types.

Kept after the challenge, with the reviewer's agreement: the rename of the
access error and request types (the cheapest honest naming, ~60 mechanical
lines), `display_names` exposed by mailbag-content (constitution IV; 6 lines
against a 4-line duplicate that would drift), `InboxPage`, `Mailbox`,
`IncompleteList` and `LoadKind`.

The reviewer's gap, empty sender names arriving as empty strings, was
checked live: over the newest 100 messages no name, address, subject, date
or body was missing or empty; 9 messages had an empty recipient list, which
the display rule already turns into no To row. Nothing is built for it.

## Simplify review (2026-09-24)

After live acceptance a fresh reviewer read the provider layer of all three
providers. Applied, with no change to behaviour except the last item: both
Online Accounts requests share their D-Bus calls and types
(`access_calls.rs`); `ServerFailure` is gone and the IMAP failure travels as
`LoadFailure::Imap(ImapError)`, worded by `imap_failure_status` next to
`graph_failure_status`; `LoadOutcome` is gone and the worker reports a
`LoadResult`; types no other crate uses are crate-private; the batch size has
one owner in `mailbag-providers`, which passes it to `fetch_rows`; `load.rs`
is `imap_batch.rs`; the Graph error line names its status and code as fields;
the incomplete-list notice appears only for a batch the window keeps (a batch
discarded by an exclusion no longer produces one). Declined: narrowing
`GraphError`, the unreachable Cancelled wording, and merging test helpers.

## Post-implementation reviews (2026-09-24)

After the simplify review, a fresh reviewer read the branch for behaviour
defects and another for security. `scripts/check.sh` was green before and
after. Applied, at the maintainer's decision: an empty subject, name or
address in the service's answer is treated as absent (`reply.rs`,
`present_text`), so the window shows "No subject" and the name rule falls
back to the address; an empty body stays the message's text (FR-005). Not
applied, at the maintainer's decision: masking the account's address in the
service's error text at debug (the error texts of `/me` requests carry no
address) and blocking a same-host redirect from `https` to `http` (only the
service itself could issue one, over a verified connection). No
vulnerability was found: the token reaches only the `Authorization` header,
the session verifies certificates with no exception, every service string
ends in a plain-text label, and the six new crates come from crates.io with
matching checksums in `Cargo.lock` and `cargo-sources.json`.

Measured after the reviews, against the estimates above: mailbag-graph
about 210 production lines (estimate 230) and about 210 in the scripted
service (estimate 80, the one clear overrun: the service records requests
and offers the stalled listener); goa-adapter about 110 (75), providers
about 70 (120), the window about 80 (35, including the simplify review's
reshaping of the IMAP wording). The feature stays under the ~510 budget.

The pull request's first run showed that the CI check job, which builds in a
Fedora container, needed `libsoup3-devel` as well; the workflow's package
list gained it beside README and `setup.sh`.
