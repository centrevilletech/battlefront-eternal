#!/usr/bin/env python3
"""Publish complete immutable desktop releases and a signed APT repository.

Runs on the project's release host, with only distribution and signing
directories mounted. Never reads or distributes original game content.
"""
import base64
import datetime as dt
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(os.environ.get('BFE_DISTRIBUTION', '/deployment/distribution'))
KEYHOME = os.environ.get('BFE_SIGNING_HOME', '/deployment/signing')
KEY = 'F210B0710D270963CB641C118043BCB331D34C9B'
# Desktop updates (docs/desktop-updates.md): the channel installed clients
# follow, signed with an Ed25519 key whose public half ships in the client
# (electron/update/keys.js).
CHANNEL = os.environ.get('BFE_UPDATE_CHANNEL', 'development-preview')
UPDATE_KEY = os.environ.get('BFE_UPDATE_KEY', str(Path(KEYHOME) / 'update-ed25519.pem'))
UPDATE_PLATFORMS = ('linux-x64', 'linux-arm64', 'win32-x64')
MAX_BLOB = 2 << 30

def run(*args, **kw):
    return subprocess.check_output(args, **kw)

def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()

def sign(path, detached=False):
    args = ['gpg', '--homedir', KEYHOME, '--batch', '--yes', '--local-user', KEY,
            '--digest-algo', 'SHA256', '--armor', '--output', str(path) + '.asc' if detached else str(path.parent / 'InRelease')]
    run(*args, '--detach-sign' if detached else '--clearsign', str(path))

def api(path):
    # The repository whose GitHub release is published (BFE_RELEASE_REPOSITORY,
    # else the one running this script).
    repository = os.environ.get('BFE_RELEASE_REPOSITORY') or os.environ['GITHUB_REPOSITORY']
    request = urllib.request.Request('https://api.github.com/repos/' + repository + path,
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json'})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)

def download_release(destination):
    release = api('/releases/tags/' + os.environ['BFE_RELEASE_TAG'])
    for asset in release['assets']:
        if not asset['name'].endswith(('.AppImage', '.exe', '.deb', '.json', '.tar', '.tar.xz')):
            continue
        if Path(asset['name']).name != asset['name']:
            raise ValueError('Invalid release asset name')
        request = urllib.request.Request(asset['url'], headers={
            'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/octet-stream'})
        with urllib.request.urlopen(request, timeout=300) as response, (destination / asset['name']).open('wb') as output:
            shutil.copyfileobj(response, output)

SAFE_PART = re.compile(r'[^\\/:*?"<>|\x00-\x1f]+')

def safe_relative(value):
    return (isinstance(value, str) and 0 < len(value) <= 512
            and all(part not in ('', '.', '..') and SAFE_PART.fullmatch(part) for part in value.split('/')))

# The dedicated server archive (build/server-archive.mjs, docs/server-updates.md):
# its root entries, and files every archive holds.
SERVER_ENTRIES = {'bfe-tree.json', 'host-runtime', 'legal', 'toolchain', 'tools'}
SERVER_FILES = ('host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'tools/prepare.py', 'bfe-tree.json')

def archive_relative(name):
    return isinstance(name, str) and '\x00' not in name and all(part not in ('', '.', '..') for part in name.split('/'))

def check_server_archive(path):
    """Only files, folders and links inside the server's own entries: nothing
    the archive could write outside the folder it is unpacked into."""
    names = set()
    with tarfile.open(path, 'r:xz') as archive:
        for member in archive:
            name = member.name[2:] if member.name.startswith('./') else member.name
            if not archive_relative(name) or name.split('/')[0] not in SERVER_ENTRIES:
                raise ValueError(f'Unexpected entry {member.name} in {path.name}')
            if member.issym() or member.islnk():
                target = member.linkname if member.islnk() else os.path.join(os.path.dirname(name), member.linkname)
                if member.linkname.startswith('/') or not archive_relative(os.path.normpath(target).replace(os.sep, '/')):
                    raise ValueError(f'Unsafe link {member.name} in {path.name}')
            elif not (member.isfile() or member.isdir()):
                raise ValueError(f'Unexpected entry type for {member.name} in {path.name}')
            names.add(name)
    for file in SERVER_FILES:
        if file not in names:
            raise ValueError(f'{path.name} lacks {file}')

def zstd_digest(path):
    """SHA-256 of a zstd blob's decompressed contents (zstd also checks the
    frame checksum the payload builder adds)."""
    digest = hashlib.sha256()
    with subprocess.Popen(['nice', '-n', '19', 'zstd', '-dcq', '--', str(path)], stdout=subprocess.PIPE) as process:
        for block in iter(lambda: process.stdout.read(1024 * 1024), b''):
            digest.update(block)
    if process.returncode != 0:
        raise ValueError(f'Corrupt update blob {path.name}')
    return digest.hexdigest()

def check_tree(tree, platform, version):
    if not isinstance(tree, dict) or tree.get('schema') != 1 or tree.get('platform') != platform or tree.get('version') != version:
        raise ValueError(f'Update manifest for {platform} does not match {version}')
    files = tree.get('files')
    if not safe_relative(tree.get('executable')) or not isinstance(files, list) or not files:
        raise ValueError(f'Malformed update manifest for {platform}')
    for item in files:
        if (not isinstance(item, dict) or not safe_relative(item.get('path')) or not re.fullmatch(r'[a-f0-9]{64}', str(item.get('sha256')))
                or not isinstance(item.get('size'), int) or not isinstance(item.get('zsize'), int) or item.get('mode') not in (0o644, 0o755)):
            raise ValueError(f'Malformed update manifest entry for {platform}')
    if tree['executable'] not in {item['path'] for item in files}:
        raise ValueError(f'Update manifest for {platform} has no executable')
    for link in tree.get('links', []):
        target = link.get('target') if isinstance(link, dict) else None
        if not safe_relative(link.get('path')) or not isinstance(target, str) or target.startswith('/') or not safe_relative(
                os.path.normpath(os.path.join(os.path.dirname(link['path']), target)).replace(os.sep, '/')):
            raise ValueError(f'Unsafe link in the update manifest for {platform}')
    return tree

def ingest_updates(source, stage, version, release_id, store):
    """Adds every platform's new blobs to the store (each checked against the
    hash it is named by) and writes the platform manifests into the release.
    Returns the signed release's platform entries and content generations,
    or None for a release built without update payloads."""
    tars = {platform: source / f'update-{platform}.tar' for platform in UPDATE_PLATFORMS}
    present = [platform for platform, path in tars.items() if path.exists()]
    if not present:
        return None
    if len(present) != len(UPDATE_PLATFORMS):
        raise ValueError('Update payloads are missing for ' + ', '.join(sorted(set(UPDATE_PLATFORMS) - set(present))))
    platforms, content = {}, None
    for platform, path in tars.items():
        tree = None
        with tarfile.open(path, 'r:') as archive:
            for member in archive:
                if not member.isfile() or member.size > MAX_BLOB:
                    raise ValueError(f'Unexpected entry {member.name} in {path.name}')
                stream = archive.extractfile(member)
                if member.name == f'update-{platform}.json':
                    tree = json.loads(stream.read())
                    continue
                match = re.fullmatch(r'blobs/([a-f0-9]{64})\.zst', member.name)
                if not match:
                    raise ValueError(f'Unexpected entry {member.name} in {path.name}')
                digest = match.group(1)
                target = store / digest[:2] / f'{digest}.zst'
                if target.exists():
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                temporary = target.parent / f'.{digest}.{os.getpid()}'
                with temporary.open('wb') as output:
                    shutil.copyfileobj(stream, output)
                if zstd_digest(temporary) != digest:
                    temporary.unlink()
                    raise ValueError(f'Update blob {digest} does not hold what its name says')
                temporary.chmod(0o644)
                os.replace(temporary, target)
        tree = check_tree(tree, platform, version)
        unique = {}
        for item in tree['files']:
            blob = store / item['sha256'][:2] / f"{item['sha256']}.zst"
            if not blob.exists() or blob.stat().st_size != item['zsize']:
                raise ValueError(f"Update blob for {item['path']} ({platform}) is missing from the store")
            unique[item['sha256']] = item['zsize']
        packed = subprocess.run(['zstd', '-19', '-q', '-c'], input=json.dumps(tree).encode(), stdout=subprocess.PIPE, check=True).stdout
        name = f'update-{platform}.json.zst'
        (stage / name).write_bytes(packed)
        platforms[platform] = {'manifest': f'{release_id}/{name}', 'sha256': hashlib.sha256(packed).hexdigest(),
                               'files': len(tree['files']), 'size': sum(item['size'] for item in tree['files']),
                               'download': sum(unique.values())}
        content = content or tree.get('content')
    return platforms, content

def sign_update(payload):
    """The channel file: the release JSON, signed with the Ed25519 update key
    and checked against its public half before anything is published."""
    with tempfile.TemporaryDirectory() as work:
        data, signature, public = Path(work) / 'payload', Path(work) / 'signature', Path(work) / 'public.pem'
        data.write_bytes(payload)
        run('openssl', 'pkeyutl', '-sign', '-inkey', UPDATE_KEY, '-rawin', '-in', str(data), '-out', str(signature))
        public.write_bytes(run('openssl', 'pkey', '-in', UPDATE_KEY, '-pubout'))
        run('openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(public), '-rawin', '-in', str(data), '-sigfile', str(signature))
        spki = run('openssl', 'pkey', '-in', UPDATE_KEY, '-pubout', '-outform', 'DER')
        return {'key': hashlib.sha256(spki).hexdigest()[:16], 'payload': base64.b64encode(payload).decode(),
                'signature': base64.b64encode(signature.read_bytes()).decode()}

def write_atomic(path, data):
    temporary = path.parent / f'.{path.name}.{os.getpid()}'
    temporary.write_bytes(data)
    temporary.chmod(0o644)
    os.replace(temporary, path)

def tree_blobs(generation):
    blobs = set()
    for manifest in generation.glob('update-*.json.zst'):
        tree = json.loads(subprocess.run(['zstd', '-dcq', '--', str(manifest)], stdout=subprocess.PIPE, check=True).stdout)
        blobs.update(item['sha256'] for item in tree['files'])
    return blobs

def collect_blobs(store, retained):
    """Deletes blobs no retained release lists, then records what the store
    holds (blobs.json: the next build leaves those out of its payload)."""
    referenced = set().union(*(tree_blobs(generation) for generation in retained)) if retained else set()
    held = {}
    for blob in store.glob('*/*.zst'):
        digest = blob.name[:-4]
        if digest not in referenced:
            blob.unlink()
        else:
            held[digest] = blob.stat().st_size
    write_atomic(store.parent / 'blobs.json', (json.dumps({'blobs': dict(sorted(held.items()))}) + '\n').encode())

def publish(source, version, release_id, revision, date):
    os.umask(0o022)  # Public static files must remain readable under restrictive runner umasks.
    if not re.fullmatch(r'[A-Za-z0-9._-]+', release_id):
        raise ValueError('Invalid release ID')
    downloads = ROOT / 'downloads'
    downloads.mkdir(parents=True, exist_ok=True)
    previous = downloads / 'latest/manifest.json'
    if previous.exists():
        current = json.loads(previous.read_text())
        if subprocess.call(['dpkg', '--compare-versions', version, 'le', current['version']]) == 0:
            raise ValueError('Refusing to publish an older or already published package version')
    generation = downloads / release_id
    if generation.exists():
        raise ValueError('Immutable release already exists')
    stage = Path(tempfile.mkdtemp(prefix='.release-', dir=downloads))
    try:
        patterns = {
            'Battlefront-Eternal-windows-x64.exe': '*win*x64.exe',
            'Battlefront-Eternal-linux-x64.AppImage': '*linux*x86_64.AppImage',
            'Battlefront-Eternal-linux-arm64.AppImage': '*linux*arm64.AppImage',
        }
        for name, pattern in patterns.items():
            matches = list(source.glob(pattern))
            if len(matches) != 1:
                raise ValueError(f'Expected one {pattern}, got {len(matches)}')
            shutil.copyfile(matches[0], stage / name)
        packages = list(source.glob('*.deb'))
        if len(packages) != 2:
            raise ValueError('Both amd64 and arm64 Debian packages are required')
        architectures = set()
        for package in packages:
            name, package_version, arch = run('dpkg-deb', '-f', str(package), 'Package', 'Version', 'Architecture', text=True).splitlines()
            # Multiple fields are printed as "Field: value" by dpkg-deb.
            name, package_version, arch = [v.split(': ', 1)[-1] for v in (name, package_version, arch)]
            if name != 'battlefront-eternal' or arch not in ('amd64', 'arm64') or package_version != version:
                raise ValueError('Unexpected Debian package metadata')
            architectures.add(arch)
            shutil.copyfile(package, stage / f'battlefront-eternal_{version}_{arch}.deb')
        if architectures != {'amd64', 'arm64'}:
            raise ValueError('Missing Debian architecture')
        # The dedicated server archives, named by the packages' version.
        for arch in ('amd64', 'arm64'):
            archives = list(source.glob(f'*-server_*_linux-{arch}.tar.xz'))
            if len(archives) != 1:
                raise ValueError(f'Expected one dedicated server archive for {arch}, got {len(archives)}')
            check_server_archive(archives[0])
            shutil.copyfile(archives[0], stage / f'battlefront-eternal-server_{version}_linux-{arch}.tar.xz')
        store = downloads / 'updates' / 'blobs'
        store.mkdir(parents=True, exist_ok=True)
        updates = ingest_updates(source, stage, version.replace('~', '-'), release_id, store)
        metadata = {'version': version, 'release_id': release_id, 'revision': revision, 'date': date,
                    'channel': 'development-preview', 'windows_signing': 'unsigned',
                    'compatibility': json.loads((source / 'compatibility.json').read_text())}
        (stage / 'manifest.json').write_text(json.dumps(metadata, indent=2) + '\n')
        sums = ''.join(sha256(p) + '  ' + p.name + '\n'
                       for p in sorted(stage.iterdir()))
        (stage / 'SHA256SUMS').write_text(sums)
        sign(stage / 'SHA256SUMS', True)
        for p in stage.iterdir():
            p.chmod(0o644)
        stage.chmod(0o755)
        stage.rename(generation)
    except BaseException:
        shutil.rmtree(stage, ignore_errors=True)
        raise

    apt = ROOT / 'apt'
    pool = apt / 'pool/main/b/battlefront-eternal'
    pool.mkdir(parents=True, exist_ok=True)
    for package in generation.glob('*.deb'):
        target = pool / package.name
        if target.exists() and sha256(target) != sha256(package):
            raise ValueError('Cannot replace existing package version')
        if not target.exists():
            shutil.copyfile(package, target)
        target.chmod(0o644)
    # Bound the download host's disk use while preserving several generations for in-flight
    # downloads. GitHub retains the full immutable release archive.
    keep_count = max(2, int(os.environ.get('BFE_KEEP_RELEASES', '5')))
    generations = sorted((p for p in downloads.iterdir() if p.is_dir() and not p.is_symlink()
                          and not p.name.startswith('.') and (p / 'manifest.json').is_file()),
                         key=lambda p: p.stat().st_mtime, reverse=True)
    retained = generations[:keep_count]
    retained_packages = {p.name for directory in retained for p in directory.glob('*.deb')}
    # Retain old immutable index generations for APT's Acquire-By-Hash requests.
    indices = apt / 'indices' / release_id
    indices.mkdir(parents=True)
    files = []
    for arch in ('amd64', 'arm64'):
        directory = indices / f'main/binary-{arch}'
        directory.mkdir(parents=True)
        entries = []
        for package in sorted(pool.glob(f'*_{arch}.deb')):
            if package.name not in retained_packages:
                continue
            fields = run('dpkg-deb', '-f', str(package), text=True).rstrip()
            entries.append(fields + f'\nFilename: {package.relative_to(apt)}\nSize: {package.stat().st_size}\nSHA256: '
                           + sha256(package) + '\n\n')
        data = ''.join(entries).encode()
        for name, body in [('Packages', data), ('Packages.gz', gzip.compress(data, mtime=0))]:
            path = directory / name
            path.write_bytes(body)
            digest = hashlib.sha256(body).hexdigest()
            files.append((path.relative_to(indices), len(body), digest))
            hashes = apt / f'by-hash/binary-{arch}/SHA256'
            hashes.mkdir(parents=True, exist_ok=True)
            (hashes / digest).write_bytes(body)
        (directory / 'by-hash').symlink_to(f'../../../../by-hash/binary-{arch}')
    release = ('Origin: Battlefront Eternal\nLabel: Battlefront Eternal previews\nSuite: stable\nCodename: stable\n'
               'Architectures: amd64 arm64\nComponents: main\nAcquire-By-Hash: yes\n'
               'Description: Battlefront Eternal development builds\n'
               + 'Date: ' + dt.datetime.now(dt.timezone.utc).strftime('%a, %d %b %Y %H:%M:%S +0000') + '\nSHA256:\n'
               + ''.join(f' {digest} {size:16d} {path}\n' for path, size, digest in files))
    (indices / 'Release').write_text(release)
    sign(indices / 'Release')
    run('gpg', '--homedir', KEYHOME, '--batch', '--yes', '--local-user', KEY, '--digest-algo', 'SHA256',
        '--output', str(indices / 'Release.gpg'), '--detach-sign', str(indices / 'Release'))
    keys = ROOT / 'keys'
    keys.mkdir(exist_ok=True)
    (keys / 'battlefront-eternal.asc').write_bytes(run('gpg', '--homedir', KEYHOME, '--armor', '--export', KEY))
    (keys / 'battlefront-eternal.asc').chmod(0o644)
    dists = apt / 'dists'
    dists.mkdir(exist_ok=True)
    for parent, name, target in [(dists, 'stable', '../indices/' + release_id), (downloads, 'latest', release_id)]:
        temporary = parent / ('.' + name + '-next')
        temporary.unlink(missing_ok=True)
        temporary.symlink_to(target)
        temporary.replace(parent / name)
    # Installed clients see the release once everything it names is live.
    if updates:
        platforms, content = updates
        release = {'schema': 1, 'channel': CHANNEL, 'version': version.replace('~', '-'), 'release_id': release_id,
                   'revision': revision, 'date': date, 'content': content, 'platforms': platforms}
        write_atomic(downloads / 'updates' / f'{CHANNEL}.json', (json.dumps(sign_update(json.dumps(release).encode())) + '\n').encode())
    # Prune only after the new signed catalogue is live. Shared by-hash files
    # are small and retained so previously fetched Release files still resolve.
    for directory in generations[keep_count:]:
        shutil.rmtree(directory)
        shutil.rmtree(apt / 'indices' / directory.name, ignore_errors=True)
    for package in pool.glob('*.deb'):
        if package.name not in retained_packages:
            package.unlink()
    if updates:
        collect_blobs(downloads / 'updates' / 'blobs', retained)
    print('Published', release_id, 'with signed APT indices, download checksums' + (' and the signed update channel ' + CHANNEL if updates else ''))

def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    with (ROOT / '.publish.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        with tempfile.TemporaryDirectory(prefix='bfe-distribution-') as work:
            source = Path(os.environ.get('BFE_RELEASE_DIR', work))
            if 'BFE_RELEASE_DIR' not in os.environ:
                download_release(source)
            publish(source, os.environ['BFE_PACKAGE_VERSION'], os.environ['BFE_RELEASE_TAG'],
                    os.environ['BFE_REVISION'], os.environ['BFE_BUILD_DATE'])

if __name__ == '__main__':
    main()
