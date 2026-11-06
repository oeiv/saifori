#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
python3 tools/check-static.py
python3 tools/check-project.py
python3 tools/check-source.py
python3 tools/check-packaging.py
command -v cargo >/dev/null || { echo 'Rust toolchain missing. Install cargo/rust before continuing.' >&2; exit 1; }
[[ -f Cargo.lock ]] || { echo 'Cargo.lock is required.' >&2; exit 1; }
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
printf '\nRust preflight passed. Proceed to GNOME Builder for GTK/Flatpak runtime QA.\n'
