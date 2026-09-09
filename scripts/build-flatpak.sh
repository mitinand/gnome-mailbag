#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check-flatpak-tools.sh
# Cargo.lock is the only dependency list; the sandbox builds without network access.
mkdir -p .flatpak-builder
cargo vendor --locked vendor > .flatpak-builder/cargo-config.toml
flatpak-builder --user --force-clean --repo=repo "$@" build-dir io.github.mitinand.Mailbag.yml
