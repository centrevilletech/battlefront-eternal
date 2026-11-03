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

export function attachVehiclesSim(vehicles, bridge) {
  vehicles.sim = new VehiclesSim(vehicles, bridge);
  return vehicles.sim;
}

export class VehiclesSim {
  constructor(vehicles, bridge) {
    this.vehicles = vehicles;
    this.bridge = bridge;
    this.x = bridge.x;
    this.world = vehicles.world;
    this.classes = new Set();
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
    const left = this.x.veh_ground(dt, acc);
    this.sync();
    return left;
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
    try { call(); } finally { this.bridge.write(KIND_VEHICLE, this.vehicles.vehicles); }
  }

  handlers() {
    const w = this.world, vs = this.vehicles;
    const hullAt = (i) => vs.vehicles[i];
    return {
      vehCast: (a) => {
        const hit = w.cast(a.origin, a.direction, a.far, this.hit, this.body(a.ignore));
        return hit ? { found: true, distance: hit.distance, point: hit.point, normal: hit.normal, terrain: Boolean(hit.terrain), body: this.bodySlot(hit.body) } : MISS;
      },
      vehPushWalls: (a) => {
        const undo = w.pushWalls(hullAt(a.hull), a.probes, a.radius, a.q, a.own, a.previous, a.position, a.velocity, a.terrain);
        return { undo: Boolean(undo), position: a.position, velocity: a.velocity };
      },
      vehGround: (a) => {
        const g = w.ground(a.x, a.z, a.top, this.groundHit);
        return g ? { found: true, height: g.height, normal: g.normal } : NO_GROUND;
      },
      vehSpringPolygons: (a) => ({ polygons: w.springPolygons(hullAt(a.hull), a.index, a.center, a.radius).map((p) => ({ id: p.id, moving: Boolean(p.moving), triangles: p.triangles })) }),
      vehMoveBody: (a) => { w.moveBody(this.body(a.body), a.matrix); },
      vehEnableBody: (a) => { w.enableBody(this.body(a.body), a.enabled); },
      vehBodyEnabled: (a) => { this.flag.value = Boolean(w.bodyEnabled(this.body(a.body))); return this.flag; },
      vehHasWater: () => { this.flag.value = Boolean(vs.water); return this.flag; },
      vehWaterOver: (a) => { this.flag.value = Boolean(w.waterOver(a.x, a.z)); return this.flag; },
      vehWaterLevel: () => { this.number.value = w.waterLevel(); return this.number; },
      vehInDeathRegion: (a) => { this.flag.value = Boolean(w.inDeathRegion(a.position)); return this.flag; },
      vehRaycastUnits: (a) => {
        const hit = w.raycastUnits(a.origin, a.direction, a.far, this.unitHit);
        return hit ? { ...MISS, found: true, distance: hit.distance } : MISS;
      },
      vehClock: () => { this.number.value = w.clock(); return this.number; },
      vehDamaged: (a) => { const v = hullAt(a.hull); this.hook(() => w.damaged(v, a.amount, a.attacker)); },
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
        // A blast with no offset goes off at the hull's own position (the
        // very vector, as the JavaScript hands it).
        const def = a.final ? v.def.finalExplosion || v.def.explosion : v.def.explosion;
        this.hook(() => w.explosion(def, a.final || v.def.fx?.explosionOffset ? a.position : v.position, v));
      },
      vehEffect: (a) => { w.effect(a.name, a.position); },
    };
  }
}
