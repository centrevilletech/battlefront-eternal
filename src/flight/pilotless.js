import * as THREE from 'three';
import { sweep } from './model.js';

// A craft in flight that nobody flies. With anyone still aboard (a gunner, a
// passenger) it holds its course with the stick let go, through the common
// model (Vehicle.fly). Empty, after its pilot bailed out (src/boarding.js)
// or was taken out of it, it comes down: nothing holds it up, its speed
// carries on and gravity bends its path, the nose following the path, until
// it strikes the ground, a building, the water or a death region, which
// destroys it; its pad then builds a new one. The 2004 data has no Eject
// keys (docs/feature-audit.md) and the original game's pilotless flight is not
// traced: a craft its pilot left in the air crashing is the port's choice.
const GRAVITY = 9.8; // m/s², as shells and grenades fall (ordnance.js)
const NOSE = 3; // 1/s: how fast the nose swings onto the path
const Z = new THREE.Vector3(0, 0, 1);
const next = new THREE.Vector3(), forward = new THREE.Vector3(), path = new THREE.Vector3();
const swing = new THREE.Quaternion(), none = new THREE.Quaternion();

export function stepPilotless(v, dt) {
  v.flightPreviousQ?.copy(v.quaternion);
  v.flightClock = (v.flightClock || 0) + dt; v.stateTime = (v.stateTime || 0) + dt;
  v.stickX = v.stickY = 0; v.boosting = false; v.trick = '';
  v.velocity.y -= GRAVITY * dt;
  next.copy(v.position).addScaledVector(v.velocity, dt);
  const contact = sweep(v, v.position, next);
  if (contact) v.position.addScaledVector(v.velocity, Math.max(0, contact.distance - .2) / Math.max(v.velocity.length(), 1));
  else v.position.copy(next);
  path.copy(v.velocity);
  if (path.lengthSq() > 1e-6) {
    forward.copy(Z).applyQuaternion(v.quaternion);
    swing.setFromUnitVectors(forward, path.normalize());
    v.quaternion.premultiply(none.identity().slerp(swing, 1 - Math.exp(-NOSE * dt))).normalize();
  }
  v.airspeed = v.throttle = v.velocity.length();
  v.grounded = false; v.sleeping = false;
  forward.copy(Z).applyQuaternion(v.quaternion);
  v.yaw = Math.atan2(forward.x, forward.z); v.tilt = Math.asin(THREE.MathUtils.clamp(forward.y, -1, 1));
  if (v.seats?.[0]) { v.seats[0].aimYaw = v.yaw; v.seats[0].pitch = v.tilt; }
  v.syncBody();
  const system = v.system, water = system.water;
  const wet = water?.over?.(v.position.x, v.position.z) && v.position.y < water.level;
  if (contact || wet || system.inDeathRegion?.(v.position) || v.position.y < -300) v.damage(Infinity);
}
