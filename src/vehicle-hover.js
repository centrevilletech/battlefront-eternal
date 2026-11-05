// Cold class geometry and replica placement. Native hover owns motion,
// contact gathers and springs; these helpers do not advance a hull.
import * as THREE from 'three';
import { classSetting } from './vehicle-motion.js';
const GRAVITY = 9.8, SPRINGS = 6;
const clamp = THREE.MathUtils.clamp;
const s = { euler: new THREE.Euler(0, 0, 0, 'YXZ') };

const classes = new WeakMap();
export function hoverClass(def) {
  let c = classes.get(def);
  if (c) return c;
  const n = (name, fallback) => { const v = parseFloat(classSetting(def, name)); return Number.isFinite(v) ? v : fallback; };
  c = {
    altitude: n("SetAltitude", 1), gravityScale: n("GravityScale", 1), liftSpring: n("LiftSpring", 5), liftDamp: n("LiftDamp", 2),
    traction: n("Traction", 10), levelSpring: n("LevelSpring", 10), levelDamp: n("LevelDamp", 3),
    velocitySpring: n("VelocitySpring", 0), velocityDamp: n("VelocityDamp", 0),
    omegaXSpring: n("OmegaXSpring", 0), omegaXDamp: n("OmegaXDamp", 0), omegaZSpring: n("OmegaZSpring", 0), omegaZDamp: n("OmegaZDamp", 0),
    // tools/zero_vehicle.py hover_springs: each sphere's centre and radius in
    // the hull's frame, its travel, and its six factors (lift, lift damping,
    // pitch, pitch damping, roll, roll damping).
    springs: (def.springs || []).slice(0, SPRINGS).map((spring) => ({
      at: new THREE.Vector3().fromArray(spring.at), radius: spring.radius, length: spring.length, factors: spring.factors,
    })),
  };
  classes.set(def, c);
  return c;
}

// The hull's own frame as the physics reads it: heading, then pitch and
// roll (Vehicle.present draws the same).
export function hullQuaternion(v, out) {
  return out.setFromEuler(s.euler.set(-v.tilt, v.yaw, v.roll));
}

export function restHeight(def) {
  const h = hoverClass(def);
  if (!h.springs.length) return Math.max(0, h.altitude - GRAVITY * (h.gravityScale - 1) / Math.max(h.liftSpring, 1e-6));
  const lift = (height) => h.springs.reduce((sum, { at, radius, length, factors }) =>
    sum + factors[0] * h.velocitySpring * clamp((radius - at.y - height) / length, 0, 1), 0);
  let low = Math.min(...h.springs.map(({ at, radius, length }) => radius - at.y - length));
  let high = Math.max(...h.springs.map(({ at, radius }) => radius - at.y));
  if (lift(low) < GRAVITY) return low;
  for (let i = 0; i < 40; i++) {
    const mid = (low + high) / 2;
    if (lift(mid) > GRAVITY) low = mid; else high = mid;
  }
  return (low + high) / 2;
}
