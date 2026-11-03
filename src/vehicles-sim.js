// The hull phase on the simulation's module (rust/sim/src/vehicles/module.rs),
// behind its switch: a world asked for it (createGame's ?hulls=rust, or
// BFE_HULLS=rust under Node) runs its ground hulls' tick start and 120 Hz
// substeps in Rust. The Vehicle objects stay the battle's: the phase writes
// them into the store, the module ticks them there, and they are synced from
// the store after it, so everything else reads and writes them as before.
// What the Rust asks of the battle comes back here as the module's imports
// (rust/sim/schema/vehicles.mjs) and goes to the same VehiclesWorld
// (src/vehicles-world.js) the JavaScript hulls call. A hook that hands the
// battle a hull (damaged, destroyedCrew, destroyed, explosion) finds every
// hull as the phase has left it, and whatever it does to any hull goes back
// into the store before the phase goes on.
//
// The collision world's queries are answered in the module itself when the
// battle's collision runs on it (src/ray-kernel.js: the module holds the
// collision world's copy then), and the map's water and death regions
// always: the module is told the kernel's world, the battle's bodies by
// the numbers the store knows them by, each hull's body and gather caches,
// and before the phase and after every hook each body's enabled flag (a
// plain field here); after the phase and before every hook the flags and
// places the hulls changed come back onto the bodies, so the battle and
// the module never see the bodies differently.
//
// A world only; the flight, the seats' latch, the poses, the fire and the
// tow cables stay JavaScript's until they move.
import * as THREE from "three";
import { KIND_VEHICLE, KIND_VEHICLES, KIND_BODY } from "./sim/gen/index.js";
import { makeGround, makeHit } from "./collision.js";

const MISS = Object.freeze({ found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: -1 });
const NO_GROUND = Object.freeze({ found: false, height: 0, normal: new THREE.Vector3() });

// A class's keys as JSON the module parses (rust/content's parser), keeping
// what JSON.stringify would lose: a -0 stays -0.
function json(v) {
  if (v === null || v === undefined) return "null";
  if (typeof v === "number") return Object.is(v, -0) ? "-0" : Number.isFinite(v) ? String(v) : "null";
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "string") return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map((x) => (x === undefined || typeof x === "function" ? "null" : json(x))).join(",")}]`;
  const parts = [];
  for (const [k, x] of Object.entries(v)) if (x !== undefined && typeof x !== "function") parts.push(`${JSON.stringify(k)}:${json(x)}`);
  return `{${parts.join(",")}}`;
}

// What the module's HullClass reads of a class (rust/sim/src/vehicles/class.rs).
const classKeys = (def) => ({
  settings: def.settings ?? null, springs: def.springs ?? [], fx: def.fx ?? null,
  explosion: Boolean(def.explosion), finalExplosion: Boolean(def.finalExplosion),
  seats: (def.seats ?? []).map((s) => ({ settings: s.settings ?? null })),
});

// map: what the hulls ask of the map (game.js): the kill height under the
// battlefield (killY), the map's death regions and the names its script
// adds (world.deathRegions, world.mission.deathRegions); null leaves the
// water and death regions to the imports.
export function attachVehiclesSim(vehicles, bridge, map = null) {
  vehicles.sim = new VehiclesSim(vehicles, bridge, map);
  return vehicles.sim;
}

export class VehiclesSim {
  constructor(vehicles, bridge, map = null) {
    this.vehicles = vehicles;
    this.bridge = bridge;
    this.x = bridge.x;
    this.world = vehicles.world;
    this.collision = vehicles.collision;
    this.classes = new Set();
    // The collision world in the module: whether it answers (the battle's
    // collision runs on the kernel), the bodies' version it last named, and
    // the hulls' bodies it knows.
    this.kernel = false;
    this.bodiesVersion = -1;
    this.colliders = new WeakSet();
    if (map) {
      const water = vehicles.water;
      const data = {
        water: water ? { cell: water.cell, offset: water.offset, grid: water.grid, cells: Array.from(water.cells) } : null,
        deathRegions: map.deathRegions, mission: { deathRegions: map.scripted },
      };
      this.x.veh_map(bridge.strings.id(json(data)), map.killY);
    }
    this.hit = makeHit();
    this.groundHit = makeGround();
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.flag = { value: false };
    this.number = { value: 0 };
    Object.assign(bridge.handlers, this.handlers());
  }

  // The hulls into the store: each one's fields, and the battle's list.
  write() {
    const b = this.bridge, vs = this.vehicles;
    for (const v of vs.vehicles) if (!this.classes.has(v.name)) {
      this.classes.add(v.name);
      this.x.veh_class(b.strings.id(v.name), b.strings.id(json(classKeys(v.def))), v.turnRate);
    }
    b.write(KIND_VEHICLE, vs.vehicles);
    b.write(KIND_VEHICLES, [vs]);
  }

  // The store's hulls back into the Vehicle objects.
  sync() {
    this.bridge.sync(KIND_VEHICLE);
  }

  // Vehicles.update's ground hulls: their tick start and their substeps
  // (`acc`: the time left over from the last update); returns what is left.
  ground(dt, acc) {
    this.write();
    this.bodiesIn();
    const water = this.vehicles.water;
    const left = this.x.veh_ground(dt, acc, water ? water.level : 0);
    this.sync();
    this.bodiesOut();
    return left;
  }

  // Room for `count` numbers handed to the module: the memory's f64 view
  // and where in it they go.
  input(count) {
    const ptr = this.x.veh_input(count);
    this.bridge.views();
    return ptr / 8;
  }

  // The battle's bodies as the module's collision world needs them, before
  // the phase and after a hook: the kernel's world (or none, when the
  // battle's collision is JavaScript's), the kernel's order of the bodies,
  // the numbers the store knows each by, every hull's body, and each
  // body's enabled flag by its place in that order.
  bodiesIn() {
    const c = this.collision, rays = c?.rays;
    if (!rays) {
      if (this.kernel) { this.x.veh_collision(-1, -1); this.kernel = false; }
      return;
    }
    if (!this.kernel) {
      const blockers = c.vehicleBlockers ? rays.fixedSet(c.blockerSet(c.vehicleBlockers), c.vehicleBlockers.bvh) : -1;
      this.x.veh_collision(c.rayWorld, blockers);
      this.kernel = true;
      this.bodiesVersion = -1;
    }
    c.rayBodies();
    const bodies = c.bodies, refs = this.bridge.refs[KIND_BODY];
    if (this.bodiesVersion !== c.bodiesVersion) {
      const at = this.input(bodies.length * 2), f = this.bridge.f;
      for (let i = 0; i < bodies.length; i++) { f[at + 2 * i] = refs.slotOf(bodies[i]); f[at + 2 * i + 1] = bodies[i].raySlot; }
      this.x.veh_bodies(bodies.length);
      this.bodiesVersion = c.bodiesVersion;
    }
    for (const v of this.vehicles.vehicles) if (!this.colliders.has(v.body)) this.collider(v);
    const ptr = this.x.veh_flags();
    this.bridge.views();
    const u8 = this.bridge.u8;
    for (let i = 0; i < bodies.length; i++) u8[ptr + i] = bodies[i].enabled ? 1 : 0;
  }

  // A hull's body for the module: its numbers, its bounds in its own frame
  // and the kernel's copies of its wall probes' and springs' gather caches.
  collider(v) {
    const rays = this.collision.rays, body = v.body;
    // (Making a cache's copy may grow the memory: the ids first.)
    const walls = (v.wallCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const springs = (v.springCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const slot = this.bridge.refs[KIND_BODY].slotOf(body);
    const at = this.input(8 + walls.length + springs.length), f = this.bridge.f, b = body.localBounds;
    f[at] = slot; f[at + 1] = body.raySlot;
    f[at + 2] = b.min.x; f[at + 3] = b.min.y; f[at + 4] = b.min.z; f[at + 5] = b.max.x; f[at + 6] = b.max.y; f[at + 7] = b.max.z;
    for (let i = 0; i < walls.length; i++) f[at + 8 + i] = walls[i];
    for (let i = 0; i < springs.length; i++) f[at + 8 + walls.length + i] = springs[i];
    this.x.veh_collider(walls.length, springs.length);
    this.colliders.add(body);
  }

  // What the hulls did to their bodies in the module since the last time
  // (enabled flags, places: matrix, inverse and bounds), onto the bodies.
  bodiesOut() {
    if (!this.kernel) return;
    const ptr = this.x.veh_changes();
    this.bridge.views();
    const f = this.bridge.f, bodies = this.collision.rays.bodies;
    let o = ptr / 8;
    const toggled = f[o], placed = f[o + 1];
    o += 2;
    for (let i = 0; i < toggled; i++, o += 2) bodies[f[o]].enabled = f[o + 1] !== 0;
    for (let i = 0; i < placed; i++, o += 39) {
      const body = bodies[f[o]];
      body.bounds.min.set(f[o + 1], f[o + 2], f[o + 3]);
      body.bounds.max.set(f[o + 4], f[o + 5], f[o + 6]);
      body.matrix.fromArray(f, o + 7);
      body.inverse.fromArray(f, o + 23);
    }
  }

  body(slot) {
    return slot < 0 ? null : this.bridge.refs[KIND_BODY].objects[slot];
  }

  bodySlot(body) {
    return body ? this.bridge.refs[KIND_BODY].slotOf(body) : -1;
  }

  // A hook that hands the battle a hull: every hull as the phase left it
  // first, and what the hook did to them back into the store after.
  hook(call) {
    this.sync();
    this.bodiesOut();
    try { call(); } finally { this.bridge.write(KIND_VEHICLE, this.vehicles.vehicles); this.bodiesIn(); }
  }

  // Each import's handler. The arguments object (and its vectors) is the
  // bridge's, reused for the next call of the same import: every argument
  // is read before anything that can come back into the module runs, and a
  // vector handed on that the battle may keep is copied.
  handlers() {
    const w = this.world, vs = this.vehicles;
    const hullAt = (i) => vs.vehicles[i];
    const origin = new THREE.Vector3(), direction = new THREE.Vector3(), position = new THREE.Vector3(), velocity = new THREE.Vector3();
    const previous = new THREE.Vector3(), q = new THREE.Quaternion(), own = new THREE.Quaternion(), matrix = new THREE.Matrix4();
    const center = new THREE.Vector3(), point = new THREE.Vector3();
    const probes = [];
    return {
      vehCast: (a) => {
        origin.copy(a.origin); direction.copy(a.direction);
        const hit = w.cast(origin, direction, a.far, this.hit, this.body(a.ignore));
        return hit ? { found: true, distance: hit.distance, point: hit.point, normal: hit.normal, terrain: Boolean(hit.terrain), body: this.bodySlot(hit.body) } : MISS;
      },
      vehPushWalls: (a) => {
        const hull = hullAt(a.hull), radius = a.radius, terrain = a.terrain;
        probes.length = a.probes.length;
        for (let i = 0; i < probes.length; i++) (probes[i] ??= new THREE.Vector3()).copy(a.probes[i]);
        q.copy(a.q); previous.copy(a.previous); position.copy(a.position); velocity.copy(a.velocity);
        const mine = a.own ? own.copy(a.own) : null;
        const undo = w.pushWalls(hull, probes, radius, q, mine, previous, position, velocity, terrain);
        return { undo: Boolean(undo), position, velocity };
      },
      vehGround: (a) => {
        const g = w.ground(a.x, a.z, a.top, this.groundHit);
        return g ? { found: true, height: g.height, normal: g.normal } : NO_GROUND;
      },
      vehSpringPolygons: (a) => {
        const hull = hullAt(a.hull), index = a.index, radius = a.radius;
        center.copy(a.center);
        return { polygons: w.springPolygons(hull, index, center, radius).map((p) => ({ id: p.id, moving: Boolean(p.moving), triangles: p.triangles })) };
      },
      vehMoveBody: (a) => { const body = this.body(a.body); matrix.copy(a.matrix); w.moveBody(body, matrix); },
      vehEnableBody: (a) => { w.enableBody(this.body(a.body), a.enabled); },
      vehBodyEnabled: (a) => { this.flag.value = Boolean(w.bodyEnabled(this.body(a.body))); return this.flag; },
      vehHasWater: () => { this.flag.value = Boolean(vs.water); return this.flag; },
      vehWaterOver: (a) => { this.flag.value = Boolean(w.waterOver(a.x, a.z)); return this.flag; },
      vehWaterLevel: () => { this.number.value = w.waterLevel(); return this.number; },
      vehInDeathRegion: (a) => { point.copy(a.position); this.flag.value = Boolean(w.inDeathRegion(point)); return this.flag; },
      vehRaycastUnits: (a) => {
        origin.copy(a.origin); direction.copy(a.direction);
        const hit = w.raycastUnits(origin, direction, a.far, this.unitHit);
        return hit ? { ...MISS, found: true, distance: hit.distance } : MISS;
      },
      vehClock: () => { this.number.value = w.clock(); return this.number; },
      vehDamaged: (a) => {
        const v = hullAt(a.hull), amount = a.amount, attacker = a.attacker;
        this.hook(() => w.damaged(v, amount, attacker));
      },
      vehDestroyedCrew: (a) => { const v = hullAt(a.hull); this.hook(() => vs.destroyedCrew?.(v)); },
      vehDestroyed: (a) => { const v = hullAt(a.hull); this.hook(() => w.destroyed(v)); },
      vehRespawnClock: (a) => { const v = hullAt(a.hull); v.slot.remaining = v.slot.desc.respawnTime; },
      vehReleaseCrew: (a) => {
        for (const seat of hullAt(a.hull).seats) {
          const lease = seat.lease;
          if (!lease) continue;
          lease.released = true;
          if (lease.unit?.seat === lease) lease.unit.seat = null;
          seat.lease = null;
        }
      },
      vehExplosion: (a) => {
        const v = hullAt(a.hull);
        // At the hull's own position (the very vector, as the JavaScript
        // hands it) when its class sets no offset, else a point of its own.
        const def = a.final ? v.def.finalExplosion || v.def.explosion : v.def.explosion;
        const at = a.final || v.def.fx?.explosionOffset ? a.position.clone() : v.position;
        this.hook(() => w.explosion(def, at, v));
      },
      vehEffect: (a) => { const name = a.name, at = a.position.clone(); w.effect(name, at); },
    };
  }
}
