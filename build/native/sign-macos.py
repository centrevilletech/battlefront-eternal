#!/usr/bin/env python3
"""Sign, notarize and verify the macOS app bundle and its disk image.

  sign-macos.py app "<folder>/Battlefront Eternal.app" --platform darwin-arm64 [--notarize]
  sign-macos.py dmg Battlefront-Eternal-<version>-macos-arm64.dmg [--notarize]
  sign-macos.py verify <app or dmg> [--notarized]

Nothing secret is in this repository. The identity and the notary credentials
come from the environment (or, by name, from a keychain):

  BFE_MAC_SIGN_IDENTITY  The certificate to sign with, by name ("Developer ID
                         Application: ...") or SHA-1. Unset or "-": an ad-hoc
                         signature, which Apple does not notarize.
  BFE_MAC_KEYCHAIN       Optional: the keychain file that holds the identity.
  BFE_MAC_SIGN_CACHE     Optional folder. A helper program that did not change
                         since an earlier release gets the signed copy made
                         then, so its update blob stays the same.
  BFE_NOTARY_KEY, BFE_NOTARY_KEY_ID, BFE_NOTARY_ISSUER_ID
                         An App Store Connect API key file and its ids; or
  BFE_NOTARY_PROFILE     a notarytool keychain profile.

With a certificate, every program is signed with the hardened runtime and a
secure timestamp and no entitlements: the client needs none (it draws, plays
sound and uses the network; it never records sound and makes no code at run
time). The helper programs are signed first, then resources/bfe-tree.json is
written with their final hashes, then the bundle is sealed.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from stage import build_bundle_tree_index

MACHO_MAGICS = {
    b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe',  # 64-bit and 32-bit, little endian
    b'\xfe\xed\xfa\xcf', b'\xfe\xed\xfa\xce',  # the same, big endian
    b'\xca\xfe\xba\xbe', b'\xca\xfe\xba\xbf',  # universal
}
MH_OBJECT = 1


def run(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def output(args):
    """stdout and stderr of a tool (codesign and spctl report on stderr)."""
    result = subprocess.run([str(arg) for arg in args], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    return result.returncode, result.stdout


def is_signable(path: Path) -> bool:
    """A program, library or loadable module: what codesign signs and the notary service checks."""
    if path.is_symlink() or not path.is_file():
        return False
    with path.open('rb') as handle:
        header = handle.read(16)
    if header[:4] not in MACHO_MAGICS:
        return False
    if header[:4] in (b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe') and int.from_bytes(header[12:16], 'little') == MH_OBJECT:
        return False
    return True


def signable_files(root: Path) -> list[Path]:
    """Deepest first, so a program is signed after anything beside or below it."""
    found = [path for path in root.rglob('*') if is_signable(path)]
    return sorted(found, key=lambda path: (-len(path.parts), str(path)))


def sign_identity() -> str:
    return os.environ.get('BFE_MAC_SIGN_IDENTITY', '').strip() or '-'


def codesign_command(identity: str) -> list[str]:
    command = ['codesign', '--force', '--sign', identity]
    if identity == '-':
        return command + ['--timestamp=none']
    command += ['--timestamp', '--options', 'runtime']
    keychain = os.environ.get('BFE_MAC_KEYCHAIN', '').strip()
    if keychain:
        command += ['--keychain', keychain]
    return command


def file_digest(path: Path, prefix: bytes = b'') -> str:
    digest = hashlib.sha256(prefix)
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def sign_file(path: Path, identity: str, cache: Path | None) -> bool:
    """Sign one program in place. Returns True when an earlier signed copy was reused."""
    command = codesign_command(identity)
    cached = None
    if cache is not None and identity != '-':
        key = file_digest(path, ('\0'.join(command[3:]) + '\0').encode())
        cached = cache / key[:2] / key
        if cached.is_file():
            mode = path.stat().st_mode
            shutil.copyfile(cached, path)
            path.chmod(mode)
            return True
    run(command + [path], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    if cached is not None:
        cached.parent.mkdir(parents=True, exist_ok=True)
        temporary = cached.with_name(f'.{cached.name}.{os.getpid()}')
        shutil.copyfile(path, temporary)
        os.replace(temporary, cached)
    return False


def main_executable(app: Path) -> Path:
    with (app / 'Contents' / 'Info.plist').open('rb') as handle:
        info = plistlib.load(handle)
    return app / 'Contents' / 'MacOS' / info['CFBundleExecutable']


def sign_app(app: Path, platform: str) -> None:
    identity = sign_identity()
    cache = os.environ.get('BFE_MAC_SIGN_CACHE', '').strip()
    cache_dir = Path(cache) if cache else None
    main = main_executable(app)
    # A ticket stapled to an earlier signature does not belong to the new one.
    (app / 'Contents' / 'CodeResources').unlink(missing_ok=True)
    shutil.rmtree(app / 'Contents' / '_CodeSignature', ignore_errors=True)
    helpers = [path for path in signable_files(app) if path != main]
    reused = sum(sign_file(path, identity, cache_dir) for path in helpers)
    build_bundle_tree_index(app, platform)
    run(codesign_command(identity) + [app])
    kind = 'ad hoc' if identity == '-' else f'as {identity}'
    print(f'Signed {app.name} {kind}: {len(helpers)} helper programs ({reused} reused from the cache) and the bundle')
    verify_app(app)


def signature_details(path: Path) -> dict:
    code, text = output(['codesign', '--display', '--verbose=4', path])
    if code != 0:
        raise RuntimeError(f'{path} is not signed: {text.strip()}')
    details = {'authority': [], 'flags': '', 'team': '', 'timestamp': ''}
    for line in text.splitlines():
        if line.startswith('Authority='):
            details['authority'].append(line.split('=', 1)[1])
        elif line.startswith('CodeDirectory') and 'flags=' in line:
            details['flags'] = line.split('flags=', 1)[1].split()[0]
        elif line.startswith('TeamIdentifier='):
            details['team'] = line.split('=', 1)[1]
        elif line.startswith('Timestamp='):
            details['timestamp'] = line.split('=', 1)[1]
    return details


def entitlements(path: Path) -> dict:
    result = subprocess.run(['codesign', '--display', '--entitlements', '-', '--xml', str(path)], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    data = result.stdout.strip()
    return plistlib.loads(data) if data.startswith(b'<?xml') else {}


def verify_app(app: Path, notarized: bool = False) -> None:
    """The signature holds for the bundle and for every program in it; with `notarized`, Gatekeeper
    accepts it as a notarized Developer ID app and the ticket is stapled."""
    code, text = output(['codesign', '--verify', '--deep', '--strict', '--verbose=2', app])
    if code != 0:
        raise RuntimeError(f'codesign does not verify {app}:\n{text}')
    main = main_executable(app)
    programs = signable_files(app)
    for path in programs:
        code, text = output(['codesign', '--verify', '--strict', path])
        if code != 0:
            raise RuntimeError(f'codesign does not verify {path.relative_to(app)}: {text.strip()}')
    for path in programs:
        granted = entitlements(app if path == main else path)
        if granted:
            raise RuntimeError(f'{path.relative_to(app)} carries entitlements it should not need: {sorted(granted)}')
    if not notarized:
        print(f'[OK] {app.name}: signature verifies ({len(programs)} programs, no entitlements)')
        return
    bundle = signature_details(app)
    if not any(name.startswith('Developer ID Application:') for name in bundle['authority']) or not bundle['team']:
        raise RuntimeError(f'{app.name} is not signed with a Developer ID Application certificate: {bundle}')
    for path in programs:
        details = signature_details(app if path == main else path)
        if 'runtime' not in details['flags'] or details['team'] != bundle['team'] or not details['timestamp']:
            raise RuntimeError(f'{path.relative_to(app)} lacks the hardened runtime, the team or a secure timestamp: {details}')
    code, text = output(['spctl', '--assess', '--type', 'execute', '--verbose=2', app])
    if code != 0 or 'source=Notarized Developer ID' not in text:
        raise RuntimeError(f'Gatekeeper does not accept {app.name} as notarized:\n{text}')
    run(['xcrun', 'stapler', 'validate', app], stdout=subprocess.DEVNULL)
    print(f"[OK] {app.name}: {bundle['authority'][0]}, hardened runtime, notarized, ticket stapled ({len(programs)} programs)")


def verify_dmg(dmg: Path, notarized: bool = False) -> None:
    run(['hdiutil', 'verify', '-quiet', dmg])
    if not notarized:
        print(f'[OK] {dmg.name}: image verifies')
        return
    code, text = output(['codesign', '--verify', '--strict', '--verbose=2', dmg])
    if code != 0:
        raise RuntimeError(f'codesign does not verify {dmg}:\n{text}')
    code, text = output(['spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature', '--verbose=2', dmg])
    if code != 0 or 'source=Notarized Developer ID' not in text:
        raise RuntimeError(f'Gatekeeper does not accept {dmg.name} as notarized:\n{text}')
    run(['xcrun', 'stapler', 'validate', dmg], stdout=subprocess.DEVNULL)
    print(f'[OK] {dmg.name}: signed, notarized, ticket stapled')


def notary_credentials() -> list[str]:
    profile = os.environ.get('BFE_NOTARY_PROFILE', '').strip()
    if profile:
        keychain = os.environ.get('BFE_MAC_KEYCHAIN', '').strip()
        return ['--keychain-profile', profile] + (['--keychain', keychain] if keychain else [])
    try:
        return ['--key', os.environ['BFE_NOTARY_KEY'], '--key-id', os.environ['BFE_NOTARY_KEY_ID'],
                '--issuer', os.environ['BFE_NOTARY_ISSUER_ID']]
    except KeyError as missing:
        raise RuntimeError(f'Notarization needs BFE_NOTARY_PROFILE, or BFE_NOTARY_KEY, BFE_NOTARY_KEY_ID and '
                           f'BFE_NOTARY_ISSUER_ID (missing {missing})') from None


def notarize(path: Path, report: Path | None = None) -> dict:
    """Send an app (zipped) or a disk image to Apple's notary service, wait for the verdict, staple the ticket."""
    credentials = notary_credentials()
    with tempfile.TemporaryDirectory(prefix='bfe-notary-') as work:
        upload = path
        if path.is_dir():
            upload = Path(work) / (path.stem + '.zip')
            run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', path, upload])
        print(f'Sending {upload.name} ({upload.stat().st_size / 1e6:.0f} MB) to the notary service', flush=True)
        result = subprocess.run(['xcrun', 'notarytool', 'submit', str(upload), *credentials, '--wait', '--timeout', '2h',
                                 '--output-format', 'json'], stdout=subprocess.PIPE, text=True)
    try:
        verdict = json.loads(result.stdout)
    except ValueError:
        raise RuntimeError(f'notarytool gave no verdict (exit {result.returncode}): {result.stdout.strip()}') from None
    summary = {'path': path.name, 'id': verdict.get('id'), 'status': verdict.get('status')}
    if report is not None:
        report.write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    if verdict.get('status') != 'Accepted':
        log = subprocess.run(['xcrun', 'notarytool', 'log', str(verdict.get('id')), *credentials], stdout=subprocess.PIPE, text=True).stdout
        raise RuntimeError(f"The notary service answered {verdict.get('status')} for {path.name} ({verdict.get('id')}):\n{log}")
    print(f"Notarized {path.name}: submission {verdict.get('id')}", flush=True)
    # The ticket can take a moment to reach the servers stapler asks.
    for attempt in range(6):
        if subprocess.run(['xcrun', 'stapler', 'staple', str(path)], stdout=subprocess.DEVNULL).returncode == 0:
            break
        if attempt == 5:
            raise RuntimeError(f'Could not staple the ticket to {path.name}')
        time.sleep(20)
    return summary


def sign_dmg(dmg: Path) -> None:
    identity = sign_identity()
    if identity == '-':
        print(f'{dmg.name}: left unsigned (no certificate)')
        return
    command = ['codesign', '--force', '--sign', identity, '--timestamp']
    keychain = os.environ.get('BFE_MAC_KEYCHAIN', '').strip()
    if keychain:
        command += ['--keychain', keychain]
    run(command + [dmg])
    print(f'Signed {dmg.name} as {identity}')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    app_parser = commands.add_parser('app', help='Sign every program of the app bundle, then the bundle')
    app_parser.add_argument('path', type=Path)
    app_parser.add_argument('--platform', required=True, help='darwin-arm64 or darwin-x64')
    app_parser.add_argument('--notarize', action='store_true', help='Also notarize the app and staple its ticket')
    app_parser.add_argument('--report', type=Path, help='Write the notary submission id and verdict here')
    dmg_parser = commands.add_parser('dmg', help='Sign the disk image')
    dmg_parser.add_argument('path', type=Path)
    dmg_parser.add_argument('--notarize', action='store_true', help='Also notarize the image and staple its ticket')
    dmg_parser.add_argument('--report', type=Path, help='Write the notary submission id and verdict here')
    verify_parser = commands.add_parser('verify', help='Check an app bundle or a disk image')
    verify_parser.add_argument('path', type=Path)
    verify_parser.add_argument('--notarized', action='store_true', help='Require a notarized Developer ID signature with a stapled ticket')
    args = parser.parse_args()

    if sys.platform != 'darwin':
        raise SystemExit('sign-macos.py runs on macOS (it drives codesign, notarytool and stapler)')
    path = args.path.resolve()
    if args.command == 'app':
        sign_app(path, args.platform)
        if args.notarize:
            notarize(path, args.report)
            verify_app(path, notarized=True)
    elif args.command == 'dmg':
        sign_dmg(path)
        if args.notarize:
            notarize(path, args.report)
            verify_dmg(path, notarized=True)
    elif path.is_dir():
        verify_app(path, notarized=args.notarized)
    else:
        verify_dmg(path, notarized=args.notarized)


if __name__ == '__main__':
    main()
