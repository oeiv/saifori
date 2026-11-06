#!/usr/bin/env bash
# Verify local development packaging; never submits or publishes anything.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
python3 tools/check-packaging.py
python3 tools/check-release-urls.py
command -v flatpak >/dev/null
flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream assets/io.github.oeiv.Saifori.metainfo.xml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.oeiv.Saifori.json
flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean --repo=repo build io.github.oeiv.Saifori.json
flatpak run --command=flatpak-builder-lint org.flatpak.Builder builddir build
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
printf '\nDevelopment package validation passed. This is not submission approval.\n'
