#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Andrey Mitin
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env

# Fails if a crate's normal dependencies, direct or indirect, include a package
# whose name matches the pattern (specs/002-imap-integration/research.md §9).
reject_crate_dependencies() {
    local crate=$1 forbidden_pattern=$2 package_names forbidden_packages
    package_names=$(cargo tree --locked --package "$crate" --edges normal --target all \
        --prefix none --format '{p}' | awk '{ print $1 }' | sort -u)
    forbidden_packages=$(grep -Ex "$forbidden_pattern" <<<"$package_names" || true)
    if [[ -n $forbidden_packages ]]; then
        printf '%s must not depend on: %s\n' "$crate" "${forbidden_packages//$'\n'/, }" >&2
        return 1
    fi
}

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
gtk_packages='gtk4(-sys|-macros)?|libadwaita(-sys)?'
glib_packages='(glib|gio|gobject)(-sys|-macros)?'
reject_crate_dependencies goa-adapter "$gtk_packages|mailbag"
reject_crate_dependencies mailbag-imap "$gtk_packages|mail-parser|mailbag|mailbag-content"
reject_crate_dependencies mailbag-content "$gtk_packages|$glib_packages|mailbag|mailbag-imap"
scripts/generate-cargo-sources.sh --check
cargo deny --version | grep -Fx "cargo-deny $CARGO_DENY_VERSION"
cargo deny check licenses sources
desktop-file-validate data/io.github.mitinand.Mailbag.desktop
appstreamcli validate --no-net data/io.github.mitinand.Mailbag.metainfo.xml
