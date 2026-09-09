#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env
version=$(flatpak-builder --version)
version=${version##* }
if ! printf '%s\n' "$FLATPAK_BUILDER_MIN_VERSION" "$version" | sort -V -C; then
    echo "flatpak-builder $FLATPAK_BUILDER_MIN_VERSION or newer is required; found $version." >&2
    exit 1
fi
