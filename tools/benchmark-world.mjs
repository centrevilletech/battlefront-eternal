// Offline CPU workload using extracted maps and their actual live-prop bodies.
// Optional PERF_REFERENCE_COLLISION module paths add
// interleaved before/after measurements and output-equivalence checks.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Soldier } from '../tests/helpers/body.js';

const root = fileURLToPath(new URL('../', import.meta.url));
const integer = (name, fallback, minimum) => Math.max(minimum, Math.floor(Number(process.env[name]) || fallback));
const rounds = integer('PERF_ROUNDS', 7, 3);
const cycles = integer('PERF_CYCLES', 10, 1);
// References are external frozen modules supplied by the caller, not a live
// engine switch. Loading them cannot re-enable a deleted battle drawer.
// The benchmark only compares the surviving collision/movement workload.
async function variants(name, symbol, current) {
  const path = process.env[name];
  return path ? [['reference', (await import(pathToFileURL(resolve(path))))[symbol]], ['current', current]] : [['current', current]];
}
function quantiles(values) {
  values.sort((a, b) => a - b);
  return { medianMs: values[Math.floor(values.length * .5)], p95Ms: values[Math.floor(values.length * .95)], samples: values.length };
}
// Build the collision scene before timing. File loading and body creation
// must not be charged to a phase that only performs movement and aim rays.
// Only the authored built/destroyed/unbuilt state is enabled for each prop.
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
// This report deliberately excludes rendering and GPU completion. Foliage
// selection is measured in drawn Rust frames rather than by resurrecting a
// CPU selector merely to preserve an offline timing entry.
const report = { scope: 'CPU only; excludes map loading, renderer, GPU, audio, and frame pacing.', collision: {} };
const collisionVariants = await variants('PERF_REFERENCE_COLLISION', 'CollisionWorld', CollisionWorld);
// Use two different scene densities so one friendly collision arrangement
// cannot dominate the conclusion. Identical final positions are required
// before timings from an optional frozen reference are comparable.
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
// The first two passes warm JIT and retained query storage for both modules.
// Record only later samples while preserving their alternating order.
      if (pass >= 2) subject.timings.push(performance.now() - start);
      subject.position = soldier.position.toArray();
    }
  }
  if (subjects.length > 1) assert.deepEqual(subjects[0].position, subjects[1].position, `${map}: movement result changed`);
  report.collision[map] = { ticksPerSample: cycles * 180, aimRaysPerSample: cycles * 180,
    enabledBodies: subjects[0].world.bodies.filter(body => body.enabled).length,
    ...Object.fromEntries(subjects.map(s => [s.name, quantiles(s.timings)])), finalPosition: subjects[0].position };
}
console.log(JSON.stringify(report, null, 2));
