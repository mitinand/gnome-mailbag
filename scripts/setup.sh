#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tool-versions.env

if [[ $(uname -s) != Linux ]]; then
    echo 'This setup supports Linux development environments.' >&2
    exit 1
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
missing=()
for tool in git cc pkg-config python3 desktop-file-validate appstreamcli meson ninja flatpak flatpak-builder; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if command -v pkg-config >/dev/null; then
    for library in gtk4 libadwaita-1; do
        pkg-config --exists "$library" || missing+=("$library development files")
    done
fi
if ((${#missing[@]})); then
    printf 'Missing prerequisite: %s\n' "${missing[@]}" >&2
    if [[ -f /etc/os-release ]]; then
        source /etc/os-release
        if [[ ${ID:-} == fedora ]]; then
            echo 'Install prerequisites, then rerun setup:' >&2
            echo 'sudo dnf install git gcc pkgconf-pkg-config gtk4-devel libadwaita-devel python3 python3-pip desktop-file-utils appstream meson ninja-build flatpak flatpak-builder' >&2
        fi
    fi
    exit 1
fi
python3 -c 'import sys; sys.exit("Python 3.11 or newer is required for Spec Kit." if sys.version_info < (3, 11) else 0)'
if ! command -v rustup >/dev/null; then
    echo 'Install rustup from https://rustup.rs, then rerun setup.' >&2
    exit 1
fi
scripts/check-flatpak-tools.sh

# The manifest owns runtime/SDK versions and extension requirements.
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-only --install-deps-from=flathub \
    build-dir io.github.mitinand.Mailbag.yml

# Rustup reads the version, profile and components from rust-toolchain.toml.
cargo --version
if [[ $(cargo deny --version 2>/dev/null || true) != "cargo-deny $CARGO_DENY_VERSION" ]]; then
    cargo install cargo-deny --version "=$CARGO_DENY_VERSION" --locked
fi

if [[ ! -d .venv ]]; then
    python3 -m venv .venv
fi
if [[ ! -x .venv/bin/python ]]; then
    echo 'Existing .venv is incomplete; repair or move it aside before rerunning setup.' >&2
    exit 1
fi
installed=$(.venv/bin/python -m pip show specify-cli 2>/dev/null | sed -n 's/^Version: //p' || true)
if [[ $installed != "$SPECIFY_VERSION" ]]; then
    .venv/bin/python -m pip install "git+https://github.com/github/spec-kit.git@$SPECIFY_REVISION"
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
.venv/bin/specify init "$scratch/project" --integration codex \
    --integration-options=--skills --script sh --ignore-agent-tools --non-interactive

# Check all conflicts before copying. Preserve generated state and local settings.
while IFS= read -r -d '' file; do
    relative=${file#"$scratch/project/"}
    case "$relative" in
        .specify/memory/constitution.md) continue ;;
        .specify/scripts/*|.specify/templates/*|.specify/workflows/speckit/*|.agents/skills/*)
            if [[ -e $relative ]] && ! cmp -s "$file" "$relative"; then
                printf 'Local tool differs from the pinned version: %s\nResolve it before rerunning setup.\n' "$relative" >&2
                exit 1
            fi ;;
    esac
done < <(find "$scratch/project/.specify" "$scratch/project/.agents/skills" -type f -print0)

while IFS= read -r -d '' file; do
    relative=${file#"$scratch/project/"}
    [[ $relative == .specify/memory/constitution.md ]] && continue
    if [[ ! -e $relative ]]; then
        mkdir -p "$(dirname "$relative")"
        cp -p "$file" "$relative"
    fi
done < <(find "$scratch/project/.specify" "$scratch/project/.agents/skills" -type f -print0)

scripts/check.sh
printf '\nSetup complete. Run the application with: cargo run --locked\n'
