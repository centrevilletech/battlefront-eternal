#!/usr/bin/env python3
"""Build Linux .deb and .AppImage packages from a staged native unpacked tree.

Output filenames match build/publish-distribution.py:
  - Battlefront-Eternal-<version>-linux-x86_64.AppImage
  - Battlefront-Eternal-<version>-linux-arm64.AppImage
  - Battlefront-Eternal-<version>-linux-amd64.deb
  - Battlefront-Eternal-<version>-linux-arm64.deb
"""

import argparse
import os
from pathlib import Path
import platform as host_platform
import shutil
import subprocess
import sys
import tempfile
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import REPO_ROOT, normalize_permissions, normalize_platform, populate_hicolor_icons

ARCH_MAP = {
    'linux-x64': {
        'deb_arch': 'amd64',
        'appimage_name_arch': 'x86_64',
        'appimagetool_arch': 'x86_64',
    },
    'linux-arm64': {
        'deb_arch': 'arm64',
        'appimage_name_arch': 'arm64',
        'appimagetool_arch': 'aarch64',
    },
}


def tree_kib(root: Path) -> int:
    total = 0
    for dirpath, _, filenames in os.walk(root):
        base = Path(dirpath)
        for name in filenames:
            full = base / name
            if not full.is_symlink():
                total += full.stat().st_size
    return max(1, (total + 1023) // 1024)


def ensure_appimagetool(explicit: Path | None = None) -> Path:
    if explicit and explicit.is_file():
        return explicit
    env_tool = os.environ.get('APPIMAGETOOL')
    if env_tool and Path(env_tool).is_file():
        return Path(env_tool)
    found = shutil.which('appimagetool')
    if found:
        return Path(found)

    machine = host_platform.machine().lower()
    host_arch = 'aarch64' if machine in ('aarch64', 'arm64') else 'x86_64'
    cache_dir = Path(os.environ.get('BFE_CACHE_DIR', Path(tempfile.gettempdir()) / 'bfe-cache'))
    cache_dir.mkdir(parents=True, exist_ok=True)
    cached = cache_dir / f'appimagetool-{host_arch}.AppImage'
    if not cached.is_file():
        url = f'https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-{host_arch}.AppImage'
        tmp = cached.with_suffix('.tmp')
        with urllib.request.urlopen(url, timeout=120) as response, tmp.open('wb') as out:
            shutil.copyfileobj(response, out)
        tmp.chmod(0o755)
        tmp.replace(cached)
    cached.chmod(0o755)
    return cached


def build_deb(staged_dir: Path, platform: str, version: str, out_dir: Path, repo_root: Path = REPO_ROOT) -> Path:
    arch_info = ARCH_MAP[platform]
    deb_arch = arch_info['deb_arch']
    deb_version = version.replace('-', '~')
    out_path = out_dir / f'Battlefront-Eternal-{version}-linux-{deb_arch}.deb'

    with tempfile.TemporaryDirectory(prefix='bfe-deb-') as tmp:
        root = Path(tmp) / 'pkg'
        root.mkdir(mode=0o755, parents=True, exist_ok=True)
        opt_app = root / 'opt' / 'Battlefront Eternal'
        opt_app.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(staged_dir, opt_app, symlinks=True)

        usr_bin = root / 'usr' / 'bin'
        usr_bin.mkdir(parents=True, exist_ok=True)
        (usr_bin / 'battlefront-eternal').symlink_to('/opt/Battlefront Eternal/bfe-client')
        (usr_bin / 'bfe-client').symlink_to('/opt/Battlefront Eternal/bfe-client')
        if (staged_dir / 'bfeserver').is_file():
            (usr_bin / 'bfeserver').symlink_to('/opt/Battlefront Eternal/bfeserver')

        apps_dir = root / 'usr' / 'share' / 'applications'
        apps_dir.mkdir(parents=True, exist_ok=True)
        desktop_src = repo_root / 'build' / 'native' / 'battlefront-eternal.desktop'
        shutil.copy2(desktop_src, apps_dir / 'battlefront-eternal.desktop')
        (apps_dir / 'battlefront-eternal.desktop').chmod(0o644)

        icons_dst = root / 'usr' / 'share' / 'icons' / 'hicolor'
        populate_hicolor_icons(icons_dst, repo_root)

        debian_dir = root / 'DEBIAN'
        debian_dir.mkdir(mode=0o755, parents=True, exist_ok=True)
        installed_kib = tree_kib(root)
        control_lines = [
            'Package: battlefront-eternal',
            f'Version: {deb_version}',
            'Section: games',
            'Priority: optional',
            f'Architecture: {deb_arch}',
            f'Installed-Size: {installed_kib}',
            'Depends: libc6, libstdc++6, libgcc-s1, libasound2t64 | libasound2, ca-certificates',
            'Recommends: libvulkan1, libx11-6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0',
            'Maintainer: Battlefront Eternal Contributors <contact@battlefronteternal.com>',
            'Homepage: https://battlefronteternal.com',
            'Description: Star Wars Battlefront II (2005) preservation and remaster runtime',
            ' Battlefront Eternal is a native client and dedicated server runtime for',
            ' Star Wars Battlefront II (2005).',
            '',
        ]
        control_file = debian_dir / 'control'
        control_file.write_text('\n'.join(control_lines), encoding='utf-8')
        normalize_permissions(root)

        out_dir.mkdir(parents=True, exist_ok=True)
        if out_path.exists():
            out_path.unlink()
        subprocess.run(
            ['dpkg-deb', '-Zxz', '--build', '--root-owner-group', str(root), str(out_path)],
            check=True,
        )
    return out_path


def build_appimage(
    staged_dir: Path,
    platform: str,
    version: str,
    out_dir: Path,
    appimagetool: Path | None = None,
    repo_root: Path = REPO_ROOT,
) -> Path:
    arch_info = ARCH_MAP[platform]
    name_arch = arch_info['appimage_name_arch']
    tool_arch = arch_info['appimagetool_arch']
    out_path = out_dir / f'Battlefront-Eternal-{version}-linux-{name_arch}.AppImage'
    tool = ensure_appimagetool(appimagetool)

    with tempfile.TemporaryDirectory(prefix='bfe-appdir-') as tmp:
        Path(tmp).chmod(0o755)
        appdir = Path(tmp) / 'Battlefront-Eternal.AppDir'
        shutil.copytree(staged_dir, appdir, symlinks=True)

        apprun_src = repo_root / 'build' / 'native' / 'AppRun'
        shutil.copy2(apprun_src, appdir / 'AppRun')
        (appdir / 'AppRun').chmod(0o755)

        appimage_desktop = '\n'.join([
            '[Desktop Entry]',
            'Name=Battlefront Eternal',
            'Comment=Star Wars Battlefront II (2005) preservation and remaster runtime',
            'Exec=bfe-client %U',
            'Icon=battlefront-eternal',
            'Terminal=false',
            'Type=Application',
            'Categories=Game;ActionGame;',
            'StartupWMClass=battlefront-eternal',
            '',
        ])
        (appdir / 'battlefront-eternal.desktop').write_text(appimage_desktop, encoding='utf-8')
        (appdir / 'battlefront-eternal.desktop').chmod(0o644)

        usr_apps = appdir / 'usr' / 'share' / 'applications'
        usr_apps.mkdir(parents=True, exist_ok=True)
        (usr_apps / 'battlefront-eternal.desktop').write_text(appimage_desktop, encoding='utf-8')
        (usr_apps / 'battlefront-eternal.desktop').chmod(0o644)

        icons_dst = appdir / 'usr' / 'share' / 'icons' / 'hicolor'
        populate_hicolor_icons(icons_dst, repo_root)

        root_icon = repo_root / 'build' / 'native' / 'assets' / 'icon-256.png'
        shutil.copy2(root_icon, appdir / 'battlefront-eternal.png')
        (appdir / '.DirIcon').symlink_to('battlefront-eternal.png')
        normalize_permissions(appdir)

        out_dir.mkdir(parents=True, exist_ok=True)
        if out_path.exists():
            out_path.unlink()

        env = os.environ.copy()
        env['ARCH'] = tool_arch
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
        subprocess.run(
            [str(tool), '--no-appstream', str(appdir), str(out_path)],
            env=env,
            check=True,
        )
        out_path.chmod(0o755)
    return out_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-dir', required=True, type=Path, help='Path to staged Linux unpacked directory')
    parser.add_argument('--platform', required=True, help='Target platform (linux-x64 or linux-arm64)')
    parser.add_argument('--version', required=True, help='Release version (for example 0.1.0-preview.1)')
    parser.add_argument('--out-dir', required=True, type=Path, help='Output directory for .deb and .AppImage')
    parser.add_argument('--appimagetool', type=Path, help='Optional path to appimagetool binary')
    parser.add_argument('--skip-appimage', action='store_true', help='Only build the .deb package')
    parser.add_argument('--skip-deb', action='store_true', help='Only build the .AppImage package')
    args = parser.parse_args()

    platform = normalize_platform(args.platform)
    if platform not in ARCH_MAP:
        raise ValueError(f'Linux packaging requires linux-x64 or linux-arm64, got {platform}')

    if not args.skip_deb:
        deb = build_deb(args.staged_dir, platform, args.version, args.out_dir)
        print(f'Built Debian package: {deb} ({deb.stat().st_size} bytes)')
    if not args.skip_appimage:
        appimage = build_appimage(args.staged_dir, platform, args.version, args.out_dir, args.appimagetool)
        print(f'Built AppImage: {appimage} ({appimage.stat().st_size} bytes)')


if __name__ == '__main__':
    main()
