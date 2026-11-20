"""Exercise real Debian metadata, signatures and atomic download publication."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('distribution', Path(__file__).parents[1] / 'build/publish-distribution.py')
distribution = importlib.util.module_from_spec(spec)
spec.loader.exec_module(distribution)

def server_archive(source, arch, entries=None, version='0.0.0', mode=0o755):
    """A dedicated server archive as build/native/server-archive.py lays it out."""
    import io
    import tarfile
    path = source / f'battlefront-eternal-server_{version}_linux-{arch}.tar.xz'
    with tarfile.open(path, 'w:xz') as archive:
        for name in entries or ['bfeserver', 'tools/prepare.py', 'tools/content-version.json',
                                'toolchain/python/bin/python3', 'legal/LICENSE', 'legal/THIRD-PARTY-NOTICES.md', 'bfe-tree.json']:
            data = f'{arch} {name}'.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode if name == 'bfeserver' else 0o644
            archive.addfile(info, io.BytesIO(data))
        link = tarfile.TarInfo('toolchain/python/bin/python')
        link.type = tarfile.SYMTYPE
        link.linkname = 'python3'
        archive.addfile(link)
    return path

# The publisher and the Linux packagers drive the release host's own tools.
RELEASE_TOOLS = unittest.skipUnless(shutil.which('gpg') and shutil.which('dpkg-deb'), 'needs gpg and dpkg-deb (Linux release tools)')


class DistributionTest(unittest.TestCase):
    @RELEASE_TOOLS
    def test_signed_repository_and_missing_asset_preserves_latest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            keyhome = root / 'keys'
            keyhome.mkdir(mode=0o700)
            subprocess.run(['gpg', '--homedir', str(keyhome), '--batch', '--passphrase', '',
                            '--quick-generate-key', 'Distribution test <test@example.invalid>', 'rsa2048', 'sign', '1d'],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            listing = subprocess.check_output(['gpg','--homedir',str(keyhome),'--with-colons','--list-keys'], text=True)
            distribution.KEY = next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
            distribution.KEYHOME = str(keyhome)
            distribution.ROOT = root / 'public'
            source = root / 'input'
            source.mkdir()
            for name in ['test-win-x64.exe','test-linux-x86_64.AppImage','test-linux-arm64.AppImage']:
                (source / name).write_text('fixture')
            (source / 'compatibility.json').write_text('{"fixture":true}')
            for arch in ['amd64', 'arm64']:
                server_archive(source, arch)
                control = root / arch / 'DEBIAN'
                control.mkdir(parents=True)
                control.chmod(0o755)
                (control / 'control').write_text(f'Package: battlefront-eternal\nVersion: 0.1.0~preview.1\nArchitecture: {arch}\nMaintainer: Test <test@example.invalid>\nDescription: fixture\n')
                subprocess.run(['dpkg-deb','--build','--root-owner-group',str(control.parent),str(source / f'{arch}.deb')],check=True,stdout=subprocess.DEVNULL)
            distribution.publish(source,'0.1.0~preview.1','preview-1','a'*40,'2026-10-15')
            public = distribution.ROOT
            sums = public / 'downloads/latest/SHA256SUMS'
            subprocess.run(['gpg','--homedir',str(keyhome),'--verify',str(sums)+'.asc',str(sums)],check=True,capture_output=True)
            subprocess.run(['sha256sum','--check',str(sums)],cwd=sums.parent,check=True,capture_output=True)
            # The dedicated server archives, named by the package version (docs/server-updates.md).
            for arch in ['amd64', 'arm64']:
                self.assertIn(f'  battlefront-eternal-server_0.1.0~preview.1_linux-{arch}.tar.xz\n', sums.read_text())
                self.assertEqual((public / f'downloads/latest/battlefront-eternal-server_0.1.0~preview.1_linux-{arch}.tar.xz').read_bytes(),
                                 (source / f'battlefront-eternal-server_0.0.0_linux-{arch}.tar.xz').read_bytes())
            release = public / 'apt/dists/stable/InRelease'
            subprocess.run(['gpg','--homedir',str(keyhome),'--verify',str(release)],check=True,capture_output=True)
            for arch in ['amd64','arm64']:
                directory = public / f'apt/dists/stable/main/binary-{arch}'
                data = (directory / 'Packages').read_bytes()
                digest = hashlib.sha256(data).hexdigest()
                self.assertEqual((directory / 'by-hash/SHA256' / digest).read_bytes(),data)
                self.assertIn(b'Version: 0.1.0~preview.1',data)
            old = os.readlink(public / 'downloads/latest')
            (source / 'test-win-x64.exe').unlink()
            with self.assertRaises(ValueError):
                distribution.publish(source,'0.1.0~preview.2','preview-2','b'*40,'2026-10-16')
            self.assertEqual(os.readlink(public / 'downloads/latest'),old)
            # A server archive reaching outside its layout, lacking the server, or
            # missing for an architecture is refused the same way.
            (source / 'test-win-x64.exe').write_text('fixture')
            for entries in [['opt/evil', 'bfeserver', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json'],
                            ['tools/../escape', 'bfeserver', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json'],
                            ['tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json'],
                            ['bfeserver', 'bfe-client', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'legal/THIRD-PARTY-NOTICES.md', 'bfe-tree.json']]:
                server_archive(source, 'arm64', entries)
                with self.assertRaises(ValueError):
                    distribution.publish(source,'0.1.0~preview.2','preview-2','b'*40,'2026-10-16')
                self.assertEqual(os.readlink(public / 'downloads/latest'),old)
            # A native server archive whose bfeserver binary is not executable is refused.
            server_archive(source, 'arm64', ['bfeserver', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'legal/THIRD-PARTY-NOTICES.md', 'bfe-tree.json'], mode=0o644)
            with self.assertRaises(ValueError):
                distribution.publish(source,'0.1.0~preview.2','preview-2','b'*40,'2026-10-16')
            self.assertEqual(os.readlink(public / 'downloads/latest'),old)
            (source / 'battlefront-eternal-server_0.0.0_linux-arm64.tar.xz').unlink()
            with self.assertRaises(ValueError):
                distribution.publish(source,'0.1.0~preview.2','preview-2','b'*40,'2026-10-16')
            self.assertEqual(os.readlink(public / 'downloads/latest'),old)
            server_archive(source, 'arm64')
            self.assertFalse(list(public.glob('downloads/.release-*')), 'a refused release leaves no stage behind')
            # A failed input cannot replace latest; a later successful release
            # ages out complete generations only after its catalogue is signed.
            from unittest.mock import patch
            with patch.dict(os.environ, {'BFE_KEEP_RELEASES': '2'}):
                for number in [2, 3]:
                    for arch in ['amd64','arm64']:
                        control = root / arch / 'DEBIAN/control'
                        control.write_text(control.read_text().replace(f'preview.{number-1}',f'preview.{number}'))
                        subprocess.run(['dpkg-deb','--build','--root-owner-group',str(control.parent.parent),str(source / f'{arch}.deb')],check=True,stdout=subprocess.DEVNULL)
                    distribution.publish(source,f'0.1.0~preview.{number}',f'preview-{number}','c'*40,'2026-10-16')
            self.assertFalse((public / 'downloads/preview-1').exists())
            self.assertTrue((public / 'downloads/preview-2').exists())
            self.assertEqual(os.readlink(public / 'downloads/latest'),'preview-3')
            packages = (public / 'apt/dists/stable/main/binary-amd64/Packages').read_text()
            self.assertNotIn('Version: 0.1.0~preview.1',packages)
            self.assertIn('Version: 0.1.0~preview.3',packages)

    @RELEASE_TOOLS
    def test_signed_update_channel_store_and_collection(self):
        """Payloads from build/native/update-payload.py become a signed channel
        the client's update verifier accepts; blobs are checked by content,
        shared across releases and collected with them."""
        repo = Path(__file__).parents[1]
        stage_spec = importlib.util.spec_from_file_location('stage', repo / 'build/native/stage.py')
        stage_mod = importlib.util.module_from_spec(stage_spec)
        stage_spec.loader.exec_module(stage_mod)
        payload_spec = importlib.util.spec_from_file_location('update_payload', repo / 'build/native/update-payload.py')
        payload_mod = importlib.util.module_from_spec(payload_spec)
        payload_spec.loader.exec_module(payload_mod)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            keyhome = root / 'keys'
            keyhome.mkdir(mode=0o700)
            subprocess.run(['gpg', '--homedir', str(keyhome), '--batch', '--passphrase', '',
                            '--quick-generate-key', 'Distribution test <test@example.invalid>', 'rsa2048', 'sign', '1d'],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            listing = subprocess.check_output(['gpg', '--homedir', str(keyhome), '--with-colons', '--list-keys'], text=True)
            distribution.KEY = next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
            distribution.KEYHOME = str(keyhome)
            distribution.ROOT = root / 'public'
            distribution.UPDATE_KEY = str(root / 'update.pem')
            subprocess.run(['openssl', 'genpkey', '-algorithm', 'ed25519', '-out', distribution.UPDATE_KEY], check=True)
            spki = subprocess.check_output(['openssl', 'pkey', '-in', distribution.UPDATE_KEY, '-pubout', '-outform', 'DER'])
            pub_pem = root / 'update-pub.pem'
            pub_pem.write_bytes(subprocess.check_output(['openssl', 'pkey', '-in', distribution.UPDATE_KEY, '-pubout']))
            expected_key_id = hashlib.sha256(spki).hexdigest()[:16]
            source = root / 'input'
            source.mkdir()

            def build(number, payload):
                for name in ['test-win-x64.exe', 'test-linux-x86_64.AppImage', 'test-linux-arm64.AppImage']:
                    (source / name).write_text('fixture')
                (source / 'compatibility.json').write_text('{"fixture":true}')
                for arch in ['amd64', 'arm64']:
                    server_archive(source, arch)
                    control = root / f'deb-{arch}' / 'DEBIAN'
                    control.mkdir(parents=True, exist_ok=True)
                    control.chmod(0o755)
                    (control / 'control').write_text(f'Package: battlefront-eternal\nVersion: 0.1.0~preview.{number}\nArchitecture: {arch}\nMaintainer: Test <test@example.invalid>\nDescription: fixture\n')
                    subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(control.parent), str(source / f'{arch}.deb')], check=True, stdout=subprocess.DEVNULL)
                for platform, exe in [
                    ('linux-x64', 'bfe-client'),
                    ('linux-arm64', 'bfe-client'),
                    ('win32-x64', 'bfe-client.exe'),
                ]:
                    tree_dir = source / f'tree-{platform}'
                    shutil.rmtree(tree_dir, ignore_errors=True)
                    (tree_dir / 'resources/tools').mkdir(parents=True, exist_ok=True)
                    (tree_dir / exe).write_text(f'runtime {platform}')
                    (tree_dir / exe).chmod(0o755)
                    (tree_dir / 'resources/payload.txt').write_text(f'app {payload}')
                    (tree_dir / 'resources/tools/content-version.json').write_text('{"state":2,"maps":8,"sides":2}')
                    stage_mod.build_tree_index(tree_dir, platform)
                    payload_mod.build_update_payload(
                        staged_dir=tree_dir,
                        platform=platform,
                        version=f'0.1.0-preview.{number}',
                        out_path=source / f'update-{platform}.tar',
                    )

            def channel():
                envelope = json.loads((distribution.ROOT / 'downloads/updates/development-preview.json').read_text())
                self.assertEqual(envelope['key'], expected_key_id)
                payload_bytes = base64.b64decode(envelope['payload'])
                sig_bytes = base64.b64decode(envelope['signature'])
                with tempfile.TemporaryDirectory() as verify_dir:
                    vpath = Path(verify_dir)
                    (vpath / 'payload').write_bytes(payload_bytes)
                    (vpath / 'sig').write_bytes(sig_bytes)
                    subprocess.run([
                        'openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(pub_pem),
                        '-rawin', '-in', str(vpath / 'payload'), '-sigfile', str(vpath / 'sig'),
                    ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                return json.loads(payload_bytes)

            from unittest.mock import patch
            downloads = root / 'public/downloads'
            store = downloads / 'updates/blobs'
            build(1, 'one')
            distribution.publish(source, '0.1.0~preview.1', 'preview-1', 'a' * 40, '2026-10-17')
            release = channel()
            self.assertEqual(release['version'], '0.1.0-preview.1')
            self.assertEqual(release['content'], {'state': 2, 'maps': 8, 'sides': 2})
            self.assertEqual(sorted(release['platforms']), ['linux-arm64', 'linux-x64', 'win32-x64'])
            for platform, entry in release['platforms'].items():
                manifest = downloads / entry['manifest']
                self.assertEqual(hashlib.sha256(manifest.read_bytes()).hexdigest(), entry['sha256'])
                tree = json.loads(subprocess.check_output(['zstd', '-dcq', str(manifest)]))
                for item in tree['files']:
                    blob = store / item['sha256'][:2] / f"{item['sha256']}.zst"
                    self.assertEqual(blob.stat().st_size, item['zsize'])
            first = set(json.loads((downloads / 'updates/blobs.json').read_text())['blobs'])

            # A blob that does not hold what its name says stops the release.
            build(2, 'two')
            tar = source / 'update-linux-x64.tar'
            data = bytearray(tar.read_bytes())
            payload_digest = hashlib.sha256(b'app two').hexdigest()
            marker = data.find(f'blobs/{payload_digest}.zst'.encode())
            self.assertGreaterEqual(marker, 0)
            body = marker + 512
            data[body + 10] ^= 0xff
            tar.write_bytes(bytes(data))
            with self.assertRaises(ValueError):
                distribution.publish(source, '0.1.0~preview.2', 'preview-2', 'b' * 40, '2026-10-17')
            self.assertEqual(channel()['version'], '0.1.0-preview.1')
            self.assertEqual(os.readlink(downloads / 'latest'), 'preview-1')

            # Releases share unchanged blobs; ones only an aged-out release
            # listed are deleted with it.
            with patch.dict(os.environ, {'BFE_KEEP_RELEASES': '2'}):
                for number, payload in [(3, 'three'), (4, 'four')]:
                    build(number, payload)
                    distribution.publish(source, f'0.1.0~preview.{number}', f'preview-{number}', 'c' * 40, '2026-10-17')
            self.assertEqual(channel()['version'], '0.1.0-preview.4')
            held = set(json.loads((downloads / 'updates/blobs.json').read_text())['blobs'])
            self.assertTrue(held & first, 'the runtimes did not change and are shared')
            only_first = first - held
            self.assertTrue(only_first, "preview 1's own payload blobs are gone")
            for digest in only_first:
                self.assertFalse((store / digest[:2] / f'{digest}.zst').exists())
            self.assertEqual(len(list(store.glob('*/*.zst'))), len(held))

            # A subsequent native release using known_blobs_path publishes into
            # the same channel cleanly.
            for arch in ['amd64', 'arm64']:
                server_archive(
                    source,
                    arch,
                    entries=['bfeserver', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'legal/THIRD-PARTY-NOTICES.md', 'bfe-tree.json'],
                    mode=0o755,
                )
                control = root / f'deb-{arch}' / 'DEBIAN/control'
                control.write_text(control.read_text().replace('preview.4', 'preview.5'))
                subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(control.parent.parent), str(source / f'{arch}.deb')], check=True, stdout=subprocess.DEVNULL)

            for platform, exe, srv in [
                ('linux-x64', 'bfe-client', 'bfeserver'),
                ('linux-arm64', 'bfe-client', 'bfeserver'),
                ('win32-x64', 'bfe-client.exe', 'bfeserver.exe'),
            ]:
                tree_dir = source / f'native-tree-{platform}'
                (tree_dir / 'resources/tools').mkdir(parents=True, exist_ok=True)
                (tree_dir / exe).write_text(f'native client {platform}')
                (tree_dir / exe).chmod(0o755)
                (tree_dir / srv).write_text(f'native server {platform}')
                (tree_dir / srv).chmod(0o755)
                if not platform.startswith('win32-'):
                    (tree_dir / 'battlefront-eternal').symlink_to('bfe-client')
                (tree_dir / 'resources/tools/content-version.json').write_text('{"state":2,"maps":8,"sides":2}')
                stage_mod.build_tree_index(tree_dir, platform)
                payload_mod.build_update_payload(
                    staged_dir=tree_dir,
                    platform=platform,
                    version='0.1.0-preview.5',
                    out_path=source / f'update-{platform}.tar',
                    known_blobs_path=downloads / 'updates/blobs.json',
                )

            distribution.publish(source, '0.1.0~preview.5', 'preview-5', 'd' * 40, '2026-10-18')
            native_release = channel()
            self.assertEqual(native_release['version'], '0.1.0-preview.5')
            self.assertEqual(native_release['platforms']['linux-x64']['executable'], 'bfe-client')
            self.assertEqual(native_release['platforms']['linux-x64']['server_executable'], 'bfeserver')
            self.assertEqual(native_release['platforms']['win32-x64']['executable'], 'bfe-client.exe')
            self.assertEqual(native_release['platforms']['win32-x64']['server_executable'], 'bfeserver.exe')

            # The Mac build is signed and notarized after the release is live and joins it then. Its record is
            # signed by a key that only the signing Mac holds; the release host has the public half.
            app_name = 'Battlefront Eternal.app'
            mac = root / 'mac'
            record_keys = {}
            for owner in ('signing-mac', 'someone-else'):
                record_keys[owner] = root / f'{owner}-record.pem'
                subprocess.run(['openssl', 'genpkey', '-algorithm', 'ed25519', '-out', str(record_keys[owner])], check=True)
            distribution.MAC_RECORD_KEY = root / 'macos-record.pub.pem'
            distribution.MAC_RECORD_KEY.write_bytes(subprocess.check_output(['openssl', 'pkey', '-in', str(record_keys['signing-mac']), '-pubout']))

            def sign_record(owner='signing-mac'):
                raw = subprocess.check_output(['openssl', 'pkeyutl', '-sign', '-inkey', str(record_keys[owner]), '-rawin', '-in', str(mac / 'macos-notarized.json')])
                (mac / 'macos-notarized.json.sig').write_text(base64.b64encode(raw).decode() + '\n')

            def mac_build(number, stapled=True, revision='d' * 40, arch='arm64', signer='signing-mac'):
                shutil.rmtree(mac, ignore_errors=True)
                contents = mac / 'tree' / app_name / 'Contents'
                (contents / 'MacOS').mkdir(parents=True)
                (contents / 'Resources/tools').mkdir(parents=True)
                (contents / '_CodeSignature').mkdir()
                for name in ('bfe-client', 'bfeserver'):
                    (contents / 'MacOS' / name).write_text(f'mac {name} {number}')
                    (contents / 'MacOS' / name).chmod(0o755)
                (contents / 'MacOS/resources').symlink_to('../Resources')
                (contents / 'Info.plist').write_text('<plist/>')
                (contents / '_CodeSignature/CodeResources').write_text(f'seal {number}')
                if stapled:
                    (contents / 'CodeResources').write_text(f'ticket {number}')
                (contents / 'Resources/tools/content-version.json').write_text('{"state":2,"maps":8,"sides":2}')
                (contents / 'Resources/bfe-tree.json').write_text('{}')
                payload_mod.build_update_payload(staged_dir=mac / 'tree', platform='darwin-arm64', version=f'0.1.0-preview.{number}',
                                                 out_path=mac / 'update-darwin-arm64.tar', known_blobs_path=downloads / 'updates/blobs.json')
                image = mac / f'Battlefront-Eternal-0.1.0-preview.{number}-macos-arm64.dmg'
                image.write_text(f'disk image {number}')
                described = lambda path: {'name': path.name, 'size': path.stat().st_size, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
                record = {'schema': 1, 'release_id': f'preview-{number}', 'version': f'0.1.0-preview.{number}', 'revision': revision,
                          'notarized': True, 'dmg': {**described(image), 'arch': arch},
                          'updates': {'darwin-arm64': described(mac / 'update-darwin-arm64.tar')}}
                (mac / 'macos-notarized.json').write_text(json.dumps(record))
                if signer:
                    sign_record(signer)
                return record

            def refused(why):
                """The Mac step refuses, and leaves nothing behind: no blob, no file of the release, no staging folder.
                Its check alone (what the release host runs before it attaches the files to the GitHub release)
                refuses the same."""
                blobs = sorted(path.name for path in store.glob('*/*.zst'))
                with self.assertRaises(ValueError, msg=why):
                    distribution.publish_macos(mac, check=True)
                with self.assertRaises(ValueError, msg=why):
                    distribution.publish_macos(mac)
                self.assertEqual(sorted(path.name for path in store.glob('*/*.zst')), blobs, f'{why}: a refused Mac build left blobs in the store')
                self.assertFalse(list(downloads.glob('.macos-*')) + list((downloads / 'updates').glob('.incoming-*')), why)
                self.assertFalse((downloads / 'preview-5/macos.json').exists(), why)
                self.assertFalse(list((downloads / 'preview-5').glob('*.dmg')), why)
                self.assertNotIn('darwin-arm64', channel()['platforms'], why)

            sums_before = (downloads / 'preview-5/SHA256SUMS').read_bytes()
            mac_build(4)
            refused('not the live release')
            mac_build(5, stapled=False)
            refused('made from an app without its notarization ticket')
            mac_build(5, revision='f' * 40)
            refused('built from another commit than the live release')
            # Whoever can write where the Mac files are handed over can write a record, but cannot sign one.
            mac_build(5, signer=None)
            refused('a record without a signature')
            mac_build(5, signer='someone-else')
            refused('a record signed with another key')
            mac_build(5)
            forged = json.loads((mac / 'macos-notarized.json').read_text())
            (mac / forged['dmg']['name']).write_text('another image')
            forged['dmg'].update(size=len('another image'), sha256=hashlib.sha256(b'another image').hexdigest())
            (mac / 'macos-notarized.json').write_text(json.dumps(forged))
            refused('a record changed after it was signed, consistent in itself')
            mac_build(5, arch='x64')
            refused('an Intel image with the Apple silicon update')
            record = mac_build(5)
            (mac / record['dmg']['name']).write_text('another image')
            refused('not the file the record describes')

            # The check alone accepts the build and writes nothing at all.
            mac_build(5)
            before = sorted(str(path.relative_to(downloads)) for path in downloads.rglob('*'))
            channel_before = (downloads / 'updates/development-preview.json').read_bytes()
            distribution.publish_macos(mac, check=True)
            self.assertEqual(sorted(str(path.relative_to(downloads)) for path in downloads.rglob('*')), before)
            self.assertEqual((downloads / 'updates/development-preview.json').read_bytes(), channel_before)

            # A step that stops before its last file lands leaves a release without a Mac build, and runs again.
            from unittest.mock import patch as patch_object
            with patch_object.object(distribution, 'sign_update', side_effect=RuntimeError('the update key is unreadable')):
                with self.assertRaises(RuntimeError):
                    distribution.publish_macos(mac)
            self.assertFalse((downloads / 'preview-5/macos.json').exists(), 'a stopped step must not look finished')
            self.assertNotIn('darwin-arm64', channel()['platforms'])
            distribution.publish_macos(mac)
            with_mac = channel()
            self.assertEqual(sorted(with_mac['platforms']), ['darwin-arm64', 'linux-arm64', 'linux-x64', 'win32-x64'])
            self.assertEqual(with_mac['platforms']['linux-x64'], native_release['platforms']['linux-x64'])
            entry = with_mac['platforms']['darwin-arm64']
            self.assertEqual(entry['executable'], f'{app_name}/Contents/MacOS/bfe-client')
            mac_tree = json.loads(subprocess.check_output(['zstd', '-dcq', str(downloads / entry['manifest'])]))
            self.assertIn({'path': f'{app_name}/Contents/MacOS/resources', 'target': '../Resources'}, mac_tree['links'])
            for item in mac_tree['files']:
                self.assertTrue((store / item['sha256'][:2] / f"{item['sha256']}.zst").is_file(), item['path'])
            self.assertEqual((downloads / 'latest-macos/Battlefront-Eternal-macos-arm64.dmg').read_text(), 'disk image 5')
            self.assertEqual(json.loads((downloads / 'preview-5/macos.json').read_text())['platforms'], ['darwin-arm64'])
            # What the release already had is untouched; the Mac files have signed checksums of their own.
            self.assertEqual((downloads / 'preview-5/SHA256SUMS').read_bytes(), sums_before)
            mac_sums = downloads / 'preview-5/SHA256SUMS-macos'
            subprocess.run(['gpg', '--homedir', str(keyhome), '--verify', str(mac_sums) + '.asc', str(mac_sums)], check=True, capture_output=True)
            subprocess.run(['sha256sum', '--check', str(mac_sums)], cwd=mac_sums.parent, check=True, capture_output=True)
            self.assertIn('  macos.json\n', mac_sums.read_text())
            with self.assertRaises(ValueError):
                distribution.publish_macos(mac)  # a release takes its Mac build once
            self.assertFalse(list(downloads.glob('.macos-*')), 'the Mac step leaves no stage behind')

            # An update manifest that names a path twice is refused.
            twice = json.loads(json.dumps(mac_tree))
            twice['files'].append(dict(twice['files'][0]))
            with self.assertRaises(ValueError):
                distribution.check_tree(twice, 'darwin-arm64', '0.1.0-preview.5')

            # Later releases go live without a Mac build: the channel then has no macOS entry, and the
            # Mac download and its update blobs stay until a newer Mac build replaces them.
            with patch.dict(os.environ, {'BFE_KEEP_RELEASES': '2'}):
                for number in (6, 7, 8):
                    for arch in ['amd64', 'arm64']:
                        control = root / f'deb-{arch}' / 'DEBIAN/control'
                        control.write_text(control.read_text().replace(f'preview.{number - 1}', f'preview.{number}'))
                        subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(control.parent.parent), str(source / f'{arch}.deb')], check=True, stdout=subprocess.DEVNULL)
                    for platform in ('linux-x64', 'linux-arm64', 'win32-x64'):
                        payload_mod.build_update_payload(staged_dir=source / f'native-tree-{platform}', platform=platform,
                                                         version=f'0.1.0-preview.{number}', out_path=source / f'update-{platform}.tar')
                    distribution.publish(source, f'0.1.0~preview.{number}', f'preview-{number}', 'e' * 40, '2026-10-19')
            self.assertEqual(sorted(channel()['platforms']), ['linux-arm64', 'linux-x64', 'win32-x64'])
            self.assertFalse((downloads / 'preview-6').exists())
            self.assertEqual(os.readlink(downloads / 'latest-macos'), 'preview-5')
            self.assertEqual((downloads / 'latest-macos/Battlefront-Eternal-macos-arm64.dmg').read_text(), 'disk image 5')
            for item in mac_tree['files']:
                self.assertTrue((store / item['sha256'][:2] / f"{item['sha256']}.zst").is_file(), item['path'])
            self.assertFalse(list((root / 'public/apt/pool').rglob('*preview.5*')), "an aged-out release's packages leave the catalogue")
            mac_build(8, revision='e' * 40)
            distribution.publish_macos(mac)
            self.assertEqual(os.readlink(downloads / 'latest-macos'), 'preview-8')
            self.assertIn('darwin-arm64', channel()['platforms'])

    def test_appimage_rejects_bundled_host_display_libraries(self):
        repo = Path(__file__).parents[1]
        spec = importlib.util.spec_from_file_location('check_packages', repo / 'build/native/check-packages.py')
        checks = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(checks)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            nested = root / 'resources' / 'lib'
            nested.mkdir(parents=True)
            (nested / 'libpython3.so').write_bytes(b'allowed runtime')
            checks.check_host_display_libraries(root)
            # Cover both ABI peers and arbitrary SONAME versions on either arch.
            for name in ('libwayland-client.so.0', 'libwayland-cursor.so.0',
                         'libwayland-egl.so.1', 'libxkbcommon.so.0',
                         'libxkbcommon-x11.so.0', 'libffi.so.8', 'libffi.so.9'):
                with self.subTest(name=name):
                    library = nested / name
                    library.write_bytes(b'incompatible host ABI')
                    with self.assertRaisesRegex(RuntimeError, 'must use host display library'):
                        checks.check_host_display_libraries(root)
                    library.unlink()
            link = root / 'libxkbcommon.so'
            link.symlink_to('absent-display-library.so')
            with self.assertRaisesRegex(RuntimeError, 'must use host display library'):
                checks.check_host_display_libraries(root)

    @RELEASE_TOOLS
    def test_native_packaging_and_package_checks_end_to_end(self):
        """Exercise build/native/{stage,package-linux,server-archive,update-payload,check-packages}.py."""
        repo = Path(__file__).parents[1]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bin_dir = root / 'bin'
            bin_dir.mkdir()
            for name in ('bfe-client', 'bfeserver'):
                exe = bin_dir / name
                exe.write_text('#!/bin/sh\necho "Usage: ' + name + ' --help"\nexit 0\n')
                exe.chmod(0o755)

            tc = root / 'toolchain/linux-x64'
            (tc / 'python/bin').mkdir(parents=True)
            shutil.copy2(sys.executable, tc / 'python/bin/python3')
            (tc / 'python/bin/python').symlink_to('python3')
            (tc / 'ffmpeg').mkdir(parents=True)
            (tc / 'ffmpeg/ffmpeg').write_text('#!/bin/sh\nexit 0\n')
            (tc / 'ffmpeg/ffmpeg').chmod(0o755)

            staged = root / 'linux-unpacked'
            packages = root / 'packages'
            subprocess.run([
                sys.executable, str(repo / 'build/native/stage.py'),
                '--platform', 'linux-x64',
                '--client-bin', str(bin_dir / 'bfe-client'),
                '--server-bin', str(bin_dir / 'bfeserver'),
                '--toolchain-dir', str(tc),
                '--out-dir', str(staged),
            ], check=True, stdout=subprocess.DEVNULL)
            subprocess.run([
                sys.executable, str(repo / 'build/native/package-linux.py'),
                '--staged-dir', str(staged),
                '--platform', 'linux-x64',
                '--version', '0.1.0-preview.10',
                '--out-dir', str(packages),
                '--skip-appimage',
            ], check=True, stdout=subprocess.DEVNULL)
            subprocess.run([
                sys.executable, str(repo / 'build/native/server-archive.py'),
                '--staged-dir', str(staged),
                '--platform', 'linux-x64',
                '--version', '0.1.0-preview.10',
                '--out-dir', str(packages),
            ], check=True, stdout=subprocess.DEVNULL)
            subprocess.run([
                sys.executable, str(repo / 'build/native/update-payload.py'),
                '--tree', str(staged),
                '--platform', 'linux-x64',
                '--version', '0.1.0-preview.10',
                '--out', str(packages / 'update-linux-x64.tar'),
            ], check=True, stdout=subprocess.DEVNULL)
            report = packages / 'size-report.md'
            subprocess.run([
                sys.executable, str(repo / 'build/native/check-packages.py'),
                '--platform', 'linux-x64',
                '--staged-dir', str(staged),
                '--deb', str(packages / 'Battlefront-Eternal-0.1.0-preview.10-linux-amd64.deb'),
                '--server-archive', str(packages / 'battlefront-eternal-server_0.1.0-preview.10_linux-amd64.tar.xz'),
                '--update-payload', str(packages / 'update-linux-x64.tar'),
                '--report-out', str(report),
            ], check=True, stdout=subprocess.DEVNULL)
            self.assertIn('Battlefront-Eternal-0.1.0-preview.10-linux-amd64.deb', report.read_text())
            # Servers set up before bfeserver update through resources/host-runtime: the launcher,
            # bfeserver beside it, the checks their updater asks for and that updater.
            runtime = staged / 'resources/host-runtime'
            for name in ('server/src/main.js', 'engine/src/runtime/network-contract.js',
                         'engine/src/runtime/content-hash.js', 'update/server.js', 'update/package.json'):
                self.assertTrue((runtime / name).is_file(), name)
            self.assertEqual((runtime / 'server/bfeserver').read_bytes(), (staged / 'bfeserver').read_bytes())
            self.assertEqual((runtime / 'server/bfeserver').stat().st_mode & 0o777, 0o755)
            tree = json.loads((staged / 'resources/bfe-tree.json').read_text())
            self.assertIn('resources/host-runtime/server/src/main.js', {f['path'] for f in tree['files']})
            import tarfile
            with tarfile.open(packages / 'battlefront-eternal-server_0.1.0-preview.10_linux-amd64.tar.xz') as archive:
                names = set(archive.getnames())
                server_tree = json.load(archive.extractfile('bfe-tree.json'))
            self.assertIn('install-server.sh', names)
            self.assertEqual(server_tree['version'], '0.1.0-preview.10')
            distribution.check_server_archive(packages / 'battlefront-eternal-server_0.1.0-preview.10_linux-amd64.tar.xz')

            # Also verify the client-only case when the bfeserver crate is absent:
            # no placeholder server binary is staged, and check-packages.py accepts
            # the absence of bfeserver and the server archive cleanly.
            staged_no_srv = root / 'linux-unpacked-no-server'
            packages_no_srv = root / 'packages-no-server'
            subprocess.run([
                sys.executable, str(repo / 'build/native/stage.py'),
                '--platform', 'linux-x64',
                '--client-bin', str(bin_dir / 'bfe-client'),
                '--toolchain-dir', str(tc),
                '--out-dir', str(staged_no_srv),
            ], check=True, stdout=subprocess.DEVNULL)
            self.assertFalse((staged_no_srv / 'bfeserver').exists())
            self.assertFalse((staged_no_srv / 'resources/host-runtime').exists())
            subprocess.run([
                sys.executable, str(repo / 'build/native/package-linux.py'),
                '--staged-dir', str(staged_no_srv),
                '--platform', 'linux-x64',
                '--version', '0.1.0-preview.11',
                '--out-dir', str(packages_no_srv),
                '--skip-appimage',
            ], check=True, stdout=subprocess.DEVNULL)
            subprocess.run([
                sys.executable, str(repo / 'build/native/update-payload.py'),
                '--tree', str(staged_no_srv),
                '--platform', 'linux-x64',
                '--version', '0.1.0-preview.11',
                '--out', str(packages_no_srv / 'update-linux-x64.tar'),
            ], check=True, stdout=subprocess.DEVNULL)
            report_no_srv = packages_no_srv / 'size-report.md'
            subprocess.run([
                sys.executable, str(repo / 'build/native/check-packages.py'),
                '--platform', 'linux-x64',
                '--staged-dir', str(staged_no_srv),
                '--deb', str(packages_no_srv / 'Battlefront-Eternal-0.1.0-preview.11-linux-amd64.deb'),
                '--server-archive', str(packages_no_srv / 'battlefront-eternal-server_0.1.0-preview.11_linux-amd64.tar.xz'),
                '--update-payload', str(packages_no_srv / 'update-linux-x64.tar'),
                '--report-out', str(report_no_srv),
            ], check=True, stdout=subprocess.DEVNULL)
            self.assertIn('Battlefront-Eternal-0.1.0-preview.11-linux-amd64.deb', report_no_srv.read_text())

            # Verify Windows NSIS extraction layout walking (both top-level and
            # nested payload root with $PLUGINSDIR and Uninstall Battlefront Eternal.exe).
            stage_spec = importlib.util.spec_from_file_location('stage', repo / 'build/native/stage.py')
            stage_mod = importlib.util.module_from_spec(stage_spec)
            stage_spec.loader.exec_module(stage_mod)
            check_spec = importlib.util.spec_from_file_location('check_packages', repo / 'build/native/check-packages.py')
            check_mod = importlib.util.module_from_spec(check_spec)
            check_spec.loader.exec_module(check_mod)

            for layout_name, payload_rel in [('flat', Path('.')), ('nested', Path('$INSTDIR'))]:
                win_extract = root / f'win-extract-{layout_name}'
                plugins_dir = win_extract / '$PLUGINSDIR'
                plugins_dir.mkdir(parents=True, exist_ok=True)
                (plugins_dir / 'System.dll').write_text('nsis-plugin')
                payload_dir = win_extract / payload_rel
                (payload_dir / 'resources' / 'tools').mkdir(parents=True, exist_ok=True)
                (payload_dir / 'bfe-client.exe').write_text('MZ-client')
                (payload_dir / 'bfe-client.exe').chmod(0o644)
                (payload_dir / 'resources' / 'tools' / 'content-version.json').write_text('{"state":2,"maps":8,"sides":2}')
                stage_mod.build_tree_index(payload_dir, 'win32-x64')
                (payload_dir / 'Uninstall Battlefront Eternal.exe').write_text('MZ-uninstaller')
                manifest = check_mod.verify_extracted_windows_tree(win_extract, 'win32-x64', run_smoke=False)
                self.assertEqual(manifest['executable'], 'bfe-client.exe')

if __name__ == '__main__':
    unittest.main()
