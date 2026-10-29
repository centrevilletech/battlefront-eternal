// Operator-owned gameplay data only. This Node module never fetches data online.
import { constants } from 'node:fs';
import { lstat, mkdir, open, readFile, realpath, rename, rm } from 'node:fs/promises';
import { basename, dirname, join, resolve, sep } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { contentHash } from './content-hash.js';

export const BUNDLE_SCHEMA = 'bfe-gameplay-bundle-1';
const MAX_FILES = 10000, MAX_FILE = 256 * 1024 ** 2, MAX_TOTAL = 8 * 1024 ** 3;
const MAP = /^(swbf[12])\/([a-z0-9_-]{1,64}):([a-z0-9_-]{1,64}):([a-z0-9_-]{1,64})$/;
const HASH = /^[0-9a-f]{64}$/;

function dataPath(path) {
  if (typeof path !== 'string' || path.length > 240 || !/^(common|maps|sides)\/[a-zA-Z0-9_./-]+\.(json|bin)$/.test(path) ||
      path.split('/').some(part => !part || part === '.' || part === '..')) throw Error('Invalid gameplay file path');
  return path;
}
function version(value) {
  if (typeof value !== 'string' || !/^[a-zA-Z0-9_.+-]{1,128}$/.test(value)) throw Error('Invalid bundle version');
  return value;
}
async function safeFile(root, path) {
  if ((await lstat(root)).isSymbolicLink()) throw Error('Gameplay bundle symlinks are forbidden');
  const base = await realpath(root);
  let current = base;
  for (const part of path.split('/')) {
    current = join(current, part);
    const entry = await lstat(current);
    if (entry.isSymbolicLink()) throw Error('Gameplay bundle symlinks are forbidden');
  }
  const actual = await realpath(current);
  if (!actual.startsWith(base + sep)) throw Error('Gameplay path escapes root');
  const file = await open(actual, constants.O_RDONLY | constants.O_NOFOLLOW);
  const stat = await file.stat();
  if (!stat.isFile() || stat.size > MAX_FILE) { await file.close(); throw Error('Invalid gameplay file size/type'); }
  return { file, size: stat.size };
}
async function copyHashed(root, path, target) {
  const { file, size } = await safeFile(root, path);
  let output, count = 0;
  const digest = createHash('sha256');
  try {
    await mkdir(dirname(target), { recursive: true, mode: 0o700 });
    output = await open(target, 'wx', 0o600);
    for await (const chunk of file.createReadStream({ autoClose: false })) {
      count += chunk.length;
      if (count > MAX_FILE || count > size) throw Error('Gameplay file changed while reading');
      digest.update(chunk); await output.writeFile(chunk);
    }
    if (count !== size) throw Error('Gameplay file changed while reading');
    await output.sync();
    return { path, size, sha256: digest.digest('hex') };
  } finally { await output?.close(); await file.close(); }
}
function validateManifest(manifest) {
  if (manifest?.schema !== BUNDLE_SCHEMA) throw Error('Unsupported gameplay bundle schema');
  version(manifest.extractorVersion); version(manifest.simulationVersion);
  if (!Array.isArray(manifest.files) || !manifest.files.length || manifest.files.length > MAX_FILES) throw Error('Invalid gameplay file count');
  const paths = new Set(); let bytes = 0;
  for (const file of manifest.files) {
    dataPath(file.path);
    if (paths.has(file.path) || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_FILE || !HASH.test(file.sha256)) throw Error('Invalid gameplay file manifest');
    paths.add(file.path); bytes += file.size;
  }
  if (bytes > MAX_TOTAL) throw Error('Gameplay bundle exceeds size limit');
  if (!Array.isArray(manifest.maps) || !manifest.maps.length || manifest.maps.length > 256) throw Error('Invalid bundle map count');
  const maps = new Set();
  for (const map of manifest.maps) {
    const match = MAP.exec(map.id);
    if (!match || maps.has(map.id) || !/^bfe-content-sha256-[0-9a-f]{64}$/.test(map.contentHash)) throw Error('Invalid bundle map identity');
    maps.add(map.id);
    if (!Array.isArray(map.dependencies) || !map.dependencies.length || map.dependencies.length > MAX_FILES || new Set(map.dependencies).size !== map.dependencies.length ||
        map.dependencies.some(path => !paths.has(path)) || !map.dependencies.includes(`maps/${match[2]}/map.json`)) throw Error('Missing gameplay dependency');
  }
  return manifest;
}
async function mapHash(root, id) {
  const match = MAP.exec(id);
  if (!match) throw Error('Invalid map identifier');
  return contentHash(`?map=${match[2]}&side=${match[3]}`, async url => {
    const path = dataPath(url.slice(1));
    try {
      const { file } = await safeFile(root, path);
      try { return new Response(await file.readFile()); } finally { await file.close(); }
    } catch (error) {
      if (error.code === 'ENOENT') return new Response('', { status: 404 });
      throw error;
    }
  });
}
async function stageFor(destination) {
  const requested = resolve(destination);
  await mkdir(dirname(requested), { recursive: true, mode: 0o700 });
  const parent = await realpath(dirname(requested));
  const target = join(parent, basename(requested));
  const stage = `${target}.staging-${randomUUID()}`;
  await mkdir(stage, { mode: 0o700 });
  return { target, stage };
}
async function activate(stage, target, replace) {
  // Serialize operator imports to this destination; validation happens first.
  const lock = `${target}.import-lock`, previous = `${target}.previous-${randomUUID()}`;
  await mkdir(lock, { mode: 0o700 });
  let moved = false;
  try {
    let old;
    try { old = await lstat(target); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (old && (!replace || !old.isDirectory() || old.isSymbolicLink())) throw Error('Bundle destination already exists or is unsafe');
    if (old) { await rename(target, previous); moved = true; }
    try { await rename(stage, target); }
    catch (error) { if (moved) await rename(previous, target); throw error; }
    if (moved) await rm(previous, { recursive: true, force: true }).catch(() => {});
  } finally { await rm(lock, { recursive: true, force: true }); }
}

export async function exportBundle({ root, files, destination, maps, extractorVersion, simulationVersion }) {
  version(extractorVersion); version(simulationVersion);
  if (!Array.isArray(files) || !files.length || files.length > MAX_FILES) throw Error('Invalid gameplay file list');
  const paths = [...new Set(files.map(dataPath))].sort();
  if (paths.length !== files.length) throw Error('Duplicate gameplay file path');
  if (!Array.isArray(maps) || !maps.length || maps.length > 256 || new Set(maps).size !== maps.length || maps.some(id => !MAP.test(id))) throw Error('Invalid map list');
  const { target, stage } = await stageFor(destination);
  try {
    const manifest = { schema: BUNDLE_SCHEMA, extractorVersion, simulationVersion, maps: [], files: [] };
    let total = 0;
    for (const path of paths) {
      const entry = await copyHashed(root, path, join(stage, 'files', path));
      total += entry.size; if (total > MAX_TOTAL) throw Error('Gameplay bundle exceeds size limit');
      manifest.files.push(entry);
    }
    for (const id of maps) manifest.maps.push({ id, contentHash: await mapHash(join(stage, 'files'), id), dependencies: paths });
    validateManifest(manifest);
    const output = await open(join(stage, 'manifest.json'), 'wx', 0o600);
    try { await output.writeFile(JSON.stringify(manifest, null, 2) + '\n'); await output.sync(); } finally { await output.close(); }
    await activate(stage, target, false);
    return { destination: target, manifest };
  } finally { await rm(stage, { recursive: true, force: true }); }
}

export async function importBundle({ bundle, destination, replace = false, simulationVersion }) {
  const requested = resolve(bundle);
  if ((await lstat(requested)).isSymbolicLink()) throw Error('Bundle root symlink forbidden');
  const source = await realpath(requested);
  const { file, size } = await safeFile(source, 'manifest.json');
  let manifest;
  try {
    if (size > 8 * 1024 ** 2) throw Error('Bundle manifest exceeds size limit');
    manifest = validateManifest(JSON.parse(await file.readFile('utf8')));
  } finally { await file.close(); }
  if (simulationVersion !== undefined && manifest.simulationVersion !== simulationVersion) throw Error('Bundle requires a different simulation data version');
  const { target, stage } = await stageFor(destination);
  if (source === target || source.startsWith(target + sep) || target.startsWith(source + sep)) {
    await rm(stage, { recursive: true, force: true }); throw Error('Bundle and imported data paths must be separate');
  }
  try {
    for (const expected of manifest.files) {
      const actual = await copyHashed(join(source, 'files'), expected.path, join(stage, expected.path));
      if (actual.size !== expected.size || actual.sha256 !== expected.sha256) throw Error(`Gameplay integrity check failed: ${expected.path}`);
    }
    for (const map of manifest.maps) if (await mapHash(stage, map.id) !== map.contentHash) throw Error(`Gameplay compatibility hash failed: ${map.id}`);
    // This record is metadata, never executable code or a server configuration.
    const record = await open(join(stage, 'bundle-manifest.json'), 'wx', 0o600);
    try { await record.writeFile(JSON.stringify(manifest, null, 2) + '\n'); } finally { await record.close(); }
    await activate(stage, target, replace);
    return { destination: target, manifest };
  } finally { await rm(stage, { recursive: true, force: true }); }
}
