# Packaging and Dependency Contract

This is portion 1 of feature 002. Prepare the dependency sources and legal
notices before transport implementation so the forks can build in Flatpak.
[Research](../research.md#7-packaging-and-dependency-policy) records the accepted
prototype evidence; this contract defines the Mailbag changes and checks.

## Source of truth and tool installation

Cargo.lock is the dependency list. Add the chosen dependencies and full-revision
patches from [research](../research.md#2-imap-and-content-dependencies) in this
portion, including log's disabled levels. Declare them in skeleton
`mailbag-imap` and `mailbag-content` crates that `mailbag` depends on: an unused
workspace dependency enters neither Cargo.lock nor the Flatpak build, which
compiles only `--package mailbag`. Resolve the lockfile before generating
sources. Do not add Tokio, async-std, rustls or separate application decoders.

Before implementation, verify the declared/resolved versions and fork revisions
against that baseline. Keep the Rust/platform dependencies already selected by
the project unless a change is separately justified. Tags document the forks;
Cargo uses rev, never a branch.

Generate `cargo-sources.json` with flatpak-cargo-generator from
flatpak-builder-tools commit `de2225a` (2026-09-12, MIT). Record the full resolved
commit corresponding to that revision in `scripts/tool-versions.env` during
implementation. Install the tool and its Python dependencies aiohttp/tomlkit in
the development environment, using a shared `scripts/setup-cargo-generator.sh`
entry point from `scripts/setup.sh` and CI. Do not copy generator source into
the application or add a production Python dependency.

Keep the generated JSON in version control. “Committed artifact” is the
repository policy; the maintainer, not the agent, creates commits.

## Generation and drift checks

Provide `scripts/generate-cargo-sources.sh` with normal generation and `--check`:

1. Use the pinned installed generator with Cargo.lock.
2. Normalize the generated Cargo configuration destination from `cargo/config`
   to `cargo/config.toml`. Preserve its content and source mappings.
3. Write deterministic JSON. Normal generation replaces cargo-sources.json.
4. In --check mode, generate into a temporary directory and compare with the
   tracked file. Report drift and exit unsuccessfully without rewriting it.

`scripts/check.sh` calls --check in addition to its existing checks. It also
runs the license gate and source-policy check after the allow-git changes.
It also enforces the crate dependency rules in
[research](../research.md#9-crate-layout) with `cargo tree`.
A changed lockfile requires regenerating the JSON in the same portion.

The current CI check job does not run the full developer setup. Add the pinned
generator/Python setup there before canonical checks, using the same installation
entry point. Do not duplicate versions in the workflow. This is needed because
the new check would otherwise fail in CI for a missing tool.

Generation/source preparation can require network access. The subsequent Cargo
build inside Flatpak must not. Distinguish those phases when reporting an
“offline build”; it does not mean dependencies were never downloaded.

## Manifest, Meson and build script

| File | Planned change |
|---|---|
| io.github.mitinand.Mailbag.yml | Include cargo-sources.json instead of the root vendor directory and .flatpak-builder Cargo config. Set CARGO_HOME=/run/build/mailbag/cargo and retain CARGO_NET_OFFLINE=true. |
| meson.options | Add a cargo_vendor_dir string option for dependency-notice installation. |
| Manifest config-opts | Pass the vendor option as /run/build/mailbag/cargo/vendor, which is $CARGO_HOME/vendor inside this build. Do not rely on unverified environment expansion in a Meson option. |
| meson.build | Consume that directory; never set or override CARGO_HOME. Cargo must inherit the manifest's configuration for git dependencies. |
| Manifest source inputs | Include meson.options and third-party-notices alongside the existing project inputs. |
| scripts/build-flatpak.sh | Remove cargo vendor and temporary config generation. Build from the prepared source manifest. |
| .gitignore and generated vendor/ | Remove the root vendor ignore entry and generated directory. Do not commit vendored sources. |

Meson already requires dependency sources to install per-crate notices. Keep that
requirement for installable builds, now using an explicit existing vendor
directory instead of assuming a source-root vendor/. Native development through
`cargo run --locked` is unchanged.

Portion 1 changes build inputs, not runtime permissions. Add `--share=network`
only in portion 5 and verify that it is the sole addition to F01. No host
filesystem, secret-store or host-command permissions accompany this migration.

## Dependency policy and installed notices

`cargo deny check licenses` remains the primary license gate. Add BSD-3-Clause
to the allowed set. In `deny.toml`, set `[sources].allow-git` to the two exact
fork repository URLs. Do not allow arbitrary git sources or weaken other gates.

Install each crate's license/notice files and Cargo.toml beneath
`$FLATPAK_DEST/share/licenses/$FLATPAK_ID`, preserving crate/version and relative
notice paths. Notice files are those named as license, licence, copying,
copyright or notice files, plus every file in the crate's `LICENSES/` directory
(the REUSE layout, used by hashify). The vendor option is an absolute source
path; never append that absolute path to the destination. The installed tree
must not escape the application's license directory.

Use an explicit exception map in meson.build keyed by package name from
Cargo.toml, containing only:

| Crate | Fallback notice source |
|---|---|
| stop-token | Standard MIT and Apache-2.0 texts with the resolved Cargo.toml author information and an explanation that upstream did not supply license files. |

Store these texts and an ORIGIN.md explanation under
`third-party-notices/stop-token/`. Do not put them in the root REUSE-owned
LICENSES directory. Record the resolved crate version, declared license
expression, source of each text and any supplied author notice; do not invent
upstream attribution.

For normal crates, install their supplied notices. Only the named exception
may use the saved fallback texts. A new package lacking license files must stop
the build. Neither a license field in Cargo.toml alone nor an arbitrary standard
text is a general fallback. Include the saved explanation in the installed notices.

The license choices for self_cell, memchr, unicode-ident and encoding_rs are
recorded once in [research](../research.md#8-license-decisions). Preserve required
Unicode/BSD notices alongside the selected permissive license.

## Distribution

Distribute a ready binary through the project's own Flatpak repository or bundle.
Users do not build it or install Rust. Keep the manifest compatible with Flathub's
source/build requirements; submission is outside scope.

Do not add a vendored-source archive to releases. The dependency source manifest,
lockfile and fork revisions identify build inputs. Publishing a repository,
uploading a bundle or creating a release is not part of this documentation task.

## Portion 1 acceptance

- `./scripts/check.sh` passes, including generated-source consistency, licenses
  and allowed git sources. Confirm a stale JSON file is detected without rewriting
  the tracked artifact. The check also rejects a synthetic forbidden dependency
  edge, such as `gio` added to `mailbag-content`.
- The resolved graph uses both exact fork revisions and mail-parser/full_encoding;
  runtime-tokio, runtime-async-std and an alternate TLS stack are not enabled.
  Both debug and release log levels are compiled out.
- Source preparation followed by `./scripts/build-flatpak.sh` compiles the app
  without network access in the build sandbox and with Cargo offline.
- Inspect the installed license tree: both fork notices, hashify's `LICENSES/`
  texts, the stop-token exception and its origin, required Unicode/BSD notices,
  correct destination paths, and failure for a synthetic newly missing notice.
- Confirm no root vendor directory/config generation is required by the build
  script and no generated config deprecation warning remains.

The maintainer's prototype passed its offline build. Do not report these Mailbag
checks as passed until this portion is implemented and checked.
