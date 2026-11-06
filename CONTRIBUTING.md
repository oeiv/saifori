# Contributing to Saifori

Thank you for helping improve the application.

## Development

Use Rust stable, GTK4, Libadwaita, libsecret and SQLite. For an ordinary online checkout, run `cargo test` and `cargo fmt --all`. A native GUI session is needed for visual and interaction checks.

Run these checks before opening a pull request:

```bash
python3 tools/check-static.py
python3 tools/check-project.py
python3 tools/check-source.py
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The source includes English and Spanish UI catalogs only. Keep new UI strings in English by default and update the Spanish catalog when editing translated strings. Do not add an unsupported locale reference without also updating the language selection and tests.

## Privacy and secure contributions

Never include API keys, access tokens, local SQLite files, exports, private financial records or credential screenshots in an issue, patch or commit. Use small, synthetic test data. When changing the database schema, add a non-destructive migration test. Avoid logging secrets or sending raw user transactions to AI providers without explicit user action.

Please describe the issue, reproduction steps, expected behavior, screenshots with fictional data, and Fedora/GTK versions for UI bugs.
