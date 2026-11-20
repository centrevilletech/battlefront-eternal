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

ROOT = Path(os.environ.get('BFE_DISTRIBUTION', '/deployment/distribution'))
KEYHOME = os.environ.get('BFE_SIGNING_HOME', '/deployment/signing')
KEY = 'F210B0710D270963CB641C118043BCB331D34C9B'
# Desktop updates (docs/desktop-updates.md): the channel installed clients
# follow, signed with an Ed25519 key whose public half ships in the client
# (rust/bfe-desktop/src/update/keys.rs).
CHANNEL = os.environ.get('BFE_UPDATE_CHANNEL', 'development-preview')
UPDATE_KEY = os.environ.get('BFE_UPDATE_KEY', str(Path(KEYHOME) / 'update-ed25519.pem'))
UPDATE_PLATFORMS = ('linux-x64', 'linux-arm64', 'win32-x64')
# A Mac build is signed and notarized after the release it belongs to is
# built, so it joins a release that is already live (publish_macos).
MAC_PLATFORMS = ('darwin-arm64', 'darwin-x64')
MAC_RECORD = 'macos-notarized.json'
MAC_TICKET = 'Battlefront Eternal.app/Contents/CodeResources'
# The update keys a disk image of each kind serves.
MAC_ARCH_PLATFORMS = {'arm64': {'darwin-arm64'}, 'x64': {'darwin-x64'}, 'universal': {'darwin-arm64', 'darwin-x64'}}
# The signing Mac signs its record with an Ed25519 key that never leaves it;
# this is the public half, installed with the publisher. Whoever can write
# the place the Mac files are handed over in can write a record; only the
# signing Mac can sign one. Without this file no Mac build is published.
MAC_RECORD_KEY = Path(os.environ.get('BFE_MAC_RECORD_KEY') or Path(__file__).resolve().parent / 'release-host' / 'macos-record.pub.pem')
MAC_SIGNATURE = MAC_RECORD + '.sig'
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

SAFE_PART = re.compile(r'[^\\/:*?"<>|\x00-\x1f]+')

def safe_relative(value):
    return (isinstance(value, str) and 0 < len(value) <= 512
            and all(part not in ('', '.', '..') and SAFE_PART.fullmatch(part) for part in value.split('/')))

# The dedicated server archive (build/native/server-archive.py,
# docs/server-updates.md): its root entries, and files every archive holds for
# the native bfeserver binary.
SERVER_ENTRIES = {'bfe-tree.json', 'bfeserver', 'install-server.sh', 'legal', 'toolchain', 'tools'}
NATIVE_SERVER_FILES = ('bfeserver', 'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json')

def archive_relative(name):
    return isinstance(name, str) and '\x00' not in name and all(part not in ('', '.', '..') for part in name.split('/'))

def check_server_archive(path):
    """Only files, folders and links inside the server's own entries: nothing
    the archive could write outside the folder it is unpacked into."""
    names = set()
    modes = {}
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
            if member.isfile():
                modes[name] = member.mode
            names.add(name)
    for file in NATIVE_SERVER_FILES:
        if file not in names:
            raise ValueError(f'{path.name} lacks {file}')
    if not (modes.get('bfeserver', 0) & 0o111):
        raise ValueError(f'bfeserver in {path.name} is not executable')

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
    by_path = {}
    for item in files:
        if (not isinstance(item, dict) or not safe_relative(item.get('path')) or not re.fullmatch(r'[a-f0-9]{64}', str(item.get('sha256')))
                or not isinstance(item.get('size'), int) or not isinstance(item.get('zsize'), int) or item.get('mode') not in (0o644, 0o755)):
            raise ValueError(f'Malformed update manifest entry for {platform}')
        if item['path'] in by_path:
            raise ValueError(f"Update manifest for {platform} names {item['path']} twice")
        by_path[item['path']] = item
    if tree['executable'] not in by_path or by_path[tree['executable']]['mode'] != 0o755:
        raise ValueError(f'Update manifest for {platform} has no executable')
    if 'server_executable' in tree:
        server_executable = tree.get('server_executable')
        if not safe_relative(server_executable) or server_executable not in by_path or by_path[server_executable]['mode'] != 0o755:
            raise ValueError(f'Update manifest for {platform} has an invalid server executable')
    linked = set()
    for link in tree.get('links', []):
        target = link.get('target') if isinstance(link, dict) else None
        if not safe_relative(link.get('path')) or not isinstance(target, str) or target.startswith('/') or not safe_relative(
                os.path.normpath(os.path.join(os.path.dirname(link['path']), target)).replace(os.sep, '/')):
            raise ValueError(f'Unsafe link in the update manifest for {platform}')
        if link['path'] in by_path or link['path'] in linked:
            raise ValueError(f"Update manifest for {platform} names {link['path']} twice")
        linked.add(link['path'])
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
        platforms[platform], tree = ingest_platform(path, platform, stage, version, release_id, store)
        content = content or tree.get('content')
    return platforms, content

def ingest_platform(path, platform, stage, version, release_id, store):
    """One platform's payload: its new blobs into the store, its manifest
    into `stage`. Returns the signed release's entry for the platform and the
    platform's tree. Nothing reaches the store unless the whole payload is
    sound."""
    incoming = Path(tempfile.mkdtemp(prefix='.incoming-', dir=store.parent))
    try:
        tree = read_payload(path, platform, version, store, incoming)
        return store_payload(tree, platform, stage, release_id, store, incoming), tree
    finally:
        shutil.rmtree(incoming, ignore_errors=True)

def read_payload(path, platform, version, store, incoming):
    """Reads a payload without touching the store: returns its tree, checked,
    and leaves the blobs the store lacks in `incoming`, each checked against
    the hash it is named by."""
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
            waiting = incoming / f'{digest}.zst'
            if (store / digest[:2] / f'{digest}.zst').exists() or waiting.exists():
                continue
            with waiting.open('wb') as output:
                shutil.copyfileobj(stream, output)
            if zstd_digest(waiting) != digest:
                raise ValueError(f'Update blob {digest} does not hold what its name says')
    tree = check_tree(tree, platform, version)
    for item in tree['files']:
        held = [blob for blob in (store / item['sha256'][:2] / f"{item['sha256']}.zst", incoming / f"{item['sha256']}.zst") if blob.exists()]
        if not held or held[0].stat().st_size != item['zsize']:
            raise ValueError(f"Update blob for {item['path']} ({platform}) is missing from the store")
    return tree

def store_payload(tree, platform, stage, release_id, store, incoming):
    """Moves a read payload's new blobs into the store and writes its manifest
    into `stage`. Returns the signed release's entry for the platform."""
    unique = {}
    for item in tree['files']:
        waiting, target = incoming / f"{item['sha256']}.zst", store / item['sha256'][:2] / f"{item['sha256']}.zst"
        if waiting.exists() and not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            waiting.chmod(0o644)
            os.replace(waiting, target)
        unique[item['sha256']] = item['zsize']
    packed = subprocess.run(['zstd', '-19', '-q', '-c'], input=json.dumps(tree).encode(), stdout=subprocess.PIPE, check=True).stdout
    name = f'update-{platform}.json.zst'
    (stage / name).write_bytes(packed)
    entry = {'manifest': f'{release_id}/{name}', 'sha256': hashlib.sha256(packed).hexdigest(),
             'files': len(tree['files']), 'size': sum(item['size'] for item in tree['files']),
             'download': sum(unique.values())}
    if 'server_executable' in tree:
        entry['executable'] = tree['executable']
        entry['server_executable'] = tree['server_executable']
    return entry

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
    # The newest Mac build can belong to an older release than these. Its
    # folder and blobs stay for as long as the Mac download points at it;
    # its Debian packages leave the catalogue like any aged-out release's.
    mac_link = downloads / 'latest-macos'
    kept = retained + [p for p in generations[keep_count:] if mac_link.is_symlink() and p.name == os.readlink(mac_link)]
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
        if directory in kept:
            continue
        shutil.rmtree(directory)
        shutil.rmtree(apt / 'indices' / directory.name, ignore_errors=True)
    for package in pool.glob('*.deb'):
        if package.name not in retained_packages:
            package.unlink()
    if updates:
        collect_blobs(downloads / 'updates' / 'blobs', kept)
    print('Published', release_id, 'with signed APT indices, download checksums' + (' and the signed update channel ' + CHANNEL if updates else ''))

def verify_mac_record(record, signature):
    """Raises unless `signature` (base64 text) is the signing Mac's Ed25519
    signature of the record's bytes."""
    if not MAC_RECORD_KEY.is_file():
        raise ValueError(f'No public key for the Mac build record is installed ({MAC_RECORD_KEY})')
    try:
        raw = base64.b64decode(signature.strip(), validate=True)
    except (ValueError, TypeError):
        raw = b''
    with tempfile.TemporaryDirectory() as work:
        data, detached = Path(work) / 'record', Path(work) / 'signature'
        data.write_bytes(record)
        detached.write_bytes(raw)
        accepted = len(raw) == 64 and subprocess.run(
            ['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(MAC_RECORD_KEY), '-rawin', '-in', str(data), '-sigfile', str(detached)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    if not accepted:
        raise ValueError('The Mac build record is not signed by the signing Mac')

def read_mac_payload(path, platform, version, store, incoming):
    """A Mac update payload, read and checked as read_payload does, that must also hold the
    file stapling adds: an update without it was not made from the notarized app."""
    tree = read_payload(path, platform, version, store, incoming)
    if not any(item['path'] == MAC_TICKET for item in tree['files']):
        raise ValueError(f'The {platform} update lacks the notarization ticket')
    return tree

def publish_macos(source, check=False):
    """Adds a signed and notarized Mac build to the live release. `source`
    holds the disk image, one update payload per platform key, the signing
    step's record of them (macos-notarized.json) and the signing Mac's
    signature of that record. Nothing the release already has changes: the
    Mac files and their own signed checksums are added beside it, the channel
    file is signed again with the Mac platforms added, which is when
    installed Mac clients see it, and downloads/latest-macos points at the
    release. macos.json is written last and is what says the release has its
    Mac build: a step that stopped anywhere before it is simply run again.
    With `check` it makes every check and writes nothing: the release host
    does that before it attaches the files to the GitHub release."""
    os.umask(0o022)
    downloads = ROOT / 'downloads'
    raw = (source / MAC_RECORD).read_bytes()
    # Before anything the record names is read: is the record the signing Mac's own?
    verify_mac_record(raw, (source / MAC_SIGNATURE).read_text() if (source / MAC_SIGNATURE).is_file() else '')
    record = json.loads(raw)
    live = json.loads((downloads / 'latest/manifest.json').read_text())
    release_id, version = record.get('release_id'), record.get('version')
    if (record.get('schema') != 1 or release_id != live['release_id'] or version != live['version'].replace('~', '-')
            or record.get('revision') != live['revision']):
        raise ValueError('The Mac build is not for the live release')
    if record.get('notarized') is not True:
        raise ValueError('The Mac build is not notarized')
    generation = downloads / release_id
    if (generation / 'macos.json').exists():
        raise ValueError('The live release already has its Mac build')

    def named(item):
        # A file of the record, checked against the size and hash it gives.
        name = item.get('name') if isinstance(item, dict) else None
        if not isinstance(name, str) or Path(name).name != name or name.startswith('.'):
            raise ValueError('Invalid file name in the Mac build record')
        path = source / name
        if not path.is_file() or path.stat().st_size != item.get('size') or sha256(path) != item.get('sha256'):
            raise ValueError(f'{name} is not the file the Mac build record describes')
        return path

    image, updates = record.get('dmg'), record.get('updates')
    if not isinstance(image, dict) or image.get('arch') not in MAC_ARCH_PLATFORMS or not str(image.get('name')).endswith('.dmg'):
        raise ValueError('The Mac build record names no disk image')
    # An arm64 image comes with the arm64 update and nothing else; a universal one with both keys.
    if not isinstance(updates, dict) or set(updates) != MAC_ARCH_PLATFORMS[image['arch']]:
        raise ValueError(f"The Mac build record's update payloads are not those of a {image.get('arch')} image")
    image_file = named(image)
    store = downloads / 'updates' / 'blobs'
    if check:
        # Every payload read and checked as below, into a folder outside the distribution.
        with tempfile.TemporaryDirectory(prefix='bfe-macos-check-') as incoming:
            for platform in sorted(updates):
                read_mac_payload(named(updates[platform]), platform, version, store, Path(incoming))
        return
    # As publish() does: a release that was published without update payloads has no store yet.
    store.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix='.macos-', dir=downloads))
    incoming = Path(tempfile.mkdtemp(prefix='.incoming-', dir=store.parent))
    try:
        # Every payload is read and checked before one blob of any reaches the store.
        trees = {platform: read_mac_payload(named(updates[platform]), platform, version, store, incoming) for platform in sorted(updates)}
        entries = {platform: store_payload(tree, platform, stage, release_id, store, incoming) for platform, tree in trees.items()}
        public = f"Battlefront-Eternal-macos-{image['arch']}.dmg"
        shutil.copyfile(image_file, stage / public)
        done = (json.dumps({
            'version': version, 'release_id': release_id, 'file': public, 'sha256': image['sha256'], 'size': image['size'],
            'arch': image['arch'], 'platforms': sorted(entries), 'minimum_macos': record.get('minimum_macos'),
            'notarization': record.get('notarization')}, indent=2) + '\n').encode()
        (stage / 'SHA256SUMS-macos').write_text(''.join(sha256(p) + '  ' + p.name + '\n' for p in sorted(stage.iterdir()))
                                                + hashlib.sha256(done).hexdigest() + '  macos.json\n')
        sign(stage / 'SHA256SUMS-macos', True)
        for p in sorted(stage.iterdir()):
            p.chmod(0o644)
            os.replace(p, generation / p.name)
    finally:
        shutil.rmtree(stage, ignore_errors=True)
        shutil.rmtree(incoming, ignore_errors=True)
    channel = downloads / 'updates' / f'{CHANNEL}.json'
    signed = False
    if channel.exists():
        release = json.loads(base64.b64decode(json.loads(channel.read_text())['payload']))
        if release.get('release_id') == release_id:
            release['platforms'].update(entries)
            write_atomic(channel, (json.dumps(sign_update(json.dumps(release).encode())) + '\n').encode())
            signed = True
    temporary = downloads / '.latest-macos-next'
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(release_id)
    temporary.replace(downloads / 'latest-macos')
    # Last: until this file exists the release has no Mac build and the whole step may run again.
    write_atomic(generation / 'macos.json', done)
    print('Published the Mac build of', release_id, 'for', ', '.join(sorted(entries)) + (' and signed the update channel again' if signed else ''))

def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    with (ROOT / '.publish.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if os.environ.get('BFE_MACOS_DIR'):
            return publish_macos(Path(os.environ['BFE_MACOS_DIR']), check=os.environ.get('BFE_MACOS_CHECK') == '1')
        # The release's files, downloaded and checked by the caller (build/publish-public.py).
        publish(Path(os.environ['BFE_RELEASE_DIR']), os.environ['BFE_PACKAGE_VERSION'], os.environ['BFE_RELEASE_TAG'],
                os.environ['BFE_REVISION'], os.environ['BFE_BUILD_DATE'])

if __name__ == '__main__':
    main()
