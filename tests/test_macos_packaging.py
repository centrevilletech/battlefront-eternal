"""The parts of the macOS packaging and signing scripts that need no Mac to test:
where the disk image tool comes from, which earlier signatures may be reused,
what an update payload refuses to be made from, and that nothing is fetched for a
Mac build without a pinned digest."""
import hashlib
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

NATIVE = Path(__file__).parents[1] / 'build/native'


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), NATIVE / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fetch_toolchain = load('fetch-toolchain')
package_macos = load('package-macos')
sign_macos = load('sign-macos')
update_payload = load('update-payload')


class DiskImageToolTest(unittest.TestCase):
    def test_a_signing_machine_names_its_own_tool_and_nothing_is_fetched(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(package_macos, 'run') as run:
            python = Path(temporary) / 'python3'
            python.write_text('')
            with mock.patch.dict(os.environ, {'BFE_DMGBUILD_PYTHON': str(python)}):
                with package_macos.dmgbuild_python() as chosen:
                    self.assertEqual(chosen, python)
            # A named tool that is not there is an error, never a reason to fetch one.
            with mock.patch.dict(os.environ, {'BFE_DMGBUILD_PYTHON': str(python) + '-missing'}):
                with self.assertRaises(FileNotFoundError):
                    with package_macos.dmgbuild_python():
                        pass
            run.assert_not_called()

    def test_without_one_the_tool_is_installed_by_hash_into_a_folder_of_this_run(self):
        commands = []
        environment = {key: value for key, value in os.environ.items() if key != 'BFE_DMGBUILD_PYTHON'}
        with mock.patch.dict(os.environ, environment, clear=True), mock.patch.object(package_macos, 'run', lambda args, **kwargs: commands.append([str(arg) for arg in args])):
            with package_macos.dmgbuild_python() as python:
                folder = python.parents[1]
                self.assertTrue(folder.is_dir())
                self.assertEqual(folder.stat().st_mode & 0o077, 0, 'the folder is this user\'s alone')
            self.assertFalse(folder.exists(), 'and it is gone afterwards')
        install = commands[1]
        self.assertIn('--require-hashes', install)
        self.assertEqual(install[install.index('-r') + 1], str(package_macos.DMGBUILD_REQUIREMENTS))
        requirements = [line for line in package_macos.DMGBUILD_REQUIREMENTS.read_text().splitlines() if line and not line.startswith('#')]
        self.assertEqual(len(requirements), 3)
        for line in requirements:
            self.assertRegex(line, r'^[a-z_]+==[0-9.]+ --hash=sha256:[0-9a-f]{64}$')


LISTING = '''  1) AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA "Developer ID Application: Example Games, LLC (TEAM000001)"
  2) BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB "Apple Development: someone (XYZ)"
     2 identities found
'''
NAME = 'Developer ID Application: Example Games, LLC (TEAM000001)'


class SigningCacheTest(unittest.TestCase):
    def test_an_identity_is_known_by_its_certificate_not_its_name(self):
        with mock.patch.object(sign_macos, 'output', return_value=(0, LISTING)):
            self.assertEqual(sign_macos.identity_fingerprint(NAME), 'A' * 40)
            self.assertEqual(sign_macos.identity_fingerprint('c' * 40), 'C' * 40)
            self.assertIsNone(sign_macos.identity_fingerprint('Developer ID Application: Someone Else (TEAM000002)'))
        # A renewed certificate beside the old one: the name no longer says which made a signature.
        renewed = LISTING.replace('2) ' + 'B' * 40 + ' "Apple Development: someone (XYZ)"', '2) ' + 'D' * 40 + f' "{NAME}"')
        with mock.patch.object(sign_macos, 'output', return_value=(0, renewed)):
            self.assertIsNone(sign_macos.identity_fingerprint(NAME))

    def test_a_signed_copy_is_reused_only_for_the_certificate_that_made_it(self):
        signatures = []

        def codesign(args, **kwargs):
            # Stands in for codesign: "signs" by appending who signed.
            path = Path(args[-1])
            signatures.append(path.name)
            path.write_bytes(path.read_bytes() + b' signed #%d' % len(signatures))

        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sign_macos, 'run', codesign):
            root = Path(temporary)
            cache = root / 'cache'

            def sign(fingerprint):
                program = root / 'ffmpeg'
                program.write_bytes(b'the same program every release')
                reused = sign_macos.sign_file(program, NAME, cache, fingerprint)
                return reused, program.read_bytes()

            self.assertEqual(sign('A' * 40), (False, b'the same program every release signed #1'))
            self.assertEqual(sign('A' * 40), (True, b'the same program every release signed #1'))
            # The same name, another certificate: signed again, and both copies are kept apart.
            self.assertEqual(sign('D' * 40), (False, b'the same program every release signed #2'))
            self.assertEqual(sign('A' * 40), (True, b'the same program every release signed #1'))
            # No fingerprint (the name is ambiguous or unknown): never from the cache.
            self.assertEqual(sign(None)[0], False)
            self.assertEqual(len(signatures), 3)


class SealedBundleTest(unittest.TestCase):
    def test_an_update_is_not_made_from_a_bundle_that_was_written_into(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            contents = folder / 'Battlefront Eternal.app' / 'Contents'
            for directory in ('MacOS', 'Resources/tools', '_CodeSignature'):
                (contents / directory).mkdir(parents=True)
            (contents / 'MacOS/bfe-client').write_text('client')
            (contents / 'MacOS/bfe-client').chmod(0o755)
            (contents / 'Info.plist').write_text('<plist/>')
            (contents / '_CodeSignature/CodeResources').write_text('seal')
            (contents / 'Resources/tools/content-version.json').write_text('{"state":2,"maps":8,"sides":2}')
            tree = update_payload.bundle_tree(folder, 'darwin-arm64')
            self.assertIn('Battlefront Eternal.app/Contents/MacOS/bfe-client', {entry['path'] for entry in tree['files']})
            (contents / 'Resources/tools/__pycache__').mkdir()
            (contents / 'Resources/tools/__pycache__/prepare.cpython-313.pyc').write_bytes(b'bytecode')
            with self.assertRaises(RuntimeError) as refused:
                update_payload.bundle_tree(folder, 'darwin-arm64')
            self.assertIn('bytecode', str(refused.exception))


class ToolchainPinsTest(unittest.TestCase):
    def test_every_macos_download_has_a_pinned_digest(self):
        for key in ('mac-arm64', 'mac-x64'):
            target = fetch_toolchain.TARGETS[key]
            self.assertTrue(fetch_toolchain.is_mac(target))
            for url in (fetch_toolchain.python_url(target['python']), target['ffmpeg'], target['ktx']):
                self.assertRegex(fetch_toolchain.SHA256.get(url, ''), r'^[0-9a-f]{64}$', url)
        self.assertFalse(fetch_toolchain.is_mac(fetch_toolchain.TARGETS['linux-x64']))

    def test_a_download_is_only_ever_the_pinned_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            served = root / 'served' / 'tool.zip'
            served.parent.mkdir()
            served.write_bytes(b'the published archive')
            url = served.as_uri()
            good = hashlib.sha256(b'the published archive').hexdigest()
            cache = root / 'cache'
            with mock.patch.object(fetch_toolchain, 'CACHE', cache), mock.patch.object(fetch_toolchain, 'SHA256', {url: good}):
                fetched = fetch_toolchain.download(url, require_digest=True)
                self.assertEqual(fetched.read_bytes(), b'the published archive')
                # The site now serves something else under the same name.
                served.write_bytes(b'another archive, same name')
                fetched.unlink()
                with self.assertRaises(fetch_toolchain.DigestMismatch) as refused:
                    fetch_toolchain.download(url, attempts=1, require_digest=True)
                self.assertIn(good, str(refused.exception))
                self.assertEqual(list(cache.iterdir()), [], 'nothing of it is kept')
                # A cached copy that is not the pinned file is not used either.
                fetched.write_bytes(b'left by an earlier pin')
                with self.assertRaises(fetch_toolchain.DigestMismatch):
                    fetch_toolchain.download(url, attempts=1, require_digest=True)
            # A macOS download with no pin is refused before anything is fetched.
            with mock.patch.object(fetch_toolchain, 'CACHE', cache), mock.patch.object(fetch_toolchain, 'SHA256', {}), \
                    mock.patch.object(fetch_toolchain.urllib.request, 'urlopen') as urlopen:
                with self.assertRaises(fetch_toolchain.DigestMismatch):
                    fetch_toolchain.download(url, require_digest=True)
                urlopen.assert_not_called()

    def test_macos_python_packages_are_installed_by_hash(self):
        def pip_command(key):
            commands = []
            with tempfile.TemporaryDirectory() as temporary, \
                    mock.patch.object(fetch_toolchain, 'download', return_value=Path('archive')), \
                    mock.patch.object(fetch_toolchain, 'ensure_host_python', return_value=Path('python3')), \
                    mock.patch.object(fetch_toolchain, 'prune_python'), \
                    mock.patch.object(fetch_toolchain, 'run', lambda command, args, **kwargs: commands.append([command, *args])):
                fetch_toolchain.fetch_python(fetch_toolchain.TARGETS[key], Path(temporary))
            return [command for command in commands if 'pip' in command]

        for key in ('mac-arm64', 'mac-x64'):
            installs = pip_command(key)
            self.assertEqual(len(installs), 1)
            self.assertIn('--require-hashes', installs[0])
            self.assertEqual(installs[0][installs[0].index('-r') + 1], str(fetch_toolchain.MAC_REQUIREMENTS))
        self.assertFalse(any('--require-hashes' in command for command in pip_command('linux-x64')))
        requirements = fetch_toolchain.MAC_REQUIREMENTS.read_text().replace('\\\n', ' ')
        pinned = [line.split() for line in requirements.splitlines() if line and not line.startswith('#')]
        names = set()
        for name, *hashes in pinned:
            self.assertRegex(name, r'^[a-z-]+==[0-9.]+$')
            self.assertIn(len(hashes), (1, 2))
            for value in hashes:
                self.assertRegex(value, r'^--hash=sha256:[0-9a-f]{64}$')
            names.add(name.split('==')[0])
        # What the preparation tools import, and what those need.
        self.assertEqual(names, {'pillow', 'numpy', 'scipy', 'soundfile', 'soxr', 'cffi', 'pycparser', 'typing-extensions'})


if __name__ == '__main__':
    unittest.main()
