// The seat phase on the simulation's module (rust/sim/src/seats/module.rs),
// behind its switch: a world asked for it (createGame's ?seats=rust, or
// BFE_SEATS=rust under Node) runs its hulls' input batch
// (Vehicles.latchCrews: stale leases let go, every bot's intent frozen for
// the update, the exits asked for carried out) in Rust. The leases, the
// hulls and the riders' units stay the battle's objects: the phase writes
// them into the store, the module works on them there, and they are synced
// from the store after it, so everything else reads and writes them as
// before.
//
// What the Rust asks of the battle meanwhile comes back here as the
// module's imports (rust/sim/schema/seats.mjs: the collision world's casts,
// an AI controller's state, a bot's new seat control, a seat's weapons'
// spread rule), served by the same SeatsWorld (src/seats-world.js) the
// JavaScript seats call, none of them coming back into the module. What
// the Rust leaves to the battle (a body's combat flags and drawing as it
// takes or leaves a seat, the battle's hooks) comes back as the answer's
// effects, done here in the order they happened once the objects are
// synced: nothing in the phase reads them back.
//
// A world only; boarding, the other ways out, a destroyed hull's crew and
// the turret crews stay JavaScript's until they move.
import * as THREE from "three";
import { CombatMirror, KIND_SEAT_LEASE, KIND_SEAT_UNIT, KIND_SEATS, KIND_TABLE, KIND_UNIT, KIND_VEHICLE, KIND_VEHICLES } from "./sim/gen/index.js";
import { makeHit } from "./collision.js";

const NO_BUTTONS = [0, 0];

export function attachSeatsSim(vehicles, bridge) {
  vehicles.seatsSim = new SeatsSim(vehicles, bridge);
  return vehicles.seatsSim;
}

export class SeatsSim {
  constructor(vehicles, bridge) {
    this.vehicles = vehicles;
    this.bridge = bridge;
    this.leases = [];
    this.units = [];
    this.riders = new Set(); // only these units cross the seat phase's boundary
    this.seats = { leases: this.leases };
    this.args = { time: 0, ticks: 0, hulls: [], controls: [] };
    this.facts = new WeakMap(); // a hull -> its facts record (its class's are fixed)
    this.controls = []; // the seat control records, reused
    this.answer = null;
    this.keep = new Set();
    this.combats = []; // the riders' combats, kept out of the sync
    this.scratch = []; // what the sync writes their combats' records into
    bridge.register(this.handlers());
  }

  // Vehicles.latchCrews.
  latchCrews() {
    const vs = this.vehicles, b = this.bridge, leases = this.leases, units = this.units;
    leases.length = units.length = 0;
    const riders = this.riders;
    riders.clear();
    let bots = 0;
    for (const v of vs.vehicles) for (const seat of v.seats) {
      const lease = seat.lease;
      if (!lease) continue;
      leases.push(lease);
      if (!lease.human) bots++;
      const unit = lease.unit;
      if (unit && !riders.has(unit)) { riders.add(unit); units.push(unit); }
    }
    // No bot aboard anything: the batch is counted and that is all.
    if (!bots) { vs.ticks += 1; return; }
    this.release();
    // Other phases may own the non-riders' rows, including their combats.
    // Leave those rows alone instead of rewriting every numbered unit.
    b.writeSome(KIND_UNIT, units);
    b.write(KIND_SEAT_LEASE, leases);
    b.write(KIND_SEATS, [this.seats]);
    // (The hulls' list is the hull kind's home: the store gives back the
    // hulls it lists.)
    b.write(KIND_VEHICLE, vs.vehicles);
    b.write(KIND_VEHICLES, [vs]);
    const a = this.args;
    a.time = vs.time;
    a.ticks = vs.ticks;
    a.hulls.length = vs.vehicles.length;
    for (let i = 0; i < vs.vehicles.length; i++) a.hulls[i] = this.hullFacts(vs.vehicles[i], i);
    a.controls.length = 0;
    for (const lease of leases) {
      const seat = lease.controllable;
      if (!seat) continue;
      const c = (this.controls[a.controls.length] ??= { seatUnit: null, driven: false, intent: null });
      c.seatUnit = seat;
      c.driven = Boolean(seat.controller);
      c.intent = seat.intent.buttons ? seat.intent : { ...seat.intent, buttons: NO_BUTTONS };
      a.controls.push(c);
    }
    const out = (this.answer = b.command("seatLatch", a, this.answer));
    b.sync(KIND_SEAT_LEASE);
    b.sync(KIND_VEHICLE);
    // The units back from the store, but for their combats (the combat
    // package's): what the batch writes into one (mounted, aimOverride,
    // gunUnderwater, the zoom) is among its effects below, and the rest it
    // only reads (a JavaScript PlayerCombat has fields the store's record
    // names only through getters; a combat on the module keeps its own).
    const combats = this.combats;
    combats.length = units.length;
    for (let i = 0; i < units.length; i++) {
      combats[i] = units[i].combat;
      units[i].combat = this.scratch[i] ??= new CombatMirror();
    }
    try {
      // Non-riders may have moved in JavaScript since their rows were
      // written. Sync only the riders, and leave retirement rows for the
      // phase that owns them (export mode 2 does not consume them).
      b.exported(KIND_UNIT, 2, (f, at, h, slot, gen, retired) => {
        const unit = b.refs[KIND_UNIT].objects[slot];
        if (!retired && riders.has(unit)) KIND_TABLE[KIND_UNIT].sync(b, f, at, h, unit);
      });
    } finally { for (let i = 0; i < units.length; i++) units[i].combat = combats[i]; }
    combats.fill(null);
    vs.ticks = out.ticks;
    // SeatInput.latch took these edges from the seat's intent it played.
    for (const seat of out.latched) {
      const intent = seat.intent;
      intent.reload = intent.use = intent.exit = false;
      intent.jump = intent.crouch = intent.prone = false;
      intent.nextPrimary = intent.previousPrimary = intent.nextSecondary = intent.zoom = false;
    }
    for (const e of out.effects) this.effect(e);
  }

  // A hull's facts as its seats read them beyond the store (its place, who
  // last hurt it, its class's base and health type, its seats' turret
  // nodes and weapons).
  hullFacts(v, index) {
    let f = this.facts.get(v);
    if (!f) {
      f = { vehicle: v, index, attacker: 0, attackerId: 0, hover: v.base === "hover", animal: v.healthType === "animal",
        turretNodes: v.seats.map((s) => Boolean(s.settings?.TurretNodeName)), weapons: v.seats.map((s) => s.weapons.length) };
      this.facts.set(v, f);
    }
    f.index = index;
    const by = v.lastAttacker;
    if (by === "player") { f.attacker = 1; f.attackerId = 0; }
    else if (by && this.vehicles.vehicles.includes(by)) { f.attacker = 3; f.attackerId = this.vehicles.vehicles.indexOf(by); }
    else if (by && Number.isSafeInteger(by.id)) { f.attacker = 2; f.attackerId = by.id; }
    else { f.attacker = 0; f.attackerId = 0; }
    return f;
  }

  // What the Rust left for the battle, as the JavaScript seats do it.
  effect(e) {
    const vs = this.vehicles, world = vs.seatsWorld;
    const unit = e.unit < 0 ? null : this.bridge.refs[KIND_UNIT].objects[e.unit];
    switch (e.kind) {
      case "seated": {
        const combat = unit.combat;
        if (combat) {
          combat.mounted = true;
          combat.zoomed = false;
          combat.gunUnderwater = false;
          combat.aimOverride = null;
        }
        break;
      }
      case "unseated":
        if (unit.combat) { unit.combat.mounted = false; unit.combat.aimOverride = null; unit.combat.zoomed = false; }
        unit.animator?.gunner?.(null);
        unit.actor?.root?.rotation?.set?.(0, e.yaw, 0);
        break;
      case "placed":
        unit.presentation?.reset?.();
        unit.animator?.gunner?.(null);
        unit.animator?.reset?.(e.yaw);
        if (unit.actor?.root) {
          unit.actor.root.visible = true;
          unit.actor.root.rotation?.set?.(0, e.yaw, 0);
        }
        break;
      case "dropped":
        unit.presentation?.reset?.();
        unit.actor?.root?.position?.copy?.(e.at);
        break;
      case "boarded":
        world.boarded(unit, vs.vehicles[e.hull], e.seat);
        break;
      case "unboarded":
        world.unboarded(unit, vs.vehicles[e.hull], e.seat, e.post, e.reason);
        break;
      case "moved":
        world.moved(unit, vs.vehicles[e.hull], e.seat);
        break;
    }
  }

  // Lets the bridge forget the leases and seat controls that no seat holds
  // now (a battle makes thousands, and each would keep its number for
  // good); one the phase meets again (a unit's old seat) is numbered anew.
  release() {
    const keep = this.keep;
    keep.clear();
    for (const lease of this.leases) {
      keep.add(lease);
      if (lease.controllable) keep.add(lease.controllable);
    }
    for (const kind of [KIND_SEAT_LEASE, KIND_SEAT_UNIT]) {
      const refs = this.bridge.refs[kind];
      for (const x of refs.objects) if (x && !keep.has(x)) refs.release(x);
    }
  }

  // Each import's handler. The arguments object (and its vectors) is the
  // bridge's, reused for the next call of the same import; nothing here
  // comes back into the module.
  handlers() {
    const vs = this.vehicles, world = vs.seatsWorld;
    const origin = new THREE.Vector3(), direction = new THREE.Vector3(), center = new THREE.Vector3();
    const hit = makeHit();
    const answer = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    const flag = { value: false }, number = { value: undefined }, made = { seatUnit: null };
    return {
      seatCast: (a) => {
        origin.copy(a.origin);
        direction.copy(a.direction);
        const h = world.cast(origin, direction, a.far, hit, a.ignore);
        answer.found = Boolean(h);
        if (h) {
          answer.distance = h.distance;
          answer.point.copy(h.point);
          answer.normal.copy(h.normal);
          answer.terrain = Boolean(h.terrain);
          answer.body = h.body ?? null;
        } else {
          answer.distance = 0;
          answer.terrain = false;
          answer.body = null;
        }
        return answer;
      },
      seatOverlaps: (a) => {
        center.copy(a.center);
        flag.value = Boolean(world.overlapsSphere(center, a.radius, a.ignore));
        return flag;
      },
      seatControllerState: (a) => {
        number.value = world.controllerState(a.unit < 0 ? null : this.bridge.refs[KIND_UNIT].objects[a.unit]);
        return number;
      },
      // A bot's new seat control, made as Vehicles.move makes it: the
      // lease in its new seat by then.
      seatControllable: (a) => {
        const lease = a.lease;
        lease.index = a.index;
        lease.role = a.role;
        made.seatUnit = world.controllable(lease);
        return made;
      },
      seatPolicy: (a) => {
        vs.vehicles[a.hull].setSeatPolicy(a.seat, a.player, a.random ? vs.seatRandom : null);
      },
    };
  }
}
