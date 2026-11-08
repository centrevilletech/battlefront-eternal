#!/usr/bin/env python3
"""Build the Windows NSIS per-user installer from a staged win-unpacked tree.

Output filename matches build/publish-distribution.py:
  Battlefront-Eternal-<version>-win-x64.exe
"""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import REPO_ROOT, write_windows_ico


def normalize_win_path(raw: str | Path) -> Path:
    text = str(raw)
    if os.name == 'nt':
        m = re.match(r'^/([a-zA-Z])/(.*)$', text)
        if m:
            return Path(f'{m.group(1).upper()}:/{m.group(2)}')
    return Path(text)


def find_makensis(explicit: Path | None = None) -> str:
    if explicit is not None and str(explicit).strip():
        candidate = normalize_win_path(explicit)
        if candidate.is_file():
            return str(candidate)
        raise FileNotFoundError(f'Explicit --makensis binary not found: {candidate}')
    env_makensis = os.environ.get('MAKENSIS')
    if env_makensis:
        candidate = normalize_win_path(env_makensis)
        if candidate.is_file():
            return str(candidate)
    found = shutil.which('makensis') or shutil.which('makensis.exe')
    if found:
        return found
    for candidate in (
        Path('C:/Program Files (x86)/NSIS/makensis.exe'),
        Path('C:/Program Files/NSIS/makensis.exe'),
        Path('C:/ProgramData/chocolatey/lib/nsis.install/tools/makensis.exe'),
        Path('C:/ProgramData/chocolatey/bin/makensis.exe'),
    ):
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError('makensis not found on PATH or standard NSIS install locations')


def build_windows_installer(
    staged_dir: Path,
    version: str,
    out_dir: Path,
    makensis: Path | None = None,
    repo_root: Path = REPO_ROOT,
) -> Path:
    if not (staged_dir / 'bfe-client.exe').is_file():
        raise FileNotFoundError(f'Missing bfe-client.exe in staged tree {staged_dir}')
    if not (staged_dir / 'resources' / 'bfe-tree.json').is_file():
        raise FileNotFoundError(f'Missing resources/bfe-tree.json in staged tree {staged_dir}')

    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f'Battlefront-Eternal-{version}-win-x64.exe'
    if out_path.exists():
        out_path.unlink()

    nsi_script = repo_root / 'build' / 'native' / 'installer.nsi'
    icon_png = repo_root / 'build' / 'native' / 'assets' / 'icon-256.png'
    makensis_bin = find_makensis(makensis)

    with tempfile.TemporaryDirectory(prefix='bfe-nsis-') as tmp:
        ico_path = Path(tmp) / 'battlefront-eternal.ico'
        write_windows_ico(icon_png, ico_path)
        def_prefix = '/' if os.name == 'nt' else '-D'
        def define(key: str, value: str) -> str:
            return f'/D{key}={value}' if def_prefix == '/' else f'-D{key}={value}'

        cmd = [
            makensis_bin,
            define('PRODUCT_NAME', 'Battlefront Eternal'),
            define('PRODUCT_VERSION', version),\
            define('SOURCE_DIR', str(staged_dir.resolve())),
            define('OUT_FILE', str(out_path.resolve())),
            define('ICON_FILE', str(ico_path.resolve())),
            str(nsi_script.resolve()),
        ]
        subprocess.run(cmd, check=True)

    if not out_path.is_file():
        raise RuntimeError(f'makensis completed without producing {out_path}')
    return out_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-dir', required=True, type=Path, help='Path to staged win-unpacked directory')
    parser.add_argument('--version', required=True, help='Release version (for example 0.1.0-preview.1)')
    parser.add_argument('--out-dir', required=True, type=Path, help='Output directory for Windows installer')
    parser.add_argument('--makensis', type=Path, default=None, help='Optional path to makensis binary')
    args = parser.parse_args()

    installer = build_windows_installer(args.staged_dir, args.version, args.out_dir, args.makensis)
    print(f'Built Windows NSIS installer: {installer} ({installer.stat().st_size} bytes)')


if __name__ == '__main__':
    main()
