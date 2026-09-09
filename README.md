# Mailbag

A native email client for GNOME, built with Rust, GTK4 and libadwaita.
Designed for personal use, with an initial focus on reading and managing incoming mail.

## Development setup

Linux is required; Fedora is the primary development environment. On Fedora,
install the system prerequisites:

```bash
sudo dnf install git gcc pkgconf-pkg-config gtk4-devel libadwaita-devel python3 python3-pip desktop-file-utils appstream meson ninja-build flatpak flatpak-builder
```

On other Linux distributions, install the equivalent packages using your package
manager. You need a C compiler, pkg-config, GTK4 and libadwaita development files,
Python 3.11+ with pip and venv, Git, Meson 1.3+, Ninja, Flatpak, flatpak-builder 1.4.0+,
and the desktop-file and AppStream validators.
The Rust bindings check the required native library versions during compilation.
Other distributions have not yet been validated.

Install [rustup](https://rustup.rs/) if it is not already available, then run from
the repository root:

```bash
./scripts/setup.sh
```

Setup installs the tools pinned in `rust-toolchain.toml` and
`scripts/tool-versions.env`, restores local Spec Kit tooling and Codex skills, and
runs the checks. cargo-deny uses Cargo's user installation; Spec Kit uses `.venv`.
Downloads require internet access. Setup never invokes sudo or changes your shell
configuration. It adds the user Flathub remote if absent and installs the
runtime, SDK and Rust SDK extension selected by the Flatpak manifest. Python is
used by Spec Kit and Meson; the application itself does not depend on Python.

Repeated runs preserve `AGENTS.md`, the constitution, specifications and existing
local settings. If a local skill, template or script differs from the pinned
version, setup stops and reports the conflicting file rather than overwriting it.
The CLI version is checked; an existing installation with the same version is reused.

```bash
cargo run --locked       # Run in your GNOME desktop session
./scripts/check.sh       # Formatting, Clippy, tests, build, licenses and metadata
.venv/bin/specify --help # Spec Kit CLI
```

CI also checks dependency advisories with `cargo deny check advisories`.
The AI agent itself must be installed separately.

## Flatpak development

After setup, build and install the application for your user:

```bash
./scripts/build-flatpak.sh --install
flatpak run io.github.mitinand.Mailbag
```

Cargo vendors the dependencies from `Cargo.lock` before the sandboxed, offline
build. Generated dependencies and build outputs are ignored by Git. Meson invokes
Cargo and installs the binary, desktop entry, icon, metadata and license notices.
Upstream notices and package metadata are included for every vendored crate,
including build and target-specific dependencies. No license collection tool is needed.
Setup and the build script require flatpak-builder 1.4.0+ (declared in
`scripts/tool-versions.env`); setup checks this before downloading SDKs.

The manifest selects GNOME runtime/SDK 50 and its Rust SDK extension; the extension's
compiler is maintained separately from the native toolchain in
`rust-toolchain.toml` and must satisfy the package's `rust-version`.

The current shell only requests Wayland and GPU access. Add integration permissions
with the features that need them. CI builds and exports the Flatpak; checking the
window, About dialog and Ctrl+Q still requires a GNOME desktop session.

Flatpak builds explicitly use Meson's `release` build type. For a native Meson
build with debug symbols, prepare the sources and select `debug`:

```bash
cargo vendor --locked vendor
meson setup target/meson --buildtype=debug
meson compile -C target/meson
```

Meson supports `debug` (Cargo `dev`) and `release` (Cargo `release`); unsupported
build types fail explicitly. Vendoring is required for Meson's license installation;
plain `cargo run --locked` remains available without this preparation.
Runtime and SDK branches receive updates, so this development setup does not
promise bit-for-bit reproducible binaries across different SDK revisions.
