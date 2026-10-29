import * as THREE from "three";
import { selectWeapon, forwardSpeed } from "../weapons-ai.js";
import { steerState } from "./steering.js";

// Vehicle combat: the engagement dispatch for walker, hover and flyer
// controls, and the attack patterns only vehicles run
// (docs/ai-vehicles.md, "Vehicle combat").
//
// Combat (src/ai/classic/combat.js) owns the dispatcher and AttackPatterns;
// it calls these through world.vehicles: dispatch() for source classifiers
// it leaves to vehicles, enterPattern/tickPattern/exitPattern for states 7
// and 13-16, and sidestep() from pattern 2.

const f = Math.fround;
const UP = new THREE.Vector3(0, 1, 0);
const _p = new THREE.Vector3();
const _d = new THREE.Vector3();
const _n = new THREE.Vector3();
const _q = new THREE.Vector3();
const _fwd = new THREE.Vector3();
const choice = { ok: false, index: -1, weapon: null, action: 6, inBand: false };

function selectOnly(controller, unit, target, switchAllowed, world, out) {
  selectWeapon(controller, unit, target, switchAllowed, world, choice);
  out.aux = choice.ok;
  out.action = choice.action;
  out.inBand = choice.inBand;
  out.weapon = choice.weapon;
  return out;
}

const isVehicleOrBuilding = (k) => k === 2 || k === 4 || k === 6 || k === 8 || k === 9;

// The AT-AT: the class whose name the 2004 game compares against the
// literal "AT-AT" (its label in the stock data).
export function isAtAt(unit) {
  const body = unit?.vehicle ?? unit;
  const def = body?.def ?? body?.classDef;
  const label = def?.label ?? def?.settings?.Label ?? null;
  return label === "AT-AT" || body?.name === "imp_walk_atat" || unit?.className === "imp_walk_atat";
}

// The dispatcher's vehicle rows. Walker: pattern 2 against walkers, hovers and
// buildings when a weapon is in band, weapon choice only otherwise. Hover:
// against vehicles and buildings the DenseEnvironment policy (pattern 7 for
// a fast, narrow hover in open terrain, else pattern 2 in band), weapon
// choice only against infantry and aircraft. Flyer: against walkers and
// buildings the AT-AT tow orbit (15) or the attack run (13); weapon choice
// only against everything else.
export function vehicleDispatch(controller, unit, target, switchAllowed, world, out, options = {}) {
  out.pattern = 0;
  out.probability = 0;
  const s = unit.kind;
  const t = target.kind;
  if (s === 2) {
    selectOnly(controller, unit, target, switchAllowed, world, out);
    if (isVehicleOrBuilding(t) && out.aux && out.inBand) out.pattern = 2;
    return out;
  }
  if (s === 6) {
    selectOnly(controller, unit, target, switchAllowed, world, out);
    if (!isVehicleOrBuilding(t) || !out.aux) return out;
    const c = unit.classDef ?? {};
    if (!options.dense && f(c.ForwardSpeed ?? 0) >= 14 && f(c.StrafeSpeed ?? 0) < 5) out.pattern = 7;
    else if (out.inBand) out.pattern = 2;
    return out;
  }
  if (s === 5) {
    selectOnly(controller, unit, target, switchAllowed, world, out);
    if (t === 2 || t === 8 || t === 9) {
      // The AT-AT branch comes before the weapon test.
      const body = unit.vehicle;
      if (isAtAt(target) && body && body.occupants === 2) out.pattern = 15;
      else if (out.aux && !(body?.driver)) out.pattern = 13;
    }
    return out;
  }
  // Classifier 4 and anything else: weapon choice only.
  return selectOnly(controller, unit, target, switchAllowed, world, out);
}

// -- pattern state ------------------------------------------------------------------------

function stateOf(patterns) {
  return (patterns.vehicle ||= {
    toggle: false, // hover pass side
    entry: new THREE.Vector3(), // hover pass origin O
    destination: new THREE.Vector3(),
    best: Infinity, // nearest distance^2 to the pass point
    progressAt: 0,
    fallback: false,
    loop: false,
    inverted: false,
    flipA: false,
    flipB: false,
  });
}

const now = (patterns) => patterns.world.time ?? 0;
const trackerPoint = (patterns, out) => {
  const t = patterns.agent.tracker;
  if (t?.point) return out.copy(t.point);
  const target = t?.freshTarget ?? t?.target ?? null;
  const u = target?.unit ?? target;
  return u?.position ? out.copy(u.position) : null;
};
const setDelay = (patterns, seconds) => patterns.agent.tracker?.setDelay?.(seconds, now(patterns));
const defaultDelay = (patterns) => patterns.agent.tracker?.defaultDelay?.(now(patterns));

// -- hover pattern 7 ------------------------------------------------------------------------

// The next pass point. Away leg (flag false): 3 x top speed
// ahead. Target leg (flag true): beside the target (the source and target
// radii plus 2 out along the heading's component across the line to it),
// then half the top speed further on, away from the hover.
function hoverPassPoint(patterns, unit, st, out) {
  const P = unit.position;
  const v = unit.classDef?.ForwardSpeed ?? unit.soldier?.maxSpeed ?? 0;
  bodyForward(unit, _fwd);
  if (!st.toggle) return out.copy(P).addScaledVector(_fwd, 3 * v);
  if (!trackerPoint(patterns, _q)) return out.copy(P).addScaledVector(_fwd, 3 * v);
  normalizeXZ(_d.subVectors(_q, P));
  const along = _fwd.dot(_d);
  normalizeXZ(_n.copy(_fwd).addScaledVector(_d, -along));
  const target = patterns.agent.tracker?.freshTarget ?? null;
  const r = (unit.sphere?.radius ?? 0) + (target?.sphere?.radius ?? 0) + 2;
  out.copy(_q).addScaledVector(_n, r);
  normalizeXZ(_p.subVectors(out, P));
  return out.addScaledVector(_p, 0.5 * v);
}

function hoverEnter(patterns) {
  const unit = patterns.controller.unit;
  const st = stateOf(patterns);
  st.toggle = true;
  st.entry.copy(unit.position);
  setDelay(patterns, 12);
  hoverLeg(patterns, unit, st);
}

function hoverLeg(patterns, unit, st) {
  hoverPassPoint(patterns, unit, st, st.destination);
  patterns.agent.helper?.goto?.(st.destination, { mode: "direct" });
  st.best = horizontalDistanceSq(unit.position, st.destination);
  st.progressAt = now(patterns);
  st.fallback = false;
}

// A new leg the other way when this one is done or crossed
// (dot(P - Q, Q - O) > 0); ten seconds without getting nearer falls back to
// an ordinary path to the target; every tick asks for turning while moving.
function hoverTick(patterns) {
  const unit = patterns.controller.unit;
  const st = stateOf(patterns);
  const P = unit.position;
  const Q = st.destination;
  const crossed = (P.x - Q.x) * (Q.x - st.entry.x) + (P.y - Q.y) * (Q.y - st.entry.y) + (P.z - Q.z) * (Q.z - st.entry.z) > 0;
  const helper = patterns.agent.helper;
  if (helper?.arrived || crossed) {
    st.toggle = !st.toggle;
    hoverLeg(patterns, unit, st);
  } else {
    const d2 = horizontalDistanceSq(P, Q);
    if (d2 < st.best) {
      st.best = d2;
      st.progressAt = now(patterns);
    } else if (!(now(patterns) <= st.progressAt + 10) && !st.fallback) {
      if (trackerPoint(patterns, _q)) helper?.goto?.(_q);
      st.fallback = true;
    }
  }
  steerState(unit).movingTurnAt = now(patterns);
  patterns.face?.(0);
}

// -- flyer patterns ---------------------------------------------------------------------------

// The attack run's distances are set once for the whole process, from the
// first aircraft that runs one (static initialization in the 2004 game).
const run = { A: null, B: null };
export function resetAttackRun() {
  run.A = run.B = null;
}

// P, then 90 above a point A short of the target, the target
// approach B short of it, and 30 above B past it; the three attack points
// held under the AI's maximum fly height.
function attackRunPath(patterns, unit, maxFly) {
  const v = unit.classDef?.MaxSpeed ?? 30;
  if (run.A === null) {
    run.A = Math.max(160, 4 * v);
    run.B = Math.max(40, v);
  }
  const P = unit.position;
  if (!trackerPoint(patterns, _q)) return null;
  normalizeXZ(_d.subVectors(_q, P));
  const clamp = (y) => Math.min(y, maxFly);
  return [
    [P.x, P.y, P.z],
    [_q.x - run.A * _d.x, clamp(_q.y + 90), _q.z - run.A * _d.z],
    [_q.x - run.B * _d.x, clamp(_q.y), _q.z - run.B * _d.z],
    [_q.x + run.B * _d.x, clamp(_q.y + 30), _q.z + run.B * _d.z],
  ];
}

function attackRunEnter(patterns, env) {
  setDelay(patterns, 15);
  const unit = patterns.controller.unit;
  const path = attackRunPath(patterns, unit, env.maxFlyHeight ?? 200);
  if (path) patterns.agent.helper?.followPath?.(path, false, false);
}

// Fire straight ahead when the target is ahead (the displacement
// is not normalized, so 0.5 is not a cone), run again when the run is flown,
// and keep at least half the top speed.
function attackRunTick(patterns, env) {
  const unit = patterns.controller.unit;
  const target = patterns.agent.tracker?.freshTarget ?? null;
  if (target) {
    unit.forward(_fwd);
    _d.subVectors(target.position, unit.position);
    if (_fwd.dot(_d) > 0.5) patterns.controller.fireA = true;
  }
  if (patterns.agent.helper?.arrived) attackRunEnter(patterns, env);
  minSpeed(unit, 0.5 * (unit.classDef?.MaxSpeed ?? 30), now(patterns));
  patterns.face?.(0);
}

// Dormant pattern 14 (no stock dispatch selects it): pursue, with loops.
function pursuitEnter(patterns) {
  const st = stateOf(patterns);
  st.loop = st.inverted = false;
  setDelay(patterns, 10);
}

function pursuitTick(patterns, env) {
  const unit = patterns.controller.unit;
  const st = stateOf(patterns);
  const random = patterns.random;
  const target = patterns.agent.tracker?.freshTarget ?? null;
  unit.forward(_fwd);
  if (target) {
    _d.subVectors(target.position, unit.position);
    if (_fwd.dot(_d) > 0 || random.int(1, 3) === 1) patterns.controller.fireA = true;
  }
  const v = unit.classDef?.MaxSpeed ?? 30;
  if (!st.loop) {
    if (random.int(1, 13) === 1) st.loop = true;
    else if (target) {
      const close = _d.subVectors(target.position, unit.position).lengthSq() <= (2 * v) ** 2;
      if (close) {
        _p.copy(unit.position).addScaledVector(_fwd, 5 * v);
        _p.y = ((env.minFlyHeight ?? -200) + (env.maxFlyHeight ?? 200)) / 2;
      } else {
        _p.copy(target.position).add(target.velocity ?? _q.set(0, 0, 0));
        _p.y = env.maxFlyHeight ?? 200;
      }
      patterns.agent.helper?.goto?.(_p, { mode: "direct" });
    }
  } else {
    const upY = unit.upY ?? 1;
    if (upY < 0) st.inverted = true;
    else if (st.inverted && upY > 0) st.loop = st.inverted = false;
  }
  const steer = steerState(unit);
  steer.loop = st.loop;
  steer.loopAt = now(patterns);
  minSpeed(unit, 0.5 * v, now(patterns));
}

// Pattern 15, the AT-AT tow orbit: circle it, close in once the cable holds
// (the run itself: orbit.js, flown by steering.js). The orbit is on the
// tracker's target whether or not the scanner sees it this cycle (entry and
// tick read the target, not the fresh-only freshTarget): circling it, the
// craft has the walker off its nose, outside the scanner's cone, most of the
// time.
function orbitEnter(patterns) {
  const st = stateOf(patterns);
  const random = patterns.random;
  patterns.agent.helper?.stop?.();
  st.flipA = (random.next() & 1) === 0;
  st.flipB = (random.next() & 1) === 0;
  setDelay(patterns, 10);
  orbitRequest(patterns, st);
}

function orbitRequest(patterns, st) {
  const unit = patterns.controller.unit;
  const target = patterns.agent.tracker?.target ?? null;
  const towed = (unit.vehicle?.towTarget ?? -1) >= 0;
  const steer = steerState(unit);
  steer.orbit = {
    target: target ? { unit: target, generation: target.generation } : null,
    offset: towed ? new THREE.Vector3(0, 0, 8.75) : new THREE.Vector3(st.flipA ? -12.5 : 12.5, 0, 25),
    radius: st.flipB ? -25 : 25,
    factor: towed ? 1 : 2.5,
  };
}

function orbitTick(patterns) {
  orbitRequest(patterns, stateOf(patterns));
  patterns.face?.(0);
}

function orbitExit(patterns) {
  const unit = patterns.controller.unit;
  if (unit?.steer) unit.steer.orbit = null;
  defaultDelay(patterns);
}

// Dormant pattern 16: go straight for the target; no shooting of its own.
function directEnter(patterns) {
  patterns.agent.helper?.stop?.();
  setDelay(patterns, 10);
}

function directTick(patterns) {
  const target = patterns.agent.tracker?.freshTarget ?? null;
  if (target) patterns.agent.helper?.goto?.(target.position);
}

// The control's minimum-speed request, renewed every tick the
// pattern runs.
function minSpeed(unit, speed, time) {
  const steer = steerState(unit);
  steer.minSpeed = speed;
  steer.minSpeedAt = time;
}

// -- the hooks Combat calls ----------------------------------------------------------------------

export function enterPattern(patterns, state, env) {
  switch (state) {
    case 7:
      return hoverEnter(patterns);
    case 13:
      return attackRunEnter(patterns, env);
    case 14:
      return pursuitEnter(patterns);
    case 15:
      return orbitEnter(patterns);
    case 16:
      return directEnter(patterns);
    default:
  }
}

export function tickPattern(patterns, state, dt, env) {
  switch (state) {
    case 7:
      return hoverTick(patterns);
    case 13:
      return attackRunTick(patterns, env);
    case 14:
      return pursuitTick(patterns, env);
    case 15:
      return orbitTick(patterns);
    case 16:
      return directTick(patterns);
    default:
  }
}

export function exitPattern(patterns, state) {
  const unit = patterns.controller?.unit;
  if (unit?.steer) {
    unit.steer.minSpeed = 0;
    unit.steer.loop = false;
  }
  if (state === 15) return orbitExit(patterns);
  if (state >= 13) defaultDelay(patterns);
}

// Pattern 2's sidestep for a slow walker or hover: a hull that
// can strafe, about once in 13 s (the per-second chance helper scales the
// draw by the AI cycles per second), steps sideways by between one and two
// of its bounding radii, either way.
export function sidestep(patterns, perSecond) {
  const unit = patterns.controller.unit;
  const c = unit.classDef ?? {};
  if (!(forwardSpeed(unit) < 13)) return false;
  if (!(f(c.StrafeSpeed ?? 0) > 0.0001)) return false;
  const random = patterns.random;
  if (random.int(1, 13 * perSecond) !== 1) return false;
  const side = random.int(0, 1) * 2 - 1;
  const r = unit.sphere?.radius ?? 1;
  const distance = r === 2 * r ? r : f(r + random.uniform() * (2 * r - r));
  const yaw = unit.bodyYaw ?? unit.soldier?.aimYaw ?? 0;
  // The transform's right row (the 2004 game's, which is the port's left).
  _d.set(Math.cos(yaw), 0, -Math.sin(yaw));
  _p.copy(unit.position).addScaledVector(_d, side * distance);
  patterns.agent.helper?.goto?.(_p, { mode: "direct" });
  return true;
}

function bodyForward(unit, out) {
  const yaw = unit.bodyYaw ?? unit.soldier?.aimYaw ?? 0;
  return out.set(Math.sin(yaw), 0, Math.cos(yaw));
}

function normalizeXZ(v) {
  v.y = 0;
  const l = Math.hypot(v.x, v.z);
  if (l > 1e-12) v.multiplyScalar(1 / l);
  return v;
}

function horizontalDistanceSq(a, b) {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz;
}

export const _test = { hoverPassPoint, attackRunPath, stateOf, run };
export { UP };
