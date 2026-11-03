// Cases for the three.js r170 arithmetic the rigs in rust/sim/src/rig are
// built on (rust/three's methods and TransformTree): random and edge inputs
// through three's own methods, written with three's answers to the last bit,
// for rust/sim/tests/rig_three.rs to replay. Run it under the port's reference Node (24.22, V8 13.6):
//
//   node tools/sim-rig/three-cases.mjs OUT.bfecap [--cases N] [--seed S]
//
// Every Math.sin, cos, atan2, asin and acos three calls while it answers is
// kept in an oracle (function, arguments, result), which the Rust replay
// answers its own calls from until rust/v8math is linked: the arithmetic
// around those functions is then proven bit for bit, and a call the
// JavaScript never made counts as a miss. The output holds no game data.
import fs from 'node:fs';
import * as THREE from 'three';
import { Writer, RECORD, MAGIC } from '../sim-capture/format.mjs';

const argv = process.argv.slice(2);
const out = argv.find((a) => !a.startsWith('--'));
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : Number(argv[i + 1]); };
if (!out) { console.error('usage: node tools/sim-rig/three-cases.mjs OUT.bfecap [--cases N] [--seed S]'); process.exit(2); }
const N = opt('cases', 20000);

// A seeded generator (mulberry32): the cases are the same every run.
let seed = opt('seed', 1) >>> 0;
const rand = () => { seed = (seed + 0x6d2b79f5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const pick = (list) => list[Math.floor(rand() * list.length)];
const SPECIAL = [0, -0, 1, -1, 0.5, -0.5, 2, 1e-300, -1e-300, 1e-8, 0.9999999, -0.9999999, 0.99999995, 1.0000001, Math.PI, -Math.PI, Math.PI / 2, 1e10, -1e10];
// A number: mostly ordinary, some special, a few extreme.
function num() {
  const r = rand();
  if (r < 0.08) return pick(SPECIAL);
  if (r < 0.1) return (rand() - 0.5) * 1e6;
  if (r < 0.5) return rand() * 2 - 1;
  return (rand() - 0.5) * 20;
}
const angle = () => (rand() < 0.1 ? pick([0, -0, Math.PI, -Math.PI, Math.PI / 2, 2 * Math.PI, 1e-9]) : (rand() - 0.5) * 4 * Math.PI);
const vec = () => new THREE.Vector3(num(), num(), num());
// A quaternion: a unit one mostly, sometimes as stored (not quite unit), zero or degenerate.
function quat() {
  const r = rand();
  if (r < 0.03) return new THREE.Quaternion(0, 0, 0, 0);
  if (r < 0.06) return new THREE.Quaternion(pick([0, -0, 1, -1]), pick([0, -0]), pick([0, 1]), pick([0, -0, 1, -1]));
  const q = new THREE.Quaternion(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1);
  if (r < 0.15) return q; // not normalized
  return q.normalize();
}
const unitAxis = () => { const v = new THREE.Vector3(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1); return rand() < 0.1 ? new THREE.Vector3(...pick([[1, 0, 0], [0, 1, 0], [0, 0, 1]])) : v.normalize(); };
// A matrix: a rigid or scaled transform mostly; sometimes mirrored, singular, gimbal-locked or arbitrary.
function mat() {
  const r = rand(), m = new THREE.Matrix4();
  if (r < 0.05) return m.fromArray(Array.from({ length: 16 }, num));
  if (r < 0.08) return m.set(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
  const s = rand() < 0.5 ? new THREE.Vector3(1, 1, 1) : new THREE.Vector3(0.1 + rand() * 3, 0.1 + rand() * 3, 0.1 + rand() * 3);
  if (rand() < 0.1) s.x = -s.x;
  let q = quat();
  // Gimbal lock for the Euler conversions: a quarter turn about y or x.
  if (rand() < 0.08) q = new THREE.Quaternion().setFromAxisAngle(pick([new THREE.Vector3(0, 1, 0), new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 0, 1)]), pick([Math.PI / 2, -Math.PI / 2]));
  return m.compose(vec(), q, s);
}
const ORDERS = ['XYZ', 'YXZ', 'ZXY', 'ZYX', 'YZX', 'XZY'];

// The oracle: what three's transcendental calls answered.
const oracle = new Map();
const CODES = { sin: 1, cos: 2, atan2: 3, asin: 4, acos: 5, atan: 6, exp: 7 };
const f64 = new Float64Array(1), u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return `${u32[0]}:${u32[1]}`; };
for (const [name, code] of Object.entries(CODES)) {
  const base = Math[name];
  Math[name] = name === 'atan2'
    ? (a, b) => { const r = base(a, b); oracle.set(`${code}|${bits(a)}|${bits(b)}`, [code, a, b, r]); return r; }
    : (a) => { const r = base(a); oracle.set(`${code}|${bits(a)}|${bits(0)}`, [code, a, 0, r]); return r; };
}

const writer = new Writer();
const fd = fs.openSync(out, 'w');
fs.writeSync(fd, MAGIC);
const flush = () => fs.writeSync(fd, writer.take());
writer.record(RECORD.header, 0, 'header', { format: 1, tool: 'sim-rig three-cases', node: process.version, v8: process.versions.v8, three: THREE.REVISION, cases: N });

// A method's cases: inputs and outputs, each a flat run of numbers a case.
function method(name, make) {
  const inputs = [], outputs = [];
  let inStride = -1, outStride = -1;
  for (let i = 0; i < N; i++) {
    const [ins, outs] = make();
    if (inStride < 0) { inStride = ins.length; outStride = outs.length; }
    if (ins.length !== inStride || outs.length !== outStride) throw Error(`${name}: case ${i} has another shape`);
    inputs.push(...ins); outputs.push(...outs);
  }
  writer.record(RECORD.frame, 0, `three.${name}`, { n: N, inStride, outStride, inputs: new Float64Array(inputs), outputs: new Float64Array(outputs) });
  flush();
}
const V = (v) => [v.x, v.y, v.z];
const Q = (q) => [q.x, q.y, q.z, q.w];
const M = (m) => [...m.elements];

method('v3.applyQuaternion', () => { const v = vec(), q = quat(); return [[...V(v), ...Q(q)], V(v.clone().applyQuaternion(q))]; });
method('v3.applyMatrix4', () => { const v = vec(), m = mat(); return [[...V(v), ...M(m)], V(v.clone().applyMatrix4(m))]; });
method('v3.transformDirection', () => { const v = vec(), m = mat(); return [[...V(v), ...M(m)], V(v.clone().transformDirection(m))]; });
method('v3.normalize', () => { const v = rand() < 0.05 ? new THREE.Vector3(0, -0, 0) : vec(); return [V(v), V(v.clone().normalize())]; });
method('v3.lerpVectors', () => { const a = vec(), b = vec(), t = num(); return [[...V(a), ...V(b), t], [...V(new THREE.Vector3().lerpVectors(a, b, t)), ...V(a.clone().lerp(b, t))]]; });
method('v3.crossVectors', () => { const a = vec(), b = vec(); return [[...V(a), ...V(b)], V(new THREE.Vector3().crossVectors(a, b))]; });
method('q.setFromEuler', () => {
  const e = new THREE.Euler(angle(), angle(), angle(), pick(ORDERS));
  return [[e.x, e.y, e.z, ORDERS.indexOf(e.order)], Q(new THREE.Quaternion().setFromEuler(e))];
});
method('q.setFromAxisAngle', () => { const a = unitAxis(), t = angle(); return [[...V(a), t], Q(new THREE.Quaternion().setFromAxisAngle(a, t))]; });
method('q.setFromRotationMatrix', () => { const m = mat(); return [M(m), Q(new THREE.Quaternion().setFromRotationMatrix(m))]; });
method('q.normalize', () => { const q = quat(); q.x *= num(); return [Q(q), Q(q.clone().normalize())]; });
method('q.multiplyQuaternions', () => { const a = quat(), b = quat(); return [[...Q(a), ...Q(b)], [...Q(new THREE.Quaternion().multiplyQuaternions(a, b)), ...Q(a.clone().premultiply(b))]]; });
method('q.slerp', () => {
  const a = quat();
  const r = rand();
  // Equal, opposite, nearly equal and ordinary pairs, at the ends and between.
  const b = r < 0.1 ? a.clone() : r < 0.15 ? new THREE.Quaternion(-a.x, -a.y, -a.z, -a.w) : r < 0.25 ? new THREE.Quaternion(a.x + 1e-9, a.y, a.z, a.w) : quat();
  const t = rand() < 0.1 ? pick([0, 1, -0, 0.5]) : rand() < 0.1 ? num() : rand();
  return [[...Q(a), ...Q(b), t], [...Q(a.clone().slerp(b, t)), ...Q(new THREE.Quaternion().slerpQuaternions(b, a, t))]];
});
method('m4.compose', () => { const p = vec(), q = quat(), s = vec(); return [[...V(p), ...Q(q), ...V(s)], M(new THREE.Matrix4().compose(p, q, s))]; });
method('m4.decompose', () => {
  const m = mat(), p = new THREE.Vector3(), q = new THREE.Quaternion(), s = new THREE.Vector3();
  m.decompose(p, q, s);
  return [M(m), [...V(p), ...Q(q), ...V(s)]];
});
method('m4.multiplyMatrices', () => { const a = mat(), b = mat(); return [[...M(a), ...M(b)], M(new THREE.Matrix4().multiplyMatrices(a, b))]; });
method('m4.invert', () => { const m = mat(); return [M(m), [...M(m.clone().invert()), m.determinant()]]; });
method('m4.extractRotation', () => { const m = mat(); return [M(m), M(new THREE.Matrix4().extractRotation(m))]; });
method('m4.makeBasis', () => { const a = vec(), b = vec(), c = vec(); return [[...V(a), ...V(b), ...V(c)], M(new THREE.Matrix4().makeBasis(a, b, c))]; });
method('euler.setFromRotationMatrix', () => {
  const m = mat(), order = pick(ORDERS), e = new THREE.Euler().setFromRotationMatrix(m, order);
  return [[...M(m), ORDERS.indexOf(order)], [e.x, e.y, e.z]];
});
method('euler.setFromQuaternion', () => {
  const q = quat(), order = pick(ORDERS), e = new THREE.Euler(0, 0, 0, pick(ORDERS)).setFromQuaternion(q, order);
  return [[...Q(q), ORDERS.indexOf(order)], [e.x, e.y, e.z]];
});
method('mathutils', () => {
  const x = rand() < 0.1 ? pick([NaN, -0, 0, Infinity]) : num(), a = num(), b = num();
  return [[x, a, b], [THREE.MathUtils.clamp(x, a, b), THREE.MathUtils.smoothstep(x, a, b), THREE.MathUtils.lerp(x, a, b)]];
});
method('box3.center', () => {
  const box = new THREE.Box3();
  const pts = Array.from({ length: 6 }, vec);
  const n = Math.floor(rand() * 7);
  box.setFromPoints(pts.slice(0, n));
  return [[n, ...pts.flatMap(V)], [...V(box.getCenter(new THREE.Vector3())), box.isEmpty() ? 1 : 0]];
});

// Object3D scripts: a tree of nodes and a run of operations on it, each
// read recorded. Ops: [code, node, ...args]; reads append their values.
const OPS = { position: 0, qset: 1, axisAngle: 2, multiply: 3, premultiply: 4, normalize: 5, slerp: 6, rotationSet: 7, rotX: 8, rotY: 9, rotZ: 10, order: 11,
  readRotation: 12, updateMatrixWorld: 13, updateWorldMatrix: 14, worldPosition: 15, worldQuaternion: 16, readWorld: 17, scale: 18, clone: 19, decompose: 20,
  slerpQuaternions: 21, noEuler: 22, readQuaternion: 23 };
const ARGS = [3, 4, 4, 4, 4, 0, 5, 4, 1, 1, 1, 1, 0, 1, 2, 0, 0, 0, 3, 1, 16, 9, 0, 0];
const scripts = [];
const SCRIPTS = Math.max(1, Math.floor(N / 50));
for (let k = 0; k < SCRIPTS; k++) {
  const count = 2 + Math.floor(rand() * 6);
  const parents = [];
  const nodes = [];
  for (let i = 0; i < count; i++) {
    nodes.push(rand() < 0.5 ? new THREE.Bone() : new THREE.Object3D());
    const parent = i === 0 || rand() < 0.2 ? -1 : Math.floor(rand() * i);
    parents.push(parent);
    if (parent >= 0) nodes[parent].add(nodes[i]);
  }
  const ops = [], reads = [];
  const steps = 10 + Math.floor(rand() * 30);
  for (let s = 0; s < steps; s++) {
    const code = Math.floor(rand() * ARGS.length), i = Math.floor(rand() * nodes.length), node = nodes[i];
    const args = [];
    const q = () => { const v = quat(); args.push(...Q(v)); return v; };
    switch (code) {
      case OPS.position: args.push(num(), num(), num()); node.position.set(args[0], args[1], args[2]); break;
      case OPS.qset: node.quaternion.copy(q()); break;
      case OPS.axisAngle: { const a = unitAxis(), t = angle(); args.push(...V(a), t); node.quaternion.setFromAxisAngle(a, t); break; }
      case OPS.multiply: node.quaternion.multiply(q()); break;
      case OPS.premultiply: node.quaternion.premultiply(q()); break;
      case OPS.normalize: node.quaternion.normalize(); break;
      case OPS.slerp: { const b = rand() < 0.3 ? node.quaternion.clone() : q(); if (args.length === 0) args.push(...Q(b)); const t = rand() < 0.2 ? pick([0, 1]) : rand(); args.push(t); node.quaternion.slerp(b, t); break; }
      case OPS.rotationSet: { const o = rand() < 0.5 ? -1 : Math.floor(rand() * 6); args.push(angle(), angle(), angle(), o); node.rotation.set(args[0], args[1], args[2], o < 0 ? undefined : ORDERS[o]); break; }
      case OPS.rotX: args.push(angle()); node.rotation.x = args[0]; break;
      case OPS.rotY: args.push(angle()); node.rotation.y = args[0]; break;
      case OPS.rotZ: args.push(angle()); node.rotation.z = args[0]; break;
      case OPS.order: args.push(Math.floor(rand() * 6)); node.rotation.order = ORDERS[args[0]]; break;
      case OPS.readRotation: reads.push(node.rotation.x, node.rotation.y, node.rotation.z, ORDERS.indexOf(node.rotation.order)); break;
      case OPS.updateMatrixWorld: args.push(rand() < 0.5 ? 1 : 0); node.updateMatrixWorld(Boolean(args[0])); break;
      case OPS.updateWorldMatrix: args.push(rand() < 0.5 ? 1 : 0, rand() < 0.5 ? 1 : 0); node.updateWorldMatrix(Boolean(args[0]), Boolean(args[1])); break;
      case OPS.worldPosition: reads.push(...V(node.getWorldPosition(new THREE.Vector3()))); break;
      case OPS.worldQuaternion: reads.push(...Q(node.getWorldQuaternion(new THREE.Quaternion()))); break;
      case OPS.readWorld: reads.push(...M(node.matrixWorld), node.matrixWorldNeedsUpdate ? 1 : 0); break;
      case OPS.scale: args.push(num(), num(), num()); node.scale.set(args[0], args[1], args[2]); break;
      case OPS.clone: { const parent = rand() < 0.5 ? -1 : Math.floor(rand() * nodes.length); args.push(parent); const c = node.clone(false); if (parent >= 0) nodes[parent].add(c); nodes.push(c); break; }
      case OPS.decompose: { const m = mat(); args.push(...M(m)); m.decompose(node.position, node.quaternion, node.scale); break; }
      case OPS.slerpQuaternions: { const a = quat(), b = rand() < 0.3 ? a.clone() : quat(), t = rand() < 0.2 ? pick([0, 1]) : rand(); args.push(...Q(a), ...Q(b), t); node.quaternion.slerpQuaternions(a, b, t); break; }
      case OPS.noEuler: node.quaternion._onChange(() => {}); break;
      case OPS.readQuaternion: reads.push(...Q(node.quaternion)); break;
    }
    if (args.length !== ARGS[code]) throw Error(`op ${code}: ${args.length} arguments, not ${ARGS[code]}`);
    ops.push(code, i, ...args);
  }
  scripts.push({ parents: new Float64Array(parents), ops: new Float64Array(ops), reads: new Float64Array(reads) });
}
writer.record(RECORD.frame, 0, 'three.object3d', { scripts });
flush();

writer.record(RECORD.frame, 0, 'three.oracle', { table: new Float64Array([...oracle.values()].flat()) });
flush();
fs.closeSync(fd);
console.log(`${out}: ${N} cases a method, ${SCRIPTS} Object3D scripts, ${oracle.size} oracle answers (node ${process.version}, V8 ${process.versions.v8}, three r${THREE.REVISION})`);
