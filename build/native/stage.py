#!/usr/bin/env python3
"""Stage an unpacked native Battlefront Eternal desktop + server tree and index it.

Produces the unpacked directory layout consumed by the native installer builders
(build/native/package-linux.py, build/native/package-windows.py) and by the
content-addressed update payload packer (build/native/update-payload.py):

  <out-dir>/
    bfe-client[.exe]
    bfeserver[.exe]              (when --server-bin is provided)
    battlefront-eternal          (Linux symlink -> bfe-client)
    resources/
      bfe-tree.json
      bfe-release.json           (when build/bfe-release.json is present)
      tools/
        *.py
        content-version.json
      toolchain/
        python/...
        ffmpeg/...
      legal/
        LICENSE
        THIRD-PARTY-NOTICES.md
        licenses/...
      assets/
        icon.png
        icon-512.png
        icon-256.png
        icon.svg
        steam/...
        icons/hicolor/...
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import struct

REPO_ROOT = Path(__file__).resolve().parents[2]

PLATFORM_ALIASES = {
    'linux-x64': 'linux-x64',
    'linux-amd64': 'linux-x64',
    'linux-arm64': 'linux-arm64',
    'win32-x64': 'win32-x64',
    'windows-x64': 'win32-x64',
    'win-x64': 'win32-x64',
}

TOOLCHAIN_DIR_NAMES = {
    'linux-x64': 'linux-x64',
    'linux-arm64': 'linux-arm64',
    'win32-x64': 'win-x64',
}


def normalize_platform(platform: str) -> str:
    if platform not in PLATFORM_ALIASES:
        raise ValueError(f'Unsupported platform {platform!r}; expected one of {sorted(PLATFORM_ALIASES)}')
    return PLATFORM_ALIASES[platform]


def default_toolchain_dir(platform: str, repo_root: Path = REPO_ROOT) -> Path:
    platform = normalize_platform(platform)
    tc_key = TOOLCHAIN_DIR_NAMES[platform]
    primary = repo_root / 'toolchain' / tc_key
    if primary.is_dir():
        return primary
    alt = repo_root / 'build' / 'toolchain' / platform
    if alt.is_dir():
        return alt
    return primary


def clean_bytecode(root: Path) -> None:
    """Remove Python bytecode caches so staged trees and bfe-tree.json are deterministic."""
    for dirpath, dirnames, filenames in os.walk(root, topdown=False):
        base = Path(dirpath)
        for name in filenames:
            if name.endswith('.pyc') or name.endswith('.pyo'):
                (base / name).unlink()
        for name in dirnames:
            if name == '__pycache__':
                shutil.rmtree(base / name)


def normalize_permissions(root: Path) -> None:
    """Ensure world-readable/executable modes regardless of the caller's umask."""
    if os.name == 'nt':
        return
    root.chmod(0o755)
    for dirpath, dirnames, filenames in os.walk(root):
        base = Path(dirpath)
        for dname in dirnames:
            full_d = base / dname
            if not full_d.is_symlink():
                full_d.chmod(0o755)
        for fname in filenames:
            full_f = base / fname
            if not full_f.is_symlink():
                mode = full_f.stat().st_mode
                full_f.chmod(0o755 if (mode & 0o111) else 0o644)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(65536), b''):
            digest.update(chunk)
    return digest.hexdigest()


def walk_tree(root: Path, is_windows: bool, exclude_paths: set[str] | None = None) -> tuple[list[dict], list[dict], list[str]]:
    """Walk `root` and return `(files, links, dirs)` for `bfe-tree.json`."""
    excluded = exclude_paths or set()
    files: list[dict] = []
    links: list[dict] = []
    dirs: list[str] = []

    def visit(directory: Path, prefix: str = '') -> None:
        kept = 0
        for name in sorted(os.listdir(directory)):
            if name == '__pycache__' or name.endswith('.pyc') or name.endswith('.pyo'):
                continue
            rel = f'{prefix}/{name}' if prefix else name
            if rel in excluded:
                continue
            full = directory / name
            st = os.lstat(full)
            kept += 1
            if stat.S_ISLNK(st.st_mode):
                target = os.readlink(full).replace('\\', '/')
                links.append({'path': rel, 'target': target})
            elif stat.S_ISDIR(st.st_mode):
                visit(full, rel)
            elif stat.S_ISREG(st.st_mode):
                if is_windows:
                    is_exec = rel.lower().endswith('.exe')
                else:
                    is_exec = bool(st.st_mode & 0o111)
                files.append({
                    'path': rel,
                    'size': st.st_size,
                    'sha256': sha256_file(full),
                    'mode': 0o755 if is_exec else 0o644,
                })
        if kept == 0 and prefix:
            dirs.append(prefix)

    visit(root)
    return files, links, dirs


def build_tree_index(root: Path, platform: str) -> dict:
    """Build `resources/bfe-tree.json` indexing the staged client and server executables."""
    platform = normalize_platform(platform)
    is_windows = platform.startswith('win32-')
    executable = 'bfe-client.exe' if is_windows else 'bfe-client'
    server_executable = 'bfeserver.exe' if is_windows else 'bfeserver'
    normalize_permissions(root)
    files, links, dirs = walk_tree(root, is_windows, exclude_paths={'resources/bfe-tree.json'})

    by_path = {entry['path']: entry for entry in files}
    client_entry = by_path.get(executable)
    if not client_entry or client_entry.get('mode') != 0o755 or not client_entry.get('sha256'):
        raise RuntimeError(f'Staged tree {root} is missing executable {executable} with mode 0o755')

    has_server = server_executable in by_path
    if has_server:
        server_entry = by_path[server_executable]
        if server_entry.get('mode') != 0o755 or not server_entry.get('sha256'):
            raise RuntimeError(f'Staged tree {root} has {server_executable} without mode 0o755')

    if 'resources/tools/content-version.json' not in by_path:
        raise RuntimeError(f'Staged tree {root} is missing resources/tools/content-version.json')

    manifest = {
        'schema': 1,
        'platform': platform,
        'executable': executable,
    }
    if has_server:
        manifest['server_executable'] = server_executable
    manifest['files'] = files
    manifest['links'] = links
    manifest['dirs'] = dirs

    tree_path = root / 'resources' / 'bfe-tree.json'
    tree_path.parent.mkdir(parents=True, exist_ok=True)
    tree_path.write_text(json.dumps(manifest) + '\n', encoding='utf-8')
    if os.name != 'nt':
        tree_path.chmod(0o644)
    return manifest


def write_windows_ico(png_path: Path, ico_path: Path) -> None:
    """Wrap a 256x256 PNG icon into a valid Windows .ico container."""
    png_bytes = png_path.read_bytes()
    header = struct.pack('<HHH', 0, 1, 1)
    entry = struct.pack('<BBBBHHII', 0, 0, 0, 0, 1, 32, len(png_bytes), 22)
    ico_path.parent.mkdir(parents=True, exist_ok=True)
    ico_path.write_bytes(header + entry + png_bytes)


def populate_hicolor_icons(hicolor_dst: Path, repo_root: Path = REPO_ROOT) -> None:
    """Populate a standard freedesktop hicolor icon theme tree from build/native/assets."""
    native_assets = repo_root / 'build' / 'native' / 'assets'
    icon_256 = native_assets / 'icon-256.png'
    icon_512 = native_assets / 'icon-512.png'
    icon_svg = native_assets / 'icon.svg'

    for size_dir, src in (
        ('256x256', icon_256),
        ('512x512', icon_512),
    ):
        apps = hicolor_dst / size_dir / 'apps'
        apps.mkdir(parents=True, exist_ok=True)
        dst = apps / 'battlefront-eternal.png'
        shutil.copy2(src, dst)
        dst.chmod(0o644)

    scalable_apps = hicolor_dst / 'scalable' / 'apps'
    scalable_apps.mkdir(parents=True, exist_ok=True)
    svg_dst = scalable_apps / 'battlefront-eternal.svg'
    shutil.copy2(icon_svg, svg_dst)
    svg_dst.chmod(0o644)


def stage_tree(
    *,
    platform: str,
    client_bin: Path,
    server_bin: Path | None = None,
    toolchain_dir: Path,
    out_dir: Path,
    repo_root: Path = REPO_ROOT,
    release_json: Path | None = None,
) -> dict:
    platform = normalize_platform(platform)
    is_windows = platform.startswith('win32-')
    if not client_bin.is_file():
        raise FileNotFoundError(f'Client binary not found: {client_bin}')
    if server_bin is not None and not server_bin.is_file():
        raise FileNotFoundError(f'Server binary not found: {server_bin}')
    if not toolchain_dir.is_dir():
        raise FileNotFoundError(f'Toolchain directory not found: {toolchain_dir}')

    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    client_name = 'bfe-client.exe' if is_windows else 'bfe-client'
    server_name = 'bfeserver.exe' if is_windows else 'bfeserver'

    staged_client = out_dir / client_name
    shutil.copy2(client_bin, staged_client)
    staged_client.chmod(0o755)

    if server_bin is not None:
        staged_server = out_dir / server_name
        shutil.copy2(server_bin, staged_server)
        staged_server.chmod(0o755)
    else:
        print(f'Note: no --server-bin provided for {platform}; staging client-only tree without {server_name}')

    if not is_windows:
        # Preserve the battlefront-eternal entrypoint name so existing desktop
        # files and ~/.local/bin launchers continue to work across updates.
        (out_dir / 'battlefront-eternal').symlink_to('bfe-client')

    resources = out_dir / 'resources'
    tools_dst = resources / 'tools'
    tools_dst.mkdir(parents=True, exist_ok=True)
    tools_src = repo_root / 'tools'
    for py_file in sorted(tools_src.glob('*.py')):
        shutil.copy2(py_file, tools_dst / py_file.name)
        (tools_dst / py_file.name).chmod(0o644)
    shutil.copy2(tools_src / 'content-version.json', tools_dst / 'content-version.json')
    (tools_dst / 'content-version.json').chmod(0o644)

    toolchain_dst = resources / 'toolchain'
    shutil.copytree(toolchain_dir, toolchain_dst, symlinks=True)
    clean_bytecode(toolchain_dst)

    legal_dst = resources / 'legal'
    legal_dst.mkdir(parents=True, exist_ok=True)
    for filename in ('LICENSE', 'THIRD-PARTY-NOTICES.md'):
        src = repo_root / filename
        if not src.is_file():
            raise FileNotFoundError(f'Required legal file missing: {src}')
        shutil.copy2(src, legal_dst / filename)
        (legal_dst / filename).chmod(0o644)
    licenses_dst = legal_dst / 'licenses'
    licenses_dst.mkdir(parents=True, exist_ok=True)
    for license_file in sorted((repo_root / 'licenses').glob('*.txt')):
        shutil.copy2(license_file, licenses_dst / license_file.name)
        (licenses_dst / license_file.name).chmod(0o644)

    assets_src = repo_root / 'build' / 'native' / 'assets'
    assets_dst = resources / 'assets'
    shutil.copytree(assets_src, assets_dst, symlinks=True)
    populate_hicolor_icons(assets_dst / 'icons' / 'hicolor', repo_root)

    rel_src = release_json if release_json is not None else (repo_root / 'build' / 'bfe-release.json')
    if rel_src and rel_src.is_file():
        shutil.copy2(rel_src, resources / 'bfe-release.json')
        (resources / 'bfe-release.json').chmod(0o644)

    clean_bytecode(resources)
    return build_tree_index(out_dir, platform)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', required=True, help='Target platform (linux-x64, linux-arm64, win32-x64)')
    parser.add_argument('--client-bin', required=True, type=Path, help='Path to compiled bfe-client binary')
    parser.add_argument('--server-bin', default=None, type=Path, help='Optional path to compiled bfeserver binary')
    parser.add_argument('--toolchain-dir', default=None, type=Path, help='Path to toolchain/<platform> directory')
    parser.add_argument('--out-dir', required=True, type=Path, help='Output directory for staged tree')
    parser.add_argument('--release-json', default=None, type=Path, help='Optional path to bfe-release.json')
    args = parser.parse_args()

    tc_dir = args.toolchain_dir or default_toolchain_dir(args.platform)
    manifest = stage_tree(
        platform=args.platform,
        client_bin=args.client_bin.resolve(),
        server_bin=args.server_bin.resolve() if args.server_bin else None,
        toolchain_dir=tc_dir.resolve(),
        out_dir=args.out_dir.resolve(),
        release_json=args.release_json.resolve() if args.release_json else None,
    )
    print(f"Staged {manifest['platform']} tree at {args.out_dir} ({len(manifest['files'])} files, {len(manifest['links'])} links)")


if __name__ == '__main__':
    main()
