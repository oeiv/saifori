#!/usr/bin/env python3
"""Validate release metadata, permissions and generated crates against Cargo.lock.

This does not certify Flathub acceptance or replace flatpak-builder-lint.
"""
import hashlib
import json
from pathlib import Path
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
APP = 'io.github.oeiv.Saifori'
manifest = json.loads((ROOT / f'{APP}.json').read_text())
assert manifest['id'] == APP
assert set(manifest['finish-args']) == {
    '--share=ipc', '--socket=wayland', '--socket=fallback-x11',
    '--device=dri', '--share=network', '--talk-name=org.freedesktop.secrets',
}
# Broad filesystem and session/system bus permissions are intentionally absent.
sources = json.loads((ROOT / 'packaging/cargo-sources.json').read_text())
archives = {item['url'].rsplit('/', 1)[-1]: item for item in sources if item['type'] == 'archive'}
lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
packages = [p for p in lock['package'] if p.get('source') == 'registry+https://github.com/rust-lang/crates.io-index']
assert len(archives) == len(packages)
for package in packages:
    source = archives[f"{package['name']}-{package['version']}.crate"]
    assert source['sha256'] == package['checksum']
    assert source['url'] == f"https://static.crates.io/crates/{package['name']}/{package['name']}-{package['version']}.crate"
    assert source['dest'] == f"cargo/vendor/{package['name']}-{package['version']}"
assert 'packaging/cargo-sources.json' in manifest['modules'][0]['sources']
assert not any(isinstance(s, dict) and s.get('path') in ('vendor', '.cargo') for s in manifest['modules'][0]['sources'])
meta = ET.parse(ROOT / f'assets/{APP}.metainfo.xml').getroot()
assert meta.findtext('id') == APP
assert meta.find('developer/name').text
assert meta.find('content_rating').get('type') == 'oars-1.1'
assert meta.find('url[@type="homepage"]') is not None
assert meta.find('releases/release').get('version') == tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
for image in meta.findall('screenshots/screenshot/image'):
    assert image.text.startswith('https://raw.githubusercontent.com/oeiv/saifori/v0.1.0/')
    path = ROOT / 'assets/screenshots' / image.text.rsplit('/', 1)[-1]
    assert path.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
print(f'PASS: {len(packages)} checksum-pinned crates, metadata, actual screenshots and restricted permissions')
print('Public release/screenshot URLs and sandboxed builds require separate validation.')
