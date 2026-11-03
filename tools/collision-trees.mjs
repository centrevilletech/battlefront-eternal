// A map's collision trees as the JavaScript builds them (three-mesh-bvh,
// indirect, as src/collision.js builds them), written for a host that has no
// JavaScript to build them (a native test, a native replay): the buildings'
// tree and the terrain's, each its packed root buffers and its indirect
// buffer. With --casts N it adds N casts through the map's CollisionWorld
// (its JavaScript answers, no ray kernel) for the host to check its copy
// against. rust/content's collision test reads the file (src/install.rs).
//
//   node tools/collision-trees.mjs --map nab2 --out FILE [--casts 2000]
//
// The file, little-endian: "BFEC", version 1; a tree is its root count, each
// root's u32 word count and words, its indirect buffer's length and words;
// the buildings' tree, then 1 and the terrain's (0 for none); the cast count
// and each cast: origin, direction, far (f64), the answer's code (i32: -2
// miss, -3 terrain, -1 the buildings) and distance, point, normal (f64).
// The content is BFE_CONTENT_DIR or public/.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const at = args.indexOf(`--${name}`);
  return at >= 0 ? args[at + 1] : fallback;
};
const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
const map = option('map', 'nab2');
const out = option('out');
const casts = Number(option('casts', 0));
if (!out) throw new Error('--out FILE');

const THREE = await import('three');
const { CollisionWorld } = await import('../src/collision.js');

const read = (name) => {
  const bytes = fs.readFileSync(path.join(content, 'maps', map, name));
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
};
// game.js 429-436, without a ray kernel.
const world = new CollisionWorld(
  new Float32Array(read('collision.bin')), new Uint32Array(read('collision_index.bin')), new Uint32Array(read('collision_poly.bin')),
  new Float32Array(read('terrain_collision.bin')), new Uint32Array(read('terrain_collision_index.bin')), null,
);

const chunks = [];
const u32 = (...values) => chunks.push(Buffer.from(new Uint32Array(values).buffer));
const i32 = (value) => chunks.push(Buffer.from(new Int32Array([value]).buffer));
const f64 = (...values) => chunks.push(Buffer.from(new Float64Array(values).buffer));
const words = (array) => { u32(array.length); chunks.push(Buffer.from(array.buffer, array.byteOffset, array.byteLength)); };
// ray-kernel.js tree(): each root's words and the indirect buffer (a
// Uint16Array under 65,536 triangles, widened as the kernel's copy is).
function tree(bvh) {
  u32(bvh._roots.length);
  for (const r of bvh._roots) words(new Uint32Array(r));
  words(Uint32Array.from(bvh._indirectBuffer));
}
chunks.push(Buffer.from('BFEC'));
u32(1);
tree(world.bvh);
if (world.terrain) { u32(1); tree(world.terrain); } else u32(0);

// Casts from points over the map's bounds in every direction, as far as a
// round flies (a fixed generator, so every run writes the same file).
u32(casts);
const bounds = world.bvh.getBoundingBox(new THREE.Box3());
if (world.terrain) bounds.union(world.terrain.getBoundingBox(new THREE.Box3()));
let state = 12345;
const random = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 4294967296; };
const origin = new THREE.Vector3(), direction = new THREE.Vector3();
const hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
for (let i = 0; i < casts; i++) {
  origin.set(bounds.min.x + random() * (bounds.max.x - bounds.min.x), bounds.min.y + random() * (bounds.max.y - bounds.min.y + 20), bounds.min.z + random() * (bounds.max.z - bounds.min.z));
  // Every fourth cast straight down, as ground probes go; the rest anywhere.
  if (i % 4 === 0) direction.set(0, -1, 0);
  else direction.set(random() * 2 - 1, random() * 2 - 1, random() * 2 - 1).normalize();
  const far = 20 + random() * 300;
  const answer = world.cast(origin, direction, far, hit);
  f64(origin.x, origin.y, origin.z, direction.x, direction.y, direction.z, far);
  if (!answer) { i32(-2); f64(0, 0, 0, 0, 0, 0, 0); continue; }
  i32(answer.terrain ? -3 : -1);
  f64(answer.distance, answer.point.x, answer.point.y, answer.point.z, answer.normal.x, answer.normal.y, answer.normal.z);
}
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, Buffer.concat(chunks));
