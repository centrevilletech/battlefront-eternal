#!/usr/bin/env python3
"""Build a dedicated Linux server archive for bfeserver.

Produces battlefront-eternal-server_<version>_linux-<amd64|arm64>.tar.xz
containing only what a headless dedicated server needs:
  - bfeserver
  - tools/
  - toolchain/
  - legal/
  - bfe-tree.json
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import clean_bytecode, normalize_permissions, normalize_platform, walk_tree

SERVER_ARCH_MAP = {
    'linux-x64': 'amd64',
    'linux-arm64': 'arm64',
}


def build_server_tree_index(root: Path, platform: str) -> dict:
    normalize_permissions(root)
    files, links, dirs = walk_tree(root, is_windows=False, exclude_paths={'bfe-tree.json'})
    manifest = {
        'schema': 1,
        'platform': platform,
        'executable': 'bfeserver',
        'files': files,
        'links': links,
        'dirs': dirs,
    }
    tree_path = root / 'bfe-tree.json'
    tree_path.write_text(json.dumps(manifest) + '\n', encoding='utf-8')
    tree_path.chmod(0o644)
    return manifest


def verify_server_archive(archive_path: Path) -> None:
    with tarfile.open(archive_path, 'r:xz') as archive:
        members = {m.name.lstrip('./'): m for m in archive.getmembers() if m.name.lstrip('./')}
    required = (
        'bfeserver',
        'tools/prepare.py',
        'tools/content-version.json',
        'toolchain/python/bin/python3',
        'legal/LICENSE',
        'legal/THIRD-PARTY-NOTICES.md',
        'bfe-tree.json',
    )
    for req in required:
        if req not in members:
            raise RuntimeError(f'Server archive {archive_path} is missing required entry {req}')
    if not (members['bfeserver'].mode & 0o111):
        raise RuntimeError(f'Server archive {archive_path} has non-executable bfeserver')
    for forbidden_prefix in ('bfe-client', 'battlefront-eternal', 'assets/', 'resources/'):
        if any(name == forbidden_prefix or name.startswith(forbidden_prefix) for name in members):
            raise RuntimeError(f'Server archive {archive_path} contains desktop-only entry matching {forbidden_prefix}')


def build_server_archive(
    *,
    staged_dir: Path,
    platform: str,
    version: str,
    out_dir: Path,
) -> Path:
    platform = normalize_platform(platform)
    if platform not in SERVER_ARCH_MAP:
        raise ValueError(f'Dedicated server archive requires linux-x64 or linux-arm64, got {platform}')
    deb_arch = SERVER_ARCH_MAP[platform]

    server_bin = staged_dir / 'bfeserver'
    resources = staged_dir / 'resources'
    if not server_bin.is_file():
        raise FileNotFoundError(f'Missing bfeserver in staged directory {staged_dir}')

    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f'battlefront-eternal-server_{version}_linux-{deb_arch}.tar.xz'
    if out_path.exists():
        out_path.unlink()

    with tempfile.TemporaryDirectory(prefix='bfe-server-archive-') as tmp:
        stage = Path(tmp) / 'stage'
        stage.mkdir(parents=True, exist_ok=True)

        shutil.copy2(server_bin, stage / 'bfeserver')
        (stage / 'bfeserver').chmod(0o755)

        for section in ('tools', 'toolchain', 'legal'):
            src = resources / section
            if not src.is_dir():
                raise FileNotFoundError(f'Missing resources/{section} in {staged_dir}')
            shutil.copytree(src, stage / section, symlinks=True)

        clean_bytecode(stage)
        build_server_tree_index(stage, platform)

        entries = ['bfeserver', 'tools', 'toolchain', 'legal', 'bfe-tree.json']
        with out_path.open('wb') as out_file:
            tar_proc = subprocess.Popen(
                ['tar', '-cf', '-', *entries],
                cwd=stage,
                stdout=subprocess.PIPE,
            )
            assert tar_proc.stdout is not None
            xz_proc = subprocess.Popen(
                ['xz', '-T0'],
                stdin=tar_proc.stdout,
                stdout=out_file,
            )
            tar_proc.stdout.close()
            xz_rc = xz_proc.wait()
            tar_rc = tar_proc.wait()
            if tar_rc != 0 or xz_rc != 0:
                raise RuntimeError(f'tar/xz failed building {out_path} (tar={tar_rc}, xz={xz_rc})')

    verify_server_archive(out_path)
    return out_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-dir', required=True, type=Path, help='Staged Linux unpacked directory')
    parser.add_argument('--platform', required=True, help='Target platform (linux-x64 or linux-arm64)')
    parser.add_argument('--version', required=True, help='Release version (for example 0.1.0-preview.1)')
    parser.add_argument('--out-dir', required=True, type=Path, help='Output directory for server archive')
    args = parser.parse_args()

    archive = build_server_archive(
        staged_dir=args.staged_dir,
        platform=args.platform,
        version=args.version,
        out_dir=args.out_dir,
    )
    print(f'Built dedicated server archive: {archive} ({archive.stat().st_size} bytes)')


if __name__ == '__main__':
    main()
