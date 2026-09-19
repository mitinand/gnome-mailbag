#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check-flatpak-tools.sh
# cargo-sources.json lists the crates from Cargo.lock; Cargo builds offline in the sandbox.
flatpak-builder --user --force-clean --repo=repo "$@" build-dir io.github.mitinand.Mailbag.yml
