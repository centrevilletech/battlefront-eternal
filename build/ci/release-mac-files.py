#!/usr/bin/env python3
"""Attach each preview's signed Mac build to its GitHub release.

Run by .github/workflows/release-mac-files.yml every 15 minutes and by hand.
The project's signing Mac signs and notarizes the Mac build of each release,
and the release host publishes it on the download site together with
macos-notarized.json, a record of the build's files, and that record's
signature, made with a key that only the signing Mac holds. This script
takes those files from the download site and checks, before it attaches
anything:
  - the record's Ed25519 signature, against build/macos-record.pub.pem;
  - that the record is for this release: its tag, the commit the tag names
    (on main), its version, and a notarized build;
  - every file's size and SHA-256 against the record.
Then it attaches the disk image, the update payload, the signature and the
record (last) to the release, with the workflow's own token. It replaces
nothing: a file the release already has is skipped when it is the same and
makes the run fail when it differs. Only releases that the release workflow
created, that carry the Mac build input and do not have their record yet are
looked at; a release whose Mac build is not on the download site yet is
left for a later run.

  release-mac-files.py --find                      print "tags=<the releases to do>"
  release-mac-files.py --attach preview-N ...      check them and attach their files
      --dry-run      check only; attach and remove nothing
      --site URL     take the files from another download site (a local copy)
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = os.environ.get('GITHUB_REPOSITORY', 'centrevilletech/battlefront-eternal')
SITE = 'https://get.battlefronteternal.com/downloads'
API = 'https://api.github.com'
# The public half of the signing Mac's record key (the release host holds the same).
KEY = ROOT / 'build' / 'macos-record.pub.pem'
# The release workflow, as GitHub names the creator of its releases and the uploader of its files.
WORKFLOW = {'login': 'github-actions[bot]', 'id': 41898282}
INPUT = 'macos-arm64-unsigned-build-input.tar.zst'
RECORD = 'macos-notarized.json'
SIGNATURE = RECORD + '.sig'
ARCH_PLATFORMS = {'arm64': {'darwin-arm64'}, 'x64': {'darwin-x64'}, 'universal': {'darwin-arm64', 'darwin-x64'}}
IMAGE = re.compile(r'Battlefront-Eternal-[0-9A-Za-z.+~-]{1,64}-macos-(arm64|x64|universal)\.dmg')
# How many of the newest releases are looked at, and the most a file may be.
NEWEST = 5
LIMIT = 1 << 30
AGENT = 'battlefront-eternal-release-mac-files'


class Refused(Exception):
    pass


def preview(tag):
    match = re.fullmatch(r'preview-([1-9][0-9]*)', tag or '')
    return int(match.group(1)) if match else -1


def by_workflow(account):
    return isinstance(account, dict) and account.get('login') == WORKFLOW['login'] and account.get('id') == WORKFLOW['id']


def github(method, url, data=None, headers=None):
    """One call to GitHub's API, with the workflow's token when there is one; the answer's JSON
    (None when it is empty)."""
    request = urllib.request.Request(url, data=data, method=method, headers={
        'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': AGENT,
        **({'Authorization': 'Bearer ' + os.environ['GH_TOKEN']} if os.environ.get('GH_TOKEN') else {}), **(headers or {})})
    with urllib.request.urlopen(request, timeout=600) as response:
        body = response.read()
    return json.loads(body) if body else None


def fetch(url, target, limit):
    """A public file by its address (no token is sent), written to `target` (at most `limit`
    bytes); returns its SHA-256, or None when the address does not exist."""
    digest, size = hashlib.sha256(), 0
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent': AGENT}), timeout=300) as response, \
                target.open('wb') as output:
            for block in iter(lambda: response.read(1 << 20), b''):
                size += len(block)
                if size > limit:
                    raise Refused(f'{url} is larger than {limit} bytes')
                digest.update(block)
                output.write(block)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise
    return digest.hexdigest()


def verify(record, signature):
    """Raises unless `signature` (base64 text) is the signing Mac's Ed25519 signature of `record`."""
    try:
        raw = base64.b64decode(signature.strip(), validate=True)
    except ValueError:
        raw = b''
    with tempfile.TemporaryDirectory() as work:
        data, detached = Path(work) / 'record', Path(work) / 'signature'
        data.write_bytes(record)
        detached.write_bytes(raw)
        accepted = len(raw) == 64 and subprocess.run(
            ['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(KEY), '-rawin', '-in', str(data), '-sigfile', str(detached)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    if not accepted:
        raise Refused('the record is not signed by the signing Mac')


def has(release, name):
    return any(item['name'] == name for item in release['assets'])


def attached(release, name):
    """Whether this workflow attached `name`: a file of that name uploaded by anyone else does
    not count, so the run goes on to attach and fails loudly on the clash instead."""
    return any(item['name'] == name and by_workflow(item.get('uploader')) for item in release['assets'])


def find(site):
    """The newest releases of the release workflow that carry the Mac build input, lack the
    record, and whose Mac build the download site has."""
    releases = github('GET', f'{API}/repos/{REPOSITORY}/releases?per_page=30')
    ours = sorted((release for release in releases if preview(release['tag_name']) >= 0 and not release['draft']
                   and by_workflow(release.get('author'))), key=lambda release: preview(release['tag_name']), reverse=True)[:NEWEST]
    tags = []
    for release in ours:
        if not has(release, INPUT) or attached(release, RECORD):
            continue
        with tempfile.TemporaryDirectory() as work:
            if fetch(f"{site}/{release['tag_name']}/{RECORD}", Path(work) / RECORD, 64 << 10) is not None:
                tags.append(release['tag_name'])
    return tags


def checked_files(tag, release, site, work):
    """Downloads the release's Mac files into `work` and checks them. Returns [(name, path)] in
    the order they are attached."""
    if release.get('draft') or not by_workflow(release.get('author')) or release['tag_name'] != tag:
        raise Refused('not a published release of the release workflow')
    if not any(item['name'] == INPUT and by_workflow(item.get('uploader')) for item in release['assets']):
        raise Refused('it has no Mac build input from the release workflow')
    revision = github('GET', f'{API}/repos/{REPOSITORY}/commits/{tag}')['sha']
    if github('GET', f'{API}/repos/{REPOSITORY}/compare/{revision}...main?per_page=1')['status'] not in ('identical', 'ahead'):
        raise Refused(f'its commit {revision[:9]} is not on main')
    if fetch(f'{site}/{tag}/{RECORD}', work / RECORD, 64 << 10) is None or fetch(f'{site}/{tag}/{SIGNATURE}', work / SIGNATURE, 1 << 10) is None:
        raise Refused(f'the download site has no {RECORD} with its signature for it')
    raw = (work / RECORD).read_bytes()
    # First the signature: nothing in an unsigned record is believed, and nothing it names is fetched.
    verify(raw, (work / SIGNATURE).read_text('ascii', 'replace'))
    record = json.loads(raw)
    number = preview(tag)
    if (record.get('schema') != 1 or record.get('release_id') != tag or record.get('revision') != revision
            or not re.fullmatch(rf'\d+\.\d+\.\d+-preview\.{number}', str(record.get('version')))):
        raise Refused(f"the record is for {record.get('release_id')} {record.get('version')} at {str(record.get('revision'))[:9]}, "
                      f'not {tag} at {revision[:9]}')
    if record.get('notarized') is not True:
        raise Refused('the record is not of a notarized build')
    image, updates = record.get('dmg'), record.get('updates')
    if (not isinstance(image, dict) or image.get('arch') not in ARCH_PLATFORMS or not isinstance(image.get('name'), str)
            or not IMAGE.fullmatch(image['name']) or IMAGE.fullmatch(image['name']).group(1) != image['arch']):
        raise Refused('the record names no disk image')
    if not isinstance(updates, dict) or set(updates) != ARCH_PLATFORMS[image['arch']]:
        raise Refused(f"its update payloads are not those of a {image['arch']} disk image")
    # The disk image is served under one name per architecture; the record names its own.
    files = [(image['name'], f"Battlefront-Eternal-macos-{image['arch']}.dmg", image)]
    for platform in sorted(updates):
        item = updates[platform]
        if not isinstance(item, dict) or item.get('name') != f'update-{platform}.tar':
            raise Refused(f'unexpected update payload name for {platform}')
        files.append((item['name'], item['name'], item))
    for name, served, item in files:
        if not isinstance(item.get('size'), int) or not 0 < item['size'] <= LIMIT or not re.fullmatch(r'[0-9a-f]{64}', str(item.get('sha256'))):
            raise Refused(f'the record does not describe {name}')
        digest = fetch(f'{site}/{tag}/{served}', work / name, item['size'])
        if digest is None or digest != item['sha256'] or (work / name).stat().st_size != item['size']:
            raise Refused(f'{name} on the download site is not the file the record describes')
    return [(name, work / name) for name, _, _ in files] + [(SIGNATURE, work / SIGNATURE), (RECORD, work / RECORD)]


def attach(tag, site, dry_run):
    """Checks the release's Mac files and attaches those it lacks. Returns what it did."""
    base = f'{API}/repos/{REPOSITORY}'
    release = github('GET', f'{base}/releases/tags/{tag}')
    with tempfile.TemporaryDirectory(prefix='bfe-mac-files-') as work:
        files = checked_files(tag, release, site, Path(work))
        upload = release['upload_url'].split('{', 1)[0]
        if upload != f"https://uploads.github.com/repos/{REPOSITORY}/releases/{release['id']}/assets":
            raise Refused('unexpected upload address')
        assets = {item['name']: item for item in release['assets']}
        done = []
        for name, path in files:
            size, digest = path.stat().st_size, 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()
            item = assets.get(name)
            if item and item.get('state') != 'uploaded':
                # An upload that did not finish leaves an empty entry under the name.
                done.append(f"{'would remove' if dry_run else 'removed'} {name} (state {item.get('state')}, "
                            f"uploaded by {(item.get('uploader') or {}).get('login')})")
                if not dry_run:
                    github('DELETE', f"{base}/releases/assets/{item['id']}")
                item = None
            if item:
                theirs = item.get('digest')
                if not theirs:
                    with tempfile.TemporaryDirectory() as other:
                        theirs = 'sha256:' + str(fetch(item['browser_download_url'], Path(other) / 'asset', LIMIT))
                if item.get('size') != size or theirs != digest:
                    raise Refused(f'the release already has a different {name}; nothing is replaced')
                done.append(f'{name} is already there')
                continue
            if dry_run:
                done.append(f'would attach {name} ({size} bytes, {digest})')
                continue
            with path.open('rb') as data:
                made = github('POST', upload + '?' + urllib.parse.urlencode({'name': name}), data=data,
                              headers={'Content-Type': 'application/octet-stream', 'Content-Length': str(size)})
            if made.get('digest') is None:
                made = github('GET', f"{base}/releases/assets/{made['id']}")
            if made.get('name') != name or made.get('size') != size or made.get('digest') != digest:
                raise Refused(f'GitHub did not store {name} as it was sent')
            done.append(f'attached {name}')
    return done


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--find', action='store_true')
    parser.add_argument('--attach', nargs='*', default=[])
    parser.add_argument('--dry-run', action='store_true')
    parser.add_argument('--site', default=SITE)
    args = parser.parse_args()
    site = args.site.rstrip('/')
    if args.find:
        print('tags=' + ' '.join(find(site)))
        return 0
    failed = 0
    for tag in args.attach:
        if preview(tag) < 0:
            print(f'{tag}: not a preview tag', file=sys.stderr)
            failed += 1
            continue
        try:
            for line in attach(tag, site, args.dry_run):
                print(f'{tag}: {line}')
        except (Refused, urllib.error.URLError, ValueError, KeyError, OSError) as error:
            print(f'{tag}: REFUSED: {error}', file=sys.stderr)
            failed += 1
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
