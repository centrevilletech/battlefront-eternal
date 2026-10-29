import * as THREE from "three";

// Pattern 15's orbit, the AT-AT tow-cable run, as the 2004 game flies
// it (docs/ai-vehicles.md "Pattern 15: the tow-cable run"). The pattern
// leaves an orbit request on the pilot's control (the target, a signed
// radius, a factor and an offset in the target's frame); each input step the
// shared movement update turns it into a movement request of mode 3 toward a
// point on an ellipse round the target:
//
//   - the centre C: the target's position 7 m up, plus the offset turned
//     into the target's frame;
//   - the ellipse: |radius| across, |radius| x factor along the target's
//     forward A;
//   - the point: the craft's flat position squashed along A by 1 / factor
//     about C (the ellipse becomes a circle of |radius|); from outside it
//     the tangent point (acos(r / L) round from the craft's bearing,
//     backward for a positive radius, forward for a negative one), from
//     inside straight out to the circle; stretched back by the factor, then
//     |radius| on along the ellipse there (across its normal, the side the
//     radius's sign gives); at the centre's height;
//   - the request's two numbers, with t = (the craft's flat distance from
//     C) / (factor x radius) - 1 on the signed radius: the share of the
//     climb over the terrain ahead the craft takes (the altitude stage's
//     weight for mode 3) clamp(t / 3, 0, 1), and its movement
//     scale (the throttle) clamp((t - 1) / 5, 0, 1). Near the
//     walker both are 0: the craft runs at its slowest, 7 m over the
//     walker's feet, among the legs. With a negative radius t is below 0
//     everywhere, so that run is slow and low all the way in.
//   - the tow pass: while the craft flies along the walker's heading (or
//     against it, as the offset's side and the radius's sign have it) and
//     is on the run's side of the centre, every seat of its hull with a tow
//     cable selects it.
//
// Before the cable holds the pattern asks for the offset (+-12.5, 0, 25),
// radius +-25 and factor 2.5: long passes down the walker's flank, the near
// one within a few metres of its legs. Once it holds, (0, 0, 8.75), +-25
// and 1: a 25 m circle round the legs that winds the cable.

const RISE = 7; // m over the target's position (the literal (0, 7, 0))
const ZERO = new THREE.Vector3();
const _x = new THREE.Vector3();
const _y = new THREE.Vector3();
const _z = new THREE.Vector3();
const _a = new THREE.Vector3();
const _f = new THREE.Vector3();
const _c2 = new THREE.Vector3();
const _p2 = new THREE.Vector3();
const _u = new THREE.Vector3();
const _n = new THREE.Vector3();
const _t = new THREE.Vector3();

// v with its part along `axis` (from `origin`) scaled by k.
function squash(v, origin, axis, k, out) {
  const d = axis.x * (v.x - origin.x) + axis.y * (v.y - origin.y) + axis.z * (v.z - origin.z);
  return out.copy(v).addScaledVector(axis, d * (k - 1));
}

// position, forward: the craft's (world); hull: the target's Vehicle;
// orbit: { offset, radius, factor }. Writes out { point, centre, blend,
// throttle, towPass } and returns it.
export function orbitSteer(position, forward, hull, orbit, out) {
  const offset = orbit.offset;
  const rho = orbit.radius;
  const r = Math.abs(rho);
  const k = orbit.factor || 1;
  const side = rho > 0;
  hull.matrix.extractBasis(_x, _y, _z);
  const C = out.centre.copy(hull.position);
  C.y += RISE;
  C.addScaledVector(_x, offset.x).addScaledVector(_y, offset.y).addScaledVector(_z, offset.z);
  _a.copy(_z).normalize();
  _f.set(forward.x, 0, forward.z);
  if (_f.lengthSq() > 1e-8) _f.normalize();
  // The tangent, in the squashed (circular) frame.
  _c2.set(C.x, 0, C.z);
  squash(_p2.set(position.x, 0, position.z), _c2, _a, 1 / k, _p2);
  const dx = _p2.x - _c2.x;
  const dz = _p2.z - _c2.z;
  const L = Math.hypot(dx, dz);
  if (!(r > L)) {
    const phi = Math.atan2(dz, dx) + (side ? -1 : 1) * Math.acos(L > 0 ? r / L : 1);
    _u.set(Math.cos(phi), 0, Math.sin(phi));
  } else if (L > 1e-9) _u.set(dx / L, 0, dz / L);
  else _u.set(1, 0, 0);
  _t.copy(_c2).addScaledVector(_u, r);
  squash(_t, _c2, _a, k, _t);
  // The ellipse's normal there, and the point |radius| on across it.
  squash(_u, ZERO, _a, 1 / k, _n);
  _n.y = 0;
  if (_n.lengthSq() > 1e-12) _n.normalize();
  out.point.set(_t.x - (side ? 1 : -1) * r * -_n.z, C.y, _t.z - (side ? 1 : -1) * r * _n.x);
  // The request's height share and movement scale.
  const t = rho ? Math.hypot(C.x - position.x, C.z - position.z) / (k * rho) - 1 : 0;
  out.blend = THREE.MathUtils.clamp(t / 3, 0, 1);
  out.throttle = THREE.MathUtils.clamp((t - 1) / 5, 0, 1);
  // The run's tow pass.
  const along = _f.dot(_a) > 0;
  const ahead = _a.x * (C.x - position.x) + _a.y * (C.y - position.y) + _a.z * (C.z - position.z) > 0;
  out.towPass = (along !== offset.x > 0) !== side && ahead !== side;
  return out;
}

export function orbitResult() {
  return { point: new THREE.Vector3(), centre: new THREE.Vector3(), blend: 0, throttle: 0, towPass: false };
}

export const _test = { squash, RISE };
