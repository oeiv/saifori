#!/usr/bin/env python3
"""Non-compilation checks that can run without Rust or GNOME SDK."""
import json
import pathlib
import re
import sqlite3
import tomllib
import xml.etree.ElementTree as ET

def compact(value): return "".join(value.split())

root = pathlib.Path(__file__).resolve().parent.parent
cargo = tomllib.loads((root / "Cargo.toml").read_text())
assert cargo["package"]["name"] == "saifori"
manifest = json.loads((root / "io.github.oeiv.Saifori.json").read_text())
assert manifest["runtime"] == "org.gnome.Platform"
assert manifest["runtime-version"] == "50"
assert not any("--filesystem=home" in p for p in manifest["finish-args"])
assert manifest["command"] == "saifori"
assert any(b.get("name") == "saifori" and b.get("path") == "src/main.rs" for b in cargo.get("bin", []))

ET.parse(root / "assets/io.github.oeiv.Saifori.metainfo.xml")
for icon in (root / "assets").glob("*.svg"):
    ET.parse(icon)

src = "\n".join(p.read_text() for p in (root / "src").rglob("*.rs"))
assert "#![deny(unsafe_op_in_unsafe_fn)]" in (root / "src/main.rs").read_text()
assert "mod secret_store;" in (root / "src/main.rs").read_text()
assert "--talk-name=org.freedesktop.secrets" in manifest["finish-args"]
assert "secret_password_store_sync" in (root / "src/secret_store.rs").read_text()
assert "secret_password_lookup_sync" in (root / "src/secret_store.rs").read_text()
assert "secret_password_clear_sync" in (root / "src/secret_store.rs").read_text()
assert "api_key.set_text(&state.api_key" not in "\n".join(x.read_text() for x in [root / "src/ui.rs", *sorted((root / "src/ui").glob("*.rs"))])
assert compact("https://api.deepseek.com/chat/completions") in compact(src)
assert compact("reqwest::redirect::Policy::none()") in compact(src)
assert compact("timeout(Duration::from_secs(45))") in compact(src)  # shared timeout for all three AI providers
assert compact("set_permissions") in compact(src)
assert compact("params![") in compact(src)
assert compact("Amount must be positive") in compact(src)
assert compact("restore_json") in compact(src)
assert compact("fn send_chat_message") in compact(src)
assert compact("fn transfers_page") in compact(src)
assert compact("fn activity_page") in compact(src)
assert compact("fn help_page") in compact(src)
assert compact("fn masked_money") in compact(src)
# Reject embedded absolute home paths and email addresses without naming anyone.
for pattern in (r"/home/[A-Za-z0-9._-]+", r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"):
    assert not re.search(pattern, src), "Unexpected personal identifier in source"

# Parse the real SQL migration string (no database schema reimplementation).
migration = re.search(r'tx\.execute_batch\(r#"(.*?)"#\)\?;', (root / "src/db.rs").read_text(), re.S)
assert migration, "Could not locate SQLite migration"
conn = sqlite3.connect(":memory:")
conn.executescript(migration.group(1))
assert conn.execute("PRAGMA user_version").fetchone()[0] == 1
conn.execute("INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note) VALUES (?,?,?,?,?,?,?,?)",
    ("2026-11-03", "expense", 2575, "MAD", "Groceries", "Test", "Cash", ""))
assert conn.execute("SELECT SUM(amount_minor) FROM transactions").fetchone()[0] == 2575
try:
    conn.execute("INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note) VALUES (?,?,?,?,?,?,?,?)",
        ("2026-11-03", "expense", -99, "MAD", "Groceries", "Bad", "Cash", ""))
except sqlite3.IntegrityError:
    pass
else:
    raise AssertionError("Negative amounts must be rejected by SQLite")
print("Static checks passed: Cargo TOML, Flatpak manifest, XML/SVG, privacy invariants, and real SQLite migration/constraints.")
import runpy
runpy.run_path(str(root / "tools" / "check-sqlite.py"), run_name="__main__")
print("Rust compilation and GUI behavior still require cargo + GNOME SDK on the target system.")
