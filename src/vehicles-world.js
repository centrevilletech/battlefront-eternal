import * as THREE from "three";
import { emitSound } from "./audio/events.js";
import { blastBlocked } from "./ordnance.js";

// What the hulls (src/vehicles.js, src/vehicle-hover.js) ask of the rest of
// the battle as they move, pose, fire and come to harm: every query of the
// collision world and the water, every call into code that is not theirs
// (rounds and explosions, the battle's hooks, the registry's units, the
// capture's observers) and the clock their barrels kick back on. Their own
// logic reaches the battle through these calls and nothing else, in the
// order it makes them, so this is the boundary the simulation's port to Rust
// moves them across (docs/sim-store.md): the same calls are the trait the
// Rust hulls reach the battle through, what the replay harness records of
// their frames (tools/sim-capture/specs.mjs), and, once Rust owns the hulls,
// what its module calls back into JavaScript.
//
// The seats, boarding and flight (src/seats.js, src/boarding.js,
// src/flight/) move with the hulls and are reached directly; so is what
// only a page draws (the player's camera, presentRiders, the tow line).

export class VehiclesWorld {
  constructor(vehicles) {
    this.vehicles = vehicles;
    // pushWalls' own scratch, for the collision without its WebAssembly.
    this.point = new THREE.Vector3();
    this.probeStart = new THREE.Vector3();
    this.normal = new THREE.Vector3();
    this.nearest = new THREE.Vector3();
  }

  // The collision world's first hit along a ray (CollisionWorld.cast), into
  // `out`, leaving out ignoreBody; null for none.
  cast(origin, direction, far, out, ignoreBody) {
    return this.vehicles.collision.cast(origin, direction, far, out, ignoreBody);
  }

  // A hull's sphere chain against walls (Vehicle.pushWalls): each of its
  // probes, placed in its frame q at `position`, pushed out of the
  // polygons of buildings, props, barriers and other hulls that are no
  // floor (and with terrain, of the terrain's steep faces too), moving
  // `position` and stopping `velocity` into them. own: a walker's frame a
  // step ago (its probes then at `previous`), against which another hull
  // gives way only as far as the probe came toward it; null for the rest.
  // Returns whether a push went deeper than 0.15 m (the hull's turn this
  // step is undone). hull: whose probes these are (their gather caches).
  pushWalls(hull, probes, radius, q, own, previous, position, velocity, terrain) {
    const c = this.vehicles.collision;
    // In the collision's WebAssembly, whole: the same pushes in the same
    // order, with no polygon or triangle lists made for them here.
    if (c.rays) try {
      return c.rayPushWalls(probes, radius, hull.wallCaches, q, own, previous, position, velocity, terrain);
    } catch (error) { c.dropRays(error); }
    let undo = false;
    for (let pass = 0; pass < 2; pass++) for (let i = 0; i < probes.length; i++) {
      const probe = probes[i];
      this.point.copy(probe).applyQuaternion(q).add(position);
      if (own) this.probeStart.copy(probe).applyQuaternion(own).add(previous);
      for (const polygon of c.gather(this.point, radius, true, hull.wallCaches[i])) for (const tri of polygon.triangles) {
        if (this.pushOut(tri, radius, position, velocity, own && polygon.moving)) undo = true;
      }
      if (terrain) for (const tri of c.terrainNear(this.point, radius)) if (this.pushOut(tri, radius, position, velocity)) undo = true;
    }
    return undo;
  }

  // One probe (this.point) out of one triangle: whether it went deeper
  // than 0.15 m. other: a moving hull's (see pushWalls).
  pushOut(tri, radius, position, velocity, other = false) {
    tri.getNormal(this.normal);
    if (Math.abs(this.normal.y) > .65) return false; // suspension handles floors
    tri.closestPointToPoint(this.point, this.nearest);
    this.normal.subVectors(this.point, this.nearest);
    const distance = this.normal.length();
    if (distance >= radius || distance < 1e-6) return false;
    this.normal.divideScalar(distance);
    let depth = radius - distance + .002;
    if (other) {
      // How far this probe came toward the other hull this step.
      const came = -(this.normal.x * (this.point.x - this.probeStart.x) + this.normal.y * (this.point.y - this.probeStart.y) + this.normal.z * (this.point.z - this.probeStart.z));
      depth = Math.min(depth, Math.max(0, came));
      if (!(depth > 0)) return false;
    }
    position.addScaledVector(this.normal, depth);
    this.point.addScaledVector(this.normal, depth);
    const into = velocity.dot(this.normal);
    if (into < 0) velocity.addScaledVector(this.normal, -into);
    return depth > .15;
  }

  // The terrain straight under (x, z) from `top` down (CollisionWorld.ground):
  // its height and normal in `out`, or null.
  ground(x, z, top, out) {
    return this.vehicles.collision.ground(x, z, top, out);
  }

  // The polygons near a hover's spring `index` (CollisionWorld.gather,
  // through the spring's own cache), each with its triangles: the list
  // holds until the next query.
  springPolygons(hull, index, center, radius) {
    return this.vehicles.collision.gather(center, radius, true, hull.springCaches?.[index] ?? null);
  }

  // A hull's collision body follows its frame (CollisionWorld.moveBody).
  moveBody(body, matrix) {
    this.vehicles.collision.moveBody(body, matrix);
  }

  // Whether a body takes part in the collision's queries, and setting it
  // (a hull leaves its own out while it pushes and probes, and when it is
  // destroyed).
  bodyEnabled(body) {
    return body.enabled;
  }

  enableBody(body, enabled) {
    body.enabled = enabled;
  }

  // Whether terrain or a building stands between a blast and a point (the
  // hull's own body left out): src/ordnance.js blastBlocked.
  blastBlocked(position, target, body) {
    return blastBlocked(this.vehicles.collision, position, target, body);
  }

  // The map's water: whether (x, z) is over it (undefined without any) and
  // its level.
  waterOver(x, z) {
    return this.vehicles.water?.over(x, z);
  }

  waterLevel() {
    return this.vehicles.water.level;
  }

  // Whether a point is in one of the map's death regions.
  inDeathRegion(position) {
    return this.vehicles.inDeathRegion?.(position);
  }

  // The first unit along a ray within `far` (Units.raycast), its hit in
  // `out`; null for none or a battle without units.
  raycastUnits(origin, direction, far, out) {
    return this.vehicles.units?.raycast?.(origin, direction, far, null, out);
  }

  // The clock barrels kick back on (a world's tick clock, a page's own).
  clock() {
    return this.vehicles.clock();
  }

  // The battle's hooks: a hull took damage, a hull was destroyed (after its
  // crew was let go), a hull's seat fired.
  damaged(vehicle, amount, attacker) {
    this.vehicles.onDamaged?.(vehicle, amount, attacker);
  }

  destroyed(vehicle) {
    this.vehicles.onDestroyed?.(vehicle);
  }

  fired(vehicle, seat, shot, owner, muzzle, direction) {
    this.vehicles.onFired?.(vehicle, seat, shot, owner, muzzle, direction);
  }

  // Rounds and explosions (src/ordnance.js): an explosion at `position`,
  // owned by `owner`; a round of `def` (Ordnance.fire, which returns a
  // rocket's, missile's or shell's record, nothing for the rest).
  explosion(def, position, owner) {
    this.vehicles.ordnance.explosion(def, position, owner);
  }

  fire(def, origin, direction, owner, ignoreBody, emitter, shot) {
    return this.vehicles.ordnance.fire(def, origin, direction, owner, null, ignoreBody, emitter, shot);
  }

  // What a page shows and hears of them: an effect at a point (recorded by
  // a world for its page), a muzzle's flash, a sound (the presentation
  // capture hears turretFire, towAttach and towRelease).
  effect(name, position) {
    this.vehicles.effects?.play(name, position);
  }

  flash(position) {
    this.vehicles.ordnance.lights.flash(position, [1, .55, .2], 5, .06);
  }

  sound(type, ...args) {
    emitSound(type, ...args);
  }
}
