"""Exercise real Debian metadata, signatures and atomic download publication."""
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
    """A dedicated server archive as build/server-archive.mjs or build/native/server-archive.py lays it out."""
    import io
    import tarfile
    path = source / f'battlefront-eternal-server_{version}_linux-{arch}.tar.xz'
    with tarfile.open(path, 'w:xz') as archive:
        for name in entries or ['host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'host-runtime/update/server.js',
                                'tools/prepare.py', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json']:
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

class DistributionTest(unittest.TestCase):
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
            import hashlib
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
            for entries in [['opt/evil', 'host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'tools/prepare.py', 'bfe-tree.json'],
                            ['host-runtime/../escape', 'host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'tools/prepare.py', 'bfe-tree.json'],
                            ['host-runtime/engine/src/game.js', 'tools/prepare.py', 'bfe-tree.json'],
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

    def test_signed_update_channel_store_and_collection(self):
        """Payloads from build/update-payload.mjs become a signed channel the
        client's own code (electron/update/feed.js) accepts; blobs are checked
        by content, shared across releases and collected with them."""
        repo = Path(__file__).parents[1]
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
                script = """
                    import { mkdirSync, writeFileSync, chmodSync } from 'node:fs';
                    import path from 'node:path';
                    const [repo, source, number, payload] = process.argv.slice(1);
                    const index = (await import(path.join(repo, 'build/tree-index.cjs'))).default.default;
                    const { buildPayload } = await import(path.join(repo, 'build/update-payload.mjs'));
                    for (const [platform, executable] of [['linux-x64', 'battlefront-eternal'], ['linux-arm64', 'battlefront-eternal'], ['win32-x64', 'Battlefront Eternal.exe']]) {
                      const tree = path.join(source, 'tree-' + platform);
                      mkdirSync(path.join(tree, 'resources', 'tools'), { recursive: true });
                      writeFileSync(path.join(tree, executable), 'runtime ' + platform);
                      chmodSync(path.join(tree, executable), 0o755);
                      writeFileSync(path.join(tree, 'resources', 'app.asar'), 'app ' + payload);
                      writeFileSync(path.join(tree, 'resources', 'tools', 'content-version.json'), '{"state":2,"maps":8,"sides":2}');
                      await index({ appOutDir: tree, electronPlatformName: platform.split('-')[0] });
                      await buildPayload({ tree, platform, out: path.join(source, 'update-' + platform + '.tar'), version: '0.1.0-preview.' + number });
                    }
                """
                subprocess.run(['node', '--input-type=module', '-e', script, str(repo), str(source), str(number), payload], check=True, stdout=subprocess.DEVNULL)

            def channel():
                script = """
                    import { readFileSync } from 'node:fs';
                    const [repo, file, spki] = process.argv.slice(1);
                    const { keyId, openEnvelope } = await import(repo + '/electron/update/feed.js');
                    const key = Buffer.from(spki, 'base64');
                    console.log(JSON.stringify(openEnvelope(JSON.parse(readFileSync(file, 'utf8')), { [keyId(key)]: key.toString('base64') })));
                """
                return json.loads(subprocess.check_output(['node', '--input-type=module', '-e', script, str(repo),
                    str(distribution.ROOT / 'downloads/updates/development-preview.json'), base64.b64encode(spki).decode()]))

            import base64
            import hashlib
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
            marker = data.find(b'blobs/')
            self.assertGreater(marker, 0)
            body = marker + 512
            data[body + 20] ^= 0xff
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
            self.assertTrue(only_first, "preview 1's own app.asar blobs are gone")
            for digest in only_first:
                self.assertFalse((store / digest[:2] / f'{digest}.zst').exists())
            self.assertEqual(len(list(store.glob('*/*.zst'))), len(held))

            # A native release using bfe-client + bfeserver update payloads and
            # native server archives publishes into the same channel cleanly.
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

            stage_spec = importlib.util.spec_from_file_location('stage', repo / 'build/native/stage.py')
            stage_mod = importlib.util.module_from_spec(stage_spec)
            stage_spec.loader.exec_module(stage_mod)
            payload_spec = importlib.util.spec_from_file_location('update_payload', repo / 'build/native/update-payload.py')
            payload_mod = importlib.util.module_from_spec(payload_spec)
            payload_spec.loader.exec_module(payload_mod)

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
