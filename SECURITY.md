# Security Policy

Saifori is a development-stage personal finance application, not a secure vault. The local SQLite database and exports are not encrypted at rest. GNOME Keyring stores API credentials separately. Read [PRIVACY.md](PRIVACY.md) before use.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting feature if enabled for this repository. Otherwise contact the maintainer through a private channel indicated in the GitHub repository profile. **Do not post API keys, private data or exploit-sensitive details in a public issue.**

Do not assume published releases are security-audited; request testing and review for any handling of secrets, backup/restore, remote APIs or database migrations.
