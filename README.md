# Mailbag

A native email client for GNOME, built with Rust, GTK4 and libadwaita.
Designed for personal use, with an initial focus on reading and managing incoming mail.

## Setup

Linux is required; Fedora is the primary development environment.
Install [rustup](https://rustup.rs/) and the system prerequisites:

```bash
sudo dnf install git curl openssl gcc pkgconf-pkg-config gtk4-devel libadwaita-devel python3 python3-pip desktop-file-utils appstream dbus-daemon meson ninja-build flatpak flatpak-builder
```

Then run from the repository root:

```bash
./scripts/setup.sh
```

Setup installs the pinned development tools, Spec Kit, and the Flatpak SDK/runtime,
and runs the checks. It requires internet access. On other Linux distributions,
install equivalent system packages; these environments are not yet validated.

Accounts are configured in GNOME Settings → Online Accounts. Mailbag requires
GNOME Online Accounts and GNOME Settings in the desktop session.

## Run and check

```bash
cargo run --locked       # Run in a GNOME desktop session
./scripts/check.sh       # Formatting, Clippy, tests, build, dependency policy, licenses and metadata
```

After changing `Cargo.lock`, run `./scripts/generate-cargo-sources.sh` and review
the regenerated `cargo-sources.json` with it; the Flatpak build reads its crates
from that file.

## Build and install Flatpak

```bash
./scripts/build-flatpak.sh --install
flatpak run io.github.mitinand.Mailbag
```

Use `--install` to update the locally installed application; without it, the script
only builds and exports the package. The build uses GNOME SDK/runtime 50.

The installed application verifies mail servers with the certificate authorities
of the GNOME runtime. A mail server whose certificate comes from a private
certificate authority, such as an internal one, is therefore not supported yet,
even when that authority is installed on the host: Flatpak does not share the
host's trust store with the sandbox. Servers with a publicly trusted certificate
work normally.

## Reporting a problem

If mail does not load or a message shows wrongly, a record of what Mailbag did
helps to find the cause. Mailbag writes one only when you ask for it. Quit
Mailbag first: if it is already running, a new start only says that logging was
not turned on. Then start it from a terminal with the record going to a file:

```bash
flatpak run io.github.mitinand.Mailbag --log-level=debug 2> mailbag.log
```

For a build from this repository, build first so that Cargo's own output stays
out of the file:

```bash
cargo build --locked
./target/debug/mailbag --log-level=debug 2> mailbag.log
```

Reproduce the problem, quit Mailbag and attach `mailbag.log` to the issue. The
levels are `error`, `warning`, `info` and `debug`; `debug` tells the most.

A debug record contains the Online Accounts identifiers of your accounts, also at
the other levels; folder names and message numbers (UIDs); the mail server's
host and port; how messages are built (content types, character sets,
sizes); the replies and alerts the server sent, with your sign-in name replaced
by `<login>`; and the reason a secure connection failed. It never contains
passwords, sign-in names, mail addresses, subjects or other header values,
attachment file names or the text of messages.

GTK and GLib print their own warnings to the same file; Mailbag does not
control them. Read the file before you attach it.

## Repository layout

| Path | Contents |
| --- | --- |
| `crates/mailbag/src/` | Application Rust code |
| `crates/mailbag/resources/ui/` | Approved UI forms |
| `data/` | App icon, desktop entry and AppStream metadata |
| `scripts/` | Setup, checks and Flatpak build tools |
| `specs/` | Accepted feature requirements and design |

The root Cargo workspace owns shared dependencies, lints and `Cargo.lock`.
`crates/goa-adapter/` provides the account data contract and GNOME account integration.
`crates/mailbag-imap/` reads mail over IMAP and `crates/mailbag-content/` decodes
message text. `third-party-notices/` holds license texts for dependencies that
publish none.
