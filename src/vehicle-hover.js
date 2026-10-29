import * as THREE from "three";
import { classSetting } from "./vehicle-motion.js";

// A hover hull on the ground, moved as the 2004 game moves one
// (docs/vehicles.md "Hovers on the ground"): every stock hover class, the
// fighter tanks, AAT, combat speeder, landspeeder, Gian speeder, STAP,
// speeder bikes, skiff and the wheeled Hailfire, which is a hover class too.
// EntityHover's physics runs before the world's collision pass, the springs'
// contacts during it and the springs' forces after it; Vehicle.tick runs
// the three in that order every tick. The hull is a body with a velocity, a
// turn rate about its own axes and a ground factor f: gravity, leveling and
// the drive act on it, and its springs (AddSpringBody spheres) carry it over
// the terrain and buildings, pitching and rolling it with the ground under
// them.
// Its heading is the port's steering (Vehicle.steer); pitch and roll are
// tilt (nose up positive) and roll (lifting the +x side), with their rates
// tiltRate and rollRate, and f is support. All are plain fields, so a
// client's replay and correction carry them (src/runtime/
// network-entity-ground.js).

const GRAVITY = 9.8;
const UP = Object.freeze(new THREE.Vector3(0, 1, 0));
const DOWN = Object.freeze(new THREE.Vector3(0, -1, 0));
// A spring meets ground whose normal is within 60 degrees of both world up
// and the hull's up; anything else it meets only once it is
// pressed in past its travel, as a wall.
const SPRING_GROUND = .5;
// Beyond its travel a spring's contact also loses this share of the speed
// into it across the ground.
const SPRING_SKID = .1;
// The look-ahead of a hull without springs, over this ground speed (m/s).
const LOOKAHEAD = 5;
const LEVEL_LIFT = .1; // the y added to its target up while it hovers
const SPRINGS = 6; // the parser admits six AddSpringBody spheres at most
// A hull at rest on its springs keeps a tick's gravity in its velocity
// (gravity acts before the move, the springs after it): asleep below this
// speed (m/s) when it stays put (Vehicle.tick).
export const HOVER_REST = .15;

const lerp = THREE.MathUtils.lerp;
const clamp = THREE.MathUtils.clamp;
const s = {
  q: new THREE.Quaternion(), euler: new THREE.Euler(0, 0, 0, "YXZ"),
  right: new THREE.Vector3(), up: new THREE.Vector3(), forward: new THREE.Vector3(),
  groundForward: new THREE.Vector3(), groundRight: new THREE.Vector3(), target: new THREE.Vector3(),
  center: new THREE.Vector3(), normal: new THREE.Vector3(), nearest: new THREE.Vector3(), ray: new THREE.Vector3(),
  hit: { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null },
  ground: { height: 0, normal: new THREE.Vector3() },
  used: new Set(), compression: new Float64Array(SPRINGS),
};

// The class's hover keys, as HoverClass's parser reads them over the
// constructor's defaults. The constructor leaves the six spring rates
// unset; no stock class omits them. The drive's speeds and rates are the
// hull's own (src/vehicle-motion.js groundMotion).
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

function axes(v) {
  hullQuaternion(v, s.q);
  s.right.set(1, 0, 0).applyQuaternion(s.q);
  s.up.set(0, 1, 0).applyQuaternion(s.q);
  s.forward.set(0, 0, 1).applyQuaternion(s.q);
}

// The drive: the speed along the forward axis moves toward
// ForwardSpeed (ReverseSpeed backing up) times the stick at
// lerp(Deceleration, Acceleration, throttle), and across it toward
// StrafeSpeed times the strafe stick at Traction, each per second, times
// the share of the hull's up that points up and its ground factor: a hull
// on its side, or off the ground, has no grip. The port's brake stops it
// at twice its Deceleration.
function drive(v, h, forward, right, upY, fwd, side, brake, dt) {
  const throttle = Math.max(Math.abs(fwd), Math.abs(side));
  const grip = Math.max(upY, 0) * v.support * dt;
  const rate = lerp(v.deceleration, v.acceleration, throttle) * (brake ? 2 : 1) * grip;
  const traction = h.traction * grip;
  const along = (fwd >= 0 ? v.speed : v.reverseSpeed) * fwd - v.velocity.dot(forward);
  v.velocity.addScaledVector(forward, clamp(along, -rate, rate));
  // +x is the hull's left: strafing right moves it along -x.
  const across = -v.strafeSpeed * side - v.velocity.dot(right);
  v.velocity.addScaledVector(right, clamp(across, -traction, traction));
}

// A hull without springs (an import from before tools/zero_vehicle.py
// exported them, or a class without AddSpringBody) hovers on a lift spring
// as the original game's branch for one does, like the recon droid
// (src/remote.js): its origin wants to be SetAltitude over what is under
// its collision centre (terrain, buildings, water), lifted along that
// ground's normal at (SetAltitude - height) x LiftSpring + 9.8 f, less
// LiftDamp x its speed along the normal, where f fades from 1 at that
// height to 0 nine times higher; its turn rates lose LevelDamp a second and
// its nose and wings turn toward that ground at LevelSpring; it drives in
// the plane of the ground.
function hoverWithoutSprings(v, h, dt, fwd, side, brake) {
  const c = v.system.collision, water = v.system.water;
  s.normal.copy(UP);
  let f = 0;
  if (h.altitude > 0) {
    // The collision box's centre, from its class geometry (a correction
    // carries only the hull's state).
    v.bounds.getCenter(s.center).applyQuaternion(s.q).add(v.position);
    const lift = s.center.y - v.position.y;
    const reach = v.boundRadius + 10 * h.altitude;
    let height = reach - lift;
    const below = c.cast(s.center, DOWN, reach, s.hit, v.body);
    if (below) { height = below.distance - lift; s.normal.copy(below.normal); }
    if (water?.over(s.center.x, s.center.z) && s.center.y - water.level < height + lift) {
      height = Math.min(height, s.center.y - water.level - lift);
      s.normal.copy(UP);
    }
    if (Math.hypot(v.velocity.x, v.velocity.z) > LOOKAHEAD) {
      s.ray.set(v.velocity.x, v.velocity.y - reach, v.velocity.z);
      const length = s.ray.length();
      const ahead = c.cast(s.center, s.ray.divideScalar(length), length, s.hit, v.body);
      if (ahead) {
        height = Math.min(height, -s.ray.multiplyScalar(ahead.distance).dot(ahead.normal));
        if (ahead.normal.y > 0) s.normal.lerp(ahead.normal, (1 - ahead.distance / length) * ahead.normal.y).normalize();
      }
    }
    f = clamp(1 - (height - h.altitude) / (9 * h.altitude), 0, 1);
    v.velocity.y -= lerp(1, h.gravityScale, f) * GRAVITY * dt;
    const push = (h.altitude - height) * h.liftSpring + GRAVITY * f - v.velocity.dot(s.normal) * h.liftDamp;
    if (push > 0) v.velocity.addScaledVector(s.normal, push * dt);
    s.target.set(s.normal.x * f, s.normal.y + LEVEL_LIFT, s.normal.z * f).normalize();
  } else {
    // Resting on its body alone: the last tick's support.
    f = v.support;
    v.velocity.y -= lerp(1, h.gravityScale, f) * GRAVITY * dt;
    s.target.copy(UP);
  }
  v.support = f;
  const damp = 1 - Math.min(1, h.levelDamp * dt), level = Math.min(1, h.levelSpring * dt);
  v.tiltRate = v.tiltRate * damp - level * s.forward.dot(s.target);
  v.rollRate = v.rollRate * damp - level * s.right.dot(s.target);
  s.groundForward.copy(s.forward).addScaledVector(s.normal, -s.forward.dot(s.normal)).normalize();
  s.groundRight.crossVectors(s.normal, s.groundForward).normalize();
  drive(v, h, s.groundForward, s.groundRight, s.up.y, fwd, side, brake, dt);
}

// One tick before the collision pass. fwd, side: the drive and
// strafe sticks (0 when nobody drives); brake: the port's brake key.
export function hoverPhysics(v, dt, fwd, side, brake) {
  const h = hoverClass(v.def);
  axes(v);
  if (!(v.strafeSpeed >= 1e-4)) side = 0;
  if (h.springs.length) {
    // Off the ground (f, its ground factor, at 0) gravity is GravityScale
    // times 9.8, on it 9.8; the springs carry the rest.
    v.velocity.y -= lerp(h.gravityScale, 1, v.support) * GRAVITY * dt;
    // LevelSpring tips the nose and wings back toward level by the square
    // of how far they point up or down.
    const level = Math.min(1, h.levelSpring * dt);
    v.tiltRate -= level * s.forward.y * Math.abs(s.forward.y);
    v.rollRate -= level * s.right.y * Math.abs(s.right.y);
    drive(v, h, s.forward, s.right, s.up.y, fwd, side, brake, dt);
  } else hoverWithoutSprings(v, h, dt, fwd, side, brake);
  v.tilt += v.tiltRate * dt;
  v.roll += v.rollRate * dt;
  v.position.addScaledVector(v.velocity, dt);
  // A class with no altitude (the Hailfire) stands only on this tick's
  // contacts.
  if (h.altitude <= 0) v.support = 0;
}

// A spring's contact: ground within 60 degrees of
// world up and of the hull's up presses it in by the depth along the hull's
// up, up to its travel; past its travel any contact holds the hull off by
// the rest and stops it moving in, as a wall would. Returns the spring's
// compression (0 to 1, the most of its contacts).
function springContact(v, spring, compression, n, depth) {
  const along = n.dot(s.up);
  if (along > SPRING_GROUND && n.y > SPRING_GROUND) compression = Math.max(compression, Math.min(along * depth, spring.length) / spring.length);
  if (depth > spring.length) {
    v.position.addScaledVector(n, depth - spring.length);
    const into = v.velocity.dot(n);
    if (into < 0 && v.alive) {
      v.velocity.addScaledVector(n, -into);
      v.velocity.x -= SPRING_SKID * into * n.x;
      v.velocity.z -= SPRING_SKID * into * n.z;
    }
  }
  return compression;
}

// The springs in the collision pass: each sphere against the terrain, as
// the engine tests a sphere on its height field (docs/formats.md, "Terrain":
// the ground straight under its centre, when the sphere's bottom is below it
// by no more than its diameter, at that drop times the ground's y, along the
// ground's normal), then against buildings, props and the vehicle-only
// barriers, the nearest polygon it overlaps first and each once (other
// hulls meet the hull's wall spheres instead, Vehicle.pushWalls), then the
// water: the depth of its bottom under the surface over its
// travel, and once it is in past its travel and sinking, the hull's fall is
// halved and its other speeds and turn rates lose a tenth.
export function hoverContacts(v) {
  const h = hoverClass(v.def), c = v.system.collision, water = v.system.water;
  for (let i = 0; i < h.springs.length; i++) {
    const spring = h.springs[i], r = spring.radius;
    hullQuaternion(v, s.q);
    s.up.set(0, 1, 0).applyQuaternion(s.q);
    let compression = 0;
    s.center.copy(spring.at).applyQuaternion(s.q).add(v.position);
    if (c.ground(s.center.x, s.center.z, s.center.y + r, s.ground)) {
      const drop = s.ground.height - (s.center.y - r);
      if (drop > 0 && drop <= 2 * r) {
        s.normal.copy(s.ground.normal);
        compression = springContact(v, spring, compression, s.normal, drop * s.normal.y);
        s.center.copy(spring.at).applyQuaternion(s.q).add(v.position);
      }
    }
    // The polygons near it, searched again from where each contact leaves
    // it (gather's list holds until its next call).
    const polygons = c.gather(s.center, r, true);
    s.used.clear();
    for (let pass = 0; pass < 3; pass++) {
      let best = r, polygon = null;
      for (const p of polygons) {
        if (p.moving || s.used.has(p)) continue;
        for (const tri of p.triangles) {
          tri.closestPointToPoint(s.center, s.ray);
          const d = s.ray.distanceTo(s.center);
          if (d >= best) continue;
          best = d; polygon = p; s.nearest.copy(s.ray);
          if (d < 1e-6) {
            tri.getNormal(s.normal);
            if (s.normal.dot(s.ray.subVectors(v.position, s.nearest)) < 0) s.normal.negate();
          }
        }
      }
      if (!polygon) break;
      s.used.add(polygon);
      if (best >= 1e-6) s.normal.subVectors(s.center, s.nearest).divideScalar(best);
      compression = springContact(v, spring, compression, s.normal, r - best);
      s.center.copy(spring.at).applyQuaternion(s.q).add(v.position);
    }
    if (water?.over(s.center.x, s.center.z)) {
      const wet = (water.level - (s.center.y - r)) / spring.length;
      if (wet > 0) {
        compression = Math.max(compression, Math.min(wet, 1));
        if (wet > 1 && v.velocity.y < 0) {
          v.velocity.y *= .5; v.velocity.x *= .9; v.velocity.z *= .9;
          v.tiltRate *= .9; v.rollRate *= .9; v.yawRate *= .9;
        }
      }
    }
    s.compression[i] = compression;
  }
}

// After the collision pass: each pressed spring
// lifts the hull along its up at its lift factor x compression x
// VelocitySpring, less its lift-damping factor x (1 - compression) x
// VelocityDamp x the speed it sinks at, and turns it about its own pitch
// and roll axes at its pitch and roll factors x compression x OmegaXSpring
// and OmegaZSpring, less their damping factors x (1 - compression) x
// OmegaXDamp and OmegaZDamp x the turn rate, each a second; and puts the
// hull on the ground (f = 1). Then f loses a second's worth.
export function hoverSprings(v, dt) {
  const h = hoverClass(v.def);
  hullQuaternion(v, s.q);
  s.up.set(0, 1, 0).applyQuaternion(s.q);
  for (let i = 0; i < h.springs.length; i++) {
    const compression = s.compression[i];
    s.compression[i] = 0;
    if (!(compression > 0)) continue;
    const k = h.springs[i].factors, slack = 1 - compression;
    v.support = 1;
    const lift = k[0] * compression * h.velocitySpring - k[1] * slack * h.velocityDamp * Math.min(v.velocity.dot(s.up), 0);
    v.velocity.addScaledVector(s.up, lift * dt);
    // The original game's pitch axis points the nose down; tilt is nose up.
    v.tiltRate += (-k[2] * compression * h.omegaXSpring - k[3] * slack * h.omegaXDamp * v.tiltRate) * dt;
    v.rollRate += (k[4] * compression * h.omegaZSpring - k[5] * slack * h.omegaZDamp * v.rollRate) * dt;
  }
  v.support = Math.max(0, v.support - dt);
}

// The height of a hull's origin over level ground where it rests: where its
// springs carry its weight (the lift factors x compression x VelocitySpring
// summed to 9.8), else, without springs, where its lift spring does. For
// placing a new hull on its pad (Vehicle.spawn); the physics settle the rest.
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
