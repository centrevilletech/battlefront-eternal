// Movement and query equivalence between two CollisionWorld implementations
// on the extracted maps: soldiers driven by seeded random input, compared bit
// for bit every tick, plus random rays, ground probes and sphere clearances.
// Also times both. Gameplay must not depend on which one runs.
//
// Usage: git show HEAD:src/collision.js > tools/.collision-ref.mjs
//   PERF_REFERENCE_COLLISION=tools/.collision-ref.mjs node tools/check-collision-equivalence.mjs
//   CHECK_MAPS=bes2,nab2 CHECK_SOLDIERS=24 CHECK_TICKS=900
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';

const root = fileURLToPath(new URL('../', import.meta.url));
const reference = process.env.PERF_REFERENCE_COLLISION;
if (!reference) throw new Error('set PERF_REFERENCE_COLLISION to the reference collision.js');
const Reference = (await import(pathToFileURL(resolve(reference)))).CollisionWorld;
const maps = (process.env.CHECK_MAPS || fs.readdirSync(`${root}public/maps`).join(',')).split(',');
const soldierCount = Number(process.env.CHECK_SOLDIERS || 24);
const ticks = Number(process.env.CHECK_TICKS || 900);

function rng(seed) {
  return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
}
function load(dir, file, Type) {
  if (!fs.existsSync(dir + file)) return null;
  const bytes = fs.readFileSync(dir + file);
  return new Type(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
}
function mapWorld(name, Kind) {
  const dir = `${root}public/maps/${name}/`;
  const world = new Kind(load(dir, 'collision.bin', Float32Array), load(dir, 'collision_index.bin', Uint32Array),
    load(dir, 'collision_poly.bin', Uint32Array), load(dir, 'terrain_collision.bin', Float32Array), load(dir, 'terrain_collision_index.bin', Uint32Array));
  const propsFile = `${dir}props.json`;
  if (fs.existsSync(propsFile)) {
    const props = JSON.parse(fs.readFileSync(propsFile));
    const positions = load(dir, 'props_collision.bin', Float32Array), indices = load(dir, 'props_collision_index.bin', Uint32Array), polygons = load(dir, 'props_collision_poly.bin', Uint32Array);
    for (const desc of props.instances || []) {
      if (!positions || !props.models?.[desc.model]) continue;
      const state = desc.unbuilt && props.models[desc.unbuilt.model] ? 'unbuilt' : 'built';
      for (const [kind, data] of [['built', desc], ['destroyed', desc.destroyed], ['unbuilt', desc.unbuilt]]) {
        if (!data?.collision) continue;
        const [first, count] = data.collision;
        world.addBody(positions, indices.subarray(first * 3, (first + count) * 3), polygons.subarray(first, first + count)).enabled = kind === state;
      }
    }
  }
  return world;
}

// Spawn spots: half on random building triangles that face up, half on the
// terrain, each a metre above the surface.
function spots(name, world, count) {
  const dir = `${root}public/maps/${name}/`;
  const p = load(dir, 'collision.bin', Float32Array), ix = load(dir, 'collision_index.bin', Uint32Array);
  const t = load(dir, 'terrain_collision.bin', Float32Array);
  const random = rng(name.split('').reduce((a, c) => a * 31 + c.charCodeAt(0), 7) >>> 0);
  const out = [], hit = { height: 0, normal: new THREE.Vector3() };
  for (let attempt = 0; out.length < count && attempt < count * 200; attempt++) {
    if (out.length % 2 === 0 && ix?.length) {
      const tri = Math.floor(random() * ix.length / 3) * 3;
      const v = [0, 1, 2].map(k => new THREE.Vector3().fromArray(p, ix[tri + k] * 3));
      const n = new THREE.Triangle(...v).getNormal(new THREE.Vector3());
      if (Math.abs(n.y) < 0.8) continue;
      const c = v[0].add(v[1]).add(v[2]).divideScalar(3);
      out.push([c.x, c.y + 1, c.z, random() * Math.PI * 2]);
    } else if (t?.length) {
      const i = Math.floor(random() * t.length / 3) * 3;
      const x = t[i] + random() * 4 - 2, z = t[i + 2] + random() * 4 - 2;
      if (!world.ground(x, z, 1e4, hit)) continue;
      out.push([x, hit.height + 1, z, random() * Math.PI * 2]);
    }
  }
  return out;
}

function state(soldier) {
  return [...soldier.position.toArray(), ...soldier.velocity.toArray(), ...soldier.normal.toArray(), soldier.grounded, soldier.posture, soldier.air];
}

const report = {};
for (const name of maps) {
  if (!fs.existsSync(`${root}public/maps/${name}/collision.bin`)) continue;
  const worlds = { reference: mapWorld(name, Reference), current: mapWorld(name, CollisionWorld) };
  const starts = spots(name, worlds.current, soldierCount);
  const timings = { reference: 0, current: 0 };
  const soldiers = {};
  for (const key of ['reference', 'current']) {
    soldiers[key] = starts.map(start => { const s = new Soldier(worlds[key], {}); s.spawn(...start); return s; });
  }
  const random = rng(4242);
  const inputs = starts.map(() => ({ forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false }));
  let mismatch = null;
  for (let tick = 0; tick < ticks && !mismatch; tick++) {
    for (const input of inputs) {
      if (random() < 0.05) { input.forward = Math.round(random() * 2 - 1); input.right = Math.round(random() * 2 - 1); }
      input.turn = random() < 0.1 ? random() * 2 - 1 : input.turn * 0.9;
      input.jump = random() < 0.01; input.crouch = random() < 0.005; input.prone = random() < 0.002;
    }
    for (const key of tick % 2 ? ['current', 'reference'] : ['reference', 'current']) {
      const start = performance.now();
      soldiers[key].forEach((s, i) => s.tick({ ...inputs[i] }, 1 / 60));
      timings[key] += performance.now() - start;
    }
    for (let i = 0; i < starts.length; i++) {
      const a = state(soldiers.reference[i]), b = state(soldiers.current[i]);
      if (a.some((v, k) => !Object.is(v, b[k]))) { mismatch = { tick, soldier: i, reference: a, current: b }; break; }
    }
  }
  // Rays, ground probes and clearances.
  const q = rng(99), ha = makeHit(), hb = makeHit(), ga = { height: 0, normal: new THREE.Vector3() }, gb = { height: 0, normal: new THREE.Vector3() };
  let queries = 0, queryMismatch = null;
  const queryTime = { reference: 0, current: 0 };
  for (let i = 0; i < 20000 && !queryMismatch; i++) {
    const s = starts[i % starts.length];
    const origin = new THREE.Vector3(s[0] + q() * 80 - 40, s[1] + q() * 20 - 5, s[2] + q() * 80 - 40);
    const dir = new THREE.Vector3(q() * 2 - 1, q() * 2 - 1.2, q() * 2 - 1);
    if (i % 5 === 0) dir.set(0, -1, 0);
    dir.normalize();
    const far = q() < 0.5 ? 500 : q() * 60;
    let t0 = performance.now();
    const a = worlds.reference.cast(origin, dir, far, ha);
    let t1 = performance.now();
    const b = worlds.current.cast(origin, dir, far, hb);
    let t2 = performance.now();
    queryTime.reference += t1 - t0; queryTime.current += t2 - t1;
    const pack = (h) => h && [h.distance, ...h.point.toArray(), ...h.normal.toArray(), h.terrain, h.body?.offset ?? null];
    if (JSON.stringify(pack(a)) !== JSON.stringify(pack(b))) queryMismatch = { kind: 'cast', origin: origin.toArray(), dir: dir.toArray(), far, reference: pack(a), current: pack(b) };
    const ra = worlds.reference.ground(origin.x, origin.z, 1e4, ga), rb = worlds.current.ground(origin.x, origin.z, 1e4, gb);
    if (JSON.stringify(ra && [ra.height, ...ra.normal.toArray()]) !== JSON.stringify(rb && [rb.height, ...rb.normal.toArray()])) queryMismatch = { kind: 'ground', origin: origin.toArray(), reference: ra && [ra.height, ...ra.normal.toArray()], current: rb && [rb.height, ...rb.normal.toArray()] };
    const radius = 0.2 + q() * 1.5;
    if (worlds.reference.overlapsSphere(origin, radius) !== worlds.current.overlapsSphere(origin, radius)) queryMismatch = { kind: 'overlap', origin: origin.toArray(), radius };
    queries++;
  }
  report[name] = { soldiers: starts.length, ticks, movementMismatch: mismatch, queries, queryMismatch,
    soldierMs: { reference: +timings.reference.toFixed(1), current: +timings.current.toFixed(1) },
    castMs: { reference: +queryTime.reference.toFixed(1), current: +queryTime.current.toFixed(1) } };
  console.log(name, JSON.stringify(report[name]));
}
const failed = Object.entries(report).filter(([, r]) => r.movementMismatch || r.queryMismatch);
assert.equal(failed.length, 0, `collision results changed on ${failed.map(([n]) => n).join(', ')}`);
