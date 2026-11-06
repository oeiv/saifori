#!/usr/bin/env python3
"""Source-release privacy, locale and repository hygiene checks.

Does not compile Rust or exercise GUI behavior.
"""
from pathlib import Path
import json
import re
import sys

root=Path(__file__).resolve().parents[1]
issues=[]
text_ext={'.rs','.md','.json','.toml','.py','.sh','.yml','.yaml','.xml','.desktop','.css','.svg','.gitignore','.gitattributes','.editorconfig'}
for p in root.rglob('*'):
    if not p.is_file():
        continue
    if p.suffix not in text_ext and p.name not in {'.gitignore','.gitattributes','.editorconfig'}:
        continue
    try:
        txt=p.read_text(encoding='utf-8')
    except UnicodeDecodeError:
        issues.append(f'unreadable text: {p.relative_to(root)}')
        continue
    if re.search(r'(?i)-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|(?:sk-|ghp_|github_pat_)[A-Za-z0-9_-]{25,}',txt):
        issues.append(f'possible credential: {p.relative_to(root)}')
    if re.search(r'/home/[A-Za-z0-9._-]+|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}',txt) and p.suffix in {'.rs','.md','.json'}:
        issues.append(f'personal-looking path/email: {p.relative_to(root)}')

for missing in ['.gitignore','.gitattributes','.editorconfig','LICENSE','README.md','CONTRIBUTING.md','SECURITY.md','PRIVACY.md','locales/en.json','locales/es.json']:
    if not (root/missing).is_file(): issues.append(f'missing repository file: {missing}')
for lang in ['en','es']:
    try: assert isinstance(json.loads((root/'locales'/f'{lang}.json').read_text()),dict)
    except Exception as e: issues.append(f'catalog {lang}: {e}')
for forbidden in ['.cargo/config.toml','.env']:
    if (root/forbidden).exists(): issues.append(f'unpublishable local file included: {forbidden}')
i18n=(root/'src/i18n.rs').read_text()
if '"ar", "' in i18n or '../locales/ar.json' in i18n:issues.append('unsupported locale still configured')
if 'io.github.oeiv.Saifori' not in (root/'io.github.oeiv.Saifori.json').read_text():issues.append('unexpected Flatpak app ID')
ignore=(root/'.gitignore').read_text()
for pattern in ['/vendor/','/.cargo/config.toml','*.sqlite3','*.env']:
    if pattern not in ignore and not (pattern=='*.env' and '.env' in ignore):issues.append(f'missing gitignore protection: {pattern}')
if issues:
    print('Source hygiene check FAILED:')
    for issue in issues:print(' - '+issue)
    sys.exit(1)
print('PASS: Supported catalogs; repository files and secret guards present.')
if not (root/'Cargo.lock').is_file():
    print('NOTICE: Cargo.lock is missing. Generate/copy the verified lockfile on Fedora and commit it before publication.')
print('This is a source hygiene audit, not a Rust compiler test or secret-scanning guarantee.')
