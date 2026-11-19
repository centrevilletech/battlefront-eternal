"""The parts of the macOS packaging and signing scripts that need no Mac to test:
where the disk image tool comes from, which earlier signatures may be reused,
and what an update payload refuses to be made from."""
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


if __name__ == '__main__':
    unittest.main()
