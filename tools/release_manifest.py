"""Validate release inputs and create the Tauri static update manifest."""
import base64
import json
import re
import sys
import tomllib
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
CONF = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
VERSION = CONF['version']
REPO = 'ChickenStrxps/dumbmanager-rust'


def decode_record(encoded, expected_length):
    text = base64.b64decode(encoded, validate=True).decode('utf-8')
    lines = text.strip().splitlines()
    if len(lines) < 2 or not lines[0].startswith('untrusted comment:'):
        raise ValueError('invalid minisign record')
    raw = base64.b64decode(lines[1], validate=True)
    if len(raw) != expected_length or raw[:2] not in (b'Ed', b'ED'):
        raise ValueError('invalid minisign record')
    return raw, lines


def check_config():
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', VERSION):
        raise ValueError('release version must be a stable semver')
    cargo = tomllib.loads((ROOT / 'src-tauri/Cargo.toml').read_text(encoding='utf-8'))
    package = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))
    if cargo['package']['version'] != VERSION or package['version'] != VERSION:
        raise ValueError('Tauri, Cargo and package.json versions must match')
    updater = CONF['plugins']['updater']
    if updater['endpoints'] != [f'https://github.com/{REPO}/releases/latest/download/latest.json'] or updater.get('requireSignedVersion') is not True:
        raise ValueError('updater endpoint or signed-version policy changed')
    return decode_record(updater['pubkey'], 42)[0]


def main():
    key = check_config()
    if len(sys.argv) == 2 and sys.argv[1] == 'check':
        print(VERSION)
        return
    if len(sys.argv) != 3 or sys.argv[1] != 'manifest':
        raise ValueError('usage: release_manifest.py check | manifest <installer>')
    installer = Path(sys.argv[2])
    if not installer.is_file() or not installer.name.endswith('-setup.exe'):
        raise ValueError('expected the NSIS setup executable')
    signature = Path(str(installer) + '.sig').read_text(encoding='utf-8').strip()
    raw, lines = decode_record(signature, 74)
    if raw[2:10] != key[2:10]:
        raise ValueError('signature key ID does not match the configured public key')
    if len(lines) < 4 or not lines[2].startswith('trusted comment: ') or f'version:{VERSION}' not in lines[2].removeprefix('trusted comment: ').split('\t'):
        raise ValueError('signature is not bound to the release version')
    url = f'https://github.com/{REPO}/releases/download/v{VERSION}/{quote(installer.name)}'
    manifest = {'version': VERSION, 'notes': f'DumbManager Rust {VERSION}', 'platforms': {'windows-x86_64': {'signature': signature, 'url': url}}}
    (installer.parent / 'latest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, UnicodeError) as exc:
        sys.exit(f'Release validation failed: {exc}')

