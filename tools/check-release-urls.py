#!/usr/bin/env python3
"""Fail closed unless public screenshot URLs serve the exact reviewed images."""
import hashlib
from pathlib import Path
import urllib.request
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
meta = ET.parse(root / 'assets/io.github.oeiv.Saifori.metainfo.xml')
for image in meta.findall('screenshots/screenshot/image'):
    local = root / 'assets/screenshots' / image.text.rsplit('/', 1)[-1]
    try:
        with urllib.request.urlopen(image.text, timeout=30) as response:
            remote = response.read(local.stat().st_size + 1)
        if hashlib.sha256(remote).digest() != hashlib.sha256(local.read_bytes()).digest():
            raise ValueError('public image differs from the reviewed screenshot')
    except Exception as error:
        raise SystemExit(f'BLOCKED: {image.text}: {error}') from error
print('PASS: immutable public screenshot URLs match the reviewed files')
