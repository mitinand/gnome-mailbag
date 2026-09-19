# Implementation Plan: IMAP Integration

**Branch**: `claude/imap` | **Feature**: `002-imap-integration`
**Revised**: 2026-09-18 | **Spec**: [spec.md](spec.md)
**Status**: Approved by the maintainer 2026-09-19. Implement by portions with review pauses; the shared goa-adapter contract still needs its own approval before portion 2.

## Summary

On Refresh Inbox, load the selected Generic IMAP account's latest 100 Inbox
messages, including only the text parts needed for reading. Keep each account's
batch in memory for the run, populate the existing list and open received text
locally. The UI only exposes this integration for evaluation; it does not
establish the future synchronization or database design.

The maintainer's prototypes replace the earlier imap-next/blocking-job design:
use the async-imap and imap-proto forks, asynchronous GIO on a worker's GLib
MainContext, and mail-parser for all message-content decoding. Package dependency
sources through a committed cargo-sources.json before implementing mail access.
Protocol and content code form two new crates, `mailbag-imap` and
`mailbag-content`, joined by a load sequence in `mailbag`
([crate layout](research.md#9-crate-layout)).

A message whose part structure cannot be read keeps its row and gets a content
explanation; no message is skipped. A network interruption fails the load and
leaves the list empty. All other permanent guarantees remain unchanged.

## Behavior for this stage

| Situation | Choice |
|---|---|
| Select an account | Show its batch from this run, or that nothing has been loaded. Never load. |
| Refresh Inbox | Clear the selected account's list and reader, load, then show the batch. Allowed with AttentionNeeded or after a GOA observation failure. |
| Refresh while a load runs | Disabled; never queued. |
| Switch accounts during a load | The load continues; its result is stored for the account it was started for. |
| Load fails | The list stays empty; the status page names the failing step. Refresh again to retry. |
| Unreadable part structure | Keep the row; the reader explains that the content could not be read. Other messages load normally. |
| GOA observation failure | No mail change; F01's account page covers the list until recovery. |
| Confirmed exclusion from F01 | Discard the account's mail and cancel its load; a late result cannot restore it. |
| Text over 64 KiB decoded UTF-8 | GtkLabel shows the first 64 KiB without an explanation; the stored text is complete. |

Loading uses the existing spinner only. Synchronization Status, success/warning
icons and the sync popover are not implemented. ALERT text is included only in a
failure explanation. Details: [UI contract](contracts/ui.md).

The socket inactivity timeout starts at 30 seconds. There is **no application
download-size budget**, per-part limit, progress watchdog or total batch deadline.
Slow continuing transfers may last longer. The dependency response ceiling and
the reader's display clipping remain distinct from a download quota.

## Review points

The chosen stack, fork revisions, lack of a custom size budget and minimal UI
follow explicit maintainer decisions. Prototype facts are accepted without
rerunning them. [Research](research.md) records evidence, rejected alternatives
and fork maintenance; versions will be checked before implementation.

The shared [goa-adapter access contract](contracts/goa-access.md) still requires
explicit approval **before portion 2**. It adds settings/password retrieval but
no second owner of account exclusion. Review the function map and five portions
below before implementation. No widget replacement is proposed; the Refresh
Inbox menu addition was already approved.

The main remaining cost of the chosen stack is maintaining two small fork patch
sets. The accepted fallback also needs fresh sessions after structure parse
failures; it cannot safely reuse the failed parser buffer. These costs belong to
002's real protocol cases, not a general provider or synchronization framework.

## Technical Context

| Area | Design |
|---|---|
| Language/platform | Existing Rust 2024 / toolchain 1.95, GLib/GIO 0.22.9, libadwaita 0.9.2 and GNOME runtime 50; verify before implementation. |
| IMAP | async-imap =0.11.3, runtime-futures only, and imap-proto from full-revision fork pins in research. |
| Content | mail-parser 0.11.9 with full_encoding; no separate application transfer/charset decoders. |
| Execution | One mail worker running its own GLib context, one load at a time. No Tokio, async-std or spawn_blocking. |
| Transport | GIO sockets/TLS and default certificate database; ThreadGuard/futures-io bridge, no certificate bypass. |
| Storage | One batch per account for the run, in memory. No disk store or schema. |
| Packaging | Generated cargo-sources.json, manifest-owned CARGO_HOME, Meson vendor-directory option and per-crate notices. |
| Validation | Existing private GOA fixture, Rust/GIO scripted IMAP server, content tests, targeted UI checks and installed Flatpak acceptance. |
| Scope | One selected account and Inbox, latest 100, explicit refresh. No proxy integration, IDLE, Graph or final account-worker allocation model. |
| Permissions | Only --share=network beyond F01, added with visible integration. |

## Ownership and function map

AccountList owns selection/exclusion. InboxController owns each account's
received mail and the single load. WindowUi projects account and mail state into
one page, preserving F01's priority. Protocol/content work remains on the mail
worker.

```text
WindowUi::select_account
  apply_account_selection      remember the selection; never load
  render_window               show that account's mail or that nothing is loaded

WindowUi::refresh_inbox
  clear_account_inbox          clear the selected account's list and reader
  start_inbox_load             one load; Refresh stays disabled until it ends

start_inbox_load
  request_imap_access          obtain settings/password on GOA's existing context
  load_inbox_batch             run on the selected-account worker
    connect_securely          implicit TLS or mandatory STARTTLS
    authenticate              supported password method over verified TLS
    read_capabilities         obtain the authenticated capability set
    examine_inbox             get the read-only Inbox identity and message count
    fetch_message_rows        latest-100 list fields, independent of structure
    fetch_part_structures     BODYSTRUCTURE by UID; isolate unreadable ones
    select_text_parts         apply MIME rules to the projected part tree
    fetch_text_groups         one UID FETCH per distinct section/header set
    decode_received_text      decode with mail-parser and release raw bytes
    finish_received_batch     complete text or an explanation for every row
    close_connection          end the session before another load starts
  finish_inbox_load            store the result for its account unless excluded
  render_window               apply F01 priority, list/reader and loading feedback
```

Steps belong to crates as follows. `mailbag-imap`: connect_securely through
fetch_part_structures, fetch_text_groups and close_connection.
`mailbag-content`: select_text_parts and decode_received_text, including the
subject/from/to display fields. `mailbag`: everything else, including the worker
and the load sequence that joins the two crates. That sequence stays in the GTK
crate only until a scheduling layer exists; review keeps widgets out of it,
because the crate cannot forbid them mechanically.

Account updates use `apply_account_update → discard_excluded_inbox →
render_window`. Opening is `open_received_message → show_received_text` and
cannot issue a network request. Settings-request failure never becomes a second
exclusion event. Detailed transitions are in [data-model.md](data-model.md).

## Project Structure

```text
specs/002-imap-integration/
  spec.md, plan.md, research.md, data-model.md, quickstart.md
  contracts/goa-access.md       shared API proposal
  contracts/imap-reading.md     transport, acquisition and content rules
  contracts/ui.md               existing surfaces and page priority
  contracts/packaging.md        source generation, licenses and build policy
  checklists/requirements.md

crates/goa-adapter/src/imap_access.rs
crates/mailbag-imap/src/        new crate: protocol
  lib.rs                       session and acquisition operations
  transport.rs                 GIO stream bridge, secure connection and errors
  part_tree.rs                 BODYSTRUCTURE projection with section paths
  test_server.rs               Rust/GIO scripted server; tests and `test-support` only
  tests/                       protocol and acquisition scenarios
crates/mailbag-content/src/lib.rs   new crate: text-part selection, mail-parser decoding
crates/mailbag/src/
  inbox.rs                     mail data and load lifecycle
  inbox_load.rs                worker and load sequence; temporary home
  mail_ui.rs                   row binding and local reader
  window_ui.rs                 account/mail coordination
tests/fixtures/mime/            synthetic MIME samples shared by crate tests
tools/make-certs.sh             disposable test certificates; no trust installation
scripts/setup-cargo-generator.sh
scripts/generate-cargo-sources.sh
cargo-sources.json, meson.options
third-party-notices/            the stop-token missing-license exception and its origin
```

Integrate with the existing client/lib, main/account UI, Cargo, manifest, Meson,
deny.toml, scripts and CI files. Tests otherwise live beside their modules.
The load-sequence tests in `mailbag` reach the scripted server through
`mailbag-imap`'s `test-support` feature, enabled only as a dev-dependency.
The two new crates follow the dependency rules in
[research](research.md#9-crate-layout); `scripts/check.sh` enforces them.
There is no Python IMAP server, vendored fork source tree or duplicate internal
design guide. The prototype is an example; its two-client comparison/reporting
framework is not an application requirement.

## Cost and implementation portions

Initial estimate: nine new production Rust modules in two new and two
existing crates, roughly 1,500–2,500 production lines; around 16–22 small
data/error/control types; one worker thread; two load lifecycle states;
cancellation and bounded parser-error isolation.
There is no application timer or size counter. Tests add one Rust/GIO server,
the 14 synthetic MIME samples and roughly 25–35 focused scenarios, reusing the
prototype's relevant coverage rather than requiring its exact test count.

Packaging adds two tooling scripts, a Meson option file, generated JSON and
the stop-token fallback notices; certificate generation adds one test script.
These costs are separate from production Rust. Reassess before exceeding about
1.5 times this estimate or expanding a portion beyond one reviewable change.

One intended feature PR: **IMAP integration**. The maintainer creates commits
and the PR. Implement in this order:

| Portion | Reviewable result | Required checks and proposed commit |
|---|---|---|
| 1 — Packaging and dependency policy | Skeleton `mailbag-imap` and `mailbag-content` crates declaring the pinned dependencies, so Cargo.lock and the Flatpak build include the forks; generated sources; manifest/Meson; license policy and the stop-token notice exception; crate dependency-rule check; setup, build/check scripts and CI generator setup. Runtime network permission waits for portion 5. | `./scripts/check.sh`, including a rejected forbidden dependency, and Flatpak compilation without build-network access; inspect installed notices. `build: prepare Flatpak sources and IMAP dependencies` |
| 2 — GOA access | **After shared-contract approval**, cancellable settings/password retrieval on the existing context. Observer alone owns exclusion. | Private-bus success/failure/cancellation tests and F01 regression checks. `feat(goa): provide IMAP access for the selected account` |
| 3 — Secure connection and acquisition | Implement `mailbag-imap`: GIO bridge, TLS/STARTTLS, authentication, EXAMINE, separate row and structure requests, structure isolation, BODYSTRUCTURE projection, grouped section retrieval, cancellation and timeout. Rust/GIO scripted server. | Security/read-only transcripts, parser/transport failure separation, grouping, cancellation and stall cases. `feat(imap): receive Inbox data over GIO` |
| 4 — Content | Implement `mailbag-content`: MIME selection, permissive mail-parser decoding and header display fields. Worker and load sequence in `mailbag` producing complete received batches. | MIME fixtures, encoding/replacement cases, no unselected payload requests. `feat(content): select and decode received plain text` |
| 5 — Visible integration | Existing list/reader/spinner, Refresh menu, page priority and accessibility; network permission; installed acceptance. | Targeted UI checks and SC-001–007 in the installed app. `feat: show and refresh received Inbox mail` |

Portion 3 tests section retrieval with explicit section requests. Portion 4
connects the MIME selection policy to that path; do not add a temporary
hard-coded body policy or pretend the end-to-end feature is complete at portion 3.

After **each** portion, run its relevant checks and `scripts/check.sh`, report
what changed, evidence, limitations, a suggested commit and intended PR, then
**stop for review**. Start the next portion only on explicit instruction. The
future tasks document must preserve these boundaries. No tasks or implementation
are generated by this planning revision.

## Constitution Check

Before design: the new evidence changes the stack and packaging, not the accepted
scope. Keep GOA ownership and a minimal in-memory evaluation view. Protocol and content
code get two dependency-bounded crates; no database, provider framework or final
synchronization architecture is imported.

After design:

| Principle | Assessment |
|---|---|
| I — Necessary complexity | The worker protects GTK; the stream bridge satisfies the chosen library; grouping avoids measured round trips; isolated fallback keeps every row after a real parser failure. No generic retries, cache, ALERT service or future-provider layer; the two new crates hold only code that exists in 002. |
| II — Clear language | This plan states behavior, costs and review boundaries. Linked contracts contain protocols and tooling mechanics. |
| III — Truthful failure | Actual failing step, no partial batch after an interruption, distinct not-loaded/loading/empty/failed states, per-message content explanations, no sensitive diagnostics. |
| IV — One owner | F01 owns exclusion, InboxController owns received mail and the load, `mailbag-content` owns body selection and uses mail-parser for decoding, one window projection chooses the visible page. |
| V — Responsive, bounded work | Latest-100 window, only selected text parts, the library's built-in response ceiling, finite service/socket waits, one acquisition and worker-side decoding. These satisfy this stage's bound; **do not reintroduce an application download budget in the name of principle V**. The 64 KiB clipping limits display, not acquisition; no claim of a fixed total RSS cap. |
| VI — Evidence | Prototype results are accepted design evidence. Mailbag checks in [quickstart](quickstart.md) remain future integration acceptance and are not reported as passed. |

No constitutional exception is proposed. Deferred choices include IDLE, Graph,
attachment fetching, database/UI interaction and account thread/pool allocation.
The shared API still awaits review. Implementation remains a separate step.
