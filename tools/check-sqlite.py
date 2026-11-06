#!/usr/bin/env python3
"""Offline structural QA; does not replace cargo test or GTK runtime testing."""
import json,re,sqlite3,sys
from pathlib import Path
def compact(value): return "".join(value.split())

root=Path(__file__).resolve().parents[1]
issues=[]
src={p.name:p.read_text() for p in (root/'src').glob('*.rs')}
src['ui.rs'] += '\n'.join(p.read_text() for p in (root/'src/ui').glob('*.rs'))
try:
    manifest=json.loads((root/'io.github.oeiv.Saifori.json').read_text())
    assert any(x.get('path')=='locales' for x in manifest['modules'][0]['sources'])
    assert 'org.freedesktop.Sdk.Extension.rust-stable' in manifest['sdk-extensions']
except Exception as ex:issues.append(f'manifest: {ex}')
try:
    catalogs={l:json.loads((root/'locales'/f'{l}.json').read_text()) for l in ['en','es']}
    assert len(catalogs['es'])>150 and len(catalogs['en'])>150
    assert all(catalogs[l].get('Overview') for l in catalogs)
    assert all(catalogs[l].get('Help & FAQ') for l in catalogs)
except Exception as ex:issues.append(f'localization: {ex}')
try:
    db=src['db.rs']
    batches=re.findall(r'tx\.execute_batch\(r#"(.*?)"#\)\?',db,re.S)
    assert len(batches)>=2
    conn=sqlite3.connect(':memory:')
    conn.executescript(batches[0]);assert conn.execute('PRAGMA user_version').fetchone()[0]==1
    conn.execute("INSERT INTO transactions(day,kind,amount_minor,currency,category,description,method,note,created_at) VALUES(?,?,?,?,?,?,?,?,?)",('2026-11-03','expense',2500,'MAD','Food & Drink','Coffee','Cash','','2026-11-03 12:05:06'))
    conn.execute("INSERT INTO budgets(currency,month,category,amount_minor) VALUES(?,?,?,?)",('MAD','2026-11','Groceries',10000))
    conn.executescript(batches[1]);conn.executescript("ALTER TABLE transactions ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';")
    prefill=re.search(r'tx\.execute_batch\("(INSERT INTO activity.*?)"\)\?',db,re.S)
    assert prefill
    conn.executescript(prefill.group(1))
    assert conn.execute('PRAGMA user_version').fetchone()[0]==2
    row=conn.execute('SELECT day,amount_minor,description,created_at,updated_at FROM transactions').fetchone()
    assert row==('2026-11-03',2500,'Coffee','2026-11-03 12:05:06','')
    assert conn.execute('SELECT amount_minor FROM budgets').fetchone()[0]==10000
    assert conn.execute("SELECT action,created_at FROM activity WHERE entity='transaction'").fetchone()==('created','2026-11-03 12:05:06')
    conn.execute("INSERT INTO transfers(day,amount_minor,currency,source,destination,note) VALUES(?,?,?,?,?,?)",('2026-11-04',1000,'MAD','Cash','Bank',''))
    before=conn.execute("SELECT SUM(amount_minor) FROM transactions WHERE kind='expense'").fetchone()[0]
    assert before==2500
    try:
        conn.execute("INSERT INTO transfers(day,amount_minor,currency,source,destination,note) VALUES(?,?,?,?,?,?)",('2026-11-04',1000,'MAD','Cash','Cash',''))
        raise AssertionError('same account transfer should fail')
    except sqlite3.IntegrityError:pass
    conn.close()
except Exception as ex:issues.append(f'sqlite migration: {ex}')
for file,name in [('ui.rs','api_key: RefCell<String>'),('ui.rs','Page::Help'),('ui.rs','masked_money'),('ui.rs','set_setting("privacy"'),('db.rs','created_at'),('db.rs','add_transfer'),('ai.rs','chat_with_ai'),('i18n.rs','include_str!("../locales/es.json")')]:
    if compact(name) not in compact(src.get(file,'')):issues.append(f'missing feature {file}:{name}')
for p in root.rglob('*'):
    if p.is_file() and p.suffix in ['.rs','.json','.md','.css']:
        txt=p.read_text()
        import re
        if re.search(r'/home/[A-Za-z0-9._-]+|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}', txt):
            issues.append(f'personal identifier found in {p.name}')
if issues:
    print('Offline QA FAILED:')
    for i in issues:print(' -',i)
    sys.exit(1)
print('Offline QA passed: manifest, 2 JSON catalogs, v1→v2 SQLite data/creation timestamps, transfer constraints, privacy and secret source checks. No Rust compiler or GTK runtime check performed.')
