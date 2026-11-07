// Native libraries live only at advertised, hashed host target paths. Scan the
// actual resource tree too: a forgotten development library is not harmless.
import { readdir, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join, relative } from 'node:path';
export async function nativeFiles(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await nativeFiles(path));
    else if (entry.name.endsWith('.node')) result.push(path);
  }
  return result;
}
export async function checkHostRuntime(root) {
  const manifest = JSON.parse(await readFile(join(root, 'engine/host-runtime.json')));
  if (manifest.format !== 2 || manifest.serverAdapterABI !== 5 || manifest.addonABI !== 2 || manifest.storeABI !== 1 || manifest.napiMinimum !== 8 || Object.hasOwn(manifest, 'worldBackend') || !Array.isArray(manifest.targets) || !/^[a-f0-9]{64}$/.test(manifest.operationTable)) throw Error('Invalid packaged host format');
  // The hashed package record anchors WASM builds as well as native targets.
  // A plausible manifest identity must not relabel a different staged build.
  const packageBytes = await readFile(join(root, 'engine/package.json'));
  if (createHash('sha256').update(packageBytes).digest('hex') !== manifest.artifacts?.['engine/package.json']) throw Error('Packaged host package hash mismatch');
  const packaged = JSON.parse(packageBytes), version = JSON.parse(await readFile(join(root, 'version.json')));
  if (!/^[a-f0-9]{64}$/.test(manifest.engineBuild) || packaged.bfeEngineBuild !== manifest.engineBuild || version.engineBuild !== manifest.engineBuild) throw Error('Packaged host engine build mismatch');
  const allowed = new Set();
  for (const entry of manifest.targets) {
    if (entry.target !== 'linux-x64-gnu' || entry.artifact !== `engine/native/${entry.target}/bfe-node.node` || entry.glibcMinimum !== '2.28' || entry.napiMinimum !== 8 || entry.addonABI !== 2 || allowed.has(entry.artifact)) throw Error('Invalid packaged host target');
    const bytes = await readFile(join(root, entry.artifact));
    const hash = createHash('sha256').update(bytes).digest('hex');
    if (manifest.artifacts?.[entry.artifact] !== hash) throw Error('Packaged host addon hash mismatch');
    allowed.add(entry.artifact);
  }
  for (const file of await nativeFiles(root)) {
    if (!allowed.has(relative(root, file).replaceAll('\\', '/'))) throw Error('Unexpected native host artifact');
  }
  return manifest;
}
