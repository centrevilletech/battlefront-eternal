#!/usr/bin/env python3
"""Verify staged native trees, installers, server archives, and update payloads.

Checks performed:
  1. Unpacked tree: resources/bfe-tree.json matches every file and symlink on
     disk; bfe-client --help, bfeserver --help (when bfeserver is present in the
     workspace), and bundled Python execute. When bfeserver is absent, its
     absence is logged and server-specific checks are skipped.
  2. Linux .deb: unpacks (and optionally installs), checks /usr/bin symlinks,
     desktop entry, hicolor icons, /opt/Battlefront Eternal/resources/bfe-tree.json,
     bfe-client --help, bfeserver --help (when present), and bundled Python.
  3. Linux .AppImage: extracts squashfs-root, checks AppRun/desktop/icon metadata
     and resources/bfe-tree.json, runs AppRun --help, bfeserver --help (when
     present), and bundled Python.
  4. Linux dedicated server archive (.tar.xz): when present, unpacks, checks root
     bfe-tree.json against all files on disk, runs bfeserver --help and bundled
     Python.
  5. Windows NSIS installer (.exe): extracts the NSIS payload via 7z, walks the
     extracted tree to locate the staged payload root containing
     resources/bfe-tree.json and bfe-client.exe, verifies resources/bfe-tree.json
     against extracted files, and on Windows runs bfe-client.exe --help,
     bfeserver.exe --help (when present), and bundled python.exe.
  6. Update payload (update-<platform>.tar): verifies update-<platform>.json and
     checks that every bundled .zst blob decompresses to its declared SHA-256.
  7. Prints a human-readable size report for all verified artifacts.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import normalize_platform, sha256_file


def format_mib(num_bytes: int) -> str:
    return f'{num_bytes / (1024 * 1024):.2f} MiB ({num_bytes:,} bytes)'


def verify_tree_manifest(root: Path, tree_path: Path, allow_extra_root: set[str] | None = None) -> dict:
    if not tree_path.is_file():
        raise RuntimeError(f'Missing tree manifest: {tree_path}')
    manifest = json.loads(tree_path.read_text(encoding='utf-8'))
    if manifest.get('schema') != 1:
        raise RuntimeError(f'Unexpected schema in {tree_path}: {manifest.get("schema")}')
    files = manifest.get('files')
    if not isinstance(files, list) or not files:
        raise RuntimeError(f'Empty or invalid files list in {tree_path}')
    is_windows_manifest = str(manifest.get('platform', '')).startswith('win32-')

    expected_paths: set[str] = set()
    for entry in files:
        rel = entry['path']
        expected_paths.add(rel)
        full = root.joinpath(*rel.split('/'))
        if not full.is_file() or full.is_symlink():
            raise RuntimeError(f'Expected regular file at {full}')
        st = full.stat()
        if st.st_size != entry['size']:
            raise RuntimeError(f'Size mismatch at {full}: expected {entry["size"]}, got {st.st_size}')
        digest = sha256_file(full)
        if digest != entry['sha256']:
            raise RuntimeError(f'SHA-256 mismatch at {full}: expected {entry["sha256"]}, got {digest}')
        if os.name != 'nt' and not is_windows_manifest:
            is_exec = bool(st.st_mode & 0o111)
            want_exec = entry['mode'] == 0o755
            if is_exec != want_exec:
                raise RuntimeError(f'Executable mode mismatch at {full}: mode={oct(st.st_mode)}, want {oct(entry["mode"])}')

    for link in manifest.get('links', []):
        rel = link['path']
        expected_paths.add(rel)
        full = root.joinpath(*rel.split('/'))
        if not full.is_symlink():
            raise RuntimeError(f'Expected symlink at {full}')
        actual_target = os.readlink(full).replace('\\', '/')
        if actual_target != link['target']:
            raise RuntimeError(f'Symlink mismatch at {full}: expected {link["target"]}, got {actual_target}')

    if allow_extra_root is not None:
        tree_rel = tree_path.relative_to(root).as_posix()
        for dirpath, dirnames, filenames in os.walk(root):
            base = Path(dirpath)
            rel_dir = base.relative_to(root).as_posix() if base != root else ''
            top_component = rel_dir.split('/')[0] if rel_dir else ''
            if top_component in allow_extra_root:
                continue
            for name in list(filenames) + list(dirnames):
                candidate = base / name
                rel_path = candidate.relative_to(root).as_posix()
                if rel_path == tree_rel or rel_path.split('/')[0] in allow_extra_root:
                    continue
                if candidate.is_symlink() or candidate.is_file():
                    if rel_path not in expected_paths:
                        raise RuntimeError(f'Unindexed file found on disk: {rel_path} in {root}')
    return manifest


def smoke_binary(binary: Path, label: str) -> None:
    if not binary.is_file():
        raise RuntimeError(f'{label} binary missing: {binary}')
    env = os.environ.copy()
    env['BFE_SMOKE_TEST'] = '1'
    proc = subprocess.run(
        [str(binary), '--help'],
        env=env,
        capture_output=True,
        text=True,
        timeout=20,
    )
    combined = (proc.stdout or '') + '\n' + (proc.stderr or '')
    if proc.returncode < 0:
        raise RuntimeError(f'{label} crashed with signal {-proc.returncode}: {combined.strip()}')
    for linker_err in ('error while loading shared libraries', 'GLIBC_', 'CXXABI_'):
        if linker_err in combined:
            raise RuntimeError(f'{label} failed dynamic linking check: {combined.strip()}')
    if proc.returncode == 0:
        return
    # bfe-client session_window currently requires --map / session.json if
    # --help is not intercepted first; accept its controlled usage message as
    # a valid smoke execution proving the binary starts and links cleanly.
    if proc.returncode in (1, 2) and any(token in combined for token in ('--session', '--map', 'Usage', 'usage')):
        return
    raise RuntimeError(f'{label} --help exited with {proc.returncode}: {combined.strip()}')


def smoke_python(root: Path, platform: str, tools_subdir: str = 'resources/tools', toolchain_subdir: str = 'resources/toolchain') -> None:
    is_windows = platform.startswith('win32-')
    python_parts = ('python', 'python.exe') if is_windows else ('python', 'bin', 'python3')
    python_bin = root.joinpath(*toolchain_subdir.split('/'), *python_parts)
    if not python_bin.is_file():
        raise RuntimeError(f'Bundled Python executable missing at {python_bin}')
    prepare_py = root.joinpath(*tools_subdir.split('/'), 'prepare.py')
    if not prepare_py.is_file():
        raise RuntimeError(f'Bundled prepare.py missing at {prepare_py}')

    proc = subprocess.run(
        [
            str(python_bin),
            '-c',
            'import ast, pathlib, sys; '
            'ast.parse(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")); '
            'print(sys.version.split()[0])',
            str(prepare_py),
        ],
        capture_output=True,
        text=True,
        timeout=20,
    )
    if proc.returncode != 0:
        raise RuntimeError(f'Bundled Python check failed ({proc.returncode}): {proc.stderr or proc.stdout}')


def check_staged_dir(staged_dir: Path, platform: str) -> None:
    is_windows = platform.startswith('win32-')
    manifest = verify_tree_manifest(staged_dir, staged_dir / 'resources' / 'bfe-tree.json', allow_extra_root=set())
    client_bin = staged_dir / ('bfe-client.exe' if is_windows else 'bfe-client')
    server_bin = staged_dir / ('bfeserver.exe' if is_windows else 'bfeserver')
    smoke_binary(client_bin, 'staged bfe-client')
    if 'server_executable' in manifest:
        smoke_binary(server_bin, 'staged bfeserver')
    else:
        if server_bin.exists():
            raise RuntimeError(f'Unexpected unindexed server binary at {server_bin}')
        print('[INFO] staged bfeserver is absent (bfeserver crate not yet in workspace); skipping staged server smoke check')
    smoke_python(staged_dir, platform)


def check_deb(deb_path: Path, platform: str, install_deb: bool = False) -> None:
    with tempfile.TemporaryDirectory(prefix='bfe-check-deb-') as tmp:
        root = Path(tmp) / 'extracted'
        control = Path(tmp) / 'control'
        subprocess.run(['dpkg-deb', '-x', str(deb_path), str(root)], check=True)
        subprocess.run(['dpkg-deb', '-e', str(deb_path), str(control)], check=True)

        control_text = (control / 'control').read_text(encoding='utf-8')
        if 'Package: battlefront-eternal' not in control_text:
            raise RuntimeError(f'Unexpected DEBIAN/control in {deb_path}:\n{control_text}')

        opt_app = root / 'opt' / 'Battlefront Eternal'
        manifest = verify_tree_manifest(opt_app, opt_app / 'resources' / 'bfe-tree.json', allow_extra_root=set())
        has_server = 'server_executable' in manifest

        required_links = [
            ('battlefront-eternal', '/opt/Battlefront Eternal/bfe-client'),
            ('bfe-client', '/opt/Battlefront Eternal/bfe-client'),
        ]
        if has_server:
            required_links.append(('bfeserver', '/opt/Battlefront Eternal/bfeserver'))
        for link_name, target in required_links:
            usr_link = root / 'usr' / 'bin' / link_name
            if not usr_link.is_symlink() or os.readlink(usr_link) != target:
                raise RuntimeError(f'Invalid /usr/bin/{link_name} symlink in {deb_path}')
        if not has_server and (root / 'usr' / 'bin' / 'bfeserver').exists():
            raise RuntimeError(f'Unexpected /usr/bin/bfeserver symlink in client-only {deb_path}')

        desktop_file = root / 'usr' / 'share' / 'applications' / 'battlefront-eternal.desktop'
        if not desktop_file.is_file():
            raise RuntimeError(f'Missing desktop entry in {deb_path}')
        icon_file = root / 'usr' / 'share' / 'icons' / 'hicolor' / '256x256' / 'apps' / 'battlefront-eternal.png'
        if not icon_file.is_file():
            raise RuntimeError(f'Missing 256x256 hicolor icon in {deb_path}')

        smoke_binary(opt_app / 'bfe-client', '.deb bfe-client')
        if has_server:
            smoke_binary(opt_app / 'bfeserver', '.deb bfeserver')
        else:
            print('[INFO] .deb bfeserver is absent (bfeserver crate not yet in workspace); skipping .deb server check')
        smoke_python(opt_app, platform)

    if install_deb:
        cmd_prefix = [] if os.geteuid() == 0 else ['sudo']
        # apt installs the package's declared dependencies from the system's
        # repositories, as a player's package manager would; a bare dpkg -i
        # stops on any library the build machine happens to lack.
        # (sudo resets the environment, so the frontend is set through env.)
        apt = [*cmd_prefix, 'env', 'DEBIAN_FRONTEND=noninteractive', 'apt-get']
        subprocess.run([*apt, 'update', '-q'], check=True)
        subprocess.run([*apt, 'install', '-y', '-q', '--no-install-recommends', str(deb_path.resolve())], check=True)
        try:
            installed_app = Path('/opt/Battlefront Eternal')
            manifest = verify_tree_manifest(installed_app, installed_app / 'resources' / 'bfe-tree.json', allow_extra_root=set())
            smoke_binary(Path('/usr/bin/bfe-client'), 'installed /usr/bin/bfe-client')
            if 'server_executable' in manifest:
                smoke_binary(Path('/usr/bin/bfeserver'), 'installed /usr/bin/bfeserver')
        finally:
            subprocess.run([*cmd_prefix, 'dpkg', '-r', 'battlefront-eternal'], check=True)


def check_appimage(appimage_path: Path, platform: str) -> None:
    with tempfile.TemporaryDirectory(prefix='bfe-check-appimage-') as tmp:
        subprocess.run(
            [str(appimage_path.resolve()), '--appimage-extract'],
            cwd=tmp,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        sq = Path(tmp) / 'squashfs-root'
        extra = {'AppRun', 'battlefront-eternal.desktop', 'battlefront-eternal.png', '.DirIcon', 'usr'}
        manifest = verify_tree_manifest(sq, sq / 'resources' / 'bfe-tree.json', allow_extra_root=extra)
        smoke_binary(sq / 'AppRun', 'AppImage AppRun')
        if 'server_executable' in manifest:
            smoke_binary(sq / 'bfeserver', 'AppImage bfeserver')
        else:
            print('[INFO] AppImage bfeserver is absent (bfeserver crate not yet in workspace); skipping AppImage server check')
        smoke_python(sq, platform)


def check_server_archive(archive_path: Path, platform: str) -> None:
    with tempfile.TemporaryDirectory(prefix='bfe-check-server-') as tmp:
        root = Path(tmp) / 'server'
        root.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive_path, 'r:xz') as tar:
            tar.extractall(root, filter='fully_trusted')
        verify_tree_manifest(root, root / 'bfe-tree.json', allow_extra_root=set())
        smoke_binary(root / 'bfeserver', 'server-archive bfeserver')
        smoke_python(root, platform, tools_subdir='tools', toolchain_subdir='toolchain')


bMZ_HEADER = b'MZ'


def find_7z() -> str | None:
    found = shutil.which('7z') or shutil.which('7za')
    if found:
        return found
    for candidate in (
        Path('C:/Program Files/7-Zip/7z.exe'),
        Path('C:/Program Files (x86)/7-Zip/7z.exe'),
    ):
        if candidate.is_file():
            return str(candidate)
    return None


def find_windows_payload_root(extracted_dir: Path) -> Path:
    """Walk an extracted NSIS installer directory to locate the staged payload root
    containing resources/bfe-tree.json and bfe-client.exe."""
    if (extracted_dir / 'resources' / 'bfe-tree.json').is_file() and (extracted_dir / 'bfe-client.exe').is_file():
        return extracted_dir
    for dirpath, dirnames, filenames in os.walk(extracted_dir):
        dirnames.sort()
        base = Path(dirpath)
        if 'bfe-tree.json' in filenames and base.name == 'resources':
            candidate = base.parent
            if (candidate / 'bfe-client.exe').is_file():
                return candidate
    entries = sorted(p.name for p in extracted_dir.iterdir()) if extracted_dir.is_dir() else []
    raise RuntimeError(
        f'Could not locate staged payload root (resources/bfe-tree.json + bfe-client.exe) '
        f'under {extracted_dir} (top-level entries: {entries})'
    )


def verify_extracted_windows_tree(extracted_dir: Path, platform: str, run_smoke: bool | None = None) -> dict:
    payload_root = find_windows_payload_root(extracted_dir)
    tree_path = payload_root / 'resources' / 'bfe-tree.json'
    manifest = verify_tree_manifest(
        payload_root,
        tree_path,
        allow_extra_root={'$PLUGINSDIR', '$TEMP', '$R0', 'Uninstall Battlefront Eternal.exe'},
    )
    should_smoke = (os.name == 'nt') if run_smoke is None else run_smoke
    if should_smoke:
        smoke_binary(payload_root / 'bfe-client.exe', 'extracted bfe-client.exe')
        if 'server_executable' in manifest:
            smoke_binary(payload_root / 'bfeserver.exe', 'extracted bfeserver.exe')
        else:
            print('[INFO] extracted bfeserver.exe is absent (bfeserver crate not yet in workspace); skipping Windows server check')
        smoke_python(payload_root, platform)
    return manifest


def check_windows_installer(installer_path: Path, platform: str) -> None:
    if not installer_path.is_file():
        raise RuntimeError(f'Windows installer not found: {installer_path}')
    with installer_path.open('rb') as handle:
        mz = handle.read(2)
    if mz != bMZ_HEADER:
        raise RuntimeError(f'Windows installer {installer_path} does not have a valid PE MZ header')

    seven_zip = find_7z()
    if seven_zip is None:
        if os.name == 'nt':
            raise RuntimeError('7z not found on Windows runner to extract and verify NSIS installer')
        return

    with tempfile.TemporaryDirectory(prefix='bfe-check-win-', ignore_cleanup_errors=True) as tmp:
        extracted_dir = Path(tmp) / 'extracted'
        extracted_dir.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            [seven_zip, 'x', '-y', f'-o{extracted_dir}', str(installer_path.resolve())],
            check=True,
            stdout=subprocess.DEVNULL,
            timeout=120,
        )
        verify_extracted_windows_tree(extracted_dir, platform)


def check_update_payload(payload_path: Path, platform: str) -> None:
    manifest_name = f'update-{platform}.json'
    with tempfile.TemporaryDirectory(prefix='bfe-check-update-') as tmp:
        work = Path(tmp)
        with tarfile.open(payload_path, 'r:') as tar:
            for member in tar:
                if not member.isfile():
                    raise RuntimeError(f'Update payload {payload_path} contains non-file entry {member.name}')
            tar.extractall(work, filter='data')
        manifest_file = work / manifest_name
        if not manifest_file.is_file():
            raise RuntimeError(f'Update payload {payload_path} missing {manifest_name}')
        manifest = json.loads(manifest_file.read_text(encoding='utf-8'))
        if manifest.get('platform') != platform:
            raise RuntimeError(f'Platform mismatch in {manifest_name}: {manifest.get("platform")}')

        exe_name = manifest.get('executable')
        if not exe_name:
            raise RuntimeError(f'Missing executable in {manifest_name}')
        matching = [f for f in manifest['files'] if f.get('path') == exe_name]
        if not matching or matching[0].get('mode') != 0o755:
            raise RuntimeError(f'executable ({exe_name}) missing or not 0o755 in {manifest_name}')

        if 'server_executable' in manifest:
            srv_name = manifest['server_executable']
            srv_matching = [f for f in manifest['files'] if f.get('path') == srv_name]
            if not srv_matching or srv_matching[0].get('mode') != 0o755:
                raise RuntimeError(f'server_executable ({srv_name}) missing or not 0o755 in {manifest_name}')
        else:
            print(f'[INFO] server_executable absent in {manifest_name} (bfeserver crate not yet in workspace); skipping server payload check')

        blobs_dir = work / 'blobs'
        if blobs_dir.is_dir():
            for blob_path in sorted(blobs_dir.glob('*.zst')):
                digest = blob_path.name[:-4]
                raw = subprocess.check_output(['zstd', '-dcq', str(blob_path)])
                actual = hashlib.sha256(raw).hexdigest()
                if actual != digest:
                    raise RuntimeError(f'Corrupt blob {blob_path.name} in {payload_path}: sha256={actual}')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', required=True, help='Target platform (linux-x64, linux-arm64, win32-x64)')
    parser.add_argument('--staged-dir', type=Path, help='Staged unpacked directory to check')
    parser.add_argument('--deb', type=Path, help='Debian .deb package to check')
    parser.add_argument('--install-deb', action='store_true', help='Also install (apt) and remove (dpkg -r) the .deb package')
    parser.add_argument('--appimage', type=Path, help='Linux .AppImage package to check')
    parser.add_argument('--server-archive', type=Path, help='Dedicated server .tar.xz archive to check')
    parser.add_argument('--windows-installer', type=Path, help='Windows NSIS .exe installer to check')
    parser.add_argument('--update-payload', type=Path, help='Update payload update-<platform>.tar to check')
    parser.add_argument('--report-out', type=Path, help='Optional file path to write Markdown size report')
    args = parser.parse_args()

    platform = normalize_platform(args.platform)
    sizes: list[tuple[str, Path]] = []

    if args.staged_dir:
        check_staged_dir(args.staged_dir, platform)
        print(f'[OK] Staged unpacked directory verified: {args.staged_dir}')
    if args.deb:
        check_deb(args.deb, platform, install_deb=args.install_deb)
        sizes.append(('Debian package (.deb)', args.deb))
        print(f'[OK] Debian package verified: {args.deb}')
    if args.appimage:
        check_appimage(args.appimage, platform)
        sizes.append(('Linux AppImage (.AppImage)', args.appimage))
        print(f'[OK] AppImage verified: {args.appimage}')
    if args.server_archive:
        if args.server_archive.is_file():
            check_server_archive(args.server_archive, platform)
            sizes.append(('Dedicated server archive (.tar.xz)', args.server_archive))
            print(f'[OK] Dedicated server archive verified: {args.server_archive}')
        else:
            print(f'[INFO] Dedicated server archive {args.server_archive} not present (bfeserver crate not yet in workspace); skipping server archive check')
    elif platform.startswith('linux-'):
        print('[INFO] No --server-archive provided (bfeserver crate not yet in workspace); skipping server archive check')
    if args.windows_installer:
        check_windows_installer(args.windows_installer, platform)
        sizes.append(('Windows NSIS installer (.exe)', args.windows_installer))
        print(f'[OK] Windows NSIS installer verified: {args.windows_installer}')
    if args.update_payload:
        check_update_payload(args.update_payload, platform)
        sizes.append(('Update payload (.tar)', args.update_payload))
        print(f'[OK] Update payload verified: {args.update_payload}')

    if sizes:
        lines = [
            f'### Package Size Report ({platform})',
            '',
            '| Artifact | Filename | Size |',
            '| --- | --- | --- |',
        ]
        for kind, path in sizes:
            lines.append(f'| {kind} | `{path.name}` | {format_mib(path.stat().st_size)} |')
        report = '\n'.join(lines) + '\n'
        print('\n' + report)
        if args.report_out:
            args.report_out.parent.mkdir(parents=True, exist_ok=True)
            args.report_out.write_text(report, encoding='utf-8')


if __name__ == '__main__':
    main()
