# Contributing to Saifori

## Development setup

See the [build instructions](README.md#build-from-source) for native dependencies.
Keep `Cargo.lock` in version control and use `--locked` for reproducible builds.
Ordinary native builds do not require vendored dependencies or a local Cargo
source override.

Run the native checks before opening a pull request:

```bash
bash tools/preflight.sh
```

This runs the repository and packaging checks, rustfmt, strict Clippy, unit tests
and a release build. Individual checks are also available:

```bash
python3 tools/check-static.py
python3 tools/check-project.py
python3 tools/check-source.py
python3 tools/check-packaging.py
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The UI supports English and Spanish. Keep canonical UI strings in English and
update the Spanish catalog when changing translated strings. Adding a language
also requires updating the language selection and tests.

## Desktop tests

Install Xvfb, D-Bus, ImageMagick and system Python's `pyatspi`. Build the release
binary, then run:

```bash
/usr/bin/python3 tools/test-desktop-probe.py
bash tools/desktop-smoke.sh
```

The smoke test uses temporary storage and synthetic records to exercise real
navigation, labelled controls and transaction creation. It also checks that test
transactions did not enter the persistent ledger. Logs and screenshots are saved
under the ignored `target/desktop-smoke-logs/` directory; set `SAIFORI_LOG_DIR` to
choose another location. Database files and credentials are not copied into the
diagnostic directory.

These tests cover native GTK behavior. Test file chooser portals, keyring
save/load/remove, keyboard navigation, Orca and narrow windows in an actual
Flatpak desktop session too. Use disposable credentials and synthetic data.

## Flatpak packaging

The development manifest uses GNOME Platform/SDK 50 and the Rust SDK extension.
Configure the Flathub remote and install those prerequisites and
`flatpak-builder`, then run:

```bash
bash tools/build-flatpak.sh
flatpak run io.github.oeiv.Saifori
```

`packaging/cargo-sources.json` is generated from `Cargo.lock` using the official
[flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo).
Flatpak Builder downloads checksum-pinned dependencies before building offline.
Regenerate the source recipe when updating dependencies, inspect its diff, and
run `python3 tools/check-packaging.py`. No developer-local vendor folder is needed
for the Flatpak build. Do not change checksums to accommodate a failed download.

The **Flatpak** workflow builds and installs a development bundle on package
changes or manual dispatch. Successful runs upload `Saifori-flatpak-x86_64`;
build logs are uploaded as `saifori-flatpak-logs-*` even on failure. Artifacts are
retained for seven days. The workflow frees unused runner SDKs before pulling
the GNOME build image, records disk usage, and preserves failing exit statuses.

The app requests display, graphics, network and Secret Service access. It has no
home/host filesystem grant or broad session/system bus permission. File dialogs
use the document portal when sandboxed, and network access serves optional AI
requests.

## Distribution

Before distribution, install `org.flatpak.Builder` and run:

```bash
bash tools/verify-flatpak.sh
```

This verifies public screenshot URLs, metadata, the development manifest and
build artifacts. Source and screenshot URLs must reference an actual public
release. Update and verify the planned `v0.1.0` screenshot URLs when publishing
that release; a successful native build alone does not establish sandbox or
Flathub readiness.

Review the [current Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
before submitting. They require disclosure of generated material, prohibit
AI-generated or AI-assisted submission manifests, and require stable releases
and meaningful development history. The manifest in this repository is
AI-assisted development packaging. A human maintainer must independently author
the submission manifest. This repository does not claim Flathub approval.

AI assistance contributed to implementation, refactoring, tests, build tooling,
metadata and documentation. Preserve an accurate disclosure when preparing a
submission, including any additional generated content known to the maintainer.

## Data storage and migration

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
When changing the database schema, add a non-destructive migration test.

## Privacy and issue reports

Never include API keys, access tokens, local databases, exports, private financial
records or credential screenshots in an issue, patch or commit. Use synthetic
data; the bundled screenshots use synthetic records. Avoid logging secrets or
sending user data to AI providers without an explicit user action. See
[PRIVACY.md](PRIVACY.md) and [SECURITY.md](SECURITY.md).

For bug reports, include reproduction steps, expected behavior, screenshots with
fictional data and the relevant Linux/GTK versions.
