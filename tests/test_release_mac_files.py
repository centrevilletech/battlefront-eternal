"""build/ci/release-mac-files.py: which releases it takes, what it checks before it attaches a
Mac build's files to a GitHub release, and that it never replaces a file."""
import base64
import contextlib
import functools
import hashlib
import http.server
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('release_mac_files', Path(__file__).parents[1] / 'build/ci/release-mac-files.py')
script = importlib.util.module_from_spec(spec)
spec.loader.exec_module(script)

REPO = 'centrevilletech/battlefront-eternal'
WORKFLOW = {'login': 'github-actions[bot]', 'id': 41898282, 'type': 'Bot'}
SOMEONE = {'login': 'someone-with-write-access', 'id': 1234, 'type': 'User'}


def ed25519():
    """Whether openssl can do what the script and these tests do: make an Ed25519 key, sign raw data
    with it and verify against its public half (OpenSSL 3; macOS's LibreSSL makes keys but cannot
    sign raw input)."""
    try:
        with tempfile.TemporaryDirectory() as work:
            key, public, data, signature = (Path(work) / name for name in ('key.pem', 'public.pem', 'data', 'signature'))
            data.write_bytes(b'probe')
            steps = (['openssl', 'genpkey', '-algorithm', 'ed25519', '-out', str(key)],
                     ['openssl', 'pkey', '-in', str(key), '-pubout', '-out', str(public)],
                     ['openssl', 'pkeyutl', '-sign', '-inkey', str(key), '-rawin', '-in', str(data), '-out', str(signature)],
                     ['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(public), '-rawin', '-in', str(data), '-sigfile', str(signature)])
            return all(subprocess.run(step, capture_output=True).returncode == 0 for step in steps)
    except OSError:
        return False


class GitHub:
    """The release API as the workflow's token sees it: releases, tag commits, and every call."""

    def __init__(self):
        self.releases, self.commits, self.on_main, self.calls, self.next_id = {}, {}, set(), [], 100

    def add(self, number, assets=('macos-arm64-unsigned-build-input.tar.zst',), author=WORKFLOW, commit=None, tag=None):
        tag = tag or f'preview-{number}'
        self.commits[tag] = commit or f'{number:040x}'
        self.on_main.add(self.commits[tag])
        release = {'id': number, 'tag_name': tag, 'draft': False, 'author': author, 'assets': [],
                   'upload_url': f'https://uploads.github.com/repos/{REPO}/releases/{number}/assets{{?name,label}}'}
        self.releases[tag] = release
        for name in assets:
            self.asset(tag, name, b'build input', uploader=WORKFLOW)
        return release

    def asset(self, tag, name, data, uploader=SOMEONE, state='uploaded', digest=True):
        self.next_id += 1
        item = {'id': self.next_id, 'name': name, 'size': len(data), 'state': state, 'uploader': uploader, 'data': data,
                'digest': 'sha256:' + hashlib.sha256(data).hexdigest() if digest else None,
                'browser_download_url': f'https://github.com/{REPO}/releases/download/{tag}/{name}'}
        self.releases[tag]['assets'].append(item)
        return item

    def __call__(self, method, url, data=None, headers=None):
        path = url.split('?')[0].replace(f'https://api.github.com/repos/{REPO}', '')
        self.calls.append((method, path if method != 'POST' else url.split('name=')[1]))
        if method == 'GET' and path == '/releases':
            return sorted(self.releases.values(), key=lambda release: -release['id'])
        if method == 'GET' and path.startswith('/releases/tags/'):
            return self.releases[path.rsplit('/', 1)[1]]
        if method == 'GET' and path.startswith('/commits/'):
            return {'sha': self.commits[path.rsplit('/', 1)[1]]}
        if method == 'GET' and path.startswith('/compare/'):
            return {'status': 'ahead' if path.split('/compare/')[1].split('...')[0] in self.on_main else 'diverged'}
        if method == 'POST' and url.startswith('https://uploads.github.com/'):
            number = int(url.split('/releases/')[1].split('/')[0])
            tag = f'preview-{number}'
            name = url.split('name=')[1]
            assert all(item['name'] != name for item in self.releases[tag]['assets']), f'{name} uploaded over an existing file'
            body = data.read()
            assert int(headers['Content-Length']) == len(body)
            item = self.asset(tag, name, body, uploader=WORKFLOW)
            return {key: value for key, value in item.items() if key != 'data'}
        if method == 'DELETE' and '/releases/assets/' in path:
            asset_id = int(path.rsplit('/', 1)[1])
            for release in self.releases.values():
                release['assets'] = [item for item in release['assets'] if item['id'] != asset_id]
            return None
        raise AssertionError((method, url))

    def names(self, tag):
        return [item['name'] for item in self.releases[tag]['assets']]

    def uploads(self):
        return [name for method, name in self.calls if method == 'POST']


@unittest.skipUnless(ed25519(), 'needs an openssl with Ed25519 (OpenSSL 3)')
class ReleaseMacFilesTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.site = self.root / 'site'
        self.site.mkdir()
        # The download site, over real HTTP.
        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass
        handler = functools.partial(Quiet, directory=str(self.site))
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.url = f'http://127.0.0.1:{server.server_address[1]}'
        self.key = self.new_key('signing-mac')
        patcher = mock.patch.object(script, 'KEY', self.root / 'signing-mac.pub.pem')
        patcher.start()
        self.addCleanup(patcher.stop)
        self.github = GitHub()
        patcher = mock.patch.object(script, 'github', self.github)
        patcher.start()
        self.addCleanup(patcher.stop)

    def new_key(self, name):
        key = self.root / f'{name}.pem'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ed25519', '-out', str(key)], check=True)
        subprocess.run(['openssl', 'pkey', '-in', str(key), '-pubout', '-out', str(self.root / f'{name}.pub.pem')], check=True)
        return key

    def publish(self, number, key=None, record=None, served=None):
        """The release's Mac build as the release host serves it: the disk image under the site's
        name, the update payload, the record and its signature. Returns the record."""
        folder = self.site / f'preview-{number}'
        shutil.rmtree(folder, ignore_errors=True)
        folder.mkdir()
        image, payload = f'disk image {number}'.encode(), f'update payload {number}'.encode()
        (folder / 'Battlefront-Eternal-macos-arm64.dmg').write_bytes(image)
        (folder / 'update-darwin-arm64.tar').write_bytes(payload)
        described = lambda name, data: {'name': name, 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        record = record or {'schema': 1, 'release_id': f'preview-{number}', 'version': f'0.1.0-preview.{number}',
                            'revision': self.github.commits.get(f'preview-{number}', f'{number:040x}'), 'notarized': True,
                            'dmg': {**described(f'Battlefront-Eternal-0.1.0-preview.{number}-macos-arm64.dmg', image), 'arch': 'arm64'},
                            'updates': {'darwin-arm64': described('update-darwin-arm64.tar', payload)}}
        raw = json.dumps(record).encode()
        (folder / 'macos-notarized.json').write_bytes(raw)
        signed = self.root / 'to-sign'
        signed.write_bytes(raw)
        signature = subprocess.run(['openssl', 'pkeyutl', '-sign', '-inkey', str(key or self.key), '-rawin', '-in', str(signed)],
                                   capture_output=True, check=True).stdout
        (folder / 'macos-notarized.json.sig').write_text(base64.b64encode(signature).decode() + '\n')
        for name, data in (served or {}).items():
            (folder / name).write_bytes(data)
        return record

    def run_script(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with mock.patch('sys.argv', ['release-mac-files.py', '--site', self.url, *args]), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = script.main()
        return code, out.getvalue() + err.getvalue()

    def test_finds_the_releases_of_the_workflow_whose_mac_build_is_published(self):
        self.github.add(380, assets=())                                 # no Mac build input
        self.github.add(381)                                            # Mac build not published yet
        self.github.add(382)
        self.github.add(383, author=SOMEONE)                            # not the workflow's
        self.github.add(384)
        self.github.add(385)
        self.github.asset('preview-385', 'macos-notarized.json', b'{}', uploader=WORKFLOW)  # already done
        self.github.add(386)
        self.github.asset('preview-386', 'macos-notarized.json', b'{}')  # someone else's: not done
        self.github.add(387, tag='preview-0387')                       # not a canonical tag
        for number in (380, 382, 383, 384, 385, 386):
            self.publish(number)
        self.assertEqual(self.run_script('--find'), (0, 'tags=preview-386 preview-384 preview-382\n'))
        # Only the newest few are looked at.
        for number in range(388, 394):
            self.github.add(number)
        self.assertEqual(self.run_script('--find'), (0, 'tags=\n'))

    def test_checks_then_attaches_the_four_files_record_last_and_never_twice(self):
        self.github.add(387)
        record = self.publish(387)
        code, output = self.run_script('--attach', 'preview-387', '--dry-run')
        self.assertEqual(code, 0, output)
        self.assertEqual(output.count('would attach'), 4)
        self.assertEqual(self.github.uploads(), [])
        code, output = self.run_script('--attach', 'preview-387')
        self.assertEqual(code, 0, output)
        order = [record['dmg']['name'], 'update-darwin-arm64.tar', 'macos-notarized.json.sig', 'macos-notarized.json']
        self.assertEqual(self.github.uploads(), order)
        attached = {item['name']: item['data'] for item in self.github.releases['preview-387']['assets']}
        self.assertEqual(attached[record['dmg']['name']], (self.site / 'preview-387/Battlefront-Eternal-macos-arm64.dmg').read_bytes())
        # Again: everything is there and the same, nothing is sent.
        code, output = self.run_script('--attach', 'preview-387')
        self.assertEqual((code, output.count('is already there'), self.github.uploads()), (0, 4, order))

    def refused(self, why, tag='preview-387'):
        """The run fails with `why` and attaches and removes nothing."""
        before = list(self.github.names(tag))
        code, output = self.run_script('--attach', tag)
        self.assertEqual(code, 1, output)
        self.assertIn(why, output)
        self.assertEqual(self.github.names(tag), before)
        self.assertEqual([call for call in self.github.calls if call[0] in ('POST', 'DELETE')], [])

    def test_refuses_whatever_the_signing_mac_did_not_sign_or_does_not_match(self):
        self.github.add(386)
        self.github.add(387)
        # A file changed on the site after it was signed (same size).
        self.publish(387, served={'Battlefront-Eternal-macos-arm64.dmg': b'disk image 38X'})
        self.refused('is not the file the record describes')
        self.publish(387, served={'update-darwin-arm64.tar': b'update payload 38X'})
        self.refused('is not the file the record describes')
        self.publish(387, served={'update-darwin-arm64.tar': b'a longer update payload than the record says'})
        self.refused('is larger than')
        # A record signed with another key; one changed after signing; no signature.
        self.publish(387, key=self.new_key('someone-else'))
        self.refused('not signed by the signing Mac')
        self.publish(387)
        (self.site / 'preview-387/macos-notarized.json').write_bytes((self.site / 'preview-387/macos-notarized.json').read_bytes().replace(b'387', b'388'))
        self.refused('not signed by the signing Mac')
        self.publish(387)
        (self.site / 'preview-387/macos-notarized.json.sig').unlink()
        self.refused('no macos-notarized.json with its signature')
        # A genuine record of another release, served for this one.
        record = self.publish(387)
        shutil.copytree(self.site / 'preview-387', self.site / 'preview-386', dirs_exist_ok=True)
        self.refused('the record is for preview-387', tag='preview-386')
        # A record for the right tag but another commit; a commit that is not on main.
        self.publish(387, record={**record, 'revision': 'f' * 40})
        self.refused('not preview-387 at')
        self.publish(387)
        self.github.on_main.clear()
        self.refused('is not on main')

    def test_refuses_a_release_that_is_not_the_workflows_or_a_build_that_is_not_notarized(self):
        self.github.add(387, author=SOMEONE)
        self.publish(387)
        self.refused('not a published release of the release workflow')
        self.github.add(387, assets=())
        self.github.asset('preview-387', 'macos-arm64-unsigned-build-input.tar.zst', b'x', uploader=SOMEONE)
        self.refused('no Mac build input from the release workflow')
        self.github.add(387)
        record = self.publish(387)
        self.publish(387, record={**record, 'notarized': False})
        self.refused('not of a notarized build')
        self.publish(387, record={**record, 'updates': {'darwin-x64': record['updates']['darwin-arm64']}})
        self.refused('not those of a arm64 disk image')

    def test_never_replaces_a_different_file_and_finishes_a_cut_off_upload(self):
        self.github.add(387)
        record = self.publish(387)
        # A different disk image is already on the release: the run fails, nothing is sent or removed.
        self.github.asset('preview-387', record['dmg']['name'], b'another disk image')
        self.refused('already has a different Battlefront-Eternal-0.1.0-preview.387-macos-arm64.dmg; nothing is replaced')
        # The same disk image without a listed digest is compared by its bytes, then skipped.
        self.github.add(387)
        dmg = (self.site / 'preview-387/Battlefront-Eternal-macos-arm64.dmg').read_bytes()
        item = self.github.asset('preview-387', record['dmg']['name'], dmg, uploader=WORKFLOW, digest=False)
        (self.site / 'asset').mkdir()
        (self.site / 'asset/dmg').write_bytes(dmg)
        item['browser_download_url'] = f'{self.url}/asset/dmg'
        # An upload that was cut off left an empty entry: it is removed and sent again.
        self.github.asset('preview-387', 'update-darwin-arm64.tar', b'', uploader=WORKFLOW, state='open', digest=False)
        code, output = self.run_script('--attach', 'preview-387')
        self.assertEqual(code, 0, output)
        self.assertIn(f"{record['dmg']['name']} is already there", output)
        self.assertIn('removed update-darwin-arm64.tar (state open', output)
        self.assertEqual(self.github.uploads(), ['update-darwin-arm64.tar', 'macos-notarized.json.sig', 'macos-notarized.json'])


if __name__ == '__main__':
    unittest.main()
