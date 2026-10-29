import * as THREE from "three";
import { NEAR_WAYPOINT, TINY } from "./math.js";
import { nearRadius } from "./executor.js";
import { KIND } from "./units.js";

// The movement request and aim point turned into the unit's ordinary four
// analogs, as the 2004 game's AI input does: movement from the request,
// the aim state's point, and the Soldier branch of its turning and pitch.
// (docs/ai-nav.md, AI input step.)
//
// Movement: d is the horizontal unit direction to the steering target, F the
// unit's horizontal facing. With a = F·d and b = −(F × d).y, a unit that can
// strafe divides by the class coefficients, u = a / MaxSpeed and
// v = b / MaxStrafeSpeed, and scales both so the larger is 1: a square stick
// envelope, so the resulting velocity points at the target. A unit without
// strafe walks forward by clamp(a, 0, 1) and turns to face the target. Inside
// the near radius (1 m for infantry) with no path left, the stick scales
// down by distance / radius; ordinary intermediate waypoints do not slow.
//
// Turning: the yaw error is the sine of the angle from the facing to the
// aim point (−(F × n).y, n the horizontal unit direction), divided by
// dt × MaxTurnSpeed so it would close in one tick, clamped to [−1, 1] (±2
// with the transient facing request). Pitch is the same in the aim frame's
// vertical plane over dt × MaxPitchSpeed, and 0 for a point behind the aim.
//
// Port conventions (docs/ai.md): forward = (sin yaw, 0, cos yaw); the
// 2004 game's strafe and yaw analogs carry over with their signs, positive
// strafe moving along (−cos yaw, 0, sin yaw) (the port's right) and positive
// turn decreasing aimYaw; positive pitch raises the aim. Class coefficients:
// forward MaxSpeed, strafe MaxStrafeSpeed, yaw MaxTurnSpeed, pitch
// MaxPitchSpeed.

export function coefficients(unit, out = {}) {
  const def = unit.classDef ?? {};
  const s = unit.soldier ?? {};
  out.forward = def.MaxSpeed ?? s.maxSpeed ?? 9;
  out.strafe = def.MaxStrafeSpeed ?? s.maxStrafe ?? 7;
  out.yaw = def.MaxTurnSpeed ?? s.maxTurn ?? 2;
  out.pitch = def.MaxPitchSpeed ?? s.maxPitch ?? 0.5;
  return out;
}

const coef = {};
const aimOut = { x: 0, y: 0, z: 0 };

// request: MovementRequest; aim: the world point to aim at for aim states 0
// (target plus lead, from Combat) and 1 (stored point), or null for the
// third state (along the path); dt: the input step; out receives
// { forward, right, turn, pitch }. options: { facing (±2 turn clamp),
// executor (the unit's PathExecutor or null) }.
export function analogs(unit, request, aim, dt, out, options = {}) {
  coefficients(unit, coef);
  const p = unit.position;
  const yaw = unit.soldier?.aimYaw ?? 0;
  const fx = Math.sin(yaw);
  const fz = Math.cos(yaw);
  const canStrafe = coef.strafe > TINY;
  let invalid = !request?.valid;
  let forward = 0;
  let right = 0;
  let slowNoPath = false;
  if (!invalid) {
    const t = request.steeringTarget;
    const dx = t.x - p.x;
    const dz = t.z - p.z;
    const zero = Math.abs(dx) < TINY && Math.abs(dz) < TINY;
    const r = nearRadius(unit);
    const dist2 = dx * dx + dz * dz;
    const inside = dist2 < r * r;
    const executor = options.executor ?? null;
    slowNoPath = inside && !executor;
    const slowHeld = inside && Boolean(executor?.hold);
    let stop = zero;
    if (!zero) {
      if (slowNoPath && unit.kind === KIND.walker) stop = true;
      // A Droideka stops within 0.2 m of where it steers. A lookahead point
      // is never nearer than 0.2 m, so this can only mean the waypoint itself;
      // at rest the lookahead lies exactly 0.2 m on, where float rounding
      // alone would decide, and a stop there never lets it start.
      else if (dist2 < NEAR_WAYPOINT && unit.kind === KIND.droideka && sameXZ(t, request.waypoint)) stop = true;
    }
    if (stop) {
      invalid = true;
    } else {
      const l = Math.sqrt(dist2);
      const ux = dx / l;
      const uz = dz / l;
      const a = fx * ux + fz * uz;
      const b = -(fz * ux - fx * uz);
      const slow = slowNoPath || slowHeld ? Math.min(Math.max(l / r, 0), 1) : 1;
      if (canStrafe) {
        const u = a / coef.forward;
        const v = b / coef.strafe;
        if (Math.abs(u) <= Math.abs(v)) {
          forward = (u / Math.abs(v)) * slow;
          right = (v <= 0 ? -1 : 1) * slow;
        } else {
          right = (v / Math.abs(u)) * slow;
          forward = (u <= 0 ? -1 : 1) * slow;
        }
      } else {
        forward = Math.min(Math.max(a, 0), 1) * slow;
      }
    }
  }
  out.forward = forward;
  out.right = right;
  // A unit that cannot strafe faces where it walks; others aim by state.
  let point;
  if (!canStrafe && !invalid) point = request.steeringTarget;
  else point = aim ?? alongPath(unit, request, aimOut);
  turnToward(unit, point, dt, coef, options.facing, out);
  return out;
}

// The third aim state: 30 m along the way to the steering target,
// or straight ahead inside the near radius or without a request.
export function alongPath(unit, request, out) {
  const p = unit.position;
  const yaw = unit.soldier?.aimYaw ?? 0;
  if (request?.valid) {
    const t = request.steeringTarget;
    const dx = t.x - p.x, dy = t.y - p.y, dz = t.z - p.z;
    const r = nearRadius(unit);
    if (dx * dx + dz * dz > r * r) {
      const l = Math.sqrt(dx * dx + dy * dy + dz * dz);
      out.x = p.x + 30 * dx / l;
      out.y = p.y + 30 * dy / l;
      out.z = p.z + 30 * dz / l;
      return out;
    }
  }
  out.x = p.x + 30 * Math.sin(yaw);
  out.y = p.y;
  out.z = p.z + 30 * Math.cos(yaw);
  return out;
}

// The Soldier branch of the aim conversion: yaw from the body's facing toward the
// point's horizontal direction, pitch in the aim frame.
function turnToward(unit, point, dt, coef, facing, out) {
  const s = unit.soldier ?? {};
  const yaw = s.aimYaw ?? 0;
  const pitch = s.aimPitch ?? 0;
  const p = unit.position;
  const fx = Math.sin(yaw);
  const fz = Math.cos(yaw);
  let nx = point.x - p.x;
  let nz = point.z - p.z;
  const hl = Math.sqrt(nx * nx + nz * nz);
  let turn = 0;
  if (hl > 0) {
    nx /= hl;
    nz /= hl;
    turn = -(fz * nx - fx * nz);
  }
  const yawStep = dt * coef.yaw;
  if (yawStep > 0) turn /= yawStep;
  const limit = facing ? 2 : 1;
  out.turn = Math.min(Math.max(turn, -limit), limit);

  // The aim frame: forward along the aim, right = normalize(up × forward).
  const cp = Math.cos(pitch);
  const ax = fx * cp, ay = Math.sin(pitch), az = fz * cp;
  const rx = fz, rz = -fx;
  const origin = unit.origin ? unit.origin(originScratch) : originScratch.set(p.x, p.y + 1.7, p.z);
  let vx = point.x - origin.x;
  let vy = point.y - origin.y;
  let vz = point.z - origin.z;
  const across = vx * rx + vz * rz;
  vx -= across * rx;
  vz -= across * rz;
  const ahead = vx * ax + vy * ay + vz * az;
  if (ahead <= 0) {
    out.pitch = 0;
    return;
  }
  const l = Math.sqrt(vx * vx + vy * vy + vz * vz);
  vx /= l;
  vy /= l;
  vz /= l;
  // −right · (forward × v)
  const cx = ay * vz - az * vy;
  const cz = ax * vy - ay * vx;
  let up = -(rx * cx + rz * cz);
  const pitchStep = dt * coef.pitch;
  if (pitchStep > 0) up /= pitchStep;
  out.pitch = Math.min(Math.max(up, -1), 1);
}

const originScratch = new THREE.Vector3();

const sameXZ = (a, b) => a.x === b.x && a.z === b.z;

// The navigation half of the AI input step for one unit (its path, jet and
// posture branches; weapons and pending actions are not here). Runs
// the path executor, fills intent.forward/right/turn/pitch from the request
// and aim point, then applies a pending jet request or the requested
// posture as the port's input edges:
// - jet: a request (or the sequence already under
//   way) jumps from the ground first, then presses jump again while the
//   soldier is in the jump state, which lights the jetpack; a nonzero
//   request vector is passed as intent.jetDirection (the launch direction
//   the 2004 game uses literally for AI soldiers). The sequence ends once
//   the soldier is neither jumping nor jetting.
// - posture: 0 stand, 1 crouch, 2 prone, pressed as the port's toggles
//   whenever the soldier is on the ground, unlocked and not yet there.
export function navigationInput(control, unit, intent, dt, aim = null, options = {}) {
  control.attach(unit);
  control.update(dt);
  analogs(unit, control.request, aim, dt, intent, { facing: options.facing, executor: control.executor });
  const s = unit.soldier;
  if (!s) return intent;
  const free = !(s.lock > 0);
  if (control.jet.pending || control.jetting) {
    control.jet.pending = false;
    if (free) {
      if (s.air === "jump") {
        intent.jump = true;
        const v = control.jet.vector;
        intent.jetDirection = v.lengthSq() > 0 ? v : null;
      } else if (!control.jetting) {
        intent.jump = true;
        control.jetting = true;
      } else if (s.air !== "jet") {
        control.jetting = false;
      }
    }
    return intent;
  }
  if (free && !s.air && !s.dive) {
    const want = control.stance === 2 ? "prone" : control.stance === 1 ? "crouch" : "stand";
    const have = s.posture;
    if (want !== have) {
      if (want === "prone") intent.prone = true;
      else if (want === "crouch") intent.crouch = true;
      else if (have === "crouch") intent.crouch = true;
      else intent.prone = true;
    }
  }
  return intent;
}
