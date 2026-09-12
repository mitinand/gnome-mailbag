# Mailbag

A native email client for GNOME, built with Rust, GTK4 and libadwaita.
Designed for personal use, with an initial focus on reading and managing incoming mail.

## Setup

Linux is required; Fedora is the primary development environment.
Install [rustup](https://rustup.rs/) and the system prerequisites:

```bash
sudo dnf install git gcc pkgconf-pkg-config gtk4-devel libadwaita-devel python3 python3-pip desktop-file-utils appstream meson ninja-build flatpak flatpak-builder
```

Then run from the repository root:

```bash
./scripts/setup.sh
```

Setup installs the pinned development tools, Spec Kit, and the Flatpak SDK/runtime,
and runs the checks. It requires internet access. On other Linux distributions,
install equivalent system packages; these environments are not yet validated.

## Run and check

```bash
cargo run --locked       # Run in a GNOME desktop session
./scripts/check.sh       # Formatting, Clippy, tests, build, licenses and metadata
```

## Build and install Flatpak

```bash
./scripts/build-flatpak.sh --install
flatpak run io.github.mitinand.Mailbag
```

Use `--install` to update the locally installed application; without it, the script
only builds and exports the package. The build uses GNOME SDK/runtime 50.

## Repository layout

| Path | Contents |
| --- | --- |
| `crates/mailbag/src/` | Application Rust code |
| `crates/mailbag/resources/ui/` | Approved UI forms awaiting application integration |
| `data/` | App icon, desktop entry and AppStream metadata |
| `scripts/` | Setup, checks and Flatpak build tools |
| `docs/spec.md` | Product requirements and architecture |

The root Cargo workspace owns shared dependencies, lints and `Cargo.lock`.
Other architecture crates will be added with their implementations.
