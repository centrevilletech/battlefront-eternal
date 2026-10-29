import * as THREE from "three";

// Classic aiming: the AI direction filter every bot's gun sits behind, the
// lead it aims with, and the point its body turns toward, following the 2004
// game (docs/ai-combat.md).
//
// The filter. A bot's body turns toward what it wants to hit (its analogs,
// Nav's analogs()), and its desired aim direction d is the body's aim. The
// gun does not point along d: an Aimer keeps its own direction q and a
// persistent rate v, and each update
//
//   1. if dot(d, q) < cos 30deg, q jumps to d turned 30deg about world Y
//      (towards q's side: +30deg when (d x q).y > 0, else -30deg) and v = 0;
//   2. a0 = ((d - q) / dt - v) / dt;
//   3. a0 is clamped to length A;
//   4. a = a0 - D v;
//   5. v += a dt;
//   6. q = normalize(q + v dt).
//
// A and D are the side's curves C1 and C2 (difficulty.js), halved when the
// weapon in hand is not a cannon (a WeaponCannon or a class built on it:
// launchers and grappling hooks are cannons; grenades, melee, dispensers,
// repair tools and shields are not). The same call hands the weapon the
// side's spread curves (C3 buildup, C4 sniper yaw). A hero's aim skips the
// filter. The arithmetic below follows the 2004 game's, operation for
// operation in float32: divisions by dt are multiplications by float32(1/dt),
// dot products round each product and sum, and the final normalize is the
// D3DX9 one (a reciprocal square root refined by one Newton step). The 2004
// game has no guard against dt <= 0 (its rate becomes NaN for good); the
// Aimer below skips such updates instead, a disclosed safe-port divergence.
//
// Leading. The target's aim point moves by h (target velocity - shooter
// velocity), then is corrected for the projectile: with no
// gravity it solves the flight time to the moved point and adds that much of
// the target's velocity; with gravity it solves the time to reach the point
// (the lower arc), raises the point by the drop, and adds sqrt(T^2) of the
// velocity. Both keep the 2004 game's fallbacks: a negative discriminant or
// flight time leaves the point, a same-speed receding target gives Infinity,
// and an unreachable lob is reported invalid yet still moves the point. The
// AIThread's aim step leads with h = the accumulated AI dt; fire validation
// leads with h = 0.

const f = Math.fround;

// ---------------------------------------------------------------------------
// The 2004 game's sine/cosine tables and its interpolating lookups: 2048
// entries of float32 sin/cos(float32(i * 2pi/2048)), linearly interpolated,
// wrapping at 2048.

const TABLE_STEP = f(0.003067961661145091); // 2pi / 2048
const TABLE_SCALE = f(325.9493103027344); // 2048 / 2pi
let sinTable = null;
let cosTable = null;

function tables() {
  if (cosTable) return;
  sinTable = new Float32Array(2048);
  cosTable = new Float32Array(2048);
  for (let i = 0; i < 2048; i++) {
    const a = f(i * TABLE_STEP);
    sinTable[i] = Math.sin(a);
    cosTable[i] = Math.cos(a);
  }
}

function lookup(table, angle) {
  const t = f(f(angle) * TABLE_SCALE);
  const i = Math.trunc(t);
  const frac = f(t - i);
  return f(f(f(1 - frac) * table[i & 2047]) + f(table[(i + 1) & 2047] * frac));
}

export function lookupCos(angle) {
  tables();
  return lookup(cosTable, angle);
}

export function lookupSin(angle) {
  tables();
  return lookup(sinTable, angle);
}

// The thresholds the 2004 game takes from its tables. 1.3526301383972168
// rad is 77.5 degrees (the sight cone and the facing cones); pi/8 and pi/4
// gate firing; pi/10 and pi/3 are the attack patterns' facing tests.
export const ANGLE = Object.freeze({
  CONE: f(1.3526301383972168),
  FIRE: f(Math.PI / 8),
  MELEE: f(Math.PI / 4),
  FACE_TIGHT: f(0.3141592741012573), // pi/10 (attack pattern 4)
  FACE_WIDE: f(1.0471975803375244), // pi/3 (attack pattern 5)
});

// ---------------------------------------------------------------------------
// Float32 vector helpers in the 2004 game's operation order.

// Dot product, y*y first, then x*x, then z*z (each step rounded to float32).
export function dot32(ax, ay, az, bx, by, bz) {
  return f(f(f(ay * by) + f(ax * bx)) + f(az * bz));
}

// Squared length, (x*x + y*y) + z*z.
export function lengthSq32(x, y, z) {
  return f(f(f(x * x) + f(y * y)) + f(z * z));
}

// Horizontal squared length x*x + z*z.
export function lengthSqXZ32(x, z) {
  return f(f(x * x) + f(z * z));
}

// D3DXVec3Normalize: squared length; below 2^-46 the result is zero;
// otherwise one Newton step on the reciprocal square root,
// y1 = (3 - s y0 y0) (y0 / 2). The hardware's reciprocal square root
// estimate is vendor specific, so the starting estimate here is
// float32(1 / float32(sqrt s)).
export function normalizeD3DX(x, y, z, out) {
  const s = lengthSq32(x, y, z);
  if (!(s >= 1.4210854715202004e-14)) {
    out.x = 0;
    out.y = 0;
    out.z = 0;
    return out;
  }
  const y0 = f(1 / f(Math.sqrt(s)));
  const t = f(f(y0 * s) * y0);
  const r = f(f(3 - t) * f(y0 * 0.5));
  out.x = f(x * r);
  out.y = f(y * r);
  out.z = f(z * r);
  return out;
}

// Normalize, but exactly zero becomes (1, 0, 0).
export function normalizeOrX(x, y, z, out) {
  if (x === 0 && y === 0 && z === 0) {
    out.x = 1;
    out.y = 0;
    out.z = 0;
    return out;
  }
  return normalizeD3DX(x, y, z, out);
}

// The horizontal direction (y = 0), normalized; a zero horizontal
// vector becomes (1, 0, 0).
export function normalizeXZ(x, z, out) {
  if (x === 0 && z === 0) {
    out.x = 1;
    out.y = 0;
    out.z = 0;
    return out;
  }
  return normalizeD3DX(x, 0, z, out);
}

// ---------------------------------------------------------------------------
// The direction filter. d: desired direction; q: the aim state's
// current direction (read only); v: the persistent rate (updated in place);
// out: the new direction (may be q). Plain {x, y, z} objects are fine.

const COS_30 = f(Math.cos(f(0.5235987901687622)));
const THETA_30 = f(0.5235987901687622);

export function filterDirection(d, q, v, dt, A, D, out) {
  dt = f(dt);
  A = f(A);
  D = f(D);
  const dx = f(d.x), dy = f(d.y), dz = f(d.z);
  let qx = f(q.x), qy = f(q.y), qz = f(q.z);
  let vx = f(v.x), vy = f(v.y), vz = f(v.z);
  // 1. The 30-degree correction: d turned about world Y, from the third row
  // of [d x (d x q); d x q; d] * RotationY(theta).
  if (COS_30 > dot32(dx, dy, dz, qx, qy, qz)) {
    const crossY = f(f(qx * dz) - f(dx * qz));
    const theta = crossY > 0 ? THETA_30 : -THETA_30;
    const c = f(Math.cos(theta));
    const s = f(Math.sin(theta));
    qx = f(f(dx * c) + f(dz * s));
    qy = dy;
    qz = f(f(dx * f(-s)) + f(dz * c));
    vx = 0;
    vy = 0;
    vz = 0;
  }
  // 2. a0 = ((d - q) / dt - v) / dt, dividing by multiplying by 1/dt.
  const inv = f(1 / dt);
  let ax = f(f(f(f(dx - qx) * inv) - vx) * inv);
  let ay = f(f(f(f(dy - qy) * inv) - vy) * inv);
  let az = f(f(f(f(dz - qz) * inv) - vz) * inv);
  // 3. Clamp its length to A.
  const len = f(Math.sqrt(lengthSq32(ax, ay, az)));
  const k = A > len ? 1 : f(A / len);
  ax = f(ax * k);
  ay = f(ay * k);
  az = f(az * k);
  // 4.-5. a = a0 - D v; v += a dt.
  ax = f(ax - f(vx * D));
  ay = f(ay - f(vy * D));
  az = f(az - f(vz * D));
  vx = f(vx + f(ax * dt));
  vy = f(vy + f(ay * dt));
  vz = f(vz + f(az * dt));
  v.x = vx;
  v.y = vy;
  v.z = vz;
  // 6. q + v dt, normalized.
  return normalizeD3DX(f(qx + f(vx * dt)), f(qy + f(vy * dt)), f(qz + f(vz * dt)), out);
}

// ---------------------------------------------------------------------------
// Weapon classes as the filter and the firing code test them. `base` is the
// ODF class label (units.json `base`); the 2004 game maps labels to engine
// classes, and its type tests follow inheritance.

const CANNON_BASES = new Set(["cannon", "weapon", "launcher", "grapplinghookweapon"]);

export function isCannon(def) {
  return Boolean(def && CANNON_BASES.has(def.base));
}

export function isLauncher(def) {
  return def?.base === "launcher";
}

// The four skill curves, for when no difficulty object is bound
// (tests, tools): Population's difficulty.js is the authority and computes
// the same values. Each curve runs through its anchors at skill 1, 14 and
// 20, with the 2004 game's float32 order (fraction, anchor difference,
// product, sum).
const CURVE_ANCHORS = { A: [0.8, 1.9, 20], D: [0.8, 1.9, 2.4], B: [3, 1.6, 1], S: [2, 1, 0] };

export function skillCurves(skill, out = {}) {
  const k = Math.min(Math.max(Math.trunc(skill), 1), 20);
  for (const name in CURVE_ANCHORS) {
    const [a, b, c] = CURVE_ANCHORS[name].map(f);
    out[name] = k < 14 ? f(a + f(f(f(k - 1) / 13) * f(b - a))) : f(b + f(f(f(k - 14) / 6) * f(c - b)));
  }
  return out;
}

// Medium's enemy skill (14), used only when a controller has no curves.
const FALLBACK_CURVES = skillCurves(14);

// The side's four curves for a unit's aim with this weapon: A and D halved
// for a non-cannon weapon; untouched when there is no weapon at all.
export function aimCurves(curves, weaponDef, out = {}) {
  const c = curves && Number.isFinite(curves.A) ? curves : FALLBACK_CURVES;
  out.A = f(c.A);
  out.D = f(c.D);
  out.B = f(c.B ?? 1);
  out.S = f(c.S ?? 0);
  if (weaponDef && !isCannon(weaponDef)) {
    out.A = f(out.A * 0.5);
    out.D = f(out.D * 0.5);
  }
  return out;
}

// One bot's Aimer: the filtered direction its weapons fire along. update()
// is what the soldier's aiming does each step after its body
// moved: d is the body's aim direction (Unit.forward), and the result goes
// to intent.aim. It also hands the weapon in hand the side's spread curves,
// as the filter's calls do (only cannons keep them; others
// ignore them in the 2004 game, so they are set on cannons only).
export class Aimer {
  constructor() {
    this.direction = new THREE.Vector3(0, 0, 1);
    this.velocity = new THREE.Vector3();
    this.started = false;
    this.curves = { A: 0, D: 0, B: 1, S: 0 };
  }

  // Start from the body's aim, at rest (a fresh spawn or a new owner).
  reset(direction) {
    this.direction.copy(direction);
    this.velocity.set(0, 0, 0);
    this.started = true;
  }

  // desired: unit aim direction; curves: the side's {A, D, B, S}; weapon:
  // the selected port Weapon or null; hero: the hero class flag.
  update(desired, dt, curves, weapon = null, hero = false) {
    if (!this.started) this.reset(desired);
    // A hero aims straight along its body (the filter is skipped).
    if (hero) {
      this.direction.copy(desired);
      this.velocity.set(0, 0, 0);
      return this.direction;
    }
    const c = aimCurves(curves, weapon?.def ?? null, this.curves);
    if (weapon && isCannon(weapon.def)) {
      weapon.spreadScale = c.B;
      weapon.sniperScale = c.S;
    }
    if (!(dt > 0)) return this.direction;
    filterDirection(desired, this.direction, this.velocity, dt, c.A, c.D, this.direction);
    // A nonfinite result (only from nonfinite input) restarts the filter at
    // the body's aim rather than poisoning every later update.
    if (!Number.isFinite(this.direction.x) || !Number.isFinite(this.velocity.x)) this.reset(desired);
    return this.direction;
  }
}

// ---------------------------------------------------------------------------
// Projectile speed and gravity as the leading code reads them from a weapon
// (the weapon's and the ordnance classes' getters): bolts, bullets, fat rays
// and tow cables fly at Velocity with no gravity; missiles report 3e8 (no
// lead: they steer); shells, grenades and sticky ordnance fall at
// Gravity x -9.8; beams and emitters report 0 (no lead). No ordnance (melee,
// dispensers, ...) is speed 0.

const FALLING = new Set(["shell", "grenade", "sticky", "haywire", "beacon"]);
const STRAIGHT = new Set(["bolt", "laser", "bullet", "fatray", "towcable", "grapplinghook"]);

export function projectileOf(def, out = { speed: 0, gravity: 0 }) {
  const ordnance = def?.ordnance;
  out.speed = 0;
  out.gravity = 0;
  if (!ordnance) return out;
  const base = ordnance.base;
  if (base === "missile") out.speed = 3e8;
  else if (STRAIGHT.has(base) || FALLING.has(base)) out.speed = f(ordnance.Velocity ?? (base === "shell" ? 35 : 150));
  if (FALLING.has(base)) out.gravity = f(f(ordnance.Gravity ?? 1) * f(-9.8));
  return out;
}

// The weapon's projectile while it charges: the 2004 game interpolates
// along the charge chain by held time; the port uses the level reached
// (provisional, see docs/ai-combat.md).
function projectileOfWeapon(weapon, out) {
  return projectileOf(weapon?.def ?? null, out);
}

// Correct point p (moved by any relative-motion lead) for the
// projectile's flight from `origin`. v: the target's velocity. Writes out;
// returns the solver's validity. All arithmetic float32, as the 2004 game.
export function ballisticLead(p, v, origin, speed, gravity, out) {
  out.x = f(p.x);
  out.y = f(p.y);
  out.z = f(p.z);
  const s = f(speed);
  if (!(s > f(0.0001))) return true;
  const rx = f(p.x - origin.x), ry = f(p.y - origin.y), rz = f(p.z - origin.z);
  const g = f(gravity);
  const s2 = f(s * s);
  const vx = f(v.x), vy = f(v.y), vz = f(v.z);
  if (g === 0) {
    const b = f(-dot32(rx, ry, rz, vx, vy, vz));
    const C = dot32(rx, ry, rz, rx, ry, rz);
    const b2 = f(b * b);
    const vv = dot32(vx, vy, vz, vx, vy, vz);
    const D = f(b2 - f(f(vv - s2) * C));
    const den = D > 0 ? f(f(Math.sqrt(D)) + b) : b;
    const t = f(C / den);
    if (t > 0) {
      out.x = f(out.x + f(t * vx));
      out.y = f(out.y + f(t * vy));
      out.z = f(out.z + f(t * vz));
    }
    return true;
  }
  let valid = true;
  const g2 = f(g * g);
  const K = f(f(ry * g) + s2);
  const K2 = f(K * K);
  const D = f(K2 - f(lengthSq32(rx, ry, rz) * g2));
  let H = K;
  if (D > 0) H = f(K - f(Math.sqrt(D)));
  else valid = false;
  const T2 = f(f(H + H) / g2);
  if (!(T2 > 0)) return false;
  out.y = f(out.y + f(f(g * -0.5) * T2));
  const t = f(Math.sqrt(T2));
  out.x = f(out.x + f(t * vx));
  out.y = f(out.y + f(t * vy));
  out.z = f(out.z + f(t * vz));
  return valid;
}

const leadP = new THREE.Vector3();
const leadRel = new THREE.Vector3();
const leadOrigin = new THREE.Vector3();
const zeroVelocity = new THREE.Vector3();
const projectileScratch = { speed: 0, gravity: 0 };

// The point a shooter leads `target` to, h seconds of relative
// motion plus the projectile correction. weapon: the selected port Weapon
// (null: no correction). Returns the solver's validity; writes out.
export function leadPoint(h, shooter, target, weapon, out) {
  target.aimPoint(leadP);
  const tv = target.velocity ?? zeroVelocity;
  const sv = shooter.velocity ?? zeroVelocity;
  leadRel.subVectors(tv, sv).multiplyScalar(f(h));
  leadP.set(f(leadP.x + f(leadRel.x)), f(leadP.y + f(leadRel.y)), f(leadP.z + f(leadRel.z)));
  if (!weapon) {
    out.copy(leadP);
    return true;
  }
  shooter.origin(leadOrigin);
  const proj = projectileOfWeapon(weapon, projectileScratch);
  return ballisticLead(leadP, tv, leadOrigin, proj.speed, proj.gravity, out);
}

// The ballistic correction with zero velocity: the drop correction the firing
// check applies to a remembered point.
export function dropCorrect(point, shooter, weapon, out) {
  shooter.origin(leadOrigin);
  const proj = projectileOfWeapon(weapon, projectileScratch);
  return ballisticLead(point, zeroVelocity, leadOrigin, proj.speed, proj.gravity, out);
}

// ---------------------------------------------------------------------------
// The point a bot's body turns toward, for aim states 0 and 1:
// state 0 leads the controller's aim target (with the AI's accumulated dt)
// when there is a target and a weapon in hand, else looks 30 m along the
// current aim; state 1 is the stored point. Other states belong to Nav's
// analogs() (30 m along the steering direction). Returns out, or null for
// the steering states.

const aimFwd = new THREE.Vector3();

export function aimPointFor(controller, unit, dt, out, resolve = defaultResolve) {
  const state = controller.aimState;
  if (state === 1) return out.copy(controller.aimPoint);
  if (state !== 0) return null;
  const target = resolve(controller.aimTarget ?? controller.target);
  const weapon = selectedWeapon(unit);
  if (target && weapon) {
    leadPoint(dt, unit, target, weapon, out);
    return out;
  }
  unit.origin(out);
  return out.addScaledVector(unit.forward(aimFwd), 30);
}

function defaultResolve(ref) {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
}

// The weapon the bot fires: the controller's chosen channel's selection.
export function selectedWeapon(unit) {
  const arsenal = unit?.combat?.arsenal;
  if (!arsenal) return null;
  const channel = unit.controller?.perception?.fire?.channel ?? 0;
  return arsenal.current(channel) || arsenal.current(0) || null;
}
