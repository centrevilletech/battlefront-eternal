"""Exercise real Debian metadata, signatures and atomic download publication."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('distribution', Path(__file__).parents[1] / 'build/publish-distribution.py')
distribution = importlib.util.module_from_spec(spec)
spec.loader.exec_module(distribution)

def server_archive(source, arch, entries=None, version='0.0.0'):
    """A dedicated server archive as build/server-archive.mjs lays it out."""
    import io
    import tarfile
    path = source / f'battlefront-eternal-server_{version}_linux-{arch}.tar.xz'
    with tarfile.open(path, 'w:xz') as archive:
        for name in entries or ['host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'host-runtime/update/server.js',
                                'tools/prepare.py', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json']:
            data = f'{arch} {name}'.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
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
                            ['host-runtime/engine/src/game.js', 'tools/prepare.py', 'bfe-tree.json']]:
                server_archive(source, 'arm64', entries)
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

if __name__ == '__main__':
    unittest.main()
