#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Andrey Mitin
# SPDX-License-Identifier: GPL-3.0-or-later
# Generates cargo-sources.json from Cargo.lock for the offline Flatpak build.
# With --check, fails if cargo-sources.json differs and leaves it unchanged.
# Generation may download the git dependencies to read their Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env

case ${1:-} in
    '') check_only=false ;;
    --check) check_only=true ;;
    *) echo "Usage: $0 [--check]" >&2; exit 2 ;;
esac
generator=.venv/share/flatpak-builder-tools/$FLATPAK_BUILDER_TOOLS_REVISION/flatpak-cargo-generator.py
if [[ ! -f $generator ]]; then
    echo 'The pinned flatpak-cargo-generator is missing; run scripts/setup-cargo-generator.sh.' >&2
    exit 1
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
if ! .venv/bin/python "$generator" Cargo.lock --output "$scratch/generated.json" >"$scratch/generator.log" 2>&1; then
    cat "$scratch/generator.log" >&2
    exit 1
fi

# Cargo warns about the legacy cargo/config name; config.toml has the same meaning.
.venv/bin/python - "$scratch/generated.json" "$scratch/cargo-sources.json" <<'PYTHON'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as generated:
    sources = json.load(generated)
cargo_configs = [
    source
    for source in sources
    if source.get("dest") == "cargo" and source.get("dest-filename") == "config"
]
if len(cargo_configs) != 1:
    sys.exit("Expected exactly one generated cargo/config source.")
cargo_configs[0]["dest-filename"] = "config.toml"
with open(sys.argv[2], "w", encoding="utf-8") as output:
    json.dump(sources, output, indent=2)
    output.write("\n")
PYTHON

if $check_only; then
    if ! cmp -s "$scratch/cargo-sources.json" cargo-sources.json; then
        echo 'cargo-sources.json does not match Cargo.lock; run scripts/generate-cargo-sources.sh and review both files.' >&2
        exit 1
    fi
else
    cp "$scratch/cargo-sources.json" cargo-sources.json
fi
