#!/usr/bin/env python3
"""Pack a staged native app tree into a content-addressed update-<platform>.tar.

Produces the archive layout and manifest format consumed by
build/publish-distribution.py and the desktop/server updaters:
  - update-<platform>.json (schema 1, version, platform, executable,
    server_executable, content, files with zsize, links, dirs)
  - blobs/<sha256>.zst for every blob not already listed in --known
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import MACOS_APP_NAME, normalize_platform, sha256_file, walk_tree


def compress_blob(src: Path, dst: Path) -> int:
    dst.parent.mkdir(parents=True, exist_ok=True)
    level = '-12' if src.stat().st_size > (32 << 20) else '-19'
    subprocess.run(
        ['zstd', level, '--check', '-q', '-f', str(src), '-o', str(dst)],
        check=True,
    )
    return dst.stat().st_size


def load_known_blobs(known_blobs_path: Path | None) -> dict[str, int]:
    if not known_blobs_path or not known_blobs_path.is_file():
        return {}
    try:
        doc = json.loads(known_blobs_path.read_text(encoding='utf-8'))
        blobs = doc.get('blobs')
        if isinstance(blobs, dict):
            return {k: v for k, v in blobs.items() if isinstance(v, int) and v > 0}
    except Exception:
        pass
    return {}


def bundle_tree(folder: Path, platform: str) -> dict:
    """The update tree of a macOS build: the folder that holds the signed app bundle, every file of
    the bundle as it is on disk. The bundle's own index cannot name the client or the signature's
    files (the signature covers the index), and an installed copy must have all of them, byte for
    byte, to keep its signature and its notarization ticket."""
    app = folder / MACOS_APP_NAME
    if not (app / 'Contents' / 'Info.plist').is_file():
        raise FileNotFoundError(f'Missing {MACOS_APP_NAME} in {folder}')
    if not (app / 'Contents' / '_CodeSignature' / 'CodeResources').is_file():
        raise RuntimeError(f'{app} is not signed: an update made from it could not start on Apple silicon')
    # The walk below leaves Python's bytecode files out, as it must for a tree that a build machine
    # may have run. A sealed bundle holds none; one that does was written into after it was signed,
    # and a payload without those files would not be the bundle that was notarized.
    bytecode = [str(path.relative_to(folder)) for path in app.rglob('*') if path.name == '__pycache__' or path.suffix in ('.pyc', '.pyo')]
    if bytecode:
        raise RuntimeError(f'{app} holds Python bytecode written after it was signed: {bytecode[:3]}')
    files, links, dirs = walk_tree(folder, False)
    outside = [entry['path'] for entry in files + links if not entry['path'].startswith(MACOS_APP_NAME + '/')]
    if outside:
        raise RuntimeError(f'{folder} holds more than the app bundle: {outside[:5]}')
    tree = {'schema': 1, 'platform': platform, 'executable': f'{MACOS_APP_NAME}/Contents/MacOS/bfe-client'}
    if (app / 'Contents' / 'MacOS' / 'bfeserver').is_file():
        tree['server_executable'] = f'{MACOS_APP_NAME}/Contents/MacOS/bfeserver'
    tree.update(files=files, links=links, dirs=dirs)
    return tree


def build_update_payload(
    *,
    staged_dir: Path,
    platform: str,
    version: str,
    out_path: Path,
    known_blobs_path: Path | None = None,
) -> dict:
    platform = normalize_platform(platform)
    if platform.startswith('darwin-'):
        tree = bundle_tree(staged_dir, platform)
        content_path = staged_dir / MACOS_APP_NAME / 'Contents' / 'Resources' / 'tools' / 'content-version.json'
    else:
        tree_path = staged_dir / 'resources' / 'bfe-tree.json'
        if not tree_path.is_file():
            raise FileNotFoundError(f'Missing resources/bfe-tree.json in {staged_dir}')
        tree = json.loads(tree_path.read_text(encoding='utf-8'))
        if tree.get('schema') != 1 or tree.get('platform') != platform:
            raise ValueError(f'Invalid bfe-tree.json header for {platform}: {tree.get("platform")}')
        content_path = staged_dir / 'resources' / 'tools' / 'content-version.json'
    content_raw = json.loads(content_path.read_text(encoding='utf-8'))
    # Every generation (state, maps, sides, common): an installed client compares
    # all of them with its own, and one left out reads as changed, so the player
    # is told the game data will be refreshed when it will not.
    content = {}
    for key, value in content_raw.items():
        value = len(value) if isinstance(value, list) else value
        if isinstance(value, int) and not isinstance(value, bool):
            content[key] = value
    for key in ('state', 'maps', 'sides'):
        if key not in content:
            raise ValueError(f'{content_path} has no {key} generation')

    known = load_known_blobs(known_blobs_path)

    by_hash: dict[str, Path] = {}
    for item in tree['files']:
        full = staged_dir / item['path']
        actual = sha256_file(full)
        if actual != item['sha256']:
            raise RuntimeError(f"Staged file {item['path']} changed after bfe-tree.json was written")
        by_hash.setdefault(item['sha256'], full)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    if out_path.exists():
        out_path.unlink()

    with tempfile.TemporaryDirectory(prefix=f'bfe-update-{platform}-') as tmp:
        work = Path(tmp)
        blobs_dir = work / 'blobs'
        blobs_dir.mkdir(parents=True, exist_ok=True)

        zsizes: dict[str, int] = {}
        fresh_files: dict[str, Path] = {}
        to_compress: list[tuple[str, Path]] = []
        for digest, src in by_hash.items():
            if digest in known:
                zsizes[digest] = known[digest]
            else:
                to_compress.append((digest, src))

        workers = min(8, max(1, os.cpu_count() or 2))

        def process_one(pair: tuple[str, Path]) -> tuple[str, int, Path]:
            digest, src = pair
            dst = blobs_dir / f'{digest}.zst'
            size = compress_blob(src, dst)
            return digest, size, dst

        with ThreadPoolExecutor(max_workers=workers) as pool:
            for digest, zsize, dst in pool.map(process_one, to_compress):
                zsizes[digest] = zsize
                fresh_files[digest] = dst

        files_out = []
        for item in tree['files']:
            entry = dict(item)
            entry['zsize'] = zsizes[item['sha256']]
            files_out.append(entry)

        manifest = {
            'schema': 1,
            'platform': platform,
            'version': version,
            'executable': tree['executable'],
            'content': content,
            'files': files_out,
            'links': tree.get('links', []),
            'dirs': tree.get('dirs', []),
        }
        if 'server_executable' in tree:
            manifest['server_executable'] = tree['server_executable']

        manifest_name = f'update-{platform}.json'
        manifest_file = work / manifest_name
        manifest_file.write_text(json.dumps(manifest) + '\n', encoding='utf-8')

        # Write flat file entries only (no directory entries) as expected by
        # build/publish-distribution.py ingest_updates.
        with tarfile.open(out_path, 'w', format=tarfile.USTAR_FORMAT) as tar:
            tar.add(manifest_file, arcname=manifest_name, recursive=False)
            for digest in sorted(fresh_files):
                tar.add(fresh_files[digest], arcname=f'blobs/{digest}.zst', recursive=False)

    return {
        'platform': platform,
        'unique_blobs': len(by_hash),
        'shipped_blobs': len(fresh_files),
        'reused_blobs': len(by_hash) - len(fresh_files),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description='Build native update-<platform>.tar payload.')
    parser.add_argument('--tree', '--staged-dir', dest='staged_dir', required=True, type=Path,
                        help='Staged app directory (macOS: the folder that holds the signed app bundle)')
    parser.add_argument('--platform', required=True, help='Target platform (linux-x64, linux-arm64, win32-x64, darwin-arm64, darwin-x64)')
    parser.add_argument('--version', required=True, help='Release version string')
    parser.add_argument('--out', required=True, type=Path, help='Output update-<platform>.tar path')
    parser.add_argument('--known', type=Path, default=None, help='Optional blobs.json path of already-published blobs')
    args = parser.parse_args(argv)

    summary = build_update_payload(
        staged_dir=args.staged_dir.resolve(),
        platform=args.platform,
        version=args.version,
        out_path=args.out.resolve(),
        known_blobs_path=args.known.resolve() if args.known else None,
    )
    print(
        f"update-payload: wrote {args.out} "
        f"({summary['shipped_blobs']} new / {summary['reused_blobs']} reused blobs)"
    )
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
