import { pilotInput } from '../../../flight/ai.js';
import * as THREE from "three";
import { switchWeapon } from "../weapons-ai.js";
import { orbitResult, orbitSteer } from "./orbit.js";

// The AI input step for a bot in a seat: the controller's movement request
// and aim become the vehicle's analogs, as the 2004 game's per-family AI
// input consumers do (docs/ai-vehicles.md, "Vehicle input conversion").
//
//   Hover (classifier 6): anticipatory ray avoidance on a copy of
//     the request, the shared translation with the hover's
//     ForwardSpeed/StrafeSpeed, the hover nonlinear yaw and pitch, the
//     moving-turn floor for MovingTurnOnly classes, and the
//     reverse-and-turn stuck recovery.
//   Aircraft use the shared virtual controller in flight/ai.js.
//   Walker (2): travel held for 7.5 s after a yielding contact,
//     forward and body yaw only (no strafe), head yaw/pitch from the aim.
//   Stationary controls (7: gunner seats, turrets): yaw and pitch toward the
//     aim at the control's rates, nothing else.
//
// Outputs are the ordinary analogs into the seat's intent: forward, right
// (strafe), turn (positive turns right, lowering yaw) and pitch (positive
// raises), plus the vehicle fields brake/up/down. The port's vehicle
// aircraft physics consumes virtual controls through flight/model.js.

const f = Math.fround;
const UP = new THREE.Vector3(0, 1, 0);
const _d = new THREE.Vector3();
const _p = new THREE.Vector3();
const _q = new THREE.Vector3();
const _aim = new THREE.Vector3();
const _fwd = new THREE.Vector3();
const _right = new THREE.Vector3();
const _origin = new THREE.Vector3();
const _hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
const COS5 = Math.cos((5 * Math.PI) / 180);
const WALKER_HOLD = 7.5; // s
// How long a walker presses on before the executor's stuck status is its
// own (port): the executor's 3 s stationary window (EXEC.stationary).
const WALKER_PRESS = 3; // s
// Recoveries ending within this of each other count as one stuck place
// (port).
const WALKER_STUCK_RADIUS = 10; // m
const REQUEST_LIFE = 0.5; // s a renewed request outlives its last renewal (provisional)

const sign = (v) => (v < 0 ? -1 : 1); // sign(0) = +1, as the 2004 game's

// Per-control steering state (the Controllable's fields the consumers keep).
export function steerState(unit) {
  return (unit.steer ||= {
    // Requests written by agents and patterns.
    // Requests a pattern renews each time it ticks; they lapse half a
    // second after the last renewal (the 2004 game clears them at the next
    // controller service).
    movingTurnAt: -Infinity,
    loop: false,
    loopAt: -Infinity,
    minSpeed: 0,
    minSpeedAt: -Infinity,
    orbit: null,
    land: false,
    pathOverride: false,
    pathAltitude: false,
    throttle: 1, // movement-request scale
    // Hover recovery.
    recovery: 0,
    recoverySince: 0,
    lastRecovery: -Infinity,
    savedForward: new THREE.Vector3(),
    // Walker hold.
    holdSince: -100,
    holding: false,
    // A walker turning before it walks: the hull's heading, and when it
    // last moved off it (port: the stuck recovery's turning case).
    turnYaw: NaN,
    turnAt: 0,
    // When a walker began pressing on without a break (NaN: it is not).
    walkSince: NaN,
    // Where its last recovery ended, and how many in a row ended there.
    stuckAt: null,
    stuckCount: 0,
  });
}

// The near-target radius: 1 for infantry, half the hover's
// ForwardSpeed, 4 for everything else.
export function arrivalRadius(unit) {
  const k = unit.kind;
  if (k === 0 || k === 1) return 1;
  if (k === 6) return 0.5 * (unit.classDef?.ForwardSpeed ?? 0);
  return 4;
}

// The hull's horizontal forward (fallback (1, 0, 0) when degenerate).
function bodyForward(unit, out) {
  const yaw = unit.bodyYaw ?? unit.soldier?.aimYaw ?? 0;
  out.set(Math.sin(yaw), 0, Math.cos(yaw));
  if (out.lengthSq() < 0.0001) out.set(1, 0, 0);
  return out.normalize();
}

// The shared translation: the movement request's steering target
// to forward and strafe analogs. maxForward/maxStrafe: the class
// coefficients. hasPath: an executor drives the request (no slowdown on
// intermediate points). Returns { forward, right, near }.
export function translate(unit, request, maxForward, maxStrafe, hasPath, out) {
  out.forward = 0;
  out.right = 0;
  out.near = false;
  if (!request?.valid) return out;
  const P = unit.position;
  _d.set(request.steeringTarget.x - P.x, 0, request.steeringTarget.z - P.z);
  // On the point itself (a stop is written where the unit stands): arrived,
  // so the facing choice takes the aim. (Port: near stayed false here, and a
  // hover or walker told to stand and shoot faced the point under it, not
  // its target, and never fired a fixed gun.)
  if (Math.abs(_d.x) < 0.0001 && Math.abs(_d.z) < 0.0001) {
    out.near = true;
    return out;
  }
  const r = arrivalRadius(unit);
  const dist = _d.length();
  const near = dist * dist < r * r;
  out.near = near;
  if (near && !hasPath && unit.kind === 2) return out; // a walker stops
  const s = near && !hasPath ? Math.min(Math.max(dist / r, 0), 1) : 1;
  _d.divideScalar(dist);
  bodyForward(unit, _fwd);
  const a = _fwd.x * _d.x + _fwd.z * _d.z;
  const b = -(_fwd.z * _d.x - _fwd.x * _d.z);
  if (maxStrafe > 0.0001) {
    const u = a / maxForward;
    const v = b / maxStrafe;
    const m = Math.max(Math.abs(u), Math.abs(v)) || 1;
    out.forward = (u / m) * s;
    out.right = (v / m) * s;
  } else {
    out.forward = Math.min(Math.max(a, 0), 1) * s;
    out.right = 0;
  }
  // A far walker turns before it walks.
  if (unit.kind === 2 && !near && a < COS5) out.forward = 0;
  return out;
}

// The facing point: the aim when the class can
// strafe, there is no movement target, or it has arrived (and for a hover
// asked to face its aim); otherwise the movement point itself.
function facingPoint(unit, request, aim, near, maxStrafe, out) {
  const useAim = maxStrafe > 0.0001 || !request?.valid || near;
  if (useAim) {
    if (aim) return out.copy(aim);
    // Mode 2 without a valid farther movement point: 30 m ahead.
    if (request?.valid && !near) {
      _q.set(request.steeringTarget.x - unit.position.x, 0, request.steeringTarget.z - unit.position.z);
      if (_q.lengthSq() > 1e-8) return out.copy(unit.position).addScaledVector(_q.normalize(), 30);
    }
    bodyForward(unit, _fwd);
    return out.copy(unit.position).addScaledVector(_fwd, 30);
  }
  return out.copy(request.steeringTarget);
}

// The hover's yaw analog toward a point, nonlinear:
// q is the point's sideways share in the hull's frame (its sign alone when
// the point is behind), g = 4.5 - clamp(TurnRate, 1, 3), t = clamp(g q,
// -1, 1), yaw = sign(t) t^2. Ground steering normalizes horizontally.
export function nonlinearYaw(unit, point) {
  _q.subVectors(point, unit.position);
  _q.y = 0;
  bodyForward(unit, _fwd);
  _right.set(-_fwd.z, 0, _fwd.x); // the port's right of the hull
  const length = Math.hypot(_q.x, _q.z);
  if (length < 1e-9) return 0;
  _q.divideScalar(length);
  let q = _q.x * _right.x + _q.z * _right.z;
  const ahead = _q.x * _fwd.x + _q.z * _fwd.z;
  if (ahead < 0) q = sign(q);
  const g = 4.5 - Math.min(Math.max(unit.classDef?.TurnRate ?? 2, 1), 3);
  const t = Math.min(Math.max(g * q, -1), 1);
  return sign(t) * t * t;
}

// ...and its pitch: the point's normalized height share in the aim frame,
// g = 5 - clamp(PitchRate, 1, 3).
export function nonlinearPitch(unit, point) {
  unit.origin(_origin);
  _q.subVectors(point, _origin);
  const length = _q.length();
  if (length < 1e-9) return 0;
  unit.forward(_fwd);
  // The aim frame: forward, its right, and its up.
  _right.crossVectors(_fwd, UP);
  if (_right.lengthSq() < 1e-9) return 0;
  _right.normalize();
  _p.crossVectors(_right, _fwd).normalize();
  const q = (_q.x * _p.x + _q.y * _p.y + _q.z * _p.z) / length;
  const g = 5 - Math.min(Math.max(unit.classDef?.PitchRate ?? 1, 1), 3);
  const t = Math.min(Math.max(g * q, -1), 1);
  return sign(t) * t * t;
}

// The ordinary yaw/pitch branch (for other actors): the signed
// sideways and vertical errors of the aim divided by dt x rate, clamped to
// +-1 (+-2 with the fast-turn request); a point behind the aim frame gives
// pitch 0.
export function linearAim(unit, point, dt, yawRate, pitchRate, fast, out) {
  unit.origin(_origin);
  _q.subVectors(point, _origin);
  const flat = Math.hypot(_q.x, _q.z);
  unit.forward(_fwd);
  const fx = _fwd.x, fz = _fwd.z;
  const fl = Math.hypot(fx, fz) || 1;
  if (flat > 1e-6) {
    const err = -((fz / fl) * (_q.x / flat) - (fx / fl) * (_q.z / flat));
    const limit = fast ? 2 : 1;
    const rate = yawRate * dt;
    out.turn = Math.max(-limit, Math.min(limit, rate > 0 ? err / rate : 0));
  } else out.turn = 0;
  const ahead = _q.x * _fwd.x + _q.y * _fwd.y + _q.z * _fwd.z;
  if (ahead <= 0) {
    out.pitch = 0;
    return out;
  }
  const want = Math.atan2(_q.y, flat);
  const have = Math.asin(Math.max(-1, Math.min(1, _fwd.y)));
  const rate = pitchRate * dt;
  out.pitch = Math.max(-1, Math.min(1, rate > 0 ? Math.sin(want - have) / rate : 0));
  return out;
}

// -- the families -----------------------------------------------------------------------------

const move = { forward: 0, right: 0, near: false };
const angles = { turn: 0, pitch: 0 };

// The facing choice's "already arrived": inside the near radius with no
// path left (the slowdown's own condition). A waypoint on the way that is
// near but behind must still be faced: a walker, which only walks where it
// faces, would otherwise stand within its 4 m radius of a waypoint it has
// to come within 1 m of, for good.
const arrived = (move, env) => move.near && !env.hasPath;

// unit: the SeatUnit; request: the controller's MovementRequest; aim: the
// aim point (target with lead, a stored point) or null; env: { world, now,
// hasPath, collision, nav } ; out: { forward, right, turn, pitch } plus
// { brake, up, down }.
export function hoverInput(unit, request, aim, dt, env, out) {
  const c = unit.classDef;
  const st = steerState(unit);
  // Anticipatory avoidance on a copy of the request (AISizeType Huge is
  // exempt): a ray ahead of 1.5 x speed when that exceeds 5 m.
  let factor = 1;
  let req = request;
  if (request?.valid && String(c.AISizeType ?? "").toLowerCase() !== "huge") {
    const v = unit.velocity;
    const D = 1.5 * Math.hypot(v.x, v.y, v.z);
    if (D > 5) {
      _d.set(request.steeringTarget.x - unit.position.x, 0.0001, request.steeringTarget.z - unit.position.z).normalize();
      _right.set(-_d.z, 0, _d.x);
      _p.copy(unit.position).add(UP);
      const hit = env.collision?.cast(_p, _d, D, _hit, unit.vehicle?.body ?? null);
      if (hit) {
        avoidRequest.copyFrom(request);
        const side = hit.normal.x * _right.x + hit.normal.z * _right.z > 0 ? 1 : -1;
        avoidRequest.steeringTarget.addScaledVector(_right, side * 0.25 * D);
        req = avoidRequest;
        factor = 0.5;
      }
    }
  }
  translate(unit, req, c.ForwardSpeed || 1, c.StrafeSpeed ?? 0, env.hasPath, move);
  facingPoint(unit, req, aim, arrived(move, env), c.StrafeSpeed ?? 0, _aim);
  out.forward = move.forward;
  out.right = move.right;
  out.turn = nonlinearYaw(unit, _aim);
  out.pitch = aim ? nonlinearPitch(unit, aim) : 0;
  // The moving-turn request with MovingTurnOnly: turning keeps at least
  // half throttle, sign kept.
  if (c.MovingTurnOnly && env.now <= st.movingTurnAt + REQUEST_LIFE && Math.abs(out.turn) > 0.0001) {
    out.forward = out.forward < 0 ? Math.min(-0.5, out.forward) : Math.max(0.5, out.forward);
  }
  out.forward *= factor;
  hoverRecovery(unit, env, st, out);
  return out;
}

const avoidRequest = {
  valid: false, steeringTarget: new THREE.Vector3(), waypoint: new THREE.Vector3(), segmentDirection: new THREE.Vector3(), mode: 0, lower: -1, upper: 1,
  copyFrom(r) {
    this.valid = r.valid;
    this.steeringTarget.copy(r.steeringTarget);
    this.waypoint.copy(r.waypoint);
    this.segmentDirection.copy(r.segmentDirection);
    this.mode = r.mode;
    this.lower = r.lower;
    this.upper = r.upper;
    return this;
  },
};

// The hover's stuck recovery: with the executor reporting no progress and
// four seconds since the last one, back up turning (forward -1, yaw +0.5)
// until the hull has turned past perpendicular to where it faced, then drive
// on turning (forward +1, yaw +0.5) for three more seconds. Strafe and pitch
// are left as they were.
function hoverRecovery(unit, env, st, out) {
  const now = env.now;
  if (st.recovery === 0) {
    if (env.status !== 0 && now > st.lastRecovery + 4) {
      st.recovery = 1;
      bodyForward(unit, st.savedForward);
    } else return;
  }
  if (st.recovery === 1) {
    bodyForward(unit, _fwd);
    if (st.savedForward.dot(_fwd) < 0) {
      st.recovery = 2;
      st.recoverySince = now;
    } else {
      out.forward = -1;
      out.turn = 0.5;
      return;
    }
  }
  out.forward = 1;
  out.turn = 0.5;
  if (now > st.recoverySince + 3) {
    st.recovery = 0;
    st.lastRecovery = now;
  }
}

// The walker adapter: while a yielding contact holds it (7.5 s
// since the last), travel stops but its guns keep working; forward and body
// yaw from the request (zero strafe, MaxTurnSpeed), head yaw and pitch from
// the aim (MaxYawSpeed/MaxPitchSpeed). The port's walker has one aim per
// seat, which its hull follows: the body yaw becomes the turn analog and
// the head pitch the pitch analog (provisional, docs/ai-vehicles.md).
export function walkerInput(unit, request, aim, dt, env, out) {
  const c = unit.classDef;
  const st = steerState(unit);
  st.holding = env.now <= st.holdSince + WALKER_HOLD;
  const req = st.holding ? null : request;
  translate(unit, req, c.MaxSpeed || 1, 0, env.hasPath, move);
  out.forward = move.forward;
  out.right = 0;
  if (req?.valid && !arrived(move, env)) {
    facingPoint(unit, req, null, false, 0, _aim);
    linearAim(unit, _aim, dt, c.MaxTurnSpeed ?? 2, c.MaxPitchSpeed ?? 0.5, false, angles);
    out.turn = angles.turn;
  } else if (aim) {
    linearAim(unit, aim, dt, c.MaxYawSpeed ?? 1, c.MaxPitchSpeed ?? 0.5, false, angles);
    out.turn = angles.turn;
  } else out.turn = 0;
  if (aim) {
    linearAim(unit, aim, dt, c.MaxYawSpeed ?? 1, c.MaxPitchSpeed ?? 0.5, false, angles);
    out.pitch = angles.pitch;
  } else out.pitch = 0;
  // Walking on and going nowhere (the executor stationary while it has
  // asked to walk for 3 s, not held), or turning before it walks with a hull that has not
  // turned in 2 s (wedged where any turn swings it into a wall): back up
  // turning and walk on turning, as a hover does. (Port: Classic gives a
  // walker no stuck recovery; a tauntaun pressed against a bank it could not
  // climb, or wedged by a wall facing away from its path, stood there for
  // good.)
  const yaw = unit.bodyYaw ?? 0;
  const turning = Boolean(req?.valid) && !move.near && move.forward === 0;
  if (!turning || !(Math.abs(yaw - st.turnYaw) < 0.01)) {
    st.turnYaw = yaw;
    st.turnAt = env.now;
  }
  const wedged = env.now > st.turnAt + 2;
  // The executor's stuck status counts the time a walker spends turning on
  // the spot before it walks too (its anchor does not move), so a slow
  // turner (an AT-TE, a spider walker: well over 3 s for a half turn) set
  // off with status 2 already up. Taken as stuck, it backed up turning,
  // turned back for longer than the 4 s between recoveries, and backed up
  // again: never walking where it faced, it drifted the other way, off
  // Geonosis. Only status earned while pressing on counts.
  if (move.forward > 0 && !st.holding) {
    if (!(st.walkSince <= env.now)) st.walkSince = env.now;
  } else st.walkSince = NaN;
  const pressing = move.forward > 0 && env.now >= st.walkSince + WALKER_PRESS;
  const recovering = st.recovery !== 0;
  if (!st.holding && (recovering || pressing || wedged)) hoverRecovery(unit, env, st, out);
  // Recoveries in a row that leave it where it was (within 10 m): the way
  // it is asked to walk to is out of its hull's reach (on Kashyyyk a spider
  // walker sent to the docks post walked into the quay wall), and backing
  // out to walk
  // into the same spot again held one there for six minutes. After two its
  // movement helper plans the trip again, which draws new waypoints on the
  // graph's hubs; after four it asks for another post (env.replan; port).
  if (recovering && st.recovery === 0) {
    const p = unit.position;
    if (st.stuckAt && Math.hypot(p.x - st.stuckAt.x, p.z - st.stuckAt.z) < WALKER_STUCK_RADIUS) st.stuckCount += 1;
    else {
      st.stuckAt = { x: p.x, z: p.z };
      st.stuckCount = 1;
    }
    if (st.stuckCount === 2) env.replan?.(false);
    else if (st.stuckCount >= 4) {
      st.stuckAt = null;
      st.stuckCount = 0;
      env.replan?.(true);
    }
  }
  return out;
}

// The walker yield predicate a contact consults: other must be a
// walker or hover; V is each one's bounding-box volume (+0.0001); the much
// larger one (more than 3x) has right of way, the much smaller yields;
// otherwise both must be moving, and the one approaching the other more
// squarely yields: dot(-d, a) < dot(d, b), d from self to other.
export function walkerYields(self, other) {
  if (!other || (other.kind !== 2 && other.kind !== 6)) return false;
  const va = volume(self);
  const vb = volume(other);
  if (va > 3 * vb) return false;
  if (vb > 3 * va) return true;
  const a = self.velocity;
  const b = other.velocity;
  if (!(a.x * a.x + a.z * a.z > 0.0001) || !(b.x * b.x + b.z * b.z > 0.0001)) return false;
  _p.copy(a).normalize();
  _q.copy(b).normalize();
  _d.subVectors(other.position, self.position).normalize();
  return -_d.dot(_p) < _d.dot(_q);
}

function volume(unit) {
  const box = unit.classDef?.bounds;
  if (!box) return 1.0001;
  return (box[1][0] - box[0][0]) * (box[1][1] - box[0][1]) * (box[1][2] - box[0][2]) + 0.0001;
}

// A yielding contact: hold the walker's travel from now.
export function walkerContact(unit, other, now) {
  if (unit.kind === 2 && walkerYields(unit, other)) steerState(unit).holdSince = now;
}

// Stationary controls (gunner seats, turrets): yaw and pitch toward the aim
// at the control's rates; no travel.
export function stationaryInput(unit, request, aim, dt, env, out) {
  out.forward = 0;
  out.right = 0;
  if (!aim) {
    out.turn = 0;
    out.pitch = 0;
    return out;
  }
  const c = unit.classDef;
  linearAim(unit, aim, dt, c.TurnRate ?? 2, c.PitchRate ?? 1, false, angles);
  out.turn = angles.turn;
  out.pitch = angles.pitch;
  return out;
}

// Pattern 15's orbit (orbit.js) while the pattern holds it and its walker
// lives: the movement request of mode 3 the shared movement update writes
// in place of the helper's, with its height share and movement
// scale; and on the run's tow pass every seat of the hull a bot holds that
// has a tow cable selects it, as the 2004 game selects it for each control
// of the hull (port: a person's seat keeps the weapon he chose). Null when
// no orbit steers.
const orbitOut = orbitResult();
const orbitRequest = {
  valid: true, steeringTarget: new THREE.Vector3(), waypoint: new THREE.Vector3(), segmentDirection: new THREE.Vector3(), mode: 3, lower: -1, upper: 1,
  blend: 0, throttle: 0,
};

export function orbitInput(unit) {
  const orbit = unit.steer?.orbit;
  const ref = orbit?.target;
  const hull = ref?.unit?.vehicle;
  const v = unit.vehicle;
  if (!v || !hull?.alive || ref.unit.generation !== ref.generation) return null;
  _fwd.set(0, 0, 1).applyQuaternion(v.quaternion);
  orbitSteer(v.position, _fwd, hull, orbit, orbitOut);
  if (orbitOut.towPass) {
    for (const seat of v.seats) {
      const lease = seat.lease;
      const control = lease && !lease.human && !lease.unit?.networkHuman ? lease.controllable : null;
      const controller = control?.controller;
      const weapons = control?.combat?.arsenal?.weapons;
      const tow = weapons ? weapons.findIndex((w) => w.def?.base === "towcableweapon") : -1;
      if (tow >= 0 && controller && controller.perception?.fire?.channel !== weapons[tow].channel) switchWeapon(controller, control, tow);
    }
  }
  const r = orbitRequest;
  r.steeringTarget.copy(orbitOut.point);
  r.waypoint.copy(orbitOut.point);
  r.segmentDirection.subVectors(orbitOut.point, v.position);
  r.segmentDirection.x += 0.0001;
  r.segmentDirection.normalize();
  r.blend = orbitOut.blend;
  r.throttle = orbitOut.throttle;
  return r;
}

// -- the step -----------------------------------------------------------------------------------

// The AI input conversion for a seated control, in place of the soldier
// analogs: returns out { forward, right, turn, pitch } and writes the
// vehicle fields to unit.intent.
export function seatAnalogs(unit, request, aim, dt, out, env) {
  out.forward = out.right = out.turn = out.pitch = 0;
  const full = { forward: 0, right: 0, turn: 0, pitch: 0, up: 0, down: 0, brake: false };
  switch (unit.kind) {
    case 6:
      hoverInput(unit, request, aim, dt, env, full);
      break;
    case 5:
      pilotInput(unit, orbitInput(unit) ?? request, aim, dt, env, full);
      break;
    case 2:
      walkerInput(unit, request, aim, dt, env, full);
      break;
    default:
      stationaryInput(unit, request, aim, dt, env, full);
  }
  out.forward = full.forward;
  out.right = full.right;
  out.turn = full.turn;
  out.pitch = full.pitch;
  const intent = unit.intent;
  if (intent) {
    intent.up = full.up ? 1 : 0;
    intent.down = full.down ? 1 : 0;
    intent.brake = Boolean(full.brake);
    intent.trick = Boolean(full.trick); intent.immelmann = Boolean(full.immelmann); intent.boost = Boolean(full.boost);
  }
  return out;
}
