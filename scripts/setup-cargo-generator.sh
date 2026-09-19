#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Andrey Mitin
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs the pinned flatpak-cargo-generator into .venv for
# scripts/generate-cargo-sources.sh. Setup and CI share this entry point.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env

if [[ ! -d .venv ]]; then
    python3 -m venv .venv
fi
if [[ ! -x .venv/bin/python ]]; then
    echo 'Existing .venv is incomplete; repair or move it aside before rerunning setup.' >&2
    exit 1
fi
.venv/bin/python -m pip install --quiet --disable-pip-version-check \
    "aiohttp==$CARGO_GENERATOR_AIOHTTP_VERSION" "tomlkit==$CARGO_GENERATOR_TOMLKIT_VERSION"

generator_dir=.venv/share/flatpak-builder-tools/$FLATPAK_BUILDER_TOOLS_REVISION
if [[ ! -f $generator_dir/flatpak-cargo-generator.py ]]; then
    mkdir -p "$generator_dir"
    curl --proto '=https' --tlsv1.2 -sSf -o "$generator_dir/download.part" \
        "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$FLATPAK_BUILDER_TOOLS_REVISION/cargo/flatpak-cargo-generator.py"
    mv "$generator_dir/download.part" "$generator_dir/flatpak-cargo-generator.py"
fi
