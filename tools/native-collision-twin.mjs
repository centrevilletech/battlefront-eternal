// Two independent WASM scenes, traced for the owned native context
// and for isolated legacy native processes. Records contain bits, not JSON
// floating-point round trips. All geometry comes from synthetic fixtures.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import * as THREE from 'three';
import { CollisionWorld } from './collision-fixture.mjs';
import { RayKernel } from '../src/sim/wasm/ray-kernel.js';
import { loadSim } from '../src/sim/module.js';

const emit = async record => { if (!process.stdout.write(JSON.stringify(record) + '\n')) await once(process.stdout, 'drain'); };
const hex = array => Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString('hex');
const f64 = values => hex(Float64Array.from(values));
const shape = geometry => ({ p: geometry.attributes.position.array, i: Uint32Array.from(geometry.index.array) });
const packed = s => ({ p: hex(s.p), wide: s.p instanceof Float64Array, i: [...s.i], polygons: [...s.polygons] });
function surface(y, n) {
  const g = new THREE.PlaneGeometry(32, 32, n, n).rotateX(-Math.PI / 2).translate(0, y, 0), s = shape(g);
  s.polygons = Uint32Array.from({ length: s.i.length / 3 }, (_, i) => (i * 7) % 1201);
  return s;
}
let queries = 0, growths = 0, removals = 0;
async function make(cx) {
  const buildings = surface(cx * 3, 28), terrain = surface(-5 - cx, 8);
  const world = new CollisionWorld(buildings.p, buildings.i, buildings.polygons, terrain.p, terrain.i);
  const k = new RayKernel((await loadSim()).exports), x = k.x;
  const grid = k.grid(world.terrainGrid), tree = k.tree(world.bvh), ground = k.tree(world.terrain), id = k.world(tree, ground, grid);
  const set = k.fixedSet(world.staticSet, world.bvh); x.world_statics(id, set);
  const cache = x.cache_new(1.5);
  const scene = { cx, world, k, x, id, grid, tree, ground, cache, bodies: [], blockers: null };
  await emit({ cx, op: 'install', buildings: packed(buildings), terrain: packed(terrain), ids: [id, grid, tree, ground, set], cache });
  for (let i = 0; i < 14; i++) await add(scene, i);
  const blocker = shape(new THREE.BoxGeometry(7, 5, 0.5).translate(0, 2, 5));
  blocker.polygons = Uint32Array.from({ length: blocker.i.length / 3 }, (_, i) => i >> 1);
  const bs = world.createBodyShape(blocker.p, blocker.i, blocker.polygons), bt = k.tree(bs.bvh);
  await emit({ cx, op: 'shape', ...packed(blocker), tree: bt });
  const fixed = world.blockerSet(bs); scene.blockers = k.fixedSet(fixed, bs.bvh);
  await emit({ cx, op: 'set', tree: bt, id: scene.blockers, bounds: hex(fixed.triBounds), polygons: [...fixed.polygonOf], offset: fixed.offset });
  return scene;
}
async function add(s, i) {
  const moving = i % 3 !== 0;
  const g = new THREE.BoxGeometry(1 + i % 4, 1.5, 2.5);
  if (!moving) g.translate((i % 9) * 3 - 12, s.cx * 3 + 1, Math.floor(i / 9) * 2 - 8);
  const data = shape(g); data.polygons = Uint32Array.from({ length: data.i.length / 3 }, (_, i) => i >> 1);
  // Some body corners stay f64, as the legacy bridge permits.
  if (i % 5 === 0) data.p = Float64Array.from(data.p);
  const form = s.world.createBodyShape(data.p, data.i, data.polygons);
  const tree = s.k.tree(form.bvh);
  await emit({ cx: s.cx, op: 'shape', ...packed(data), tree });
  const body = moving ? s.world.addMovingBody(form, new THREE.Matrix4(), { i }) : s.world.addBody(data.p, data.i, data.polygons, { i });
  // addBody builds an equivalent immutable tree; use the one just copied.
  if (!moving) body.bvh = form.bvh;
  body.raySlot = s.k.body(body, moving);
  s.bodies.push(body);
  if (moving) s.world.moveBody(body, new THREE.Matrix4().makeTranslation(i % 7 - 3, s.cx * 3 + 2, i % 5 - 2));
  const values = placement(body);
  s.k.place(body.raySlot, body.bounds, moving ? body.matrix : null, moving ? body.inverse : null);
  s.k.order(s.id, s.world.bodies);
  await emit({ cx: s.cx, op: 'body', tree, id: body.raySlot, moving, offset: body.offset, v: f64(values) });
}
function placement(body) {
  const identity = new THREE.Matrix4().elements;
  return [...body.bounds.min.toArray(), ...body.bounds.max.toArray(), ...(body.matrix?.elements ?? identity), ...(body.inverse?.elements ?? identity)];
}
async function query(s, name, v, options = {}) {
  const flags = s.world.bodies.map((b, i) => [0, 1, 2, 255][(i + options.round) % 4]);
  s.world.bodies.forEach((b, i) => { b.enabled = flags[i] !== 0; });
  const { k, x, id } = s; let n = 0, code;
  k.view().set(v);
  if (name === 'cast') {
    n = x.world_candidates(id, options.ignore, +options.staticOnly, +options.movingOnly);
    for (let c = 0; c < n; c++) k.flags()[c] = flags[k.candidates()[c]];
    code = options.movingOnly ? x.world_cast_moving(id, n) : x.world_cast(id, n);
  } else if (name === 'gather') {
    n = x.gather_begin(id, options.cache, options.blockers, options.ignore);
    for (let c = 0; c < n; c++) k.flags()[c] = flags[k.candidates()[c]];
    code = x.gather_end(id, n);
  } else if (name === 'overlap') {
    n = x.overlap_begin(id, options.ignore);
    if (n < 0) { code = 1; n = 0; }
    else { for (let c = 0; c < n; c++) k.flags()[c] = flags[k.candidates()[c]]; code = x.overlap_end(id, n); }
  } else if (name === 'tree') code = x.tree_ray(s.tree);
  else if (name === 'grid') code = x.grid_ray(s.grid);
  else if (name === 'near') code = x.terrain_near_ray(s.grid);
  const io = k.view();
  const expected = { code, hit: f64(io.slice(10, 17)), candidates: [...k.candidates().slice(0, n)], enabled: [...k.flags().slice(0, n)] };
  if (name === 'gather') expected.words = f64(k.outF64(io[58]).slice(0, io[59]));
  if (name === 'near') expected.near = [...k.outU32(io[58]).slice(0, code)];
  await emit({ cx: s.cx, op: 'query', name, v: f64(v), flags, ...options, expected }); queries++;
}
const scenes = [await make(0), await make(1)];
let seed = 9187;
const random = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296);
for (let round = 0; round < 240; round++) for (const s of scenes) {
  if (round % 8 === 0) {
    const body = s.bodies[1], matrix = new THREE.Matrix4().compose(new THREE.Vector3(Math.sin(round * .1) * 8, 2 + s.cx * 3, Math.cos(round * .1) * 8), new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), round * .07), new THREE.Vector3(1,1,1));
    s.world.moveBody(body, matrix); s.k.place(body.raySlot, body.bounds, body.matrix, body.inverse);
    await emit({ cx: s.cx, op: 'place', id: body.raySlot, v: f64(placement(body)) });
  }
  if (round === 80) {
    const old = s.bodies[3]; s.world.removeBody(old); s.k.order(s.id, s.world.bodies); removals++;
    await emit({ cx: s.cx, op: 'remove', id: old.raySlot });
  }
  if (round === 120) {
    s.world.bodies.reverse(); s.k.order(s.id, s.world.bodies);
    await emit({ cx: s.cx, op: 'order', order: s.world.bodies.map(b => b.raySlot) });
    for (let i = 20; i < 56; i++) await add(s, i);
    const pointers = [s.x.io_ptr(), s.x.enabled_ptr(), s.x.candidates_ptr()], old = s.k.view();
    s.x.memory.grow(10); growths++;
    assert.equal(old.byteLength, 0);
    assert.deepEqual([s.x.io_ptr(), s.x.enabled_ptr(), s.x.candidates_ptr()], pointers);
    await emit({ cx: s.cx, op: 'grow', bytes: 700_000 });
  }
  const origin = [random() * 36 - 18, random() * 17 + 5, random() * 36 - 18];
  const direction = new THREE.Vector3(random() - .5, -1, random() - .5).normalize().toArray();
  const ray = [...origin, ...direction, round % 9 === 0 ? Infinity : random() * 70, Math.hypot(direction[0], direction[2]), 0];
  const ignore = round % 3 === 0 ? s.bodies[round % s.bodies.length].raySlot : -1;
  await query(s, 'cast', ray, { round, ignore, staticOnly: round % 5 === 0, movingOnly: false });
  await query(s, 'cast', ray, { round, ignore, staticOnly: false, movingOnly: true });
  const center = [random() * 24 - 12, s.cx * 3 + random() * 3, random() * 24 - 12], radius = round % 12 ? .2 + random() * 3 : 40;
  for (const cache of [-1, s.cache]) await query(s, 'gather', [...center, radius], { round, ignore, cache, blockers: round % 2 ? s.blockers : -1 });
  await query(s, 'overlap', [...center, radius], { round, ignore });
  await query(s, 'tree', ray, { round });
  await query(s, 'grid', ray, { round });
  await query(s, 'near', [...center, radius], { round });
}
for (const s of scenes) for (const v of [[0,20,0,-0,-1,0,Infinity,0,0], [0,20,0,0,0,0,5,0,0], [0,20,0,NaN,-1,0,10,NaN,0], [Infinity,20,0,0,-1,0,10,0,0]]) {
  // Raw non-finite probes exercise the kernel's existing branches directly.
  await query(s, 'tree', v, { round: 0 });
  await query(s, 'grid', v, { round: 0 });
}
const summary = { contexts: 2, queries, removals, growths };
await emit({ op: 'summary', ...summary }); console.error(JSON.stringify(summary));
