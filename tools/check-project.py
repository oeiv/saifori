#!/usr/bin/env python3
"""Source, identity, timeline and SQLite migration acceptance checks.

No Rust compiler or GTK runtime is invoked here: those require a compatible GNOME development environment.
"""
from pathlib import Path
import json, sqlite3, subprocess, sys, tempfile, tomllib, xml.etree.ElementTree as ET

def compact(value): return "".join(value.split())

root=Path(__file__).resolve().parents[1]
app='io.github.oeiv.Saifori'
manifest_path=root/f'{app}.json'
manifest=json.loads(manifest_path.read_text())
assert manifest['id']==app
assert manifest['command']=='saifori'
assert manifest['modules'][0]['name']=='saifori'
assert any('--talk-name=org.freedesktop.secrets' == a for a in manifest['finish-args'])
assert not any('--filesystem=home' in a for a in manifest['finish-args'])
assert not (root/'io.github.sarf.ExpenseTracker.json').exists()
assert not list((root/'assets').rglob('*sarf*'))
assert not list(root.glob('*ExpenseTracker*'))
assert not list(root.glob('*sarf*'))
assert 'name = "saifori"' in (root/'Cargo.toml').read_text()
cargo=tomllib.loads((root/'Cargo.toml').read_text())
assert cargo['package']['name']=='saifori'
assert cargo['bin'][0]['name']=='saifori'
assert 'Icon='+app in (root/'assets'/f'{app}.desktop').read_text()
assert 'Exec=saifori' in (root/'assets'/f'{app}.desktop').read_text()
ET.parse(root/'assets'/f'{app}.metainfo.xml')
assert f'<id>{app}</id>' in (root/'assets'/f'{app}.metainfo.xml').read_text()
assert f'<launchable type="desktop-id">{app}.desktop</launchable>' in (root/'assets'/f'{app}.metainfo.xml').read_text()
for command in manifest['modules'][0]['build-commands']:
    if 'install -D' in command and command.split()[2].startswith('assets/'):
        assert (root/command.split()[2]).exists(), command
for p in (root/'assets').rglob('*.svg'): ET.parse(p)
for lang in ('en','es'): json.loads((root/'locales'/f'{lang}.json').read_text())

ui="\n".join(x.read_text() for x in [root / "src/ui.rs", *sorted((root / "src/ui").glob("*.rs"))])
assert compact('enum TimelineLevel { Month, Day, }') in compact(ui)
assert ui.count('enum TimelineLevel')==1
assert compact('fn timeline_set_open(content: &gtk::Box, open: bool)') in compact(ui)
assert compact('content.set_visible(open);') in compact(ui)
assert compact('let expand=!refs.borrow().all_open();') in compact(ui)
assert compact(ui).count(compact('controls.append(&toggle)'))==1
assert compact('gtk::Revealer::new()') not in compact(ui)
assert compact('join("saifori")') in compact(ui)
assert compact('glib::markup_escape_text(&i18n::tr(title))') in compact(ui)
assert compact('assets/io.github.oeiv.Saifori.svg') in compact(ui)
assert compact('sarf-') not in compact(ui) and compact('ExpenseTracker') not in compact(ui)
assert '.sarf-' not in (root/'assets/style.css').read_text()
assert 'sarf-' not in (root/'assets/icons/hicolor/index.theme').read_text()
assert 'io.github.oeiv.Saifori.Credentials' in (root/'src/secret_store.rs').read_text()
assert 'LEGACY_SCHEMA_NAME' in (root/'src/secret_store.rs').read_text()
assert (root/'src/secret_store.rs').read_text().count('fn lookup(')==1

# SQLite backup handles old databases containing WAL-mode transactions.
with tempfile.TemporaryDirectory() as tmp:
    src=Path(tmp)/'previous/ledger.sqlite3'; dst=Path(tmp)/'new/ledger.sqlite3'
    src.parent.mkdir()
    connection=sqlite3.connect(src)
    connection.execute('PRAGMA journal_mode=WAL')
    connection.execute('CREATE TABLE finance(amount INTEGER NOT NULL, note TEXT NOT NULL)')
    connection.execute("INSERT INTO finance VALUES (1200,'legacy transaction')")
    connection.commit()
    args=[sys.executable,str(root/'tools/migrate-data.py'),'--source',str(src),'--destination',str(dst)]
    p=subprocess.run(args,text=True,capture_output=True,check=True)
    assert 'MIGRATED:' in p.stdout,p.stdout
    assert dst.is_file() and src.is_file()
    con=sqlite3.connect(dst)
    assert con.execute('SELECT amount,note FROM finance').fetchone()==(1200,'legacy transaction')
    con.close()
    # No-clobber check: the destination remains byte-for-byte identical.
    before=dst.read_bytes()
    p=subprocess.run(args,text=True,capture_output=True,check=True)
    assert 'SKIP:' in p.stdout
    assert dst.read_bytes()==before
    connection.close()

print('PASS: app ID, package, metadata, icons, CSS, timeline enum, markup and safe SQLite migration')
print('Rust compilation and GNOME/Flatpak GUI interaction are NOT verified by this test.')
