// Portable prepared content: ZIP64, with an explicit version and SHA-256 for
// every byte-bearing file. Engine code, saves, identities and locks stay out.
import { createReadStream, createWriteStream, constants, promises as fs } from 'node:fs';
import { createHash, randomUUID } from 'node:crypto';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import path from 'node:path';
import yazl from 'yazl';
import yauzl from 'yauzl';
import { CONTENT_ROOTS } from './content.js';

const MANIFEST = 'bfe-content-manifest.json';
const RECEIPT = '.bfe-import.json';
const MAX_FILES = 300000, MAX_BYTES = 256 * 1024 ** 3, MAX_MANIFEST = 64 * 1024 ** 2;
const FORMAT = 'battlefront-eternal-content';
const abort = signal => signal?.throwIfAborted();
const digest = () => createHash('sha256');

function safePath(name, contentOnly = true) {
  if (typeof name !== 'string' || name.length > 1024 || /[\\\x00-\x1f:]/.test(name)) throw Error('Invalid archive path');
  const parts = name.split('/');
  if (parts.some(p => !p || p === '.' || p === '..' || /[. ]$/.test(p) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p))) throw Error(`Unsafe archive path: ${name}`);
  if (contentOnly && name !== '.prepare.json' && !(CONTENT_ROOTS.includes(parts[0]) && parts.length > 1)) throw Error(`Not prepared game content: ${name}`);
  return name;
}

async function walk(root, relative = '') {
  const result = [];
  for (const entry of await fs.readdir(path.join(root, relative), { withFileTypes: true })) {
    const name = relative ? `${relative}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw Error(`Cannot export a symbolic link: ${name}`);
    if (entry.isDirectory()) result.push(...await walk(root, name));
    else if (entry.isFile()) result.push(name);
    else throw Error(`Cannot export a special file: ${name}`);
  }
  return result.sort();
}

export async function hashFile(file, { signal, onBytes = () => {} } = {}) {
  const hash = digest();
  for await (const chunk of createReadStream(file, { signal })) { hash.update(chunk); onBytes(chunk.length); }
  return hash.digest('hex');
}

async function sourceFingerprint(root, signal) {
  const files = [];
  for (const name of (await walk(root)).filter(name => /\.lvl$/i.test(name))) {
    abort(signal); safePath(name, false);
    const stat = await fs.stat(path.join(root, name));
    files.push({ path: name, size: stat.size, sha256: await hashFile(path.join(root, name), { signal }) });
  }
  return files.length ? { base: 'GameData', files } : null;
}

async function provenance(root, signal) {
  const state = await fs.readFile(path.join(root, '.prepare.json'), 'utf8').then(JSON.parse).catch(() => null);
  if (typeof state?.game === 'string' && path.basename(state.game).toLowerCase() === '_lvl_pc') {
    const gameData = path.dirname(path.dirname(state.game));
    if (await fs.stat(gameData).then(s => s.isDirectory()).catch(() => false)) return sourceFingerprint(gameData, signal);
  }
  return fs.readFile(path.join(root, RECEIPT), 'utf8').then(JSON.parse).then(receipt => receipt.source || null).catch(() => null);
}

function validateManifest(value) {
  if (value?.format !== FORMAT || value.version !== 1 || !Array.isArray(value.files) || !value.files.length || value.files.length > MAX_FILES) throw Error('Unsupported or invalid content export');
  let bytes = 0;
  const seen = new Map(), folded = new Map();
  for (const file of value.files) {
    safePath(file.path);
    const key = file.path.normalize('NFC').toLowerCase();
    if (seen.has(file.path)) throw Error(`Duplicate archive path: ${file.path}`);
    seen.set(file.path, file);
    const previous = folded.get(key);
    if (previous && !(file.linkTo === previous.path && file.sha256 === previous.sha256 && file.size === previous.size)) throw Error(`Conflicting case variants: ${file.path}`);
    if (!previous) folded.set(key, file);
    if (!Number.isSafeInteger(file.size) || file.size < 0 || !/^[a-f0-9]{64}$/.test(file.sha256)) throw Error(`Invalid checksum record: ${file.path}`);
    bytes += file.size;
    if (bytes > MAX_BYTES) throw Error('Content export is too large');
  }
  for (const file of value.files) {
    if (!file.linkTo) continue;
    safePath(file.linkTo);
    const target = seen.get(file.linkTo);
    if (!target || target.linkTo || target === file || target.sha256 !== file.sha256 || target.size !== file.size) throw Error(`Invalid content alias: ${file.path}`);
  }
  if (!seen.has('common/common.json') || ![...seen.keys()].some(name => /^maps\/[^/]+\/map\.json$/.test(name))) throw Error('Export has no playable prepared maps');
  if (value.source != null) {
    if (value.source.base !== 'GameData' || !Array.isArray(value.source.files) || value.source.files.length > MAX_FILES) throw Error('Invalid source fingerprint');
    for (const file of value.source.files) {
      safePath(file.path, false);
      if (!Number.isSafeInteger(file.size) || file.size < 0 || !/^[a-f0-9]{64}$/.test(file.sha256)) throw Error('Invalid source checksum');
    }
  }
  return bytes;
}

async function diskSpace(dir, needed) {
  const stat = await fs.statfs(dir);
  if (stat.bavail * stat.bsize < needed + 32 * 1024 ** 2) throw Error(`Not enough free disk space. About ${Math.ceil(needed / 1024 ** 3)} GB is needed.`);
}

export async function exportContent(root, destination, { signal, onProgress = () => {} } = {}) {
  root = await fs.realpath(root);
  destination = path.resolve(destination);
  // Resolve junctions, short Windows names and parent symlinks before comparing.
  await fs.mkdir(path.dirname(destination), { recursive: true });
  destination = path.join(await fs.realpath(path.dirname(destination)), path.basename(destination));
  const relative = path.relative(root, destination);
  if (!relative || (relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative))) throw Error('Save the export outside the game data folder');
  let names = [];
  for (const dir of CONTENT_ROOTS) {
    const folder = path.join(root, dir);
    const stat = await fs.lstat(folder).catch(() => null);
    if (stat?.isSymbolicLink()) throw Error(`Cannot export a symbolic link: ${dir}`);
    if (stat?.isDirectory()) names.push(...(await walk(root, dir)));
  }
  if (await fs.lstat(path.join(root, '.prepare.json')).then(s => s.isFile() && !s.isSymbolicLink()).catch(() => false)) names.push('.prepare.json');
  if (names.length > MAX_FILES) throw Error('Too many content files');
  const files = [];
  const inodes = new Map(), cases = new Map();
  let total = 0;
  for (const name of names.sort()) {
    safePath(name);
    const stat = await fs.lstat(path.join(root, name));
    if (!stat.isFile() || stat.isSymbolicLink()) throw Error(`Content changed while exporting: ${name}`);
    total += stat.size;
    const file = { path: name, size: stat.size, mtime: stat.mtimeMs };
    const inode = `${stat.dev}:${stat.ino}`;
    const linked = stat.nlink > 1 ? inodes.get(inode) : null;
    const variant = cases.get(name.normalize('NFC').toLowerCase());
    const original = linked || variant;
    if (original) { file.linkTo = original.linkTo || original.path; file.hardlink = !!linked; }
    else { inodes.set(inode, file); cases.set(name.normalize('NFC').toLowerCase(), file); }
    files.push(file);
  }
  const filesByPath = new Map(files.map(file => [file.path, file]));
  let done = 0;
  const progress = (phase, file) => onProgress({ phase, file, done, total });
  for (const file of files) {
    abort(signal);
    if (file.hardlink) {
      file.sha256 = filesByPath.get(file.linkTo).sha256;
      done += file.size; progress('Checking content', file.path);
    } else file.sha256 = await hashFile(path.join(root, file.path), { signal, onBytes: count => { done += count; progress('Checking content', file.path); } });
  }
  progress('Recording original source hashes', '');
  const manifest = { format: FORMAT, version: 1, created: new Date().toISOString(), source: await provenance(root, signal), files };
  validateManifest(manifest);
  const encoded = Buffer.from(JSON.stringify(manifest));
  if (encoded.length > MAX_MANIFEST) throw Error('Content manifest is too large');
  await fs.mkdir(path.dirname(destination), { recursive: true });
  await diskSpace(path.dirname(destination), total);
  const temp = destination + '.' + randomUUID() + '.partial';
  const zip = new yazl.ZipFile();
  zip.on('error', error => zip.outputStream.destroy(error));
  done = 0;
  const writing = pipeline(zip.outputStream, createWriteStream(temp, { flags: 'wx' }), { signal });
  // Attach immediately: errors during setup must not become unhandled rejections.
  writing.catch(() => {});
  try {
    zip.addBuffer(encoded, MANIFEST);
    for (const file of files) {
      if (file.linkTo) {
        const current = await fs.lstat(path.join(root, file.path));
        if (!current.isFile() || current.size !== file.size || current.mtimeMs !== file.mtime) throw Error(`Content changed while exporting: ${file.path}`);
        if (file.hardlink) {
          const original = await fs.stat(path.join(root, file.linkTo));
          if (current.dev !== original.dev || current.ino !== original.ino) throw Error(`Content link changed while exporting: ${file.path}`);
        }
        done += file.size;
        continue;
      }
      zip.addReadStreamLazy(file.path, { size: file.size, mtime: new Date(file.mtime), mode: 0o100644, compressionLevel: 3 }, callback => {
        const hash = digest(); let size = 0;
        const checking = new Transform({
          transform(chunk, _encoding, next) { hash.update(chunk); size += chunk.length; done += chunk.length; progress('Exporting content', file.path); next(null, chunk); },
          flush(next) { next(size !== file.size || hash.digest('hex') !== file.sha256 ? Error(`Content changed while exporting: ${file.path}. Try again after preparation finishes.`) : null); },
        });
        pipeline(createReadStream(path.join(root, file.path), { signal }), checking, { signal }).catch(error => zip.outputStream.destroy(error));
        callback(null, checking);
      });
    }
    zip.end();
    await writing;
    abort(signal);
    await fs.rename(temp, destination);
    return { path: destination, files: files.length, bytes: total, manifest };
  } catch (error) {
    zip.outputStream.destroy();
    await writing.catch(() => {});
    await fs.rm(temp, { force: true });
    throw error;
  }
}

export async function importContent(archive, parent, { signal, onProgress = () => {} } = {}) {
  await fs.mkdir(parent, { recursive: true });
  const stage = await fs.mkdtemp(path.join(parent, '.import-'));
  let zip;
  try {
    zip = await yauzl.openPromise(archive, { autoClose: false, strictFileNames: true, validateEntrySizes: true });
    const entries = [];
    let manifestEntry;
    for await (const entry of zip.eachEntry()) {
      abort(signal);
      if (entries.length > MAX_FILES) throw Error('Too many archive entries');
      const type = (entry.externalFileAttributes >>> 16) & 0o170000;
      if (type && type !== 0o100000) throw Error('Only regular files are allowed in content exports');
      if (entry.isEncrypted()) throw Error('Encrypted content exports are not supported');
      if (entry.fileName === MANIFEST) {
        if (manifestEntry || entry.uncompressedSize > MAX_MANIFEST) throw Error('Invalid content manifest');
        manifestEntry = entry;
      } else { safePath(entry.fileName); entries.push(entry); }
    }
    if (!manifestEntry) throw Error('This file is not a Battlefront Eternal content export');
    const chunks = [];
    for await (const chunk of await zip.openReadStreamPromise(manifestEntry)) { abort(signal); chunks.push(chunk); }
    const manifest = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    const total = validateManifest(manifest);
    const storedFiles = manifest.files.filter(file => !file.linkTo);
    if (entries.length !== storedFiles.length) throw Error('Archive files do not match the manifest');
    await diskSpace(parent, total);
    const expected = new Map(storedFiles.map(file => [file.path, file]));
    let done = 0;
    for (const entry of entries) {
      abort(signal);
      const file = expected.get(entry.fileName);
      if (!file || file.size !== entry.uncompressedSize) throw Error(`Unexpected archive entry: ${entry.fileName}`);
      expected.delete(entry.fileName);
      const target = path.join(stage, file.path);
      await fs.mkdir(path.dirname(target), { recursive: true });
      const hash = digest();
      const checking = new Transform({ transform(chunk, _encoding, next) { hash.update(chunk); done += chunk.length; onProgress({ phase: 'Importing and verifying', file: file.path, done, total }); next(null, chunk); } });
      await pipeline(await zip.openReadStreamPromise(entry), checking, createWriteStream(target, { flags: 'wx', mode: 0o644 }), { signal });
      if (hash.digest('hex') !== file.sha256) throw Error(`Checksum mismatch: ${file.path}. The export is damaged or incomplete.`);
      if (Number.isFinite(file.mtime)) await fs.utimes(target, new Date(), new Date(file.mtime));
    }
    if (expected.size) throw Error('Missing content files');
    for (const file of manifest.files.filter(file => file.linkTo)) {
      abort(signal);
      const source = path.join(stage, file.linkTo), target = path.join(stage, file.path);
      await fs.mkdir(path.dirname(target), { recursive: true });
      // Windows and default macOS filesystems may already resolve a case alias
      // to the canonical file. Accept it only after checking its bytes.
      if (await fs.lstat(target).catch(() => null)) {
        if (await hashFile(target, { signal }) !== file.sha256) throw Error(`Conflicting content alias: ${file.path}`);
      } else if (file.hardlink) {
        try { await fs.link(source, target); }
        catch (error) {
          if (!['EPERM', 'EOPNOTSUPP', 'ENOTSUP', 'EXDEV'].includes(error.code)) throw error;
          await fs.copyFile(source, target, constants.COPYFILE_EXCL);
        }
      } else await fs.copyFile(source, target, constants.COPYFILE_EXCL);
      done += file.size;
      onProgress({ phase: 'Restoring shared content', file: file.path, done, total });
    }
    // Retain provenance outside the playable roots; no identity or preferences.
    await fs.writeFile(path.join(stage, RECEIPT), JSON.stringify(manifest), { flag: 'wx' });
    abort(signal);
    const destination = path.join(parent, 'content-' + randomUUID());
    await fs.rename(stage, destination);
    return { path: destination, files: manifest.files.length, bytes: total };
  } catch (error) {
    await fs.rm(stage, { recursive: true, force: true });
    throw error;
  } finally { zip?.close(); }
}

// When the player later attaches their retail installation, reuse the recorded
// preparation checkpoints only if the source archives still match byte-for-byte.
// New add-ons are allowed; prepare.py invalidates their dependent steps itself.
export async function relocatePreparation(content, install, { signal, onProgress = () => {} } = {}) {
  const receipt = await fs.readFile(path.join(content, RECEIPT), 'utf8').then(JSON.parse).catch(() => null);
  if (!receipt?.source?.files?.length) return false;
  validateManifest(receipt);
  for (const file of receipt.source.files) {
    abort(signal);
    const target = path.join(install, 'GameData', file.path);
    onProgress({ phase: 'Checking original game archives', file: file.path });
    if ((await fs.stat(target).catch(() => null))?.size !== file.size || await hashFile(target, { signal }) !== file.sha256) return false;
  }
  const statePath = path.join(content, '.prepare.json');
  const state = await fs.readFile(statePath, 'utf8').then(JSON.parse).catch(() => null);
  if (!state) return false;
  state.game = path.join(install, 'GameData', 'Data', '_LVL_PC');
  const temp = statePath + '.' + randomUUID() + '.tmp';
  await fs.writeFile(temp, JSON.stringify(state, null, 1));
  await fs.rename(temp, statePath);
  return true;
}
