# Implementation Plan: Gmail Integration

**Branch**: `claude/gmail` | **Feature**: `004-gmail-integration`
**Date**: 2026-09-23 | **Spec**: [spec.md](spec.md)
**Status**: Draft for maintainer review; revised 2026-09-23 after the plan
challenge (see the last section). The goa-adapter contract amendment below
was approved on 2026-09-23; tasks are in [tasks.md](tasks.md).

## Size

The budget agreed on 2026-09-22, and this plan's estimate after the
specification challenge of 2026-09-23. Reassess with the maintainer before
exceeding about 1.5 times an estimate.

| Item | Budget (2026-09-22) | This plan |
|---|---|---|
| New modules and production lines | goa-adapter ~120; mailbag-imap ~120; new crate ~150 new + ~400 moved; UI ~40. New 450–550 | goa-adapter ~60; mailbag-imap ~185 plus ~145 in the scripted test server and ~45 in the fake GOA; `mailbag-providers` ~160 new + ~640 moved (tests are the larger part); UI ~25. New ~435 |
| Call sites or existing files touched | ~12 | 14: goa-adapter (`imap_access.rs`, `accounts.rs`, `lib.rs`), mailbag-imap (`lib.rs`, `session.rs`, `reader.rs`, `test_server.rs`), mailbag (`main.rs`, `window_ui.rs`, `accounts.rs`, `inbox.rs`, `mail_ui/tests.rs`; `inbox_load.rs` leaves), `Cargo.toml`, `scripts/check.sh` |
| New threads, timers, queues | 0 | 0; the existing mail worker serves both providers |
| New state, types, error types | 4–6 | 7 types and one variant: `ImapCredential` (goa-adapter); `Credential`, `OpenOptions`, `ClientIdentity`, `RowItems`, `GmailRow` (mailbag-imap); `MailProvider` (mailbag-providers); `ImapAccessError::AccessToken`. Over the budget by one: `ClientIdentity` and `RowItems` are plain argument types of `open` and `fetch_rows`, holding no state |
| New fields in existing data | row attributes, provider identity on the message, Google eligibility | `MessageRow.gmail` and `ReceivedMessage.gmail`, both `Option<GmailRow>`; eligibility is a function, not a field |
| Changes to other features' contracts or documents | goa-access contract; 002 FR-012; 002 research §9 | The same three, plus `LoadsInbox::start_load` gains the provider argument (internal to the workspace) |
| New dependencies | 0, maybe a UTF-7 decoder | 0. No fork change |
| Tests | ~15 scenarios, ~500 lines | ~17 scenarios: goa-adapter 5, mailbag-imap 8, providers 2 (plus the 16 moved), UI 2 |

The move of the load sequence is mechanical and is its own portion, so a
reviewer can separate it from behaviour changes.

Revised 2026-09-23 after portion 1, with the maintainer's agreement. The
`mailbag-imap` figure rose from ~150 to ~185 for two decisions taken while
building it: the client identification carries `vendor`, `contact` and
`support-url` as Google's example asks, and `ID` is sent with
`run_command_and_check_ok` instead of `Session::id` so that a refusal is a
real error rather than a reply without fields ([research §6](research.md)).
The scripted server rose with it, to record what the client sent. The
feature's total stays inside the agreed 450–550.

## Summary

On Refresh Inbox for a Google account, `mailbag-providers` asks goa-adapter for
the account's IMAP settings and access token, signs in to Gmail with the
documented OAuth mechanism instead of a password, offers UTF-8 names, opens
the Inbox, identifies Mailbag to Gmail, and loads the same batch as for a
Generic IMAP account with two extra attributes per row: Gmail's message
identifier and labels. Those go into the record at debug and onto the received
message. The window changes only in that Google accounts become refreshable and
the sign-in explanation no longer talks about a password.

The load sequence, the mail worker and the batch types move out of the
`mailbag` UI crate into the new `mailbag-providers` crate, the provider layer
that [002 research §9](../002-imap-integration/research.md#9-crate-layout)
left for the second provider. Gmail and Generic IMAP are two load sequences in
that crate over one protocol crate; the protocol crate learns to sign in with
a token and to fetch the attributes it is asked for, and nothing else about
Gmail.

## Minimal version

Everything below is built. Each line names its cost.

| Step | What it does | Cost |
|---|---|---|
| Credential from GOA | `request_imap_access` reads the Mail settings as today, then asks the credential interface the account object exports: PasswordBased → `GetPassword`, OAuth2Based → `GetAccessToken`. `ImapAccess.password` becomes `credential: ImapCredential { Password, AccessToken }` | ~60 lines, one error variant `AccessToken`, 5 tests with the fake GOA |
| Sign-in with a token | `mailbag-imap` `Credential::AccessToken` signs in with `AUTHENTICATE XOAUTH2`; the response is the documented string; a second challenge (Gmail's error JSON) gets the empty reply the fork already sends. No `AUTH=XOAUTH2` in the capabilities → `NoSignInMethod` | ~50 lines, 3 transcripts in the scripted server |
| Readable names and identification | `InboxReader::open(account, OpenOptions)` with two fields: `readable_names: bool` sends `ENABLE UTF8=ACCEPT` after sign-in and before EXAMINE; `client_identity: Option<ClientIdentity { name, version, vendor, contact, support_url }>` sends `ID` right after it (`ID` is allowed in any state). The fields are the ones Google's example asks for, plus `support-url` ([research §6](research.md)). Both commands go through `run_command_and_check_ok`, so a refusal is logged at debug and never fails the load; the server's untagged reply reaches `ServerNotices`, which logs the ID reply's `name`, `vendor` and `version`, nothing else. The Gmail load sets both, the Generic IMAP load neither | ~85 lines, 4 tests |
| Gmail row attributes | `fetch_rows(RowItems)`: `Standard` or `WithGmailAttributes`, which adds `X-GM-MSGID X-GM-LABELS`; `MessageRow.gmail: Option<GmailRow { message_id, labels }>` from the fork's accessors | ~40 lines, 1 test |
| Capabilities after sign-in | `ServerNotices::keep` logs an untagged capability list at debug. Gmail sends its full list only after sign-in, and imap-proto parses the `ENABLED` reply into the same response type, so the line says "the server announced" a list, not "capabilities" | ~8 lines, covered by the sign-in and ENABLE tests |
| Provider crate | `mailbag-providers`: worker, `MailLoader`, `LoadsInbox`, batch types and the load sequence move as they are; `gmail.rs` adds the Gmail load; `start_load` takes the provider; `ReceivedMessage.gmail: Option<GmailRow>` | ~640 moved, ~160 new, 2 tests; workspace member, two `check.sh` rules; fake GOA gains an OAuth2Based object (~45 test lines) |
| Window | Refresh Inbox and the "no mail loaded" hint accept Google; one neutral sentence; one wording for the `AccessToken` error; `ScriptedLoader` takes the provider | ~25 lines, 2 tests |

Not built: a UTF-7 decoder (UTF-8 mode makes it unnecessary), a Gmail failure
kind, a thread identifier, a token cache, any retry, `EnsureCredentials`, a
provider trait, a data model, folder listing.

## Optional mechanisms

None is planned. Each would need the situation named beside it to occur.

| Mechanism | Situation that would require it | Cost if needed |
|---|---|---|
| `EnsureCredentials` after a refused token | Users report that Online Accounts shows nothing wrong for up to an hour after they revoke access at Google. Checked in GOA's source: for an OAuth account the method only calls the same token function as `GetAccessToken` without forcing a refresh, so it returns the cached token without an error until that token is near expiry; its one effect is that a refresh failure sets AttentionNeeded. It would not shorten the hour | ~30 lines and a contract change, for no earlier warning; GOA re-checks on its own when the network changes |
| Retry sign-in once with a fresh token | Gmail refuses a token that GOA renewed after Mailbag fetched it; a load takes seconds, GOA renews at ten minutes left | ~25 lines, a second GOA call inside the load |
| `X-GM-THRID` accessor in the async-imap fork and a `thread_id` field | Conversations (feature 012) | fork patch, tag, rev bump, ~15 lines |

## Behavior for this stage

| Situation | Choice |
|---|---|
| Google account selected, Refresh Inbox | Same load as 002 with a token; up to 100 rows; Gmail fields in the record at debug |
| Token refused by Gmail | `Failed(SignIn)` with Gmail's reason, as any rejected sign-in; the sentence "Check this account's sign-in in Online Accounts." replaces the password sentence for every provider |
| GOA cannot provide the token | `LoadFailure::OnlineAccounts(AccessToken)`: "Unable to get this account's authorization from Online Accounts. No server sign-in was attempted." |
| `ENABLE UTF8=ACCEPT` refused | Logged at debug; names recorded as sent; load continues |
| `ID` refused | Logged at debug; load continues |
| Gmail closes the session, refuses for a limit or for an administrator's policy | 002's failure paths: the step and the server's text; no retry |
| Microsoft 365 account selected | Unchanged: Refresh Inbox unavailable, "Mailbag cannot load mail for this account yet." |

## Technical Context

| Area | Design |
|---|---|
| Language/platform | Rust 2024, toolchain 1.95, GLib/GIO 0.22.9, libadwaita 0.9.2; unchanged |
| IMAP | async-imap fork 3c4cdde, imap-proto fork caa2c81; unchanged. `Client::authenticate` handles the XOAUTH2 error exchange; `Fetch::gmail_msg_id`/`gmail_labels` exist; ENABLE and ID are sent with `run_command_and_check_ok`, because `Session::id` does not check its command's completion ([research §6](research.md)) |
| GOA | `org.gnome.OnlineAccounts.OAuth2Based.GetAccessToken` → `(s i)`; Google accounts export Mail with `ImapUseSsl` true and no PasswordBased ([research §1](research.md)) |
| Content | mail-parser through `mailbag-content`; unchanged |
| Logging | 003 rules; new lines at debug only ([research §6](research.md)) |
| Storage | None; memory only |
| Testing | `cargo test`; fake GOA on the private bus (`tests/support/goa.rs`), scripted IMAP server (`mailbag-imap/test_server.rs`), record capture (`tests/support/record.rs`, shared by `#[path]`) |
| Packaging | New workspace crate with no new external dependency: `cargo-sources.json` unchanged; Meson builds the workspace; `scripts/check.sh` gets rules for the new crate |

## Constitution Check

| Principle | Assessment |
|---|---|
| I — Necessary complexity | Every mechanism above names a present situation. The provider crate is the maintainer's decision for the second provider. No retry, cache, decoder, trait or failure kind. |
| II — Clear language | Names say what they hold: `ImapCredential`, `RowItems`, `GmailRow`, `identify_client`. Gmail's own terms appear where the code talks to Gmail. |
| III — Truthful failure | A refused token is a rejected sign-in with the server's reason; a missing token is an account-service failure; ENABLE and ID refusals are logged, never hidden as success. |
| IV — One owner | GOA's object decides the credential kind; the UI owns eligibility; `mailbag-providers` owns the load and the batch; the protocol crate owns commands. |
| V — Responsive, bounded work | Same worker, same window of 100, one connection per load. |
| VI — Evidence | The 2026-09-22 probe results are recorded in research; live acceptance follows [quickstart](quickstart.md). |

No exception is proposed.

## Project Structure

### Documentation

```text
specs/004-gmail-integration/
├── spec.md
├── plan.md          # this file
├── research.md      # Gmail and GOA facts, probe results, decisions
├── quickstart.md    # acceptance on the live account
└── tasks.md         # portions and review pauses
specs/002-imap-integration/contracts/goa-access.md   # amended (approved 2026-09-23)
specs/002-imap-integration/research.md               # §9 crate table amended
```

### Source code

```text
crates/goa-adapter/src/imap_access.rs      # credential by interface; ImapCredential
crates/mailbag-imap/src/lib.rs             # Credential, OpenOptions, RowItems, GmailRow
crates/mailbag-imap/src/session.rs         # XOAUTH2 sign-in, ENABLE, ID, announced-list line
crates/mailbag-imap/src/reader.rs          # open(account, options), fetch_rows(RowItems)
crates/mailbag-imap/src/test_server.rs     # XOAUTH2, ENABLE, ID, X-GM items
crates/mailbag-providers/                  # new crate
├── Cargo.toml
└── src/
    ├── lib.rs        # MailLoader, LoadsInbox, CancelsLoadOnDrop, LoadResult
    ├── worker.rs     # MailWorker, LoadRequest, run_worker (moved)
    ├── batch.rs      # ReceivedBatch, ReceivedMessage, ReceivedContent, failures (moved)
    ├── load.rs       # shared steps after the row FETCH (extracted in portion 4)
    ├── imap.rs       # Generic IMAP load (moved)
    ├── gmail.rs      # Gmail load (new)
    └── tests/        # moved load tests + Gmail tests
crates/mailbag/src/inbox.rs                # InboxController keeps only UI state
crates/mailbag/src/window_ui.rs            # eligibility, wording
crates/mailbag/src/main.rs                 # builds MailLoader from the new crate
crates/mailbag/src/mail_ui/tests.rs        # ScriptedLoader takes the provider
tests/support/goa.rs                       # fake OAuth2Based object, GetAccessToken
```

Dependency rules, extending 002 research §9:

| Crate | May depend on | Must not depend on |
|---|---|---|
| `mailbag-providers` | glib, gio, goa-adapter, mailbag-imap, mailbag-content | gtk, libadwaita, `mailbag` |
| `mailbag` | as before, plus `mailbag-providers` | — |

`scripts/check.sh` rejects gtk/libadwaita in `mailbag-providers` and adds it
to the lists the other library crates must not depend on.

## Function map

**goa-adapter, `request_imap_access`** (unchanged entry point):
1. `read_settings` — GetManagedObjects; find the account's object.
2. `find_imap_settings` — host, login, encryption as today; also which
   credential interface the object exports (OAuth2Based, else PasswordBased,
   else `Settings`).
3. `read_credential` — GetAccessToken or GetPassword; build `ImapAccess` with
   `credential`. A D-Bus error is `AccessToken` or `Password` for its kind.

**mailbag-providers, `load_gmail_inbox(access)`**:
1. `InboxReader::open(account, OpenOptions { readable_names: true,
   client_identity: Some(Mailbag, version, vendor, contact, support-url) })` — connect, sign in with the
   token, offer UTF-8 names, identify, EXAMINE INBOX.
2. `reader.fetch_rows(RowItems::WithGmailAttributes)` — rows with
   `GmailRow`; one debug line per row: uid, `gmail_message_id`, `labels`.
3. `load_batch_from_rows(reader, rows)` — shared with the Generic IMAP load:
   structures, part selection, text, assembly.
4. Move each row's `GmailRow` onto its `ReceivedMessage.gmail`.

**mailbag-providers, `load_imap_inbox(access)`**: today's
`load_inbox_batch` with `OpenOptions::default()` and `RowItems::Standard`.

**mailbag-providers, `MailLoader::start_load(account_id, provider, report)`**:
`provider` is `MailProvider { GenericImap, Gmail }`, the crate's own type; the
window turns the account's `AccountProvider` into it in one eligibility
function (`mail_provider` in `accounts.rs`), so `AccountProvider` never enters
this crate. Request access from GOA; on success hand `access` and `provider`
to the worker, which runs `load_imap_inbox` or `load_gmail_inbox`.

**mailbag-imap, `sign_in`**: capabilities as today; `Credential::Password` →
PLAIN or LOGIN as today; `Credential::AccessToken` → `AUTH=XOAUTH2`
advertised → `authenticate("XOAUTH2", XOAuth2Credentials)`, else
`NoSignInMethod`. Then, from `OpenOptions`: ENABLE, then ID, then EXAMINE.

## Contract amendment (approved 2026-09-23)

The shared goa-adapter access contract changes in one place: the credential
step. The text is in
[goa-access.md](../002-imap-integration/contracts/goa-access.md) under
"Amendment proposed by 004". In short: `ImapAccess.password` becomes
`credential`; the object's exported interface chooses GetPassword or
GetAccessToken; `ImapAccessError` gains `AccessToken`; `expires_in` is read and
discarded; everything else, including "no EnsureCredentials", stays.

## Implementation portions

One intended feature PR: **Gmail integration**. The maintainer creates commits
and the PR. After each portion: run its checks and `scripts/check.sh`, report
what changed, evidence, limitations, a suggested commit and the intended PR,
then **stop for review**. Start the next portion only on explicit instruction.

| Portion | Reviewable result | Checks and proposed commit |
|---|---|---|
| 1 — Protocol | `Credential`, XOAUTH2 sign-in, `OpenOptions` (ENABLE and ID), `RowItems`/`GmailRow`, announced-list line; scripted server support. `mailbag` adapted without behaviour change. | mailbag-imap transcripts: token accepted; token refused with the JSON challenge; no XOAUTH2 offered; ENABLE OK and NO; ID OK and NO with only three fields logged; rows with Gmail items; token absent from the record. `feat(imap): sign in with an access token and read Gmail attributes` |
| 2 — GOA credential | `ImapCredential`, credential by exported interface, `AccessToken` error; fake GOA answers `GetAccessToken`; window wording for the new error. | goa-adapter tests: token account, password account, token refused, held reply, neither interface. `feat(goa): provide the access token of an OAuth account` |
| 3 — Provider crate (move only) | `mailbag-providers` with worker, loader, batch types and the load sequence moved; `mailbag` wired to it; `check.sh` rules. No behaviour change. | All existing tests pass unchanged in their new home; `check.sh` rejects a forbidden dependency; Flatpak build. `refactor: move the load sequence into mailbag-providers` |
| 4 — Gmail load and window | `gmail.rs`, `start_load` with provider, `ReceivedMessage.gmail`, debug lines; Google eligible in the window; neutral sign-in sentence. | Providers tests: Gmail batch with its fields and the ENABLE and ID commands in the transcript; token never logged. Window tests for eligibility; live acceptance per quickstart. `feat: load the Inbox of Google accounts` |

The protocol comes first because the credential from GOA can only be mapped
once the protocol crate accepts a token; with GOA first, portion 1 would have
no honest way to carry an `AccessToken` into `ImapAccount`.

Compare the size with the table above at every pause.

## Plan challenge (2026-09-23)

A fresh reviewer challenged this plan's mechanisms. Applied without changing
scope: `ReceivedMessage.gmail` reuses `mailbag_imap::GmailRow` instead of a
second type; `open` takes a named `OpenOptions` instead of a list of extension
names; `ID` is sent inside `open` right after `ENABLE` instead of a separate
reader step; the untagged list line is worded for both CAPABILITY and
`ENABLED`, which imap-proto parses into one response type; the fake GOA
object and `mail_ui/tests.rs` are now counted. The estimate fell from ~475 to
~400 new lines.

**D1, decided by the maintainer 2026-09-23: two load functions over shared
steps, as planned.** Reasoning kept for the record. The reviewer notes that the Gmail load differs from the Generic IMAP
load in exactly four places (open options, row items, attaching `GmailRow`)
and proposes one `load_inbox_batch(access, provider)` with `match provider`
arms, saving `load_batch_from_rows` and about 60 lines. The plan keeps two
functions, `load_imap_inbox` and `load_gmail_inbox`, over shared steps.
Recommendation: keep the plan's shape. A `match provider` inside one sequence
is the provider flag that 002 research §9 ruled out, small today and the
pattern that grew to 56 sites in the earlier client; and Graph will not share
this sequence at all, so the two-functions shape is what 005 extends. Cost of
the recommendation: about 60 lines. The maintainer agreed: a provider check
inside one sequence would be checked again and again, and the two servers'
behaviours would conflict inside it.
