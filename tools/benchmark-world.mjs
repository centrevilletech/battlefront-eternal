// Offline CPU workload using extracted maps and their actual live-prop bodies.
// Optional PERF_REFERENCE_COLLISION / PERF_REFERENCE_FOLIAGE module paths add
// interleaved before/after measurements and output-equivalence checks.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';
import { Foliage } from '../src/foliage.js';

const root = fileURLToPath(new URL('../', import.meta.url));
const integer = (name, fallback, minimum) => Math.max(minimum, Math.floor(Number(process.env[name]) || fallback));
const rounds = integer('PERF_ROUNDS', 7, 3);
const cycles = integer('PERF_CYCLES', 10, 1);
const frames = integer('PERF_FRAMES', 720, 30);
async function variants(name, symbol, current) {
  const path = process.env[name];
  return path ? [['reference', (await import(pathToFileURL(resolve(path))))[symbol]], ['current', current]] : [['current', current]];
}
function quantiles(values) {
  values.sort((a, b) => a - b);
  return { medianMs: values[Math.floor(values.length * .5)], p95Ms: values[Math.floor(values.length * .95)], samples: values.length };
}
function mapWorld(name, Kind) {
  const dir = `${root}public/maps/${name}/`;
  const load = (file, Type) => {
    const bytes = fs.readFileSync(dir + file);
    return new Type(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  };
  const world = new Kind(load('collision.bin', Float32Array), load('collision_index.bin', Uint32Array),
    load('collision_poly.bin', Uint32Array), load('terrain_collision.bin', Float32Array), load('terrain_collision_index.bin', Uint32Array));
  const props = JSON.parse(fs.readFileSync(dir + 'props.json'));
  const positions = load('props_collision.bin', Float32Array), indices = load('props_collision_index.bin', Uint32Array), polygons = load('props_collision_poly.bin', Uint32Array);
  for (const desc of props.instances) {
    if (!['powerupstation', 'armedbuilding'].includes(desc.base) || !props.models[desc.model]) continue;
    const state = desc.unbuilt && props.models[desc.unbuilt.model] ? 'unbuilt' : 'built';
    for (const [kind, data] of [['built', desc], ['destroyed', desc.destroyed], ['unbuilt', desc.unbuilt]]) {
      if (!data?.collision) continue;
      const [first, count] = data.collision;
      world.addBody(positions, indices.subarray(first * 3, (first + count) * 3), polygons.subarray(first, first + count)).enabled = kind === state;
    }
  }
  return world;
}
function foliage(Kind, data) {
  const original = Math.random;
  let seed = 991;
  Math.random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
  try { return new Kind(new THREE.Scene(), data, { texture: () => new THREE.Texture(), nearEnd: () => 250 }); }
  finally { Math.random = original; }
}
const report = { scope: 'CPU only; excludes map loading, renderer, GPU, audio, and frame pacing.', collision: {} };
const collisionVariants = await variants('PERF_REFERENCE_COLLISION', 'CollisionWorld', CollisionWorld);
for (const map of ['nab2', 'bes2']) {
  const subjects = collisionVariants.map(([name, Kind]) => ({ name, world: mapWorld(map, Kind), timings: [], position: null }));
  const route = map === 'nab2' ? [-168, -49, -25, Math.PI] : [-163.068, -41, 81.088, Math.PI / 2];
  for (let pass = 0; pass < rounds; pass++) {
    // Alternate order so the first implementation does not own warm-cache bias.
    for (const subject of pass % 2 ? [...subjects].reverse() : subjects) {
      const soldier = new Soldier(subject.world, {});
      const start = performance.now();
      for (let cycle = 0; cycle < cycles; cycle++) {
        soldier.spawn(...route);
        for (let tick = 0; tick < 180; tick++) {
          soldier.tick({ forward: 1, right: 0 }, 1 / 60);
          const eye = soldier.position.clone().add(new THREE.Vector3(0, 1.8, 0));
          subject.world.cast(eye, new THREE.Vector3(Math.sin(tick * .07), -.1, Math.cos(tick * .07)).normalize(), 500, makeHit());
        }
      }
      if (pass >= 2) subject.timings.push(performance.now() - start);
      subject.position = soldier.position.toArray();
    }
  }
  if (subjects.length > 1) assert.deepEqual(subjects[0].position, subjects[1].position, `${map}: movement result changed`);
  report.collision[map] = { ticksPerSample: cycles * 180, aimRaysPerSample: cycles * 180,
    enabledBodies: subjects[0].world.bodies.filter(body => body.enabled).length,
    ...Object.fromEntries(subjects.map(s => [s.name, quantiles(s.timings)])), finalPosition: subjects[0].position };
}
const foliageVariants = await variants('PERF_REFERENCE_FOLIAGE', 'Foliage', Foliage);
const data = JSON.parse(fs.readFileSync(`${root}public/maps/nab2/map.json`)).foliage;
const subjects = foliageVariants.map(([name, Kind]) => ({ name, foliage: foliage(Kind, data), timings: [] }));
const camera = new THREE.PerspectiveCamera(60, 1.6, .1, 500);
camera.position.set(-168, -47, -25);
let minimumCards = Infinity, maximumCards = 0, maxAttributeDifference = 0;
for (let pass = 0; pass < rounds; pass++) {
  for (let frame = 0; frame < frames; frame++) {
    camera.rotation.set(Math.sin(frame * .011) * .6, frame * Math.PI / 360, 0);
    for (const subject of frame % 2 ? [...subjects].reverse() : subjects) {
      const start = performance.now(); subject.foliage.update(1 / 60, camera);
      if (pass >= 2) subject.timings.push(performance.now() - start);
    }
    if (pass !== 0) continue;
    let cards = 0;
    for (const [name, kind] of subjects[0].foliage.kinds) {
      cards += kind.batch.count;
      if (subjects.length < 2) continue;
      const other = subjects[1].foliage.kinds.get(name);
      assert.equal(kind.batch.count, other.batch.count, 'visible foliage count changed');
      for (const [attribute, stride] of [['positions', 12], ['colors', 16], ['uvs', 8]]) {
        for (let i = 0; i < kind.batch.count * stride; i++) {
          const delta = Math.abs(kind.batch[attribute][i] - other.batch[attribute][i]);
          maxAttributeDifference = Math.max(maxAttributeDifference, delta);
          assert(delta <= 3e-5, `${name}.${attribute}: vertex output changed by ${delta}`);
        }
      }
    }
    minimumCards = Math.min(minimumCards, cards); maximumCards = Math.max(maximumCards, cards);
  }
}
report.foliage = { ...Object.fromEntries(subjects.map(s => [s.name, quantiles(s.timings)])),
  visibleCards: [minimumCards, maximumCards], maxAttributeDifference: subjects.length > 1 ? maxAttributeDifference : null };
console.log(JSON.stringify(report, null, 2));
