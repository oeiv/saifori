# Saifori

A personal finance application for Linux, built with Rust, GTK4, Libadwaita and
SQLite. Record income and expenses, plan budgets, track transfers and review
spending trends. Core features work offline without an account.

- **Application ID:** `io.github.oeiv.Saifori`
- **Languages:** English and Spanish; first launch follows a supported system language
- **License:** MIT

## Features

- Transactions, categories, budgets, monthly reports and product summaries.
- Account transfers tracked separately from income and spending.
- Activity history, grouped transaction timelines and CSV/JSON exports.
- Optional Smart Capture and chat using your own DeepSeek, Claude or OpenAI key.
- Provider credentials stored in GNOME Keyring / Secret Service.
- Test Mode with synthetic USD records, separate from the personal ledger.

The ledger, conversations and exports are **not encrypted**. Privacy Mode masks
information on screen only. AI requests send submitted text, recent chat context
and financial aggregates to the selected provider and may incur provider charges.
See [PRIVACY.md](PRIVACY.md) and [SECURITY.md](SECURITY.md).

## Build

Requirements: Rust 1.92 or newer, GTK 4.12+, Libadwaita 1.4+, libsecret,
`pkg-config` and a C compiler. For example, on Fedora:

```bash
sudo dnf install rust cargo gtk4-devel libadwaita-devel libsecret-devel pkgconf-pkg-config gcc
cargo build --release --locked
./target/release/saifori
```

Keep `Cargo.lock` in version control. Ordinary native builds do not require a
`vendor/` directory or local Cargo source overrides.

## Development

```bash
python3 tools/check-static.py
python3 tools/check-project.py
python3 tools/check-source.py
python3 tools/check-packaging.py
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

`bash tools/preflight.sh` runs native build checks. For a real desktop smoke test,
install Xvfb, D-Bus, ImageMagick and system Python's `pyatspi`, build the release
binary, then run `bash tools/desktop-smoke.sh`. It uses temporary storage and
synthetic records to exercise navigation, labelled controls and transaction
creation. Logs and screenshots are saved under the ignored
`target/desktop-smoke-logs/` directory; set `SAIFORI_LOG_DIR` to choose another
location. These local logs contain test diagnostics and synthetic data;
database files and credentials are not included in diagnostic exports.

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidance.

## Flatpak

The **Flatpak** GitHub Actions workflow builds an installable `Saifori.flatpak`
bundle on pushes affecting the package. You can also start it from
**Actions → Flatpak → Run workflow**. Download `Saifori-flatpak-x86_64` from the
run's artifacts, extract it, then install and launch on a Flatpak-enabled host:

```bash
flatpak install --user ./Saifori.flatpak
flatpak run io.github.oeiv.Saifori
```

Build diagnostics are uploaded as `saifori-flatpak-logs-*` even when the build
fails. The workflow builds and installs the development bundle; interactive
portal/keyring testing and Flathub submission review remain separate.

The root manifest is for **local development**, not a Flathub submission. It uses
GNOME Platform/SDK 50 and the Rust SDK extension. Configure the Flathub remote,
install those prerequisites and `flatpak-builder`, then run:

```bash
bash tools/build-flatpak.sh
flatpak run io.github.oeiv.Saifori
```

`packaging/cargo-sources.json` is generated from Cargo.lock using the official
[flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo).
Flatpak Builder downloads checksum-pinned dependencies before building offline.
Regenerate that file when updating dependencies, then run
`python3 tools/check-packaging.py`. No local vendor folder is needed.

The app requests display, graphics, network and Secret Service access. It has no
home/host filesystem grant or broad session/system bus permission. File dialogs
use the document portal when sandboxed. Network access serves optional AI calls.

Before distribution, run `bash tools/verify-flatpak.sh` with `org.flatpak.Builder`
installed. It validates public screenshots, metadata, the manifest and build
artifacts. Source and screenshot URLs must refer to an actual public release;
the planned `v0.1.0` screenshot URLs are not published yet. Full sandboxed
portal/keyring testing remains required.

AI assistance contributed to implementation, refactoring, tests, build tooling,
metadata and documentation. The development manifest is also AI-assisted.
[Current Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
require disclosure of generated material, prohibit AI-generated or AI-assisted
submission manifests, and require stable releases and meaningful development
history. A human maintainer must independently author the submission manifest.
This repository does not claim Flathub approval.

## Data and backups

Native data lives at `$XDG_DATA_HOME/saifori/ledger.sqlite3`, normally
`~/.local/share/saifori/ledger.sqlite3`. Flatpak stores it inside its per-app data
directory. Export and verify a backup before replacing or moving a database.

For an existing installation with a different data path, close both applications
and use the non-destructive SQLite migration helper:

```bash
python3 tools/migrate-data.py --source /path/to/old/ledger.sqlite3 --destination /path/to/new/ledger.sqlite3
```

It captures WAL data with SQLite's backup API and never overwrites an existing
destination. Keep the original backup until you have checked the migrated data.
Screenshots in `assets/screenshots/` use synthetic data only.
