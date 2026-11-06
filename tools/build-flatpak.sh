#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
command -v flatpak-builder >/dev/null || { echo 'Install flatpak-builder first.' >&2; exit 1; }
# The manifest downloads checksum-pinned crates before the offline build.
flatpak-builder --user --install-deps-from=flathub --repo=repo --install --force-clean build io.github.oeiv.Saifori.json
printf '\nInstalled locally. Launch with: flatpak run io.github.oeiv.Saifori\n'
