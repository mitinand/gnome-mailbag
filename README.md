# Mailbag

A native email client for GNOME, built with Rust, GTK4 and libadwaita.
Designed for personal use, with an initial focus on reading and managing incoming mail.

## Development setup

Linux is required; Fedora is the primary development environment. On Fedora,
install the system prerequisites:

```bash
sudo dnf install git gcc pkgconf-pkg-config gtk4-devel libadwaita-devel python3 python3-pip desktop-file-utils appstream
```

On other Linux distributions, install the equivalent packages using your package
manager. You need a C compiler, pkg-config, GTK4 and libadwaita development files,
Python 3.11+ with pip and venv, Git, and the desktop-file and AppStream validators.
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
configuration. Python is needed only for Spec Kit.

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
