// The JavaScript side of the vehicle AI's twin checks: pure functions of
// src/ai/classic/vehicles run on generated cases (the tow-cable orbit's
// steering, the mounted policies' tables, Number.prototype.toFixed(1) as
// the support cache's key uses it), written with every number's bits for
// rust/sim's test `vehicle_ai_twin` to reproduce:
//
//   node tools/vehicle-ai-twin.mjs --out DIR [--seed N] [--cases N]
//   (cd rust && BFE_VAI_TWIN=DIR cargo test -p sim --test vehicle_ai_twin)
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { orbitResult, orbitSteer } from '../src/ai/classic/vehicles/orbit.js';
import { FLY_POLICY, MOBILE_POLICY, STATIONARY_POLICY } from '../src/ai/classic/vehicles/controllers.js';
import { ClassicRandom } from '../src/ai/random.js';

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i < 0 ? fallback : args[i + 1]; };
const out = opt('out', null);
if (!out) throw Error('vehicle-ai-twin: --out DIR');
const seed = Number(opt('seed', 1)) >>> 0, count = Number(opt('cases', 20000));
const gen = new ClassicRandom(seed);
const r = (lo, hi) => lo + gen.uniform() * (hi - lo);
const bits = (x) => { const b = new DataView(new ArrayBuffer(8)); b.setFloat64(0, x); return b.getBigUint64(0).toString(16).padStart(16, '0'); };
const v = (p) => [p.x, p.y, p.z].map(bits);

// The orbit: a craft about a walker, both anywhere within 300 m, the walker
// turned and pitched a little, every pattern-15 request shape and some odd
// ones (a zero radius, a zero factor, the craft on the centre).
const orbit = [];
const hull = new THREE.Object3D();
for (let i = 0; i < count; i++) {
  hull.position.set(r(-300, 300), r(-20, 40), r(-300, 300));
  hull.rotation.set(r(-0.2, 0.2), r(-Math.PI, Math.PI), r(-0.2, 0.2), 'YXZ');
  hull.updateMatrix();
  const position = i % 97 === 0 ? hull.position.clone().add(new THREE.Vector3(0, 7, 0)) : new THREE.Vector3(r(-300, 300), r(-10, 120), r(-300, 300));
  const forward = new THREE.Vector3(r(-1, 1), r(-0.3, 0.3), r(-1, 1));
  const towed = gen.int(0, 1) === 1;
  const flipA = gen.int(0, 1) === 1, flipB = gen.int(0, 1) === 1;
  const spec = {
    offset: towed ? new THREE.Vector3(0, 0, 8.75) : new THREE.Vector3(flipA ? -12.5 : 12.5, 0, 25),
    radius: i % 211 === 0 ? 0 : flipB ? -25 : 25,
    factor: i % 223 === 0 ? 0 : towed ? 1 : 2.5,
  };
  const res = orbitResult();
  orbitSteer(position, forward, { matrix: hull.matrix, position: hull.position }, spec, res);
  orbit.push({
    position: v(position), forward: v(forward), hullPosition: v(hull.position), matrix: hull.matrix.elements.map(bits),
    offset: v(spec.offset), radius: bits(spec.radius), factor: bits(spec.factor),
    point: v(res.point), centre: v(res.centre), blend: bits(res.blend), throttle: bits(res.throttle), towPass: res.towPass,
  });
}

// The policies' goal tables and the mounted state translation (index.js
// mountedState, its MOBILE_GOAL inverse).
const MOBILE_GOAL = new Map([[1, 1], [2, 2], [3, 3], [4, 4], [5, 5], [6, 6], [9, 8], [10, 9], [11, 10]]);
const policies = [['Mobile', MOBILE_POLICY], ['Stationary', STATIONARY_POLICY], ['Fly', FLY_POLICY]];
const tables = [];
for (const [name, p] of policies) {
  for (let k = -1; k <= 101; k++) {
    const goal = MOBILE_GOAL.get(k);
    const mounted = k === 100 || k === 0 ? k : goal === undefined ? k : p.goalState(goal);
    tables.push({ policy: name, kind: k, goalState: p.goalState(k), mounted, agent: Boolean(p.agentFor(k, null)) });
  }
}

// toFixed(1): ties, near-ties, negatives, -0, large and tiny values.
const fixed = [];
const edge = [0, -0, 0.05, 0.15, 0.25, 0.35, 1.45, 2.25, -0.05, -0.04, 0.04999999999999999, 1e20, 9.95, 99.95, 0.476, 1.0000000000000002, 5e-324, 123456.75];
for (const x of edge) fixed.push({ x: bits(x), text: x.toFixed(1) });
for (let i = 0; i < count; i++) {
  const x = Math.fround(r(0, 12)) * (i % 3 === 0 ? 1 : Math.fround(r(0.1, 3)));
  fixed.push({ x: bits(x), text: x.toFixed(1) });
}

fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'twin.json'), JSON.stringify({ seed, orbit, tables, fixed }));
console.log(`vehicle-ai-twin: ${orbit.length} orbits, ${tables.length} policy rows, ${fixed.length} toFixed cases -> ${out}`);
