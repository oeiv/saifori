use crate::matching;
use crate::model::{
    month_offset, Activity, CategoryTotal, Draft, MonthTotal, Summary, Transaction, Transfer,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::Duration;

pub struct TransactionSearch<'a> {
    pub currency: &'a str,
    pub limit: usize,
    pub needle: &'a str,
    pub kind: &'a str,
    pub from: Option<&'a str>,
    pub until: Option<&'a str>,
    pub category: Option<&'a str>,
    pub day: Option<&'a str>,
}
pub type DuplicateCandidate = (i64, i64, String, String, String, i64, u8);
pub type ProductTotal = (String, i64, i64, i64, String);

pub struct Store {
    conn: Connection,
}

// SQLite schema revision. Keep this aligned with the final migration step.
const LATEST_SCHEMA_VERSION: i64 = 6;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).context("Could not open local database")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        Self::initialize(conn)
    }
    // Private in-memory workspace for the optional demonstration mode and tests.
    pub(crate) fn memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > LATEST_SCHEMA_VERSION {
            bail!("This database was created by a newer version of Saifori.");
        }
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch(r#"
                CREATE TABLE IF NOT EXISTS transactions (
                    id INTEGER PRIMARY KEY,
                    day TEXT NOT NULL CHECK(length(day)=10),
                    kind TEXT NOT NULL CHECK(kind IN ('income','expense')),
                    amount_minor INTEGER NOT NULL CHECK(amount_minor BETWEEN 1 AND 99999999999),
                    currency TEXT NOT NULL CHECK(length(currency) BETWEEN 3 AND 4),
                    category TEXT NOT NULL CHECK(length(category) BETWEEN 1 AND 60),
                    description TEXT NOT NULL CHECK(length(description) BETWEEN 1 AND 960),
                    method TEXT NOT NULL,
                    note TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS tx_currency_day ON transactions(currency,day DESC,id DESC);
                CREATE INDEX IF NOT EXISTS tx_currency_kind_day ON transactions(currency,kind,day DESC);
                CREATE TABLE IF NOT EXISTS budgets (
                    currency TEXT NOT NULL,
                    month TEXT NOT NULL CHECK(length(month)=7),
                    category TEXT NOT NULL,
                    amount_minor INTEGER NOT NULL CHECK(amount_minor BETWEEN 0 AND 99999999999),
                    PRIMARY KEY(currency,month,category)
                );
                CREATE TABLE IF NOT EXISTS settings (
                    name TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                INSERT OR IGNORE INTO settings(name,value) VALUES ('currency','MAD');
                PRAGMA user_version = 1;
            "#)?;
            tx.commit()?;
        }
        // Non-destructive migration: preserve every existing transaction, amount and date.
        if version < 2 {
            let tx = conn.transaction()?;
            tx.execute_batch(r#"
                CREATE TABLE IF NOT EXISTS transfers (
                    id INTEGER PRIMARY KEY,
                    day TEXT NOT NULL CHECK(length(day)=10),
                    amount_minor INTEGER NOT NULL CHECK(amount_minor BETWEEN 1 AND 99999999999),
                    currency TEXT NOT NULL,
                    source TEXT NOT NULL CHECK(length(source) BETWEEN 1 AND 120),
                    destination TEXT NOT NULL CHECK(length(destination) BETWEEN 1 AND 120),
                    note TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    CHECK(source <> destination)
                );
                CREATE INDEX IF NOT EXISTS transfer_currency_day ON transfers(currency,day DESC,id DESC);
                CREATE TABLE IF NOT EXISTS activity (
                    id INTEGER PRIMARY KEY,
                    entity TEXT NOT NULL CHECK(entity IN ('transaction','transfer')),
                    entity_id INTEGER NOT NULL,
                    action TEXT NOT NULL CHECK(action IN ('created','updated','deleted','restored')),
                    label TEXT NOT NULL,
                    amount_minor INTEGER NOT NULL,
                    currency TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS activity_created ON activity(created_at DESC,id DESC);

                INSERT OR IGNORE INTO settings(name,value) VALUES ('privacy','0');
            "#)?;
            if version == 1 || version == 0 {
                tx.execute_batch(
                    "ALTER TABLE transactions ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';",
                )?;
            }
            tx.execute_batch("INSERT INTO activity(entity,entity_id,action,label,amount_minor,currency,created_at) SELECT 'transaction',id,'created',description,amount_minor,currency,created_at FROM transactions WHERE NOT EXISTS(SELECT 1 FROM activity WHERE activity.entity='transaction' AND activity.entity_id=transactions.id AND activity.action='created'); PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        // Non-destructive v2→v3 migration for exact item quantities.
        // Existing totals are untouched; all historic transactions have quantity one.
        if version < 3 {
            let tx = conn.transaction()?;
            tx.execute_batch("ALTER TABLE transactions ADD COLUMN quantity INTEGER NOT NULL DEFAULT 1 CHECK(quantity BETWEEN 1 AND 10000); PRAGMA user_version = 3;")?;
            tx.commit()?;
        }
        // v3 → v4: private local conversation history; no secrets are stored here.
        if version < 4 {
            let tx = conn.transaction()?;
            tx.execute_batch(r#"
                CREATE TABLE IF NOT EXISTS chat_sessions (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 240),
                    month TEXT NOT NULL CHECK(length(month)=7),
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS chat_messages (
                    id INTEGER PRIMARY KEY,
                    session_id INTEGER NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
                    mine INTEGER NOT NULL CHECK(mine IN (0,1)),
                    body TEXT NOT NULL CHECK(length(body) BETWEEN 1 AND 40000),
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS chat_sessions_recent ON chat_sessions(updated_at DESC,id DESC);
                CREATE INDEX IF NOT EXISTS chat_messages_session ON chat_messages(session_id,id);
                INSERT OR IGNORE INTO settings(name,value) VALUES ('assistant_tabs','1');
                PRAGMA user_version=4;
            "#)?;
            tx.commit()?;
        }
        // v4 → v5: lossless product identity, optional aliases, categories and duplicate review.
        if version < 5 {
            let tx = conn.transaction()?;
            tx.execute_batch(r#"
                ALTER TABLE transactions ADD COLUMN product_key TEXT NOT NULL DEFAULT '';
                CREATE INDEX IF NOT EXISTS tx_product_lookup ON transactions(currency,kind,product_key,day);
                CREATE TABLE IF NOT EXISTS product_aliases(
                    alias_key TEXT PRIMARY KEY NOT NULL,
                    alias_name TEXT NOT NULL,
                    canonical_key TEXT NOT NULL,
                    canonical_name TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS product_alias_canonical ON product_aliases(canonical_key);
                CREATE TABLE IF NOT EXISTS custom_categories(
                    name TEXT PRIMARY KEY COLLATE NOCASE NOT NULL CHECK(length(name) BETWEEN 2 AND 240),
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS tx_duplicate_candidate_lookup ON transactions(currency,kind,day,amount_minor,quantity,method);
                CREATE TABLE IF NOT EXISTS ignored_duplicates(
                    a_id INTEGER NOT NULL,
                    b_id INTEGER NOT NULL,
                    PRIMARY KEY (a_id,b_id), CHECK(a_id<b_id)
                );
                INSERT OR IGNORE INTO settings(name,value) VALUES ('transaction_page','0');
            "#)?;
            let descriptions = {
                let mut statement = tx.prepare("SELECT id,description FROM transactions")?;
                let rows = statement
                    .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            {
                let mut statement =
                    tx.prepare("UPDATE transactions SET product_key=?1 WHERE id=?2")?;
                for (id, description) in descriptions {
                    statement.execute(params![matching::normalize_name(&description), id])?;
                }
            }
            tx.execute_batch("PRAGMA user_version=5;")?;
            tx.commit()?;
        }
        // v5 → v6: saved Smart Capture reviews. Secrets are never stored in this table.
        if version < 6 {
            let tx = conn.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS capture_history (
                    id INTEGER PRIMARY KEY,
                    source TEXT NOT NULL CHECK(source IN ('local','deepseek')),
                    submitted_text TEXT NOT NULL CHECK(length(submitted_text) BETWEEN 1 AND 16000),
                    drafts_json TEXT NOT NULL CHECK(length(drafts_json) BETWEEN 2 AND 120000),
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS capture_history_recent ON capture_history(id DESC);
                PRAGMA user_version=6;
            "#,
            )?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    pub fn save_capture_review(
        &mut self,
        source: &str,
        submitted: &str,
        drafts: &[Draft],
    ) -> Result<()> {
        if !["local", "deepseek"].contains(&source)
            || submitted.is_empty()
            || submitted.chars().count() > 4000
            || drafts.is_empty()
            || drafts.len() > 50
        {
            bail!("Invalid capture review");
        }
        let serialized = serde_json::to_string(drafts)?;
        if serialized.len() > 120000 {
            bail!("Capture review exceeds storage limit");
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO capture_history(source,submitted_text,drafts_json) VALUES (?1,?2,?3)",
            params![source, submitted, serialized],
        )?;
        tx.execute("DELETE FROM capture_history WHERE id NOT IN (SELECT id FROM capture_history ORDER BY id DESC LIMIT 60)",[])?;
        tx.commit()?;
        Ok(())
    }
    pub fn capture_reviews(&self) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt=self.conn.prepare("SELECT id,source,created_at,substr(submitted_text,1,96) FROM capture_history ORDER BY id DESC LIMIT 30")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn load_capture_review(&self, id: i64) -> Result<(String, Vec<Draft>)> {
        let (text, json): (String, String) = self.conn.query_row(
            "SELECT submitted_text,drafts_json FROM capture_history WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let drafts: Vec<Draft> = serde_json::from_str(&json)?;
        if drafts.len() > 50 {
            bail!("Invalid stored draft count");
        }
        Ok((text, drafts))
    }
    pub fn setting(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn set_setting(&mut self, name: &str, value: &str) -> Result<()> {
        if ![
            "language",
            "privacy",
            "assistant_tabs",
            "transaction_page",
            "transfer_compact_months",
            "test_mode",
            "ai_provider",
        ]
        .contains(&name)
        {
            bail!("Unsupported setting");
        }
        if (name == "language" && !crate::i18n::valid(value))
            || (name == "ai_provider" && !["deepseek", "claude", "openai"].contains(&value))
            || ((name == "privacy"
                || name == "assistant_tabs"
                || name == "transaction_page"
                || name == "transfer_compact_months"
                || name == "test_mode")
                && value != "0"
                && value != "1")
        {
            bail!("Invalid setting value");
        }
        self.conn.execute("INSERT INTO settings(name,value) VALUES (?1,?2) ON CONFLICT(name) DO UPDATE SET value=excluded.value",params![name,value])?;
        Ok(())
    }

    pub fn category_names(&self) -> Result<Vec<String>> {
        let mut names = crate::model::CATEGORIES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut statement = self
            .conn
            .prepare("SELECT name FROM custom_categories ORDER BY name COLLATE NOCASE")?;
        for name in statement.query_map([], |r| r.get::<_, String>(0))? {
            names.push(name?);
        }
        Ok(names)
    }
    fn category_allowed(&self, category: &str) -> Result<bool> {
        if crate::model::CATEGORIES.contains(&category) {
            return Ok(true);
        }
        let found = self
            .conn
            .query_row(
                "SELECT 1 FROM custom_categories WHERE name=?1 COLLATE NOCASE",
                [category],
                |r| r.get::<_, i32>(0),
            )
            .optional()?;
        Ok(found.is_some())
    }
    fn validate_draft(&self, draft: &Draft) -> Result<()> {
        draft.validate().map_err(anyhow::Error::msg)?;
        if !self.category_allowed(&draft.category)? {
            bail!("Unknown category. Add it in Settings first.");
        }
        Ok(())
    }
    pub fn add_custom_category(&mut self, name: &str) -> Result<()> {
        let value = matching::validate_category_name(name).map_err(anyhow::Error::msg)?;
        if crate::model::CATEGORIES
            .iter()
            .any(|base| base.eq_ignore_ascii_case(&value))
            || self.category_allowed(&value)?
        {
            bail!("This category already exists.");
        }
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM custom_categories", [], |r| r.get(0))?;
        if count >= 300 {
            bail!("Maximum 300 custom categories.");
        }
        self.conn
            .execute("INSERT INTO custom_categories(name) VALUES(?1)", [&value])?;
        Ok(())
    }
    pub fn remove_custom_category(&mut self, name: &str) -> Result<()> {
        if self
            .conn
            .query_row(
                "SELECT 1 FROM transactions WHERE category=?1 LIMIT 1",
                [name],
                |r| r.get::<_, i32>(0),
            )
            .optional()?
            .is_some()
            || self
                .conn
                .query_row(
                    "SELECT 1 FROM budgets WHERE category=?1 LIMIT 1",
                    [name],
                    |r| r.get::<_, i32>(0),
                )
                .optional()?
                .is_some()
        {
            bail!("This category is in use. Reassign transactions and budgets first.");
        }
        self.conn.execute(
            "DELETE FROM custom_categories WHERE name=?1 COLLATE NOCASE",
            [name],
        )?;
        Ok(())
    }
    /// Only user-reviewed aliases affect product statistics. Financial transactions stay untouched.
    pub fn link_product_name(&mut self, alias: &str, canonical: &str) -> Result<()> {
        let alias = alias.trim();
        let canonical = canonical.trim();
        let alias_key = matching::normalize_name(alias);
        let canonical_key = matching::normalize_name(canonical);
        if alias_key.chars().count() < 2
            || canonical_key.chars().count() < 2
            || alias.chars().count() > 240
            || canonical.chars().count() > 240
            || alias_key == canonical_key
        {
            bail!("Choose two distinct, nonempty product names.");
        }
        // Do not allow alias chains or cycles.
        let target: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT canonical_key,canonical_name FROM product_aliases WHERE alias_key=?1",
                [&canonical_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (canonical_key, canonical_name) =
            target.unwrap_or((canonical_key, canonical.to_string()));
        if alias_key == canonical_key {
            bail!("The names already represent the same product.");
        }
        let alias_exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM transactions WHERE product_key=?1 LIMIT 1",
                [&alias_key],
                |r| r.get(0),
            )
            .optional()?;
        if alias_exists.is_none() {
            bail!("The variant name must exist in saved transactions.");
        }
        let current: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM product_aliases WHERE alias_key=?1",
                [&alias_key],
                |r| r.get(0),
            )
            .optional()?;
        let aliases: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM product_aliases", [], |r| r.get(0))?;
        if current.is_none() && aliases >= 2000 {
            bail!("Maximum 2,000 product aliases.");
        }
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO product_aliases(alias_key,alias_name,canonical_key,canonical_name) VALUES (?1,?2,?3,?4) ON CONFLICT(alias_key) DO UPDATE SET alias_name=excluded.alias_name,canonical_key=excluded.canonical_key,canonical_name=excluded.canonical_name",
            params![alias_key,alias,canonical_key,canonical_name])?;
        // Existing aliases pointing at an old canonical alias must be updated too.
        tx.execute("UPDATE product_aliases SET canonical_key=?1,canonical_name=?2 WHERE canonical_key=?3 AND alias_key<>?3",
            params![canonical_key,canonical_name,alias_key])?;
        tx.commit()?;
        Ok(())
    }
    pub fn product_aliases(&self) -> Result<Vec<(String, String)>> {
        let mut statement=self.conn.prepare("SELECT alias_name,canonical_name FROM product_aliases ORDER BY alias_name COLLATE NOCASE LIMIT 2000")?;
        let result = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }
    pub fn unlink_product_name(&mut self, name: &str) -> Result<()> {
        let key = matching::normalize_name(name);
        self.conn
            .execute("DELETE FROM product_aliases WHERE alias_key=?1", [&key])?;
        Ok(())
    }
    pub fn similar_product_names(&self, currency: &str) -> Result<Vec<(String, String, u8)>> {
        let mut statement = self.conn.prepare(
            r#"SELECT MIN(TRIM(t.description)),COALESCE(p.canonical_key,t.product_key)
            FROM transactions t LEFT JOIN product_aliases p ON p.alias_key=t.product_key
            WHERE t.currency=?1 AND t.kind='expense' AND t.product_key<>''
            GROUP BY COALESCE(p.canonical_key,t.product_key) ORDER BY MAX(t.day) DESC LIMIT 300"#,
        )?;
        let rows = statement
            .query_map([currency], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result = Vec::new();
        for (i, (name, key)) in rows.iter().enumerate() {
            for (other_name, other_key) in rows.iter().skip(i + 1) {
                if key == other_key {
                    continue;
                }
                if let Some(score) = matching::similarity(name, other_name) {
                    result.push((name.clone(), other_name.clone(), score));
                }
            }
        }
        result.sort_by_key(|row| std::cmp::Reverse(row.2));
        result.truncate(30);
        Ok(result)
    }
    /// Strict candidates: same date, amount, quantity, payment method and similar names.
    /// Multiple legitimate purchases can satisfy this; never delete or merge automatically.
    pub fn duplicate_candidates(&self, currency: &str) -> Result<Vec<DuplicateCandidate>> {
        let mut statement = self.conn.prepare(
            r#"SELECT a.id,b.id,a.day,a.description,b.description,a.amount_minor
            FROM transactions a JOIN transactions b ON a.id<b.id
                AND a.currency=b.currency AND a.day=b.day AND a.kind=b.kind
                AND a.amount_minor=b.amount_minor AND a.quantity=b.quantity AND a.method=b.method
            WHERE a.currency=?1 AND a.kind='expense' AND NOT EXISTS(
                SELECT 1 FROM ignored_duplicates i WHERE i.a_id=a.id AND i.b_id=b.id)
            ORDER BY a.day DESC,a.id DESC LIMIT 500"#,
        )?;
        let rows = statement
            .query_map([currency], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut matches = Vec::new();
        for (first, second, day, a, b, amount) in rows {
            if let Some(score) = matching::similarity(&a, &b) {
                matches.push((first, second, day, a, b, amount, score));
            }
        }
        matches.truncate(50);
        Ok(matches)
    }
    pub fn ignore_duplicate_pair(&mut self, a: i64, b: i64) -> Result<()> {
        let (a, b) = (a.min(b), a.max(b));
        if a == b {
            bail!("Choose two different transactions.");
        }
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM ignored_duplicates", [], |r| r.get(0))?;
        if count >= 30000 {
            bail!("Maximum 30,000 ignored duplicate pairs.");
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO ignored_duplicates(a_id,b_id) VALUES(?1,?2)",
            params![a, b],
        )?;
        Ok(())
    }

    /// Conversation data remains local; API keys are managed separately by GNOME Keyring.
    pub fn chat_sessions(&self, limit: usize) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt=self.conn.prepare("SELECT id,title,month,updated_at FROM chat_sessions ORDER BY updated_at DESC,id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit.min(100) as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn chat_create(&mut self, title: &str, month: &str) -> Result<i64> {
        month_bounds(month)?;
        let title = title.trim().chars().take(60).collect::<String>();
        if title.is_empty() {
            bail!("Conversation title is empty.");
        }
        self.conn.execute(
            "INSERT INTO chat_sessions(title,month) VALUES (?1,?2)",
            params![title, month],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn chat_month(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT month FROM chat_sessions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn chat_set_month(&mut self, id: i64, month: &str) -> Result<()> {
        month_bounds(month)?;
        self.conn.execute(
            "UPDATE chat_sessions SET month=?1 WHERE id=?2",
            params![month, id],
        )?;
        Ok(())
    }
    pub fn chat_append(&mut self, id: i64, mine: bool, body: &str) -> Result<()> {
        if body.is_empty() || body.len() > 40_000 {
            bail!("Message size is not supported.");
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO chat_messages(session_id,mine,body) VALUES (?1,?2,?3)",
            params![id, i64::from(mine), body],
        )?;
        tx.execute(
            "UPDATE chat_sessions SET updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn chat_messages(&self, id: i64) -> Result<Vec<(bool, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT mine,body FROM chat_messages WHERE session_id=?1 ORDER BY id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([id], |r| Ok((r.get::<_, i64>(0)? == 1, r.get(1)?)))?;
        let mut result = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        result.reverse();
        Ok(result)
    }
    pub fn chat_export_json(&self) -> Result<Vec<u8>> {
        let mut stmt=self.conn.prepare("SELECT s.id,s.title,s.month,s.created_at,s.updated_at,m.mine,m.body,m.created_at FROM chat_sessions s LEFT JOIN chat_messages m ON m.session_id=s.id ORDER BY s.id,m.id")?;
        let rows = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "session_id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,
                "month":r.get::<_,String>(2)?,"session_created_at":r.get::<_,String>(3)?,
                "session_updated_at":r.get::<_,String>(4)?,
                "mine":r.get::<_,Option<i64>>(5)?.map(|v|v==1),
                "body":r.get::<_,Option<String>>(6)?,
                "message_created_at":r.get::<_,Option<String>>(7)?,
            }))
        })?;
        let mut entries = Vec::new();
        for record in rows {
            if entries.len() >= 100_000 {
                bail!("Chat export exceeds 100,000 rows.");
            }
            entries.push(record?);
        }
        let bytes =
            serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"entries":entries}))?;
        if bytes.len() > 40_000_000 {
            bail!("Chat export exceeds 40 MB.");
        }
        Ok(bytes)
    }
    pub fn chat_delete(&mut self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM chat_sessions WHERE id=?1", [id])?;
        Ok(())
    }
    /// Aggregated by normalized product description, with exact integer arithmetic.
    /// Filter by currency, expense kind and optionally calendar month.
    pub fn product_totals(
        &self,
        currency: &str,
        month: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ProductTotal>> {
        let (from, until) = if let Some(month) = month {
            month_bounds(month)?
        } else {
            ("0000-01-01".into(), "9999-12-31".into())
        };
        let mut stmt = self.conn.prepare(
            r#"SELECT COALESCE(MAX(p.canonical_name),MIN(TRIM(t.description))),
            COUNT(*),SUM(t.quantity),SUM(t.amount_minor),MAX(t.day)
           FROM transactions t LEFT JOIN product_aliases p ON p.alias_key=t.product_key
           WHERE t.currency=?1 AND t.kind='expense' AND t.day>=?2 AND t.day<?3 AND t.product_key<>''
           GROUP BY COALESCE(p.canonical_key,t.product_key)
           ORDER BY SUM(t.amount_minor) DESC LIMIT ?4"#,
        )?;
        let rows = stmt.query_map(params![currency, from, until, limit.min(100) as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn currency(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE name='currency'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "MAD".into()))
    }
    pub fn set_currency(&mut self, currency: &str) -> Result<()> {
        if !crate::model::CURRENCIES.contains(&currency) {
            bail!("Invalid currency");
        }
        self.conn.execute("INSERT INTO settings(name,value) VALUES ('currency',?1) ON CONFLICT(name) DO UPDATE SET value=excluded.value", [currency])?;
        Ok(())
    }
    fn log(
        tx: &rusqlite::Transaction<'_>,
        entity: &str,
        id: i64,
        action: &str,
        label: &str,
        amount: i64,
        currency: &str,
    ) -> Result<()> {
        tx.execute("INSERT INTO activity(entity,entity_id,action,label,amount_minor,currency) VALUES (?1,?2,?3,?4,?5,?6)",
            params![entity,id,action,label,amount,currency])?;
        Ok(())
    }
    pub fn add(&mut self, draft: &Draft) -> Result<i64> {
        self.validate_draft(draft)?;
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note,quantity,product_key) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![draft.day,draft.kind,draft.amount_minor,draft.currency,draft.category,draft.description.trim(),draft.method,draft.note,draft.quantity,matching::normalize_name(&draft.description)])?;
        let id = tx.last_insert_rowid();
        Self::log(
            &tx,
            "transaction",
            id,
            "created",
            &draft.description,
            draft.amount_minor,
            &draft.currency,
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn add_many(&mut self, drafts: &[Draft]) -> Result<()> {
        if drafts.is_empty() || drafts.len() > 50 {
            bail!("Expected 1–50 reviewed transactions.");
        }
        for draft in drafts {
            self.validate_draft(draft)?;
        }
        let tx = self.conn.transaction()?;
        {
            let mut statement=tx.prepare("INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note,quantity,product_key) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
            for draft in drafts {
                statement.execute(params![
                    draft.day,
                    draft.kind,
                    draft.amount_minor,
                    draft.currency,
                    draft.category,
                    draft.description.trim(),
                    draft.method,
                    draft.note,
                    draft.quantity,
                    matching::normalize_name(&draft.description)
                ])?;
                let id = tx.last_insert_rowid();
                Self::log(
                    &tx,
                    "transaction",
                    id,
                    "created",
                    &draft.description,
                    draft.amount_minor,
                    &draft.currency,
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn update(&mut self, id: i64, draft: &Draft) -> Result<()> {
        self.validate_draft(draft)?;
        let tx = self.conn.transaction()?;
        let count=tx.execute("UPDATE transactions SET day=?1,kind=?2,amount_minor=?3,currency=?4,category=?5,description=?6,method=?7,note=?8,quantity=?9,product_key=?10,updated_at=CURRENT_TIMESTAMP WHERE id=?11",
            params![draft.day,draft.kind,draft.amount_minor,draft.currency,draft.category,draft.description.trim(),draft.method,draft.note,draft.quantity,matching::normalize_name(&draft.description),id])?;
        if count != 1 {
            bail!("Transaction not found.");
        }
        Self::log(
            &tx,
            "transaction",
            id,
            "updated",
            &draft.description,
            draft.amount_minor,
            &draft.currency,
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete(&mut self, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let existing: Option<(String, i64, String)> = tx
            .query_row(
                "SELECT description,amount_minor,currency FROM transactions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((_label, _amount, currency)) = existing else {
            bail!("Transaction not found.");
        };
        tx.execute(
            "DELETE FROM ignored_duplicates WHERE a_id=?1 OR b_id=?1",
            [id],
        )?;
        tx.execute("DELETE FROM transactions WHERE id=?1", [id])?;
        // Removing a transaction also removes its descriptive audit trail.
        // Keep only a non-identifying deletion event.
        tx.execute(
            "DELETE FROM activity WHERE entity='transaction' AND entity_id=?1",
            [id],
        )?;
        Self::log(
            &tx,
            "transaction",
            id,
            "deleted",
            "Deleted transaction",
            0,
            &currency,
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn add_transfer(
        &mut self,
        day: &str,
        amount: i64,
        currency: &str,
        source: &str,
        destination: &str,
        note: &str,
    ) -> Result<i64> {
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").context("Invalid transfer date")?;
        let source = source.trim();
        let destination = destination.trim();
        if amount <= 0
            || amount > 99_999_999_999
            || !crate::model::CURRENCIES.contains(&currency)
            || source.is_empty()
            || destination.is_empty()
            || source == destination
            || source.chars().count() > 80
            || destination.chars().count() > 80
            || note.chars().count() > 2000
        {
            bail!("Check the amount, source account and destination account.");
        }
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO transfers(day,amount_minor,currency,source,destination,note) VALUES (?1,?2,?3,?4,?5,?6)",params![day,amount,currency,source,destination,note])?;
        let id = tx.last_insert_rowid();
        Self::log(
            &tx,
            "transfer",
            id,
            "created",
            &format!("{source} → {destination}"),
            amount,
            currency,
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn delete_transfer(&mut self, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let currency: Option<String> = tx
            .query_row("SELECT currency FROM transfers WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(currency) = currency else {
            bail!("Transfer not found.");
        };
        tx.execute("DELETE FROM transfers WHERE id=?1", [id])?;
        tx.execute(
            "DELETE FROM activity WHERE entity='transfer' AND entity_id=?1",
            [id],
        )?;
        Self::log(
            &tx,
            "transfer",
            id,
            "deleted",
            "Deleted transfer",
            0,
            &currency,
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn transfers(&self, currency: &str, limit: usize) -> Result<Vec<Transfer>> {
        let mut stmt=self.conn.prepare("SELECT id,day,amount_minor,currency,source,destination,note,created_at FROM transfers WHERE currency=?1 ORDER BY day DESC,id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![currency, limit.min(500) as i64], |r| {
            Ok(Transfer {
                id: r.get(0)?,
                day: r.get(1)?,
                amount_minor: r.get(2)?,
                currency: r.get(3)?,
                source: r.get(4)?,
                destination: r.get(5)?,
                note: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn activities(&self, limit: usize) -> Result<Vec<Activity>> {
        let mut stmt=self.conn.prepare("SELECT id,entity,entity_id,action,label,amount_minor,currency,created_at FROM activity ORDER BY created_at DESC,id DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit.min(500) as i64], |r| {
            Ok(Activity {
                id: r.get(0)?,
                entity: r.get(1)?,
                entity_id: r.get(2)?,
                action: r.get(3)?,
                label: r.get(4)?,
                amount_minor: r.get(5)?,
                currency: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn transaction(&self, id: i64) -> Result<Option<Transaction>> {
        Ok(self.conn.query_row("SELECT id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity FROM transactions WHERE id=?1", [id], row_tx).optional()?)
    }
    pub fn recent(
        &self,
        currency: &str,
        limit: usize,
        needle: &str,
        kind: &str,
    ) -> Result<Vec<Transaction>> {
        let limit = limit.min(250) as i64;
        let needle = format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.conn.prepare("SELECT id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity FROM transactions WHERE currency=?1 AND (?2='' OR kind=?2) AND (description LIKE ?3 ESCAPE '\\' OR category LIKE ?3 ESCAPE '\\' OR note LIKE ?3 ESCAPE '\\') ORDER BY day DESC,id DESC LIMIT ?4")?;
        let rows = stmt.query_map(params![currency, kind, needle, limit], row_tx)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Apply date/category filters in SQL before limiting results, so calendar
    /// searches work for older records beyond the most recent 250 transactions.
    pub fn search_transactions(&self, search: TransactionSearch<'_>) -> Result<Vec<Transaction>> {
        let TransactionSearch {
            currency,
            limit,
            needle,
            kind,
            from,
            until,
            category,
            day,
        } = search;
        let escaped = format!(
            "%{}%",
            needle
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt=self.conn.prepare(r#"SELECT id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity
            FROM transactions WHERE currency=?1
            AND (?2='' OR kind=?2)
            AND (description LIKE ?3 ESCAPE '\' OR category LIKE ?3 ESCAPE '\' OR note LIKE ?3 ESCAPE '\')
            AND (?4 IS NULL OR day>=?4) AND (?5 IS NULL OR day<?5)
            AND (?6 IS NULL OR category=?6) AND (?7 IS NULL OR day=?7)
            ORDER BY day DESC,id DESC LIMIT ?8"#)?;
        let rows = stmt.query_map(
            params![
                currency,
                kind,
                escaped,
                from,
                until,
                category,
                day,
                limit.min(1000) as i64
            ],
            row_tx,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn summary(&self, currency: &str, month: &str) -> Result<Summary> {
        let (from, until) = month_bounds(month)?;
        let result = self.conn.query_row(
            r#"
            SELECT COALESCE(SUM(CASE WHEN kind='income' THEN amount_minor ELSE 0 END),0),
                   COALESCE(SUM(CASE WHEN kind='expense' THEN amount_minor ELSE 0 END),0),
                   COUNT(*) FROM transactions WHERE currency=?1 AND day>=?2 AND day<?3
        "#,
            params![currency, from, until],
            |r| {
                Ok(Summary {
                    income: r.get(0)?,
                    expense: r.get(1)?,
                    count: r.get(2)?,
                })
            },
        )?;
        Ok(result)
    }
    /// Financial-year aggregates use transaction dates, never creation timestamps.
    /// Internal transfers are intentionally excluded from income and spending.
    pub fn year_summary(&self, currency: &str, year: i32) -> Result<Summary> {
        if !(1900..=2100).contains(&year) {
            bail!("Invalid financial year.");
        }
        let from = format!("{year:04}-01-01");
        let until = format!("{:04}-01-01", year + 1);
        let totals = self.conn.query_row(
            r#"
            SELECT COALESCE(SUM(CASE WHEN kind='income' THEN amount_minor ELSE 0 END),0),
                   COALESCE(SUM(CASE WHEN kind='expense' THEN amount_minor ELSE 0 END),0),
                   COUNT(*) FROM transactions
            WHERE currency=?1 AND day>=?2 AND day<?3
        "#,
            params![currency, from, until],
            |r| {
                Ok(Summary {
                    income: r.get(0)?,
                    expense: r.get(1)?,
                    count: r.get(2)?,
                })
            },
        )?;
        Ok(totals)
    }
    pub fn months(&self, currency: &str, last: &str, count: usize) -> Result<Vec<MonthTotal>> {
        let mut result = Vec::new();
        for offset in -(count.min(24) as i32 - 1)..=0 {
            let month =
                month_offset(last, offset).ok_or_else(|| anyhow::anyhow!("Invalid month"))?;
            let summary = self.summary(currency, &month)?;
            result.push(MonthTotal {
                month,
                income: summary.income,
                expense: summary.expense,
            });
        }
        Ok(result)
    }
    pub fn categories(&self, currency: &str, month: &str) -> Result<Vec<CategoryTotal>> {
        let (from, until) = month_bounds(month)?;
        let mut stmt = self.conn.prepare("SELECT category,SUM(amount_minor) FROM transactions WHERE currency=?1 AND kind='expense' AND day>=?2 AND day<?3 GROUP BY category ORDER BY 2 DESC")?;
        let rows = stmt.query_map(params![currency, from, until], |r| {
            Ok(CategoryTotal {
                category: r.get(0)?,
                amount_minor: r.get(1)?,
            })
        })?;
        let totals = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(totals)
    }
    pub fn budgets(&self, currency: &str, month: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare("SELECT category,amount_minor FROM budgets WHERE currency=?1 AND month=?2 ORDER BY category")?;
        let rows = stmt.query_map(params![currency, month], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let budgets = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(budgets)
    }
    pub fn set_budget(
        &mut self,
        currency: &str,
        month: &str,
        category: &str,
        amount_minor: i64,
    ) -> Result<()> {
        month_bounds(month)?;
        if !(0..=99_999_999_999).contains(&amount_minor) {
            bail!("Invalid budget amount.");
        }
        if !crate::model::CURRENCIES.contains(&currency) || !self.category_allowed(category)? {
            bail!("Invalid currency or category.");
        }
        self.conn.execute("INSERT INTO budgets(currency,month,category,amount_minor) VALUES(?1,?2,?3,?4) ON CONFLICT(currency,month,category) DO UPDATE SET amount_minor=excluded.amount_minor",
            params![currency,month,category,amount_minor])?;
        Ok(())
    }
    pub fn export_csv(&self) -> Result<Vec<u8>> {
        let mut writer = csv::Writer::from_writer(Vec::new());
        writer.write_record([
            "date",
            "type",
            "amount",
            "currency",
            "category",
            "description",
            "payment_method",
            "note",
            "created_at",
            "updated_at",
            "quantity",
            "unit_price",
        ])?;
        let mut stmt = self.conn.prepare("SELECT id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity FROM transactions ORDER BY day DESC,id DESC")?;
        let rows = stmt.query_map([], row_tx)?;
        for result in rows {
            let tx = result?;
            // Prefix spreadsheet formula-sensitive cells to make CSV safe when opened in spreadsheet apps.
            let safe = |s: &str| -> String {
                if matches!(
                    s.chars().next(),
                    Some('=') | Some('+') | Some('-') | Some('@') | Some('\t') | Some('\r')
                ) {
                    format!("'{s}")
                } else {
                    s.to_string()
                }
            };
            writer.write_record(&[
                tx.day,
                tx.kind,
                format!("{}.{:02}", tx.amount_minor / 100, tx.amount_minor % 100),
                tx.currency,
                safe(&tx.category),
                safe(&tx.description),
                safe(&tx.method),
                safe(&tx.note),
                tx.created_at,
                tx.updated_at,
                tx.quantity.to_string(),
                format!(
                    "{}.{:02}",
                    tx.amount_minor / tx.quantity / 100,
                    tx.amount_minor / tx.quantity % 100
                ),
            ])?;
        }
        Ok(writer.into_inner()?)
    }
    /// Atomically restore both old (v1) and current (v2) backups.
    pub fn restore_json(&mut self, data: &[u8]) -> Result<usize> {
        #[derive(serde::Deserialize)]
        struct Budget {
            month: String,
            currency: String,
            category: String,
            amount_minor: i64,
        }
        #[derive(serde::Deserialize)]
        struct Backup {
            schema_version: u8,
            transactions: Vec<Transaction>,
            budgets: Vec<Budget>,
            currency: String,
            #[serde(default)]
            transfers: Vec<Transfer>,
            #[serde(default)]
            activity: Vec<Activity>,
            #[serde(default)]
            custom_categories: Vec<String>,
            #[serde(default)]
            product_aliases: Vec<(String, String)>,
            #[serde(default)]
            ignored_duplicates: Vec<(i64, i64)>,
        }
        if data.len() > 40_000_000 {
            bail!("Backup exceeds 40 MB.");
        }
        let backup: Backup = serde_json::from_slice(data).context("Invalid backup JSON")?;
        if ![1, 2, 3, 4].contains(&backup.schema_version) {
            bail!("Unsupported backup schema.");
        }
        if backup.transactions.len() > 50_000
            || backup.budgets.len() > 10_000
            || backup.transfers.len() > 50_000
            || backup.activity.len() > 200_000
        {
            bail!("Too many records in backup.");
        }
        if !crate::model::CURRENCIES.contains(&backup.currency.as_str()) {
            bail!("Unsupported currency.");
        }
        if backup.custom_categories.len() > 300
            || backup.product_aliases.len() > 2000
            || backup.ignored_duplicates.len() > 30000
        {
            bail!("Backup metadata exceeds limits.");
        }
        let mut categories = std::collections::HashSet::new();
        for name in &backup.custom_categories {
            let name = matching::validate_category_name(name).map_err(anyhow::Error::msg)?;
            if crate::model::CATEGORIES
                .iter()
                .any(|base| base.eq_ignore_ascii_case(&name))
                || !categories.insert(name.to_lowercase())
            {
                bail!("Duplicate or invalid custom category.");
            }
        }
        for entry in &backup.transactions {
            Draft::from(entry).validate().map_err(anyhow::Error::msg)?;
            if !crate::model::CATEGORIES.contains(&entry.category.as_str())
                && !categories.contains(&entry.category.to_lowercase())
            {
                bail!("Backup references an unknown category.");
            }
            validate_timestamp(&entry.created_at)?;
            validate_timestamp(&entry.updated_at)?;
        }
        for b in &backup.budgets {
            month_bounds(&b.month)?;
            if !crate::model::CURRENCIES.contains(&b.currency.as_str())
                || (!crate::model::CATEGORIES.contains(&b.category.as_str())
                    && !categories.contains(&b.category.to_lowercase()))
                || !(0..=99_999_999_999).contains(&b.amount_minor)
            {
                bail!("Invalid budget.");
            }
        }
        for v in &backup.transfers {
            chrono::NaiveDate::parse_from_str(&v.day, "%Y-%m-%d")?;
            validate_timestamp(&v.created_at)?;
            if v.amount_minor <= 0
                || v.amount_minor > 99_999_999_999
                || !crate::model::CURRENCIES.contains(&v.currency.as_str())
                || v.source.trim().is_empty()
                || v.destination.trim().is_empty()
                || v.source == v.destination
                || v.source.chars().count() > 80
                || v.destination.chars().count() > 80
                || v.note.chars().count() > 2000
            {
                bail!("Invalid transfer in backup.");
            }
        }
        for event in &backup.activity {
            validate_timestamp(&event.created_at)?;
            if !["transfer", "transaction"].contains(&event.entity.as_str())
                || !["created", "updated", "deleted", "restored"].contains(&event.action.as_str())
                || event.label.len() > 960
                || !crate::model::CURRENCIES.contains(&event.currency.as_str())
            {
                bail!("Invalid activity record.");
            }
        }
        for (alias, canonical) in &backup.product_aliases {
            if matching::normalize_name(alias).len() < 2
                || matching::normalize_name(canonical).len() < 2
                || alias.chars().count() > 240
                || canonical.chars().count() > 240
            {
                bail!("Invalid product alias in backup.");
            }
        }
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM ignored_duplicates", [])?;
        tx.execute("DELETE FROM product_aliases", [])?;
        tx.execute("DELETE FROM custom_categories", [])?;
        tx.execute("DELETE FROM activity", [])?;
        tx.execute("DELETE FROM transfers", [])?;
        tx.execute("DELETE FROM transactions", [])?;
        tx.execute("DELETE FROM budgets", [])?;
        for name in &backup.custom_categories {
            tx.execute("INSERT INTO custom_categories(name) VALUES(?1)", [name])?;
        }
        for (alias, canonical) in &backup.product_aliases {
            let ak = matching::normalize_name(alias);
            let ck = matching::normalize_name(canonical);
            if ak != ck {
                tx.execute("INSERT INTO product_aliases(alias_key,alias_name,canonical_key,canonical_name) VALUES (?1,?2,?3,?4)",params![ak,alias,ck,canonical])?;
            }
        }
        for v in &backup.transactions {
            tx.execute("INSERT INTO transactions(id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity,product_key) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,COALESCE(NULLIF(?10,''),CURRENT_TIMESTAMP),?11,?12,?13)",params![v.id,v.day,v.kind,v.amount_minor,v.currency,v.category,v.description,v.method,v.note,v.created_at,v.updated_at,v.quantity,matching::normalize_name(&v.description)])?;
        }
        for v in &backup.transfers {
            tx.execute("INSERT INTO transfers(id,day,amount_minor,currency,source,destination,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,COALESCE(NULLIF(?8,''),CURRENT_TIMESTAMP))",params![v.id,v.day,v.amount_minor,v.currency,v.source,v.destination,v.note,v.created_at])?;
        }
        for b in &backup.budgets {
            tx.execute(
                "INSERT INTO budgets(month,currency,category,amount_minor) VALUES (?1,?2,?3,?4)",
                params![b.month, b.currency, b.category, b.amount_minor],
            )?;
        }
        if backup.schema_version >= 2 {
            for v in &backup.activity {
                tx.execute("INSERT INTO activity(id,entity,entity_id,action,label,amount_minor,currency,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,COALESCE(NULLIF(?8,''),CURRENT_TIMESTAMP))",params![v.id,v.entity,v.entity_id,v.action,v.label,v.amount_minor,v.currency,v.created_at])?;
            }
        } else {
            for v in &backup.transactions {
                tx.execute("INSERT INTO activity(entity,entity_id,action,label,amount_minor,currency,created_at) VALUES ('transaction',?1,'restored',?2,?3,?4,COALESCE(NULLIF(?5,''),CURRENT_TIMESTAMP))",params![v.id,v.description,v.amount_minor,v.currency,v.created_at])?;
            }
        }
        for (a, b) in &backup.ignored_duplicates {
            if a >= b {
                bail!("Invalid ignored duplicate pair.");
            }
            tx.execute(
                "INSERT OR IGNORE INTO ignored_duplicates(a_id,b_id) VALUES (?1,?2)",
                params![a, b],
            )?;
        }
        tx.execute("INSERT INTO settings(name,value) VALUES ('currency',?1) ON CONFLICT(name) DO UPDATE SET value=excluded.value",[&backup.currency])?;
        tx.commit()?;
        Ok(backup.transactions.len())
    }
    pub fn backup_json(&self) -> Result<Vec<u8>> {
        #[derive(serde::Serialize)]
        struct Budget {
            month: String,
            currency: String,
            category: String,
            amount_minor: i64,
        }
        #[derive(serde::Serialize)]
        struct Backup {
            schema_version: u8,
            transactions: Vec<Transaction>,
            transfers: Vec<Transfer>,
            activity: Vec<Activity>,
            budgets: Vec<Budget>,
            currency: String,
            custom_categories: Vec<String>,
            product_aliases: Vec<(String, String)>,
            ignored_duplicates: Vec<(i64, i64)>,
        }
        let mut stmt=self.conn.prepare("SELECT id,day,kind,amount_minor,currency,category,description,method,note,created_at,updated_at,quantity FROM transactions ORDER BY id")?;
        let transactions = stmt
            .query_map([], row_tx)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT month,currency,category,amount_minor FROM budgets ORDER BY month,category",
        )?;
        let budgets = stmt
            .query_map([], |r| {
                Ok(Budget {
                    month: r.get(0)?,
                    currency: r.get(1)?,
                    category: r.get(2)?,
                    amount_minor: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt=self.conn.prepare("SELECT id,day,amount_minor,currency,source,destination,note,created_at FROM transfers ORDER BY id")?;
        let transfers = stmt
            .query_map([], |r| {
                Ok(Transfer {
                    id: r.get(0)?,
                    day: r.get(1)?,
                    amount_minor: r.get(2)?,
                    currency: r.get(3)?,
                    source: r.get(4)?,
                    destination: r.get(5)?,
                    note: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt=self.conn.prepare("SELECT id,entity,entity_id,action,label,amount_minor,currency,created_at FROM activity ORDER BY id")?;
        let activity = stmt
            .query_map([], |r| {
                Ok(Activity {
                    id: r.get(0)?,
                    entity: r.get(1)?,
                    entity_id: r.get(2)?,
                    action: r.get(3)?,
                    label: r.get(4)?,
                    amount_minor: r.get(5)?,
                    currency: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let custom_categories = {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM custom_categories ORDER BY name")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let product_aliases = self.product_aliases()?;
        let ignored_duplicates = {
            let mut stmt = self
                .conn
                .prepare("SELECT a_id,b_id FROM ignored_duplicates ORDER BY a_id,b_id")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(serde_json::to_vec_pretty(&Backup {
            schema_version: 4,
            transactions,
            transfers,
            activity,
            budgets,
            currency: self.currency()?,
            custom_categories,
            product_aliases,
            ignored_duplicates,
        })?)
    }
}

fn validate_timestamp(value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .context("Invalid saved timestamp")?;
    Ok(())
}

fn row_tx(row: &rusqlite::Row<'_>) -> rusqlite::Result<Transaction> {
    Ok(Transaction {
        id: row.get(0)?,
        day: row.get(1)?,
        kind: row.get(2)?,
        amount_minor: row.get(3)?,
        currency: row.get(4)?,
        category: row.get(5)?,
        description: row.get(6)?,
        method: row.get(7)?,
        note: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        quantity: row.get(11)?,
    })
}

fn month_bounds(month: &str) -> Result<(String, String)> {
    let next = month_offset(month, 1).context("Invalid month")?;
    Ok((format!("{month}-01"), format!("{next}-01")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Draft {
        Draft {
            day: "2026-11-03".into(),
            kind: "expense".into(),
            amount_minor: 2575,
            currency: "MAD".into(),
            category: "Groceries".into(),
            description: "Sample".into(),
            method: "Cash".into(),
            note: String::new(),
            quantity: 1,
        }
    }
    #[test]
    fn annual_totals_respect_calendar_year_currency_and_transfer_neutrality() -> Result<()> {
        let mut db = Store::memory()?;
        let mut expense = sample();
        expense.day = "2026-01-01".into();
        expense.amount_minor = 2500;
        db.add(&expense)?;
        let mut income = sample();
        income.day = "2026-12-31".into();
        income.kind = "income".into();
        income.category = "Salary".into();
        income.amount_minor = 10000;
        db.add(&income)?;
        let mut next_year = sample();
        next_year.day = "2027-01-01".into();
        next_year.amount_minor = 3000;
        db.add(&next_year)?;
        let mut other_currency = sample();
        other_currency.currency = "EUR".into();
        other_currency.amount_minor = 9999;
        db.add(&other_currency)?;
        db.add_transfer("2026-02-01", 5000, "MAD", "Cash", "Bank", "")?;
        let year = db.year_summary("MAD", 2026)?;
        assert_eq!((year.income, year.expense, year.count), (10000, 2500, 2));
        assert_eq!(year.balance(), 7500);
        assert_eq!(db.year_summary("MAD", 2027)?.expense, 3000);
        assert!(db.year_summary("MAD", 1000).is_err());
        Ok(())
    }
    #[test]
    fn crud_and_money_are_exact() -> Result<()> {
        let mut db = Store::memory()?;
        let id = db.add(&sample())?;
        assert_eq!(db.summary("MAD", "2026-11")?.expense, 2575);
        let mut edited = sample();
        edited.amount_minor = 2576;
        db.update(id, &edited)?;
        assert_eq!(db.transaction(id)?.unwrap().amount_minor, 2576);
        db.delete(id)?;
        assert_eq!(db.summary("MAD", "2026-11")?.count, 0);
        Ok(())
    }
    #[test]
    fn atomic_batch_rejects_invalid() -> Result<()> {
        let mut db = Store::memory()?;
        let mut bad = sample();
        bad.amount_minor = -1;
        assert!(db.add_many(&[sample(), bad]).is_err());
        assert_eq!(db.summary("MAD", "2026-11")?.count, 0);
        Ok(())
    }
    #[test]
    fn currency_isolated_and_budgets() -> Result<()> {
        let mut db = Store::memory()?;
        db.add(&sample())?;
        assert_eq!(db.summary("USD", "2026-11")?.count, 0);
        db.set_budget("MAD", "2026-11", "Groceries", 10000)?;
        assert_eq!(db.budgets("MAD", "2026-11")?.len(), 1);
        Ok(())
    }
    #[test]
    fn restore_is_atomic_and_validates() -> Result<()> {
        let mut db = Store::memory()?;
        db.add(&sample())?;
        let backup = db.backup_json()?;
        db.add(&sample())?;
        assert_eq!(db.restore_json(&backup)?, 1);
        assert_eq!(db.summary("MAD", "2026-11")?.count, 1);
        let malformed = b"{not json}";
        assert!(db.restore_json(malformed).is_err());
        assert_eq!(db.summary("MAD", "2026-11")?.count, 1);
        Ok(())
    }
    #[test]
    fn transfer_is_neutral_and_activity_tracked() -> Result<()> {
        let mut db = Store::memory()?;
        let id = db.add(&sample())?;
        let created = db.transaction(id)?.unwrap().created_at;
        assert!(!created.is_empty());
        db.add_transfer("2026-11-03", 5000, "MAD", "Cash", "Bank", "")?;
        assert_eq!(db.summary("MAD", "2026-11")?.expense, 2575);
        assert_eq!(db.transfers("MAD", 10)?.len(), 1);
        assert_eq!(db.activities(10)?.len(), 2);
        let saved = db.backup_json()?;
        db.restore_json(&saved)?;
        assert_eq!(db.transfers("MAD", 10)?.len(), 1);
        assert_eq!(db.activities(10)?.len(), 2);
        assert_eq!(db.transaction(id)?.unwrap().created_at, created);
        let transfer_id = db.transfers("MAD", 10)?[0].id;
        db.delete_transfer(transfer_id)?;
        assert_eq!(db.transfers("MAD", 10)?.len(), 0);
        assert_eq!(db.summary("MAD", "2026-11")?.expense, 2575);
        Ok(())
    }
    #[test]
    fn quantity_roundtrip_and_legacy_backup() -> Result<()> {
        let mut db = Store::memory()?;
        let mut item = sample();
        item.quantity = 3;
        item.amount_minor = 3750;
        let id = db.add(&item)?;
        assert_eq!(db.transaction(id)?.unwrap().quantity, 3);
        let exported = db.backup_json()?;
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&exported)?["schema_version"],
            4
        );
        db.restore_json(&exported)?;
        assert_eq!(db.transaction(id)?.unwrap().quantity, 3);
        assert_eq!(db.summary("MAD", "2026-11")?.expense, 3750);
        let mut legacy: serde_json::Value = serde_json::from_slice(&exported)?;
        legacy["schema_version"] = serde_json::json!(2);
        for item in legacy["transactions"].as_array_mut().unwrap() {
            item.as_object_mut().unwrap().remove("quantity");
        }
        db.restore_json(&serde_json::to_vec(&legacy)?)?;
        assert_eq!(db.transaction(id)?.unwrap().quantity, 1);
        Ok(())
    }
    #[test]
    fn migrate_old_v2_without_losing_transactions() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE transactions(id INTEGER PRIMARY KEY, day TEXT NOT NULL, kind TEXT NOT NULL, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL, category TEXT NOT NULL, description TEXT NOT NULL, method TEXT NOT NULL, note TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT ''); CREATE TABLE settings(name TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note) VALUES('2025-03-02','expense',1250,'MAD','Other','Old purchase','Cash',''); PRAGMA user_version=2;")?;
        let store = Store::initialize(conn)?;
        let tx = store.transaction(1)?.unwrap();
        assert_eq!(tx.day, "2025-03-02");
        assert_eq!(tx.amount_minor, 1250);
        assert_eq!(tx.quantity, 1);
        assert_eq!(
            store
                .conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            LATEST_SCHEMA_VERSION
        );
        // Migration includes v6 Smart Capture history without losing the old transaction.
        assert_eq!(
            store.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='capture_history'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        assert_eq!(store.summary("MAD", "2025-03")?.expense, 1250);
        Ok(())
    }
    #[test]
    fn language_and_privacy_settings() -> Result<()> {
        let mut db = Store::memory()?;
        assert_eq!(db.setting("privacy")?, Some("0".into()));
        db.set_setting("privacy", "1")?;
        db.set_setting("language", "es")?;
        assert_eq!(db.setting("language")?, Some("es".into()));
        assert!(db.set_setting("language", "ar").is_err());
        assert!(db.set_setting("language", "invalid").is_err());
        // The Settings provider selector needs a whitelisted persisted choice.
        db.set_setting("ai_provider", "claude")?;
        assert_eq!(db.setting("ai_provider")?, Some("claude".into()));
        db.set_setting("ai_provider", "openai")?;
        assert_eq!(db.setting("ai_provider")?, Some("openai".into()));
        assert!(db.set_setting("ai_provider", "unknown").is_err());
        assert!(db.set_setting("ai_provider", "OpenAI").is_err());
        Ok(())
    }
    #[test]
    fn csv_injection_escaped() -> Result<()> {
        let mut db = Store::memory()?;
        let mut tx = sample();
        tx.description = "=1+1".into();
        db.add(&tx)?;
        let bytes = db.export_csv()?;
        let csv = String::from_utf8(bytes)?;
        assert!(csv.contains("'=1+1"));
        Ok(())
    }
    #[test]
    fn chat_persists_and_deletes_without_touching_finances() {
        let mut store = Store::memory().unwrap();
        let id = store.chat_create("Coffee and rent", "2026-10").unwrap();
        store.chat_append(id, true, "October spending?").unwrap();
        store.chat_append(id, false, "684.00 MAD").unwrap();
        assert_eq!(store.chat_messages(id).unwrap().len(), 2);
        let exported = store.chat_export_json().unwrap();
        let archive: serde_json::Value = serde_json::from_slice(&exported).unwrap();
        assert_eq!(archive["entries"].as_array().unwrap().len(), 2);
        assert_eq!(store.chat_month(id).unwrap().as_deref(), Some("2026-10"));
        store.chat_delete(id).unwrap();
        assert!(store.chat_messages(id).unwrap().is_empty());
        assert_eq!(store.summary("MAD", "2026-10").unwrap().count, 0);
    }
    #[test]
    fn aggregates_products_by_name_and_quantity() {
        let mut store = Store::memory().unwrap();
        let mut d = sample();
        d.day = "2026-10-15".into();
        d.description = " Coffee ".into();
        d.amount_minor = 3000;
        d.quantity = 3;
        store.add(&d).unwrap();
        d.description = "coffee".into();
        d.amount_minor = 2000;
        d.quantity = 2;
        store.add(&d).unwrap();
        let found = store.product_totals("MAD", Some("2026-10"), 10).unwrap();
        let row = found
            .iter()
            .find(|r| r.0.trim().eq_ignore_ascii_case("coffee"))
            .unwrap();
        assert_eq!((row.1, row.2, row.3), (2, 5, 5000));
        assert!(store
            .product_totals("MAD", Some("2026-11"), 10)
            .unwrap()
            .is_empty());
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    fn sample(name: &str, day: &str) -> Draft {
        Draft {
            day: day.into(),
            kind: "expense".into(),
            amount_minor: 1250,
            quantity: 1,
            currency: "MAD".into(),
            category: "Groceries".into(),
            description: name.into(),
            method: "Cash".into(),
            note: String::new(),
        }
    }
    #[test]
    fn custom_categories_are_persisted_and_validated() -> Result<()> {
        let mut db = Store::memory()?;
        assert!(db.add_custom_category("Groceries").is_err());
        assert!(db.add_custom_category("<unsafe>").is_err());
        let mut invalid = sample("Cat food", "2026-11-04");
        invalid.category = "Cat food".into();
        assert!(db.add(&invalid).is_err());
        db.add_custom_category("Cat food")?;
        assert!(db.category_names()?.contains(&"Cat food".to_string()));
        let mut item = sample("Cat food", "2026-11-04");
        item.category = "Cat food".into();
        let id = db.add(&item)?;
        assert!(db.remove_custom_category("Cat food").is_err());
        assert_eq!(db.transaction(id)?.unwrap().category, "Cat food");
        db.set_budget("MAD", "2026-11", "Cat food", 5000)?;
        assert_eq!(db.budgets("MAD", "2026-11")?.len(), 1);
        Ok(())
    }
    #[test]
    fn alias_merges_statistics_not_transactions() -> Result<()> {
        let mut db = Store::memory()?;
        let one = db.add(&sample("Coffee Latte", "2026-11-04"))?;
        let two = db.add(&sample("Coffe Latte", "2026-11-05"))?;
        assert_eq!(db.product_totals("MAD", None, 10)?.len(), 2);
        assert!(!db.similar_product_names("MAD")?.is_empty());
        db.link_product_name("Coffe Latte", "Coffee Latte")?;
        let totals = db.product_totals("MAD", None, 10)?;
        assert_eq!(totals.len(), 1);
        assert_eq!((totals[0].1, totals[0].2, totals[0].3), (2, 2, 2500));
        assert_eq!(db.transaction(one)?.unwrap().description, "Coffee Latte");
        assert_eq!(db.transaction(two)?.unwrap().description, "Coffe Latte");
        db.unlink_product_name("Coffe Latte")?;
        assert_eq!(db.product_totals("MAD", None, 10)?.len(), 2);
        Ok(())
    }
    #[test]
    fn duplicate_candidates_are_review_only_and_dismissable() -> Result<()> {
        let mut db = Store::memory()?;
        let first = db.add(&sample("Cafe Latte", "2026-11-04"))?;
        let second = db.add(&sample("Cafe Latte", "2026-11-04"))?;
        assert_eq!(db.duplicate_candidates("MAD")?.len(), 1);
        db.ignore_duplicate_pair(first, second)?;
        assert!(db.duplicate_candidates("MAD")?.is_empty());
        let backup = db.backup_json()?;
        let mut restored = Store::memory()?;
        restored.restore_json(&backup)?;
        assert!(restored.duplicate_candidates("MAD")?.is_empty());
        assert_eq!(db.summary("MAD", "2026-11")?.expense, 2500);
        Ok(())
    }
    #[test]
    fn backup_preserves_aliases_and_custom_categories() -> Result<()> {
        let mut db = Store::memory()?;
        db.add_custom_category("Health supplies")?;
        let mut item = sample("Coffe Latte", "2026-11-04");
        item.category = "Health supplies".into();
        db.add(&item)?;
        db.link_product_name("Coffe Latte", "Coffee Latte")?;
        let backup = db.backup_json()?;
        let json: serde_json::Value = serde_json::from_slice(&backup)?;
        assert_eq!(json["schema_version"], 4);
        let mut restored = Store::memory()?;
        restored.restore_json(&backup)?;
        assert_eq!(
            restored.category_names()?.last().unwrap(),
            "Health supplies"
        );
        assert_eq!(restored.product_aliases()?.len(), 1);
        assert_eq!(
            restored.product_totals("MAD", None, 10)?[0].0,
            "Coffee Latte"
        );
        Ok(())
    }
    #[test]
    fn migrating_v5_to_v6_keeps_financial_data() -> Result<()> {
        let mut db = Store::memory()?;
        let id = db.add(&sample("Book purchase", "2026-11-04"))?;
        // Simulate a v5 file: its capture history did not exist yet.
        db.conn.execute_batch("DROP INDEX IF EXISTS capture_history_recent; DROP TABLE capture_history; PRAGMA user_version=5;")?;
        let upgraded = Store::initialize(db.conn)?;
        assert_eq!(
            upgraded
                .conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            LATEST_SCHEMA_VERSION
        );
        assert_eq!(
            upgraded.transaction(id)?.unwrap().description,
            "Book purchase"
        );
        assert_eq!(
            upgraded.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='capture_history'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        Ok(())
    }
    #[test]
    fn current_schema_stores_product_keys_and_capture_history() -> Result<()> {
        let mut db = Store::memory()?;
        let id = db.add(&sample("Café Latte", "2026-11-04"))?;
        assert_eq!(
            db.conn.query_row(
                "SELECT product_key FROM transactions WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0)
            )?,
            "cafe latte"
        );
        assert_eq!(
            db.conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            LATEST_SCHEMA_VERSION
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='capture_history'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        assert_eq!(db.transaction(id)?.unwrap().description, "Café Latte");
        Ok(())
    }
}
