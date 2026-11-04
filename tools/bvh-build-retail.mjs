// Verify every tree built while a real headless battle loads and steps.
// --out optionally retains a closed, checksummed private geometry corpus;
// it requires --source-sha and must remain outside the source checkout.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { headlessGame, idleCommand } from './headless-world.mjs';
import { RayKernel } from '../src/ray-kernel.js';
import { referenceTree, recordFor } from '../tests/fixtures/bvh-build-cases.js';

const args = process.argv.slice(2), option = (name, fallback) => { const at = args.indexOf(`--${name}`); return at < 0 ? fallback : args[at + 1]; };
const query = new URLSearchParams(option('query', '?map=nab2&side=rep&ai=classic&seed=1'));
const ticks = Number(option('ticks', 0)), sourceSha = option('source-sha', null), out = option('out', null);
if (!Number.isInteger(ticks) || ticks < 0 || ticks > 10000) throw Error('Invalid BVH gate tick count');
const directory = out && path.resolve(out), root = fileURLToPath(new URL('../', import.meta.url));
if (directory && (directory === path.resolve(root) || directory.startsWith(path.resolve(root) + path.sep))) throw Error('BVH corpus must be outside the checkout');
if (directory && !/^[0-9a-f]{40}$/.test(sourceSha ?? '')) throw Error('--out requires the full --source-sha');
if (directory) {
  if (fs.existsSync(directory)) throw Error('BVH corpus output already exists');
  fs.mkdirSync(directory, { recursive: true });
}
const sha = data => createHash('sha256').update(data).digest('hex');
const raw = array => Buffer.from(array.buffer, array.byteOffset, array.byteLength);
const cases = new Map(), treeHashes = [], built = RayKernel.prototype.buildBVH;
let trees = 0, triangles = 0;
function compare(actual, expected, label) {
  if (actual.length !== expected.length) throw Error(`${label}: ${actual.length} words != ${expected.length}`);
  for (let i = 0; i < actual.length; i++) if (actual[i] !== expected[i]) throw Error(`${label}: word ${i}: ${actual[i]} != ${expected[i]}`);
}
RayKernel.prototype.buildBVH = function (geometry) {
  const actual = built.call(this, geometry);
  if (!this.treeIds.has(actual)) throw Error('Live collision tree bypassed native construction');
  const positions = geometry.attributes.position.array, indices = Uint32Array.from(geometry.index.array), label = `tree-${trees++}`;
  triangles += indices.length / 3;
  const key = createHash('sha256').update(`${positions.BYTES_PER_ELEMENT}:${positions.length}:${indices.length}:`).update(raw(positions)).update(raw(indices)).digest('hex');
  if (!cases.has(key)) {
    const expected = referenceTree(positions, indices);
    if (actual._roots.length !== expected._roots.length) throw Error(`${label}: root count`);
    actual._roots.forEach((root, i) => compare(new Uint32Array(root), new Uint32Array(expected._roots[i]), `${label} root ${i}`));
    compare(actual._indirectBuffer, expected._indirectBuffer, `${label} indirect`);
    if (actual._indirectBuffer.BYTES_PER_ELEMENT !== expected._indirectBuffer.BYTES_PER_ELEMENT) throw Error(`${label}: indirect width`);
    const rootsHash = sha(Buffer.concat(actual._roots.map(b => Buffer.from(b))));
    const entry = { file: `${key}.json`, triangles: indices.length / 3, rootsHash, indirectHash: sha(raw(Uint32Array.from(actual._indirectBuffer))) };
    if (directory) {
      const data = Buffer.from(JSON.stringify(recordFor({ label: `${query.get('map')}:${label}`, positions, indices }, expected)) + '\n');
      fs.writeFileSync(path.join(directory, entry.file), data); entry.sha256 = sha(data); entry.bytes = data.length;
    }
    cases.set(key, entry);
  } else {
    const expected = cases.get(key);
    if (sha(Buffer.concat(actual._roots.map(b => Buffer.from(b)))) !== expected.rootsHash
      || sha(raw(Uint32Array.from(actual._indirectBuffer))) !== expected.indirectHash) throw Error(`${label}: repeated tree changed`);
  }
  treeHashes.push(key);
  return actual;
};
try {
  const game = await headlessGame('?' + query);
  for (let tick = 0; tick < ticks; tick++) game.step(idleCommand(game));
  if (!trees || !game.collision.rays) throw Error('Native collision builder did not run');
  const result = { version: 1, closed: true, sourceSha, node: process.version, query: '?' + query, ticks, trees, uniqueTrees: cases.size, triangles,
    bodies: game.collision.bodies.length, orderHash: sha(JSON.stringify(treeHashes)), cases: [...cases.values()] };
  if (directory) fs.writeFileSync(path.join(directory, 'manifest.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify({ ...result, cases: undefined }));
} finally { RayKernel.prototype.buildBVH = built; }
