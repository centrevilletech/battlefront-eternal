// Differential test: rust/three against three.js r170 itself,
// under the production server's Node (24.22, V8 13.6).
//
//   cargo build -p three --release --bin three-probe
//   node tools/diff.mjs [scale]
//
// Needs three r170 importable as "three" (the repo's node_modules,
// after npm ci), or set BFE_THREE_MODULE to a three module path.
// Every mode must be bit-identical (Object.is, NaNs alike), those
// that reach a transcendental (sin, cos, asin, acos, atan2, exp)
// included: jsrt's seam is rust/v8math, V8 13.6's own functions.

import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname, join} from 'node:path';

const THREE = await import(process.env.BFE_THREE_MODULE || 'three');
const here = dirname(fileURLToPath(import.meta.url));
const probe = process.env.THREE_PROBE || join(here, '..', '..', 'target', 'release', 'three-probe');
const scale = Number(process.argv[2] || 1);

function generator(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rnd = generator(0x1234abcd);
const R = (lo, hi) => lo + rnd() * (hi - lo);

const dv = new DataView(new ArrayBuffer(8));
function sameBits(a, b) {
  if (Number.isNaN(a) && Number.isNaN(b)) return true;
  dv.setFloat64(0, a); const ah = dv.getUint32(0), al = dv.getUint32(4);
  dv.setFloat64(0, b); const bh = dv.getUint32(0), bl = dv.getUint32(4);
  return ah === bh && al === bl;
}
function ulpDistance(a, b) {
  if (Number.isNaN(a) && Number.isNaN(b)) return 0;
  const key = x => { dv.setFloat64(0, x); const hi = dv.getUint32(0), lo = dv.getUint32(4); const bits = (BigInt(hi) << 32n) | BigInt(lo); return bits & (1n << 63n) ? ~(bits) & 0xffffffffffffffffn : bits | (1n << 63n); };
  const d = key(a) - key(b);
  return Number(d < 0n ? -d : d);
}

function runProbe(mode, inputF64) {
  const r = spawnSync(probe, [mode], {input: Buffer.from(inputF64.buffer, inputF64.byteOffset, inputF64.byteLength), maxBuffer: 1 << 30});
  if (r.status !== 0) throw new Error(`probe ${mode} failed: ${r.stderr}`);
  return new Float64Array(r.stdout.buffer.slice(r.stdout.byteOffset, r.stdout.byteOffset + r.stdout.byteLength));
}

let totalBad = 0, totalCases = 0;
function check(name, got, want, gated) {
  let bad = 0, first = '', worstUlp = 0;
  for (let i = 0; i < want.length; i++) {
    if (!sameBits(got[i], want[i])) {
      bad++;
      worstUlp = Math.max(worstUlp, ulpDistance(got[i], want[i]));
      if (!first) first = `at ${i}: got=${got[i]} want=${want[i]}`;
    }
  }
  if (gated) { totalBad += bad; totalCases += want.length; }
  console.log(`${name.padEnd(20)} ${String(want.length).padStart(11)} cases  ${bad} mismatches${gated ? '' : ' (not gated)'}${bad ? `  worst ${worstUlp} ulp  first: ${first}` : ''}`);
}

// Case shapes.
function vec() {
  const p = rnd();
  if (p < 0.03) return [0, 0, 0];
  if (p < 0.06) return [1e-300, -1e-300, 1e300];
  if (p < 0.09) return [Infinity, -Infinity, NaN];
  return [R(-10, 10), R(-10, 10), R(-10, 10)];
}
function quat(normalized = true) {
  const q = new THREE.Quaternion(R(-1, 1), R(-1, 1), R(-1, 1), R(-1, 1));
  if (normalized) q.normalize();
  return [q.x, q.y, q.z, q.w];
}
function matrix(kind) {
  const m = new THREE.Matrix4();
  if (kind === 'raw') {
    m.elements = Array.from({length: 16}, () => R(-3, 3));
  } else if (kind === 'rotation') {
    const q = new THREE.Quaternion(...quat());
    m.makeRotationFromQuaternion(q);
  } else {
    m.compose(new THREE.Vector3(...vec()), new THREE.Quaternion(...quat()),
      new THREE.Vector3(R(-2, 2), R(-2, 2), R(-2, 2)));
  }
  return [...m.elements];
}
function cases(n, make) {
  const list = [];
  for (let i = 0; i < n; i++) list.push(make(i));
  return list;
}
function flat(list) { return Float64Array.from(list.flat(Infinity)); }

const N = Math.floor(200_000 * scale);

// v3a
{
  const cs = cases(N, () => [...vec(), ...vec(), R(-2, 2)]);
  const want = [];
  for (const c of cs) {
    const a = new THREE.Vector3(c[0], c[1], c[2]);
    const b = new THREE.Vector3(c[3], c[4], c[5]);
    const s = c[6];
    const P = v => want.push(v.x, v.y, v.z);
    P(a.clone().add(b)); P(a.clone().sub(b)); P(a.clone().multiply(b)); P(a.clone().divide(b));
    P(new THREE.Vector3().crossVectors(a, b));
    want.push(a.dot(b), a.lengthSq(), a.length(), a.distanceToSquared(b), a.distanceTo(b), a.manhattanDistanceTo(b));
    P(a.clone().addScaledVector(b, s)); P(new THREE.Vector3().lerpVectors(a, b, s));
    P(a.clone().normalize()); P(a.clone().setLength(s)); P(a.clone().clampLength(0.5, 2));
    P(a.clone().projectOnVector(b)); P(a.clone().projectOnPlane(b)); P(a.clone().reflect(b));
    P(a.clone().min(b)); P(a.clone().max(b)); P(a.clone().floor()); P(a.clone().round());
  }
  check('v3a', runProbe('v3a', flat(cs)), want, true);
}
// v3libm
{
  const cs = cases(N, () => [...vec(), ...vec(), R(-7, 7)]);
  const want = [];
  for (const c of cs) {
    const a = new THREE.Vector3(c[0], c[1], c[2]);
    const b = new THREE.Vector3(c[3], c[4], c[5]);
    const angle = c[6];
    const P = v => want.push(v.x, v.y, v.z);
    want.push(a.angleTo(b));
    P(a.clone().applyAxisAngle(b, angle));
    P(new THREE.Vector3().setFromSphericalCoords(a.x, angle, b.x));
    const s = new THREE.Spherical().setFromVector3(a);
    want.push(s.radius, s.phi, s.theta);
    const cy = new THREE.Cylindrical().setFromVector3(a);
    want.push(cy.radius, cy.theta, cy.y);
    P(new THREE.Vector3().setFromCylindricalCoords(a.x, angle, b.y));
  }
  check('v3libm', runProbe('v3libm', flat(cs)), want, true);
}
// v3m
{
  const cs = cases(N, i => [...vec(), ...matrix(i % 3 === 0 ? 'raw' : 'compose')]);
  const want = [];
  for (const c of cs) {
    const a = new THREE.Vector3(c[0], c[1], c[2]);
    const m = new THREE.Matrix4().fromArray(c.slice(3, 19));
    const P = v => want.push(v.x, v.y, v.z);
    P(a.clone().applyMatrix4(m)); P(a.clone().transformDirection(m));
    P(new THREE.Vector3().setFromMatrixPosition(m));
    P(new THREE.Vector3().setFromMatrixScale(m));
    const m3 = new THREE.Matrix3().setFromMatrix4(m);
    P(a.clone().applyMatrix3(m3)); P(a.clone().applyNormalMatrix(m3));
  }
  check('v3m', runProbe('v3m', flat(cs)), want, true);
}
// v3q
{
  const cs = cases(N, () => [...vec(), ...quat()]);
  const want = [];
  for (const c of cs) {
    const a = new THREE.Vector3(c[0], c[1], c[2]);
    const q = new THREE.Quaternion(c[3], c[4], c[5], c[6]);
    const r = a.applyQuaternion(q);
    want.push(r.x, r.y, r.z);
  }
  check('v3q', runProbe('v3q', flat(cs)), want, true);
}
// quat
{
  const cs = cases(N, i => [...quat(i % 5 !== 0), ...quat(i % 7 !== 0), R(-0.5, 1.5), ...matrix(i % 4 === 0 ? 'raw' : 'rotation')]);
  const want = [];
  for (const c of cs) {
    const q = new THREE.Quaternion(...c.slice(0, 4));
    const r = new THREE.Quaternion(...c.slice(4, 8));
    const m = new THREE.Matrix4().fromArray(c.slice(9, 25));
    const P = v => want.push(v.x, v.y, v.z, v.w);
    P(new THREE.Quaternion().multiplyQuaternions(q, r));
    P(q.clone().premultiply(r));
    want.push(q.dot(r), q.lengthSq(), q.length());
    P(q.clone().normalize()); P(q.clone().invert());
    P(new THREE.Quaternion().setFromRotationMatrix(m));
    P(new THREE.Quaternion().setFromUnitVectors(
      new THREE.Vector3(q.x, q.y, q.z), new THREE.Vector3(r.x, r.y, r.z)));
  }
  check('quat', runProbe('quat', flat(cs)), want, true);
}
// quatlibm
{
  const cs = cases(N, () => [...quat(), ...quat(), R(-0.5, 1.5), ...vec()]);
  const want = [];
  for (const c of cs) {
    const q = new THREE.Quaternion(...c.slice(0, 4));
    const r = new THREE.Quaternion(...c.slice(4, 8));
    const t = c[8];
    const axis = new THREE.Vector3(c[9], c[10], c[11]);
    const P = v => want.push(v.x, v.y, v.z, v.w);
    P(q.clone().slerp(r, t));
    P(new THREE.Quaternion().slerpQuaternions(q, r, t));
    want.push(q.angleTo(r));
    P(new THREE.Quaternion().setFromAxisAngle(axis, t));
    P(q.clone().rotateTowards(r, Math.abs(t)));
    const dst = [0, 0, 0, 0];
    THREE.Quaternion.slerpFlat(dst, 0, [q.x, q.y, q.z, q.w], 0, [r.x, r.y, r.z, r.w], 0, t);
    want.push(...dst);
  }
  check('quatlibm', runProbe('quatlibm', flat(cs)), want, true);
}
// euler + eulerm
{
  const orders = ['XYZ', 'YXZ', 'ZXY', 'ZYX', 'YZX', 'XZY'];
  const cs = cases(N, i => [R(-7, 7), R(-7, 7), R(-7, 7), i % 6]);
  const want = [];
  for (const c of cs) {
    const e = new THREE.Euler(c[0], c[1], c[2], orders[c[3]]);
    const q = new THREE.Quaternion().setFromEuler(e);
    want.push(q.x, q.y, q.z, q.w);
    want.push(...new THREE.Matrix4().makeRotationFromEuler(e).elements);
    const back = new THREE.Euler().setFromQuaternion(q, orders[c[3]]);
    want.push(back.x, back.y, back.z);
    const re = e.clone().reorder(orders[(c[3] + 1) % 6]);
    want.push(re.x, re.y, re.z);
  }
  check('euler', runProbe('euler', flat(cs)), want, true);

  const cs2 = cases(N, i => [...matrix(i % 5 === 0 ? 'raw' : 'rotation'), i % 6]);
  const want2 = [];
  for (const c of cs2) {
    const m = new THREE.Matrix4().fromArray(c.slice(0, 16));
    const e = new THREE.Euler().setFromRotationMatrix(m, orders[c[16]]);
    want2.push(e.x, e.y, e.z);
  }
  check('eulerm', runProbe('eulerm', flat(cs2)), want2, true);
}
// mat4
{
  const cs = cases(N, i => [...matrix(i % 3 === 0 ? 'raw' : 'compose'), ...matrix('compose'), ...vec(), R(-3, 3)]);
  const want = [];
  for (const c of cs) {
    const a = new THREE.Matrix4().fromArray(c.slice(0, 16));
    const b = new THREE.Matrix4().fromArray(c.slice(16, 32));
    const s = c[35];
    want.push(...new THREE.Matrix4().multiplyMatrices(a, b).elements);
    want.push(a.determinant());
    want.push(...a.clone().invert().elements);
    want.push(...a.clone().transpose().elements);
    want.push(...a.clone().multiplyScalar(s).elements);
    want.push(a.getMaxScaleOnAxis());
    want.push(...new THREE.Matrix4().extractRotation(a).elements);
    want.push(...new THREE.Matrix3().getNormalMatrix(a).elements);
    const m3 = new THREE.Matrix3().setFromMatrix4(a);
    want.push(m3.determinant());
    want.push(...m3.invert().elements);
  }
  check('mat4', runProbe('mat4', flat(cs)), want, true);
}
// compose
{
  const cs = cases(N, () => [...vec(), ...quat(), R(-2, 2), R(-2, 2), R(-2, 2)]);
  const want = [];
  for (const c of cs) {
    const pos = new THREE.Vector3(c[0], c[1], c[2]);
    const q = new THREE.Quaternion(c[3], c[4], c[5], c[6]);
    const sc = new THREE.Vector3(c[7], c[8], c[9]);
    const m = new THREE.Matrix4().compose(pos, q, sc);
    want.push(...m.elements);
    const p2 = new THREE.Vector3(), q2 = new THREE.Quaternion(), s2 = new THREE.Vector3();
    m.decompose(p2, q2, s2);
    want.push(p2.x, p2.y, p2.z, q2.x, q2.y, q2.z, q2.w, s2.x, s2.y, s2.z);
    want.push(...new THREE.Matrix4().makeRotationFromQuaternion(q).elements);
  }
  check('compose', runProbe('compose', flat(cs)), want, true);
}
// lookat
{
  const cs = cases(N, i => {
    if (i % 17 === 0) return [1, 2, 3, 1, 2, 3, 0, 1, 0];       // eye == target
    if (i % 19 === 0) return [0, 0, 0, 0, 5, 0, 0, 1, 0];       // parallel up
    if (i % 23 === 0) return [0, 0, 0, 0, 0, 5, 0, 0, 1];       // parallel, z up
    return [...vec(), ...vec(), ...vec()];
  });
  const want = [];
  for (const c of cs) {
    const m = new THREE.Matrix4().lookAt(
      new THREE.Vector3(c[0], c[1], c[2]),
      new THREE.Vector3(c[3], c[4], c[5]),
      new THREE.Vector3(c[6], c[7], c[8]));
    want.push(...m.elements);
  }
  check('lookat', runProbe('lookat', flat(cs)), want, true);
}
// box
{
  const box = () => { const a = vec(), b = vec(); return [...a, ...b]; };
  const cs = cases(Math.floor(N / 2), i => [
    ...box(), ...box(), ...vec(), Math.abs(R(-1, 5)), ...vec(), R(-5, 5),
    ...vec(), ...vec(), ...vec(), ...vec(), ...matrix(i % 3 === 0 ? 'raw' : 'compose'),
  ]);
  const want = [];
  for (const c of cs) {
    const b1 = new THREE.Box3(new THREE.Vector3(c[0], c[1], c[2]), new THREE.Vector3(c[3], c[4], c[5]));
    const b2 = new THREE.Box3(new THREE.Vector3(c[6], c[7], c[8]), new THREE.Vector3(c[9], c[10], c[11]));
    const sph = new THREE.Sphere(new THREE.Vector3(c[12], c[13], c[14]), c[15]);
    const pl = new THREE.Plane(new THREE.Vector3(c[16], c[17], c[18]), c[19]);
    const pt = new THREE.Vector3(c[20], c[21], c[22]);
    const tri = new THREE.Triangle(
      new THREE.Vector3(c[23], c[24], c[25]), new THREE.Vector3(c[26], c[27], c[28]),
      new THREE.Vector3(c[29], c[30], c[31]));
    const m = new THREE.Matrix4().fromArray(c.slice(32, 48));
    const B = x => want.push(x ? 1 : 0);
    const P = v => want.push(v.x, v.y, v.z);
    B(b1.isEmpty()); B(b1.containsPoint(pt)); B(b1.containsBox(b2)); B(b1.intersectsBox(b2));
    B(b1.intersectsSphere(sph)); B(b1.intersectsPlane(pl)); B(b1.intersectsTriangle(tri));
    want.push(b1.distanceToPoint(pt));
    P(b1.getCenter(new THREE.Vector3())); P(b1.getSize(new THREE.Vector3()));
    P(b1.clampPoint(pt, new THREE.Vector3())); P(b1.getParameter(pt, new THREE.Vector3()));
    const s2 = b1.getBoundingSphere(new THREE.Sphere());
    P(s2.center); want.push(s2.radius);
    const u = b1.clone().union(b2); P(u.min); P(u.max);
    const u2 = b1.clone().intersect(b2); P(u2.min); P(u2.max);
    const u3 = b1.clone().applyMatrix4(m); P(u3.min); P(u3.max);
    B(sph.containsPoint(pt)); want.push(sph.distanceToPoint(pt)); B(sph.intersectsPlane(pl));
    want.push(pl.distanceToPoint(pt), pl.distanceToSphere(sph));
    P(sph.clampPoint(pt, new THREE.Vector3())); P(pl.projectPoint(pt, new THREE.Vector3()));
    const bb = sph.getBoundingBox(new THREE.Box3()); P(bb.min); P(bb.max);
    const s3 = sph.clone().expandByPoint(pt); P(s3.center); want.push(s3.radius);
    const s4 = sph.clone().applyMatrix4(m); P(s4.center); want.push(s4.radius);
    const pl2 = pl.clone().normalize(); P(pl2.normal); want.push(pl2.constant);
  }
  check('box', runProbe('box', flat(cs)), want, true);
}
// ray
{
  const cs = cases(Math.floor(N / 2), () => [
    ...vec(), ...vec(), ...vec(), Math.abs(R(0, 5)),
    ...vec(), ...vec(), ...vec(), R(-5, 5),
    ...vec(), ...vec(), ...vec(), ...vec(), ...vec(), ...vec(),
  ]);
  const want = [];
  for (const c of cs) {
    const ray = new THREE.Ray(new THREE.Vector3(c[0], c[1], c[2]), new THREE.Vector3(c[3], c[4], c[5]));
    const sph = new THREE.Sphere(new THREE.Vector3(c[6], c[7], c[8]), c[9]);
    const bx = new THREE.Box3(new THREE.Vector3(c[10], c[11], c[12]), new THREE.Vector3(c[13], c[14], c[15]));
    const pl = new THREE.Plane(new THREE.Vector3(c[16], c[17], c[18]), c[19]);
    const ta = new THREE.Vector3(c[20], c[21], c[22]);
    const tb = new THREE.Vector3(c[23], c[24], c[25]);
    const tc = new THREE.Vector3(c[26], c[27], c[28]);
    const pt = new THREE.Vector3(c[29], c[30], c[31]);
    const sa = new THREE.Vector3(c[32], c[33], c[34]);
    const sb = new THREE.Vector3(c[35], c[36], c[37]);
    const B = x => want.push(x ? 1 : 0);
    const P = v => want.push(v.x, v.y, v.z);
    const T = () => new THREE.Vector3(9, 9, 9);
    for (const culling of [false, true]) {
      const t = T();
      B(ray.intersectTriangle(ta, tb, tc, culling, t) !== null); P(t);
    }
    { const t = T(); B(ray.intersectSphere(sph, t) !== null); P(t); }
    { const t = T(); B(ray.intersectBox(bx, t) !== null); P(t); }
    { const t = T(); B(ray.intersectPlane(pl, t) !== null); P(t); }
    const dp = ray.distanceToPlane(pl);
    want.push(dp === null ? NaN : dp);
    want.push(ray.distanceToPoint(pt), ray.distanceSqToPoint(pt));
    P(ray.closestPointToPoint(pt, T()));
    const onRay = T(), onSeg = T();
    want.push(ray.distanceSqToSegment(sa, sb, onRay, onSeg)); P(onRay); P(onSeg);
    B(ray.intersectsSphere(sph)); B(ray.intersectsPlane(pl)); B(ray.intersectsBox(bx));
    const tri = new THREE.Triangle(ta, tb, tc);
    P(tri.getNormal(T()));
    want.push(tri.getArea());
    const bary = T();
    B(tri.getBarycoord(pt, bary) !== null); P(bary);
    B(tri.containsPoint(pt)); B(tri.isFrontFacing(ray.direction));
    P(tri.closestPointToPoint(pt, T())); P(tri.getMidpoint(T()));
  }
  check('ray', runProbe('ray', flat(cs)), want, true);
}
// mathutils
{
  const cs = cases(N, () => [R(-20, 20), R(-20, 20), R(-20, 20), R(-20, 20), R(-2, 2)]);
  const want = [];
  const wantL = [];
  for (const [x, y, a, b, t] of cs) {
    const U = THREE.MathUtils;
    want.push(U.clamp(x, a, b), U.euclideanModulo(x, y), U.mapLinear(x, a, b, y, t),
      U.inverseLerp(a, b, x), U.lerp(x, y, t), U.smoothstep(x, a, b), U.smootherstep(x, a, b),
      U.pingpong(x, y), U.degToRad(x), U.radToDeg(x), U.isPowerOfTwo(x) ? 1 : 0);
    wantL.push(U.damp(x, y, a, t), U.ceilPowerOfTwo(Math.abs(x)), U.floorPowerOfTwo(Math.abs(x)));
  }
  check('mathutils', runProbe('mathutils', flat(cs)), want, true);
  check('mathutils libm', runProbe('mathutilslibm', flat(cs)), wantL, true);
}
// v24
{
  const cs = cases(N, i => [
    R(-10, 10), R(-10, 10), R(-10, 10), R(-10, 10),
    R(-10, 10), R(-10, 10), R(-10, 10), R(-10, 10),
    R(-10, 10), R(-10, 10), R(-10, 10), R(-10, 10),
    R(-2, 2), ...matrix(i % 3 === 0 ? 'raw' : 'compose'),
  ]);
  const want = [];
  for (const c of cs) {
    const a2 = new THREE.Vector2(c[0], c[1]);
    const b2 = new THREE.Vector2(c[2], c[3]);
    const a4 = new THREE.Vector4(c[4], c[5], c[6], c[7]);
    const b4 = new THREE.Vector4(c[8], c[9], c[10], c[11]);
    const s = c[12];
    const m = new THREE.Matrix4().fromArray(c.slice(13, 29));
    const r2 = a2.clone().add(b2); want.push(r2.x, r2.y);
    want.push(a2.dot(b2), a2.cross(b2), a2.length());
    const n2 = a2.clone().normalize(); want.push(n2.x, n2.y);
    const l2 = a2.clone().lerp(b2, s); want.push(l2.x, l2.y);
    const m3 = new THREE.Matrix3().setFromMatrix4(m);
    const p2 = a2.clone().applyMatrix3(m3); want.push(p2.x, p2.y);
    const p4 = a4.clone().applyMatrix4(m); want.push(p4.x, p4.y, p4.z, p4.w);
    want.push(a4.dot(b4), a4.length());
    const c4 = a4.clone().clamp(b4, a4); want.push(c4.x, c4.y, c4.z, c4.w);
    const aa = new THREE.Vector4().setAxisAngleFromRotationMatrix(m);
    want.push(aa.x, aa.y, aa.z, aa.w);
  }
  // setAxisAngleFromRotationMatrix ends in acos: its 4 outputs are
  // reported as a mode of their own.
  const got = runProbe('v24', flat(cs));
  const stride = 29 - 13 + 9; // unused; outputs counted below
  void stride;
  const perOut = want.length / cs.length;
  const gotExact = [], wantExact = [], gotTail = [], wantTail = [];
  for (let i = 0; i < cs.length; i++) {
    gotExact.push(...got.slice(i * perOut, i * perOut + perOut - 4));
    wantExact.push(...want.slice(i * perOut, i * perOut + perOut - 4));
    gotTail.push(...got.slice(i * perOut + perOut - 4, (i + 1) * perOut));
    wantTail.push(...want.slice(i * perOut + perOut - 4, (i + 1) * perOut));
  }
  check('v24', gotExact, wantExact, true);
  check('v24 axisangle', gotTail, wantTail, true);
}
// tree
{
  const orders = ['XYZ', 'YXZ', 'ZXY', 'ZYX', 'YZX', 'XZY'];
  const cs = cases(Math.floor(20_000 * scale), () => {
    const one = [];
    for (let i = 0; i < 8; i++) {
      one.push(...vec(), R(-3, 3), R(-3, 3), R(-3, 3),
        R(0.4, 1.8) * (rnd() < 0.15 ? -1 : 1), R(0.4, 1.8), R(0.4, 1.8),
        i === 0 ? -1 : Math.floor(rnd() * i));
    }
    one.push(...quat(), R(-3, 3), R(-3, 3), R(-3, 3), ...vec(), R(-2, 2));
    return one;
  });
  const want = [];
  for (const c of cs) {
    const nodes = [];
    for (let i = 0; i < 8; i++) {
      const base = i * 10;
      const o = new THREE.Object3D();
      o.position.set(c[base], c[base + 1], c[base + 2]);
      o.rotation.set(c[base + 3], c[base + 4], c[base + 5], orders[i % 6]);
      o.scale.set(c[base + 6], c[base + 7], c[base + 8]);
      if (i === 2) { o.matrixAutoUpdate = false; o.updateMatrix(); }
      if (i === 4) { o.matrixWorldAutoUpdate = false; }
      nodes.push(o);
    }
    for (let i = 0; i < 8; i++) {
      const parent = c[i * 10 + 9];
      if (parent >= 0 && parent < i) nodes[parent].add(nodes[i]);
    }
    nodes[0].updateMatrixWorld(false);
    for (const o of nodes) want.push(...o.matrixWorld.elements);
    nodes[3].quaternion.set(c[80], c[81], c[82], c[83]);
    want.push(nodes[3].rotation.x, nodes[3].rotation.y, nodes[3].rotation.z);
    nodes[3].rotation.set(c[84], c[85], c[86], 'YXZ');
    const q3 = nodes[3].quaternion;
    want.push(q3.x, q3.y, q3.z, q3.w);
    const wp = nodes[7].getWorldPosition(new THREE.Vector3());
    want.push(wp.x, wp.y, wp.z);
    const wq = nodes[7].getWorldQuaternion(new THREE.Quaternion());
    want.push(wq.x, wq.y, wq.z, wq.w);
    const ws = nodes[7].getWorldScale(new THREE.Vector3());
    want.push(ws.x, ws.y, ws.z);
    const wd = nodes[7].getWorldDirection(new THREE.Vector3());
    want.push(wd.x, wd.y, wd.z);
    const target = new THREE.Vector3(c[87], c[88], c[89]);
    nodes[5].lookAt(target);
    const q5 = nodes[5].quaternion;
    want.push(q5.x, q5.y, q5.z, q5.w);
    const local = target.clone();
    nodes[7].localToWorld(local);
    want.push(local.x, local.y, local.z);
    nodes[7].worldToLocal(local);
    want.push(local.x, local.y, local.z);
    nodes[1].attach(nodes[6]);
    want.push(...nodes[6].matrixWorld.elements);
    want.push(nodes[6].position.x, nodes[6].position.y, nodes[6].position.z);
    const q6 = nodes[6].quaternion;
    want.push(q6.x, q6.y, q6.z, q6.w);
    nodes[2].rotateY(c[90]);
    nodes[2].translateZ(c[90]);
    nodes[2].updateWorldMatrix(true, true);
    want.push(...nodes[2].matrixWorld.elements);
  }
  // The tree runs through Euler syncs (asin/atan2) whenever a
  // quaternion is written.
  check('tree', runProbe('tree', flat(cs)), want, true);
}

console.log(`TOTAL gated: ${totalCases} cases, ${totalBad} mismatches`);
process.exit(totalBad === 0 ? 0 : 1);
