#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Andrey Mitin
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
cargo deny --version | grep -Fx "cargo-deny $CARGO_DENY_VERSION"
cargo deny check licenses
desktop-file-validate data/io.github.mitinand.Mailbag.desktop
appstreamcli validate --no-net data/io.github.mitinand.Mailbag.metainfo.xml
