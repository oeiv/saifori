#!/usr/bin/env python3
"""One-time, non-destructive SQLite ledger migration to the new Saifori app ID.

Run on the Fedora HOST, with both versions closed, before first launch of the
new Flatpak. Uses SQLite online backup to capture WAL data correctly. It never
modifies or deletes a source and never overwrites an existing target.
"""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import sqlite3
from contextlib import closing
import sys
import tempfile

OLD_APP = 'io.github.sarf.ExpenseTracker'
NEW_APP = 'io.github.oeiv.Saifori'


def defaults() -> list[tuple[str, Path, Path]]:
    home=Path.home()
    xdg=Path(os.environ.get('XDG_DATA_HOME',str(home/'.local/share')))
    return [
      ('Native', xdg/'sarf'/'ledger.sqlite3', xdg/'saifori'/'ledger.sqlite3'),
      ('Flatpak', home/'.var/app'/OLD_APP/'data/sarf/ledger.sqlite3',
          home/'.var/app'/NEW_APP/'data/saifori/ledger.sqlite3'),
    ]


def migrate(source: Path, target: Path) -> str:
    if not source.is_file():
        return f'SKIP: source not found: {source}'
    if source.is_symlink():
        raise RuntimeError(f'Refusing to read a symlink: {source}')
    if os.path.lexists(target):
        return f'SKIP: destination already exists (never overwrite): {target}'
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    target.parent.chmod(0o700)
    fd, temp_name = tempfile.mkstemp(prefix='.saifori-migration-', suffix='.sqlite3', dir=target.parent)
    os.close(fd)
    temp=Path(temp_name)
    try:
        # File URI with explicit read-only mode avoids creating/mutating source.
        source_uri=source.resolve().as_uri()+'?mode=ro'
        with closing(sqlite3.connect(source_uri, uri=True, timeout=8)) as old:
            check=old.execute('PRAGMA quick_check').fetchone()
            if not check or check[0] != 'ok':
                raise RuntimeError(f'Original database failed SQLite validation: {source}')
            with closing(sqlite3.connect(temp)) as new:
                old.backup(new)
                new_check=new.execute('PRAGMA quick_check').fetchone()
                if not new_check or new_check[0] != 'ok':
                    raise RuntimeError('Backup failed SQLite validation')
        os.chmod(temp,0o600)
        # Atomic no-clobber create: the original and WAL files stay unchanged.
        os.link(temp,target)
        return f'MIGRATED: {source}  →  {target}'
    finally:
        temp.unlink(missing_ok=True)


def main() -> int:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,help='Explicit legacy SQLite path (requires --destination)')
    parser.add_argument('--destination',type=Path,help='Explicit new SQLite path')
    args=parser.parse_args()
    if bool(args.source)!=bool(args.destination):
        parser.error('--source and --destination must be supplied together')
    tasks=[('Custom',args.source,args.destination)] if args.source else defaults()
    try:
        for kind,source,target in tasks:
            print(f'{kind}: {migrate(source,target)}')
    except (OSError,sqlite3.Error,RuntimeError) as err:
        print(f'Migration failed (old data unchanged): {err}',file=sys.stderr)
        return 1
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
