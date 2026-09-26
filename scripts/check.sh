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

# async-imap writes whole commands, sign-in included, through the log crate, so
# its levels stay compiled out (specs/003-logging/research.md §1).
require_log_levels_compiled_out() {
    local log_features feature
    log_features=$(cargo tree --locked --package mailbag --edges normal --target all \
        --prefix none --format '{p} {f}' | awk '$1 == "log" { print $3 }' | sort -u)
    for feature in max_level_off release_max_level_off; do
        if [[ ,$log_features, != *",$feature,"* ]]; then
            printf 'The log dependency of mailbag-imap must keep %s.\n' "$feature" >&2
            return 1
        fi
    done
}

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
gtk_packages='gtk4(-sys|-macros)?|libadwaita(-sys)?'
glib_packages='(glib|gio|gobject)(-sys|-macros)?'
# The shared definitions sit below every layer (specs/006-error-handling/research.md §1).
reject_crate_dependencies mailbag-domain "$gtk_packages|$glib_packages|goa-adapter|mailbag|mailbag-content|mailbag-graph|mailbag-imap|mailbag-providers"
reject_crate_dependencies goa-adapter "$gtk_packages|mailbag|mailbag-graph|mailbag-providers"
reject_crate_dependencies mailbag-imap "$gtk_packages|mail-parser|mailbag|mailbag-content|mailbag-graph|mailbag-providers"
reject_crate_dependencies mailbag-content "$gtk_packages|$glib_packages|mailbag|mailbag-graph|mailbag-imap|mailbag-providers"
reject_crate_dependencies mailbag-graph "$gtk_packages|mail-parser|mailbag|mailbag-imap|mailbag-content|mailbag-providers"
# The provider layer joins the library crates; the widgets stay above it.
reject_crate_dependencies mailbag-providers "$gtk_packages|mailbag"
# No bridge between the log crate and tracing (specs/003-logging/research.md §1).
reject_crate_dependencies mailbag 'tracing-log'
require_log_levels_compiled_out
scripts/generate-cargo-sources.sh --check
cargo deny --version | grep -Fx "cargo-deny $CARGO_DENY_VERSION"
cargo deny check licenses sources
desktop-file-validate data/io.github.mitinand.Mailbag.desktop
appstreamcli validate --no-net data/io.github.mitinand.Mailbag.metainfo.xml
