#!/usr/bin/env python3
"""Build "Battlefront Eternal.app" and its disk image from a staged native tree.

Output (names match build/publish-distribution.py):
  <out-dir>/macos-<arch>/Battlefront Eternal.app
  <out-dir>/Battlefront-Eternal-<version>-macos-<arch>.dmg
  <out-dir>/macos-<arch>-unsigned-build-input.tar.zst   (--build-input: the app
      alone, for the signing step that makes the published download from it)

The bundle:
  Contents/Info.plist, PkgInfo
  Contents/MacOS/bfe-client, bfeserver
  Contents/MacOS/resources -> ../Resources
  Contents/Resources/         the staged resources/ folder (preparation tools,
                              Python and FFmpeg toolchain, legal notices,
                              artwork, release record) and AppIcon.icns

The programs find `resources/` beside themselves on every platform; the link
gives them that inside a bundle while everything that is not a program stays
in Contents/Resources, where a code signature seals it without touching it.

The app is signed by build/native/sign-macos.py: ad hoc unless
BFE_MAC_SIGN_IDENTITY names a certificate. The disk image shows the app beside
a link to /Applications. Runs on macOS only (sips, iconutil, hdiutil,
codesign).
"""

import argparse
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import MACOS_APP_NAME, REPO_ROOT, normalize_permissions, normalize_platform

BUNDLE_IDENTIFIER = 'com.battlefronteternal.client'
ARCH_NAMES = {'darwin-arm64': 'arm64', 'darwin-x64': 'x64'}
# The disk image's window layout is written without Finder (and so without a
# desktop session) by dmgbuild; pinned, and installed into a folder of its own.
DMGBUILD_REQUIREMENTS = ('dmgbuild==1.6.7', 'ds_store==1.3.3', 'mac_alias==2.2.3')
ICON_SIZES = ((16, '16x16'), (32, '16x16@2x'), (32, '32x32'), (64, '32x32@2x'), (128, '128x128'), (256, '128x128@2x'),
              (256, '256x256'), (512, '256x256@2x'), (512, '512x512'), (1024, '512x512@2x'))


def run(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def bundle_versions(version: str) -> tuple[str, str]:
    """(what players read, the build number macOS compares) for a release version."""
    match = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)(?:-preview\.(\d+))?', version)
    if not match:
        raise ValueError(f'Unexpected release version {version!r}')
    major, minor, patch, preview = match.groups()
    return version, preview or f'{major}.{minor}.{patch}'


def minimum_macos(app: Path) -> str:
    """The oldest macOS on which every program in the bundle starts: the newest of the minimum
    versions they were built for (the client, the server, Python and its modules, FFmpeg, ktx)."""
    newest = (0, 0)
    for path in app.rglob('*'):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open('rb') as handle:
            if handle.read(4) not in (b'\xcf\xfa\xed\xfe', b'\xca\xfe\xba\xbe'):
                continue
        text = subprocess.run(['otool', '-l', str(path)], check=True, capture_output=True, text=True).stdout
        for found in re.finditer(r'(?:LC_BUILD_VERSION.*?minos|LC_VERSION_MIN_MACOSX.*?version) (\d+)\.(\d+)', text, re.S):
            newest = max(newest, (int(found.group(1)), int(found.group(2))))
    if newest == (0, 0):
        raise RuntimeError(f'No program in {app} names a minimum macOS version')
    return f'{newest[0]}.{newest[1]}'


def make_icns(png: Path, icns: Path) -> None:
    with tempfile.TemporaryDirectory(prefix='bfe-icon-') as work:
        iconset = Path(work) / 'AppIcon.iconset'
        iconset.mkdir()
        for size, name in ICON_SIZES:
            run(['sips', '-z', size, size, png, '--out', iconset / f'icon_{name}.png'], stdout=subprocess.DEVNULL)
        run(['iconutil', '--convert', 'icns', '--output', icns, iconset])
    icns.chmod(0o644)


def build_app(staged_dir: Path, platform: str, version: str, out_parent: Path, repo_root: Path = REPO_ROOT) -> Path:
    if not (staged_dir / 'bfe-client').is_file():
        raise FileNotFoundError(f'Missing bfe-client in staged tree {staged_dir}')
    unexpected = sorted(entry.name for entry in staged_dir.iterdir() if entry.name not in ('bfe-client', 'bfeserver', 'resources'))
    if unexpected:
        raise RuntimeError(f'Staged tree {staged_dir} holds entries the app bundle has no place for: {unexpected}')
    if out_parent.exists():
        shutil.rmtree(out_parent)
    app = out_parent / MACOS_APP_NAME
    contents = app / 'Contents'
    macos_dir = contents / 'MacOS'
    resources = contents / 'Resources'
    macos_dir.mkdir(parents=True)
    shutil.copytree(staged_dir / 'resources', resources, symlinks=True)
    # Written again once the helper programs carry their signatures (sign-macos.py).
    (resources / 'bfe-tree.json').unlink(missing_ok=True)
    for name in ('bfe-client', 'bfeserver'):
        if (staged_dir / name).is_file():
            shutil.copy2(staged_dir / name, macos_dir / name)
            (macos_dir / name).chmod(0o755)
    (macos_dir / 'resources').symlink_to('../Resources')
    make_icns(repo_root / 'build' / 'native' / 'assets' / 'icon.png', resources / 'AppIcon.icns')

    shown, build = bundle_versions(version)
    info = {
        'CFBundleDevelopmentRegion': 'en',
        'CFBundleDisplayName': 'Battlefront Eternal',
        'CFBundleExecutable': 'bfe-client',
        'CFBundleIconFile': 'AppIcon',
        'CFBundleIdentifier': BUNDLE_IDENTIFIER,
        'CFBundleInfoDictionaryVersion': '6.0',
        'CFBundleName': 'Battlefront Eternal',
        'CFBundlePackageType': 'APPL',
        'CFBundleShortVersionString': shown,
        'CFBundleSupportedPlatforms': ['MacOSX'],
        'CFBundleVersion': build,
        'LSApplicationCategoryType': 'public.app-category.action-games',
        'LSMinimumSystemVersion': minimum_macos(app),
        'LSSupportsGameMode': True,
        'GCSupportsGameMode': True,
        'NSHighResolutionCapable': True,
        'NSPrincipalClass': 'NSApplication',
        'NSHumanReadableCopyright': 'Battlefront Eternal contributors. GPL-3.0-only. Star Wars and the original games belong to their rights holders.',
    }
    with (contents / 'Info.plist').open('wb') as handle:
        plistlib.dump(info, handle)
    (contents / 'PkgInfo').write_bytes(b'APPL????')
    normalize_permissions(app)
    return app


def ensure_dmgbuild() -> Path:
    """The Python of a private environment that holds the pinned dmgbuild."""
    cache = Path(os.environ.get('BFE_CACHE_DIR', Path(tempfile.gettempdir()) / 'bfe-cache'))
    environment = cache / ('dmgbuild-' + '-'.join(requirement.split('==')[1] for requirement in DMGBUILD_REQUIREMENTS))
    python = environment / 'bin' / 'python3'
    if not (environment / '.ready').is_file():
        shutil.rmtree(environment, ignore_errors=True)
        environment.parent.mkdir(parents=True, exist_ok=True)
        run([sys.executable, '-m', 'venv', environment])
        run([python, '-m', 'pip', 'install', '--quiet', '--disable-pip-version-check', '--no-deps', *DMGBUILD_REQUIREMENTS])
        (environment / '.ready').write_text('')
    return python


def build_dmg(app: Path, platform: str, version: str, out_dir: Path) -> Path:
    out_path = out_dir / f'Battlefront-Eternal-{version}-macos-{ARCH_NAMES[platform]}.dmg'
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path.unlink(missing_ok=True)
    # Beside this script, wherever it is installed.
    native = Path(__file__).resolve().parent
    run([ensure_dmgbuild(), '-m', 'dmgbuild',
         '-s', native / 'macos' / 'dmg-settings.py',
         '-D', f'app={app}',
         '-D', f"icon={app / 'Contents' / 'Resources' / 'AppIcon.icns'}",
         '-D', f"background={native / 'macos' / 'dmg-background.png'}",
         'Battlefront Eternal', out_path])
    return out_path


def build_input(app: Path, platform: str, out_dir: Path) -> Path:
    """The app bundle alone as a tar.zst: files, modes and links, no owner names and no extended attributes."""
    out_path = out_dir / f'macos-{ARCH_NAMES[platform]}-unsigned-build-input.tar.zst'
    out_path.unlink(missing_ok=True)

    def plain(member: tarfile.TarInfo) -> tarfile.TarInfo:
        member.uid = member.gid = 0
        member.uname = member.gname = ''
        return member

    with subprocess.Popen(['zstd', '-T0', '-15', '-q', '-o', str(out_path)], stdin=subprocess.PIPE) as packer:
        with tarfile.open(fileobj=packer.stdin, mode='w|', format=tarfile.PAX_FORMAT) as archive:
            archive.add(app, arcname=app.name, filter=plain)
        packer.stdin.close()
    if packer.returncode != 0:
        raise RuntimeError('zstd failed to pack the build input')
    return out_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--staged-dir', type=Path, help='Staged macOS tree (build/native/stage.py): build and sign the app, then the image')
    source.add_argument('--app', type=Path, help='An app bundle that is already signed: only build the image')
    parser.add_argument('--platform', required=True, help='darwin-arm64 or darwin-x64')
    parser.add_argument('--version', required=True, help='Release version (for example 0.1.0-preview.1)')
    parser.add_argument('--out-dir', required=True, type=Path, help='Output folder')
    parser.add_argument('--skip-dmg', action='store_true', help='Only build and sign the app bundle')
    parser.add_argument('--build-input', action='store_true', help='Also pack the app as the signing step\'s input archive')
    args = parser.parse_args()

    if sys.platform != 'darwin':
        raise SystemExit('package-macos.py runs on macOS')
    platform = normalize_platform(args.platform)
    if platform not in ARCH_NAMES:
        raise ValueError(f'macOS packaging requires darwin-arm64 or darwin-x64, got {platform}')
    out_dir = args.out_dir.resolve()

    if args.app:
        app = args.app.resolve()
    else:
        app = build_app(args.staged_dir.resolve(), platform, args.version, out_dir / f'macos-{ARCH_NAMES[platform]}')
        run([sys.executable, Path(__file__).resolve().parent / 'sign-macos.py', 'app', app, '--platform', platform])
        print(f'Built app bundle: {app}')
    if not args.skip_dmg:
        dmg = build_dmg(app, platform, args.version, out_dir)
        print(f'Built disk image: {dmg} ({dmg.stat().st_size} bytes)')
    if args.build_input:
        archive = build_input(app, platform, out_dir)
        print(f'Packed build input: {archive} ({archive.stat().st_size} bytes)')


if __name__ == '__main__':
    main()
