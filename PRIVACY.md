# Privacy and Data Handling

- **Local storage:** The application uses SQLite in its per-user application data directory. Transactions, chat history and capture review history may contain personal information, stored as plaintext unless protected by your system's separate storage encryption.
- **API keys:** Provider keys are stored in GNOME Keyring / Secret Service, not in the SQLite database. Keys are loaded into application memory while in use.
- **Network traffic:** Remote AI functions require a configured provider and an explicit request; they connect to the selected provider's API over HTTPS. The chat sends the submitted prompt, recent conversation context and aggregated financial totals. Smart Capture sends the text submitted for extraction. See the provider's own privacy terms for their retention and data handling.
- **Exports:** CSV and JSON backups are not encrypted. Store them privately; never commit them to a public repository.
- **Privacy Mode:** Masks values in the UI but does not encrypt the underlying storage.
- **Test Mode:** Generates synthetic data separately from the personal ledger.

No telemetry or analytics service is intentionally added by the application's own source, but provider APIs and your operating system have their own infrastructure and policies.
