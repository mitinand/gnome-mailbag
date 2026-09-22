# Tasks: Gmail Integration

**Feature**: F04 / `004-gmail-integration`
**Created**: 2026-09-23 · **Branch**: `claude/gmail` · **Status**: Portions 1 and 2 built, awaiting review

[Spec](spec.md) owns the rules, [plan](plan.md) owns the size table,
boundaries and portions, [research](research.md) owns decisions and probe
facts, [quickstart](quickstart.md) owns live acceptance. Follow
[AGENTS.md](../../AGENTS.md#commits-prs-and-review-pauses): implement one
portion, run its checks, compare the size with the plan's table, report and
stop. The maintainer creates commits and PRs. Do not start code before
document approval. Nothing committed may mention where an idea came from
outside this repository.

Phases follow the plan's portions. The protocol comes first because the
credential from GOA can only be mapped once the protocol crate accepts a
token. Story labels trace tasks to US1 (load recent Gmail Inbox mail), US2
(read received text), US3 (sign in with the account's authorization) and US4
(see Gmail's mechanisms in the record). Tests are part of every portion and
live beside their modules.

| Portion | Tasks | Suggested commit subject | Intended PR |
|---|---|---|---|
| Documents | T001 | docs(gmail): specify and plan Gmail integration | Gmail integration |
| 1. Protocol | T002–T009 | feat(imap): sign in with an access token and read Gmail attributes | Gmail integration |
| 2. GOA credential | T010–T014 | feat(goa): provide the access token of an OAuth account | Gmail integration |
| 3. Provider crate, move only | T015–T020 | refactor: move the load sequence into mailbag-providers | Gmail integration |
| 4. Gmail load and window | T021–T027 | feat: load the Inbox of Google accounts | Gmail integration |

## Phase 1: documents and review

- [X] T001 STOP: present specs/004-gmail-integration/ (spec.md, plan.md, research.md, quickstart.md, checklists/requirements.md, this tasks.md) and the amendments in specs/002-imap-integration/ (spec.md FR-012, research.md §9, contracts/goa-access.md) and wait for explicit maintainer approval before any code change. The contract amendment and decision D1 were approved on 2026-09-23.

## Phase 2: protocol (portion 1)

**Purpose:** `mailbag-imap` signs in with a token, offers readable names,
identifies the client and fetches Gmail's row attributes when asked. Serves
US3 and US4 without touching GOA or the window. Plan rows: sign-in with a
token, readable names and identification, Gmail row attributes, capabilities
after sign-in.
**Independent check:** scripted-server transcripts in crates/mailbag-imap.

- [X] T002 [US3] In crates/mailbag-imap/src/lib.rs replace `ImapAccount.password: String` with `credential: Credential` where `Credential { Password(String), AccessToken(String) }` has no Debug, Display or Clone; add `OpenOptions { readable_names: bool, client_identity: Option<ClientIdentity> }` with `Default`, `ClientIdentity { name: String, version: String, vendor: String, contact: String, support_url: String }`, `RowItems { Standard, WithGmailAttributes }`, `GmailRow { message_id: u64, labels: Vec<String> }` and `MessageRow.gmail: Option<GmailRow>`. Export them.
- [X] T003 [US3] In crates/mailbag-imap/src/session.rs add `XOAuth2Credentials` next to `PlainCredentials`: the first challenge gets `user=<login>\x01auth=Bearer <token>\x01\x01`, every later challenge an empty reply (research.md §2). In `sign_in`, `Credential::AccessToken` requires `AUTH=XOAUTH2` in the pre-authentication capabilities and calls `authenticate("XOAUTH2", …)`; otherwise `NoSignInMethod`. `Credential::Password` keeps today's PLAIN or LOGIN path. The "signed in" line reports `method = "XOAUTH2"`. In `ServerNotices::keep` log `Response::Capabilities` at debug with the names joined, worded as "the server announced" a list, because imap-proto parses `ENABLED` into the same response (research.md §2, §4).
- [X] T004 [US3] [US4] Change `open_inbox` in crates/mailbag-imap/src/session.rs to take `OpenOptions`: after sign-in, when `readable_names`, run `ENABLE UTF8=ACCEPT` with `run_command_and_check_ok` and log the tagged result at debug (OK, or the NO/BAD text through `server_text_for_log`), then collect notices; when `client_identity` is set, send `ID` with `name`, `version`, `vendor`, `contact` and `support-url` through `run_command_and_check_ok` (not `Session::id`, which does not check the completion) and log its NO or BAD as a refusal; the server's untagged reply reaches `ServerNotices::keep`, which logs only its `name`, `vendor` and `version`, never `remote-host` or `connection-token`; a refusal is logged and the load continues; a lost connection fails the OpenInbox step. Then EXAMINE as today. In crates/mailbag-imap/src/reader.rs give `InboxReader::open` and `open_with_short_socket_timeout` the `options` parameter.
- [X] T005 [US4] In crates/mailbag-imap/src/reader.rs give `fetch_rows` a `RowItems` parameter: `WithGmailAttributes` appends `X-GM-MSGID X-GM-LABELS` to the row FETCH items; `collect_rows` fills `MessageRow.gmail` from `Fetch::gmail_msg_id` and `Fetch::gmail_labels` (labels as owned strings, as sent) and leaves it `None` for `Standard`. A row without the attributes under `WithGmailAttributes` keeps `gmail: None`.
- [X] T006 [US3] [US4] Extend crates/mailbag-imap/src/test_server.rs: `FixtureSetup` may advertise `AUTH=XOAUTH2` and hold an expected token; `AUTHENTICATE XOAUTH2` sends an empty `+`, decodes the base64 reply, and on a match signs in, otherwise sends `+ <base64 of {"status":"400","schemes":"Bearer","scope":"https://mail.google.com/"}>`, reads the empty line and answers `NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)`; `ENABLE` answers `* ENABLED UTF8=ACCEPT` and OK, or BAD when the setup says so; `ID` answers `* ID ("name" "Scripted" "vendor" "Mailbag tests" "version" "1" "remote-host" "203.0.113.7" "connection-token" "secret-token")` and OK, or NO when the setup says so; `FixtureMessage` gains `gmail_message_id` and `gmail_labels` answered for the X-GM items; add `account_with_token`. Record every command in the fixture log.
- [X] T007 [US3] [US4] Add tests in crates/mailbag-imap/src/tests/ (secure_session.rs and acquisition.rs, or a new gmail.rs): token accepted, transcript has `AUTHENTICATE XOAUTH2` and no LOGIN; token refused, the client sent the empty line, the result is `Failed(SignIn)` with code `AUTHENTICATIONFAILED`; `AUTH=XOAUTH2` absent gives `NoSignInMethod` without sending the token; ENABLE OK and ENABLE BAD both reach EXAMINE, each with its debug line, and ENABLE precedes EXAMINE; ID OK logs three fields and the record contains neither `203.0.113.7` nor `secret-token`, ID NO reaches EXAMINE; `WithGmailAttributes` rows carry `GmailRow` with the fixture's values and `Standard` rows carry `None`; the token's text never appears in the record at debug, including after a refused sign-in.
- [X] T008 [US3] Adapt crates/mailbag/src/inbox_load.rs and its tests to the new protocol API without behaviour change: `server_account` builds `Credential::Password`, `InboxReader::open(account, OpenOptions::default())`, `fetch_rows(RowItems::Standard)`; `ImapAccess.password` stays as it is until portion 2.
- [X] T009 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare the portion's size with plan.md's table (mailbag-imap ~150 production, ~110 scripted server, 8 tests); review constitution I/II; report what changed, the evidence and limitations, suggest the commit, and wait before portion 2.

## Phase 3: GOA credential (portion 2)

**Purpose:** goa-adapter hands over a token for an OAuth account and a
password for a password account, chosen by the interface the account object
exports. Serves US3. Plan row: credential from GOA; contract amendment in
specs/002-imap-integration/contracts/goa-access.md (approved 2026-09-23).
**Independent check:** the fake GOA on the private bus.

- [X] T010 [US3] In crates/goa-adapter/src/accounts.rs add `OAUTH2_BASED_INTERFACE = "org.gnome.OnlineAccounts.OAuth2Based"`. In crates/goa-adapter/src/imap_access.rs replace `ImapAccess.password` with `credential: ImapCredential { Password(String), AccessToken(String) }` (no Debug, Display or Clone); add `ImapAccessError::AccessToken`; let `find_imap_settings` also return which credential interface the object exports (OAuth2Based first, else PasswordBased, else `Settings`); rename `read_password` to `read_credential`, calling `GetAccessToken` with reply type `(si)` and discarding `expires_in`, or `GetPassword("imap-password")` as today; a D-Bus error maps to `AccessToken` or `Password` for its kind, timeout and cancellation as today. Export `ImapCredential` in crates/goa-adapter/src/lib.rs.
- [X] T011 [US3] In tests/support/goa.rs let the fake export `org.gnome.OnlineAccounts.OAuth2Based` with `GetAccessToken` (out `s`, `i`) in the node XML, add a Google account builder (provider `google`, Mail with `ImapHost imap.gmail.com`, `ImapUseSsl` true, `ImapUserName`, no PasswordBased), a reply behaviour for the token like the password one, and a log of token requests.
- [X] T012 [US3] Add tests in crates/goa-adapter/src/imap_access/tests.rs: a Google account yields `ImapCredential::AccessToken` with the fake's token and no GetPassword call; a password account is unchanged; GetAccessToken answered with an error yields `AccessToken`; a held GetAccessToken reply yields `Timeout`; an object with neither interface yields `Settings` before any credential call.
- [X] T013 [US3] In crates/mailbag/src/inbox_load.rs map `ImapCredential` to `Credential` in `server_account`; in crates/mailbag/src/window_ui.rs add the `AccessToken` arm to `online_accounts_status`: title "Authorization unavailable", text "Unable to get this account's authorization from Online Accounts. No server sign-in was attempted."; update tests that build `ImapAccess` and add a window test for the new wording.
- [ ] T014 STOP: run `cargo test --workspace` and ./scripts/check.sh; run git diff --check; compare the size with plan.md's table (goa-adapter ~60 production, ~45 fake lines, 5 tests); review constitution I/II; report, suggest the commit, and wait before portion 3.

## Phase 4: provider crate, move only (portion 3)

**Purpose:** the mail worker, the loader, the batch types and the load
sequence move into `mailbag-providers` with their tests. No behaviour change;
no Gmail code yet. Plan row: provider crate; dependency rules in plan.md.
**Independent check:** every moved test passes unchanged; `check.sh` rejects
a forbidden dependency.

- [ ] T015 Create crates/mailbag-providers/Cargo.toml (edition, rust-version, license and repository as the other crates; dependencies glib, gio, async-channel, futures-util, goa-adapter, mailbag-imap, mailbag-content, tracing; dev-dependencies mailbag-imap with `test-support` and tracing-subscriber; workspace lints) and add it to `members` in the root Cargo.toml. Run scripts/generate-cargo-sources.sh --check: no external source changes.
- [ ] T016 Move code without changing it beyond paths and visibility: from crates/mailbag/src/inbox_load.rs, `MailWorker`, `LoadRequest`, `LoadHandle`, `report_outcome` and `run_worker` into crates/mailbag-providers/src/worker.rs; `MailLoader`, `LoadsInbox`, `LoadCancellation`, `LoadStep`, `load_result` and `LoadOutcome` into crates/mailbag-providers/src/lib.rs; `run_load`, `load_inbox_batch` (renamed `load_imap_inbox`), `server_account`, `describe_part`, `text_parts` and `decode_message_text` into crates/mailbag-providers/src/imap.rs; from crates/mailbag/src/inbox.rs, `ReceivedBatch`, `ReceivedMessage`, `ReceivedContent`, `LoadFailure`, `ServerFailure`, `LoadResult` and `CancelsLoadOnDrop` into crates/mailbag-providers/src/batch.rs. `InboxController`, `AccountInbox` and the log functions stay in crates/mailbag/src/inbox.rs.
- [ ] T017 Move crates/mailbag/src/inbox_load/tests.rs to crates/mailbag-providers/src/tests.rs; include tests/support/record.rs through `#[path]` in crates/mailbag-providers/src/lib.rs under `cfg(test)` as crates/mailbag/src/main.rs does, and replace the use of `crate::logging` (`LogLevel`, `capture::start_record`) with that support module's capture at the same levels. Every test keeps its name and assertions.
- [ ] T018 Wire crates/mailbag: `main.rs` builds `mailbag_providers::MailLoader` and drops `mod inbox_load`; `window_ui.rs`, `inbox.rs`, `mail_ui.rs` and `mail_ui/tests.rs` import the moved types from `mailbag_providers`; delete crates/mailbag/src/inbox_load.rs and its tests directory. `cargo build --workspace` with no warnings.
- [ ] T019 In scripts/check.sh add `reject_crate_dependencies mailbag-providers "$gtk_packages|mailbag"` and add `mailbag-providers` to the forbidden lists of goa-adapter, mailbag-imap and mailbag-content. Update the dependency table in specs/002-imap-integration/research.md §9 only if the rules differ from what it already says.
- [ ] T020 STOP: run `cargo test --workspace` and ./scripts/check.sh, including a temporary gtk dependency in crates/mailbag-providers/Cargo.toml that must fail and is reverted; run ./scripts/build-flatpak.sh with Cargo offline in the build sandbox, since the workspace changed; run git diff --check; confirm the diff is a move (same test names, same line counts within a few lines); compare with plan.md's table (~640 moved, no new behaviour); report, suggest the commit, and wait before portion 4.

## Phase 5: Gmail load and window (portion 4)

**Purpose:** the Gmail load over the shared steps; Google accounts refreshable;
one neutral sign-in sentence. Serves US1, US2, US3 and US4. Plan rows:
provider crate (Gmail part), window; decision D1 (two load functions).
**Independent check:** providers tests with the scripted server, window tests,
then live acceptance per quickstart.md.

- [ ] T021 [US1] [US4] In crates/mailbag-providers/src/imap.rs extract the steps after the row FETCH into `load_batch_from_rows(reader, listed, account_id)` in crates/mailbag-providers/src/load.rs (structures, part selection, text, assembly; the "window emptied" check included), used by `load_imap_inbox`. Create crates/mailbag-providers/src/gmail.rs with `load_gmail_inbox(access)`: `InboxReader::open(account, OpenOptions { readable_names: true, client_identity: Some(ClientIdentity { name: "Mailbag", version: env!("CARGO_PKG_VERSION"), vendor: "Andrey Mitin", contact: "mitin.andrey@outlook.com", support_url: env!("CARGO_PKG_REPOSITORY") }) })`, `fetch_rows(RowItems::WithGmailAttributes)`, one debug line per row inside the `message` span with `gmail_message_id` and `labels`, then `load_batch_from_rows`, then move each row's `GmailRow` onto `ReceivedMessage.gmail: Option<GmailRow>` (new field in batch.rs, `None` for Generic IMAP).
- [ ] T022 [US1] In crates/mailbag-providers/src/lib.rs add `MailProvider { GenericImap, Gmail }`; `LoadsInbox::start_load(account_id, provider, report)` and `LoadRequest` carry it; `run_load` in worker.rs calls `load_imap_inbox` or `load_gmail_inbox` by it. `AccountProvider` is not used below `MailLoader`.
- [ ] T023 [US1] [US3] In crates/mailbag/src/accounts.rs add `mail_provider(provider: AccountProvider) -> Option<MailProvider>` (ImapSmtp → GenericImap, Google → Gmail, others → None) and use it in crates/mailbag/src/window_ui.rs for `refreshable_account`, the Refresh Inbox sensitivity, `nothing_loaded_status` (the "Choose Refresh Inbox…" hint for both loadable providers) and the `start_load` call; replace "You can change this account's password in Online Accounts." with "Check this account's sign-in in Online Accounts."; update `ScriptedLoader` in crates/mailbag/src/mail_ui/tests.rs for the new signature; add window tests: a Google account is refreshable and a Microsoft 365 account is not; a refused sign-in shows the new sentence.
- [ ] T024 [US1] [US4] Add tests in crates/mailbag-providers/src/tests.rs with the scripted server and a token: the Gmail load publishes a batch whose messages carry the fixture's `GmailRow` values; the transcript has `ENABLE UTF8=ACCEPT` and `ID` after sign-in and before the row FETCH and no LOGIN; the record at debug has the message identifiers and labels and not the token; a Generic IMAP load's transcript has neither ENABLE nor ID and its messages have `gmail: None`.
- [ ] T025 [US2] Confirm by the moved tests and one Gmail test that text acquisition is unchanged for Gmail: the same part requests and the same content explanations for a fixture with plain, HTML-only and attachment messages loaded through `load_gmail_inbox`.
- [ ] T026 STOP: run `cargo test --workspace` and ./scripts/check.sh; run ./scripts/build-flatpak.sh and confirm that the Flatpak manifest and its permissions are unchanged (FR-010); run the live acceptance of quickstart.md on the maintainer's Google account (SC-001–008), including the record comparison with the web interface and the revocation steps, and record what was verified live and what only by tests; run git diff --check; compare the whole feature with plan.md's size table; review constitution I/II over the whole feature; report, suggest the commit and the PR description, and wait.
- [ ] T027 After the last portion is accepted, run the `simplify-review` skill in a fresh subagent on the branch diff against main; bring findings that would add scope to the maintainer with alternatives and costs; apply only what the maintainer approves.

## Dependencies

- T001 before everything.
- Portion 1 (T002–T009) before portion 2: T013 maps `ImapCredential` to the protocol's `Credential`.
- Portion 2 before portion 3: the move carries the credential mapping with it.
- Portion 3 before portion 4: `gmail.rs` lives in the moved crate.
- Within a portion, tasks run in order; the STOP task ends it.

## Parallel opportunities

- Portion 1: T005 (row items) and T006 (scripted server) touch different files and may proceed together after T002.
- Portion 2: T011 (fake GOA) may proceed together with T010.
- Portion 4: T023 (window) may proceed together with T021–T022 once the `MailProvider` type exists.
- Review pauses (T009, T014, T020, T026) are never parallel with anything.

## Implementation strategy

Each portion is one reviewable commit. Portions 1 and 2 add capabilities
nothing uses yet, so Mailbag's behaviour is unchanged until portion 4; that
keeps the move in portion 3 free of behaviour changes. Live acceptance
happens once, at the end, on the maintainer's account.
