
import { makeUnitHit } from "./registry.js";

import * as THREE from "three";
import { SeatsWorld } from "./seats-world.js";

// Seats: who sits in which vehicle seat or turret, the player and the bots
// alike, following the 2004 game where its rules are known.
//
// A seat is held by a lease: the occupant (a unit, or the player), the life
// of that unit it was granted to (its generation), its role (pilot, gunner,
// passenger), and when it was granted. At most one lease holds a seat.
// Boarding, leaving, a death, and the vehicle's destruction clear a lease at
// once, before the next input batch; a lease whose unit has died or
// respawned since steers nothing and is let go at the next batch.
//
// The lease is also the unit's seat reference: while a unit sits,
// unit.seat === its lease, with .vehicle or .turret, and .index.
//
// A bot in a seat plays its intent (docs/ai.md) through the seat: the intent
// is frozen once per vehicle update (SeatInput), so every physics substep of
// that update sees the same held values and each edge (a trigger press,
// reload, use) is taken exactly once.

const UP = new THREE.Vector3(0, 1, 0);
const DOWN = new THREE.Vector3(0, -1, 0);
const f32 = Math.fround;
const ZERO_BUTTONS = Object.freeze([0, 0]);

export const PILOT = "pilot";
export const GUNNER = "gunner";
export const PASSENGER = "passenger";

// The 2004 game's object classifier, as the AI's target matchups use it. It
// never produces 4.
export const KIND = Object.freeze({
  soldier: 0, droid: 1, walker: 2, droideka: 3, flyer: 5, hover: 6,
  mounted: 7, armedBuilding: 8, building: 9, item: 10,
});

// A vehicle class's classifier: walkers (the creatures and command walkers
// too) are EntityWalker, flyers EntityFlyer, and everything else on the
// ground is EntityHover, the Hailfire included (its BASE is hover).
export function vehicleKind(base) {
  if (base === "walker" || base === "commandwalker") return KIND.walker;
  if (base === "flyer") return KIND.flyer;
  return KIND.hover;
}

// The blank intent a seat's controller fills (docs/ai.md ActorIntent, with
// the vehicle fields brake, up and down).
export function blankSeatIntent() {
  return {
    forward: 0, right: 0, turn: 0, pitch: 0,
    jump: false, crouch: false, prone: false, use: false, exit: false,
    buttons: [0, 0],
    reload: false, nextPrimary: false, previousPrimary: false, nextSecondary: false, zoom: false,
    aim: null,
    brake: false, up: 0, down: 0,
  };
}

// Seat 0 drives; a seat with weapons guns; any other carries.
export function seatRole(seats, index) {
  if (index === 0) return PILOT;
  return seats[index]?.weapons?.length ? GUNNER : PASSENGER;
}

// A shared weapon survives changes of occupant. Return its aiming modifiers
// to the player's defaults when a human takes the seat, and its generator to
// the seat's: random, the world's seat stream (null: none, for a page's own
// player, whose seat fire gives it one). The Classic combat binding gives a
// bot's weapons the AI's generator when its seat controller binds, so the
// seat stream draws a network gunner's spread and a bot's before that.
export function seatWeaponPolicy(weapon, human, random = null) {
  if (!weapon) return;
  weapon.floor = human ? -(weapon.def.SpreadThreshold || 0) : 0;
  if (weapon.kick < weapon.floor) weapon.kick = weapon.floor;
  weapon.spreadScale = 1;
  weapon.sniperScale = 0;
  weapon.yawSpreadOverride = null;
  weapon.random = random;
}

export class SeatLease {
  constructor({ unit = null, human = false, team = 0, vehicle = null, turret = null, index = 0, role = PILOT, time = 0, tick = 0 } = {}) {
    this.unit = unit;
    this.generation = unit ? unit.generation : 0;
    this.human = human;
    this.team = team;
    this.vehicle = vehicle;
    this.turret = turret;
    this.index = index;
    this.role = role;
    this.since = time;
    this.tick = tick;
    this.was = [0, 0]; // button counts at the last batch, for press edges
    this.exitRequest = null; // { post, promote, placement } until the next batch takes it
    this.released = false;
    this.input = new SeatInput(); // this update's frozen intent (idle until the first batch)
    this.controllable = null; // the seat as the AI drives it (SeatUnit), for a bot
    const lease = this;
    // Beam motion belongs to this particular occupancy, not to the shooter
    // on foot or a replacement gunner who takes the same physical seat.
    this.emitter = {
      get matrix() {
        if (lease.vehicle) return lease.vehicle.matrix;
        const turret = lease.turret;
        turret?.rig?.root?.updateMatrixWorld?.(true);
        return (turret?.gun || turret?.rig?.root)?.matrixWorld ?? turret?.matrix;
      },
      get alive() { return lease.live && Boolean((lease.vehicle || lease.turret)?.alive); },
      viewDirection(out) {
        if (lease.turret) { lease.turret.muzzle(beamMuzzle, out); return out; }
        return lease.forward(out);
      },
    };
  }

  // Whose intent the seat plays: the seat's own controller's when one drives
  // it (the original game gives every occupied control its own controller),
  // else the occupant's.
  get intent() {
    const seat = this.controllable;
    return seat?.controller ? seat.intent : this.unit.intent;
  }

  // Whether this lease still holds: not let go, and (for a bot) the same
  // life of its unit, alive, still sitting here.
  get live() {
    if (this.released) return false;
    if (this.human) return true;
    const unit = this.unit;
    return Boolean(unit && unit.alive && !unit.dead && unit.generation === this.generation && unit.seat === this);
  }

  // Whether rounds meet the body on this seat: a vehicle seat whose rider
  // shows in the open (Vehicle.exposedSeat) or a turret whose gunner shows
  // (Turret.exposedSeat), whoever sits there, the player, a bot or a
  // network human.
  get exposed() {
    return !this.released && Boolean((this.vehicle || this.turret)?.exposedSeat?.(this.index));
  }

  // Whether a blast hurts him here (game.js onExplode): only as a turret's
  // shown gunner. The 2004 game's soldier area damage passes
  // over anyone seated unless the seat is an armed building's, and its
  // push moves no one seated (Combat.explode).
  get blastable() {
    return Boolean(this.turret) && this.exposed;
  }

  // The rider's hit shapes this tick, written to shapes (Vehicle.riderShapes,
  // Turret.riderShapes): their count.
  riderShapes(unit, shapes) {
    return (this.vehicle || this.turret)?.riderShapes?.(this.index, unit, shapes) ?? 0;
  }

  // The seat's eye (a turret's gun pivot) and its aim: what the unit sees
  // and fires from while it sits (docs/ai.md unit.origin / unit.forward).
  origin(out) {
    if (this.vehicle) return this.vehicle.seatOrigin(this.index, out);
    if (this.turret) return turretOrigin(this.turret, out);
    return out;
  }

  forward(out) {
    if (this.vehicle) return this.vehicle.seatForward(this.index, out);
    if (this.turret) return this.turret.viewDirection(out);
    return out.set(0, 0, 1);
  }
}
const beamMuzzle = new THREE.Vector3();

// One controller tick of a bot's intent as a seat plays it. Field names
// match the human's vehicle input ({ forward, right, turn, brake, up, down })
// so a bot pilot's batch drives the hull through the same code.
export class SeatInput {
  constructor() {
    this.forward = 0;
    this.right = 0;
    this.turn = 0;
    this.pitch = 0;
    this.brake = false;
    this.up = 0;
    this.down = 0;
    this.aim = new THREE.Vector3(0, 0, 1);
    this.aiming = false;
    this.held = [false, false];
    this.pressed = [false, false];
    this.reload = false;
    this.exit = false;
  }

}

// A body let go where it rode (a rider killed on his seat, src/boarding.js
// leaveKilled), its feet at `position`: at rest, falling from there, drawn
// there at once. unseatBody has let go of the seat's pose already. mount:
// a body it falls through (a riding animal's collision, which encloses its
// rider: Vehicle.shieldsRiders), so it drops off the animal to the ground.
export function dropBody(unit, position, mount = null) {
  const s = unit.soldier;
  if (!s?.position) return;
  s.passThrough = mount;
  s.position.copy(position);
  s.previous?.copy(position);
  s.settled?.copy(position);
  s.velocity?.set(0, 0, 0);
  s.grounded = false;
  unit.presentation?.reset?.();
  unit.actor?.root?.position?.copy?.(position);
  if (unit.tickCount !== undefined) unit.tickCount += 1;
}

// Into a seat: the body stops being a soldier on foot until it leaves. It
// is no sight target while it sits (perception targets the vehicle); the
// original game takes a boarding soldier out of collision the same way.
export function seatBody(unit) {
  unit.targetable = false;
  const combat = unit.combat;
  if (combat) {
    combat.mounted = true;
    combat.zoomed = false;
    combat.gunUnderwater = false;
    combat.aimOverride = null;
  }
  const s = unit.soldier;
  if (s) {
    s.air = null;
    s.velocity?.set(0, 0, 0);
  }
}

export function unseatBody(unit) {
  unit.targetable = true;
  if (unit.combat) { unit.combat.mounted = false; unit.combat.aimOverride = null; unit.combat.zoomed = false; }
  const soldier = unit.soldier;
  if (soldier) {
    // poseRider copied the hull's velocity into this body. Never carry it
    // into walking, a death animation, or a newly spawned infantry body.
    soldier.velocity?.set(0, 0, 0);
    soldier.air = null; soldier.dive = 0; soldier.lock = 0;
    soldier.lookX = soldier.lookY = 0; soldier.grounded = false;
    if (soldier.position) { soldier.previous?.copy(soldier.position); soldier.settled?.copy(soldier.position); }
    const limits = soldier.pitchRange?.() || [-1.4, 1.4];
    soldier.aimPitch = Math.max(limits[0], Math.min(limits[1], soldier.aimPitch || 0));
  }
  unit.animator?.gunner?.(null);
  // A seat turned the body with its node (the hull's heading, a reclined
  // seat's lean, an animal's tilt); out of it, every way out (a destroyed
  // hull's crew included), it stands upright, facing its aim.
  unit.actor?.root?.rotation?.set?.(0, unit.soldier?.aimYaw ?? 0, 0);
}

const scratchCenter = new THREE.Vector3();

// A vehicle as the AI's perception sees it: one entry in the units
// registry (src/registry.js), with the fields docs/ai.md gives units. It has
// no hitSpheres: rounds strike vehicles through their collision bodies, and
// the registry's soldier raycast skips it.
export class VehicleTarget {
  constructor(vehicle) {
    this.vehicle = vehicle;
    this.id = 0;
    this.generation = 0;
    this.kind = vehicleKind(vehicle.base);
    this.className = vehicle.name;
    this.classDef = vehicle.def;
    this.human = false;
    this.hero = false;
    this.walker = false;
    this.character = null;
    this.controller = null;
    this.seat = null;
    this.bounds = { center: new THREE.Vector3(), radius: vehicle.boundRadius };
    const target = this;
    this.combat = {
      get health() { return target.vehicle.health; },
      get maxHealth() { return target.vehicle.maxHealth; },
      get dead() { return !target.vehicle.alive; },
      get team() { return target.team; },
      // Blasts reach vehicles through Vehicles.blast, not here.
      explode() {},
    };
  }

  get alive() { return this.vehicle.alive; }
  get dead() { return !this.vehicle.alive; }
  // The target category weapon masks test: a creature is
  // an animal (1), any other hull a vehicle (3).
  get category() { return this.vehicle.healthType === "animal" ? 1 : 3; }
  // The team in control: its first occupant's (pilot first), else its own.
  get team() { return this.vehicle.controllingTeam; }
  get position() { return this.vehicle.position; }
  get velocity() { return this.vehicle.velocity; }

  get sphere() {
    this.vehicle.sphere(this.bounds.center);
    this.bounds.radius = this.vehicle.boundRadius;
    return this.bounds;
  }

  // The middle of the hull's collision bounds (provisional: the 2004 game's
  // rule for a vehicle's aim point is not known).
  aimPoint(out) {
    this.vehicle.sphere(out);
    return out;
  }

  origin(out) { return this.vehicle.seatOrigin(0, out); }
  forward(out) { return this.vehicle.seatForward(0, out); }
}

// A setting by name, or by the name's hash when the extractor could only
// keep the hash (hashes are the engine's FNV-1a over bytes OR 0x20).
function fnv(name) {
  let h = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    h ^= (name.charCodeAt(i) & 0xff) | 0x20;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}
export function setting(settings, name) {
  if (!settings) return undefined;
  return settings[name] ?? settings[fnv(name)];
}
function numberOf(value, fallback) {
  const n = Number.parseFloat(Array.isArray(value) ? value[0] : value);
  return Number.isFinite(n) ? n : fallback;
}

// Shared AI class metadata. An aircraft's dynamics come from its live flight
// configuration (its own ODF speeds and handling under its class's tuning);
// a ground unit's from its imported movement parameters.
const classViews = new WeakMap();
export function vehicleClassView(vehicle) {
  let view = classViews.get(vehicle.def);
  if (view && vehicle.base !== 'flyer') return view;
  const s = vehicle.def.settings || {};
  const get = (name, fallback) => numberOf(setting(s, name), fallback);
  const base = vehicle.base;
  const b = vehicle.bounds;
  view = {
    name: vehicle.name,
    base,
    VehicleType: String(setting(s, "VehicleType") ?? "").toLowerCase() || null,
    // The extractor files class keys authored after the last section under
    // that section's settings, so look there too.
    AISizeType: setting(s, "AISizeType") ?? vehicle.def.seats?.map((seat) => setting(seat.settings, "AISizeType")).find((v) => v !== undefined)
      ?? (base === "flyer" ? "FLYER" : base === "hover" ? "HOVER" : undefined),
    MaxHealth: get("MaxHealth", vehicle.maxHealth),
    bounds: [[b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]],
  };
  if (base === "flyer") {
    const c = vehicle.flightConfig;
    Object.assign(view, {
      MinSpeed: c.minSpeed, MidSpeed: c.cruiseSpeed, MaxSpeed: c.maxSpeed,
      StrafeSpeed: 0, TurnRate: c.turnRate, PitchRate: c.pitchRate,
      LandingTime: c.landingTime, LandingSpeed: c.landingSpeed, LandedHeight: c.landedHeight,
    });
  } else if (base === "walker" || base === "commandwalker") {
    const turn = get("MaxTurnSpeed", 2);
    Object.assign(view, {
      MaxSpeed: get("MaxSpeed", 20), MaxTurnSpeed: turn,
      MaxYawSpeed: setting(s, "MaxYawSpeed") === undefined ? 1 : Math.max(get("MaxYawSpeed", 1), turn),
      MaxPitchSpeed: get("MaxPitchSpeed", 0.5), StrafeSpeed: 0, TurnRate: turn, PitchRate: get("MaxPitchSpeed", 0.5),
    });
  } else {
    Object.assign(view, {
      ForwardSpeed: get("ForwardSpeed", vehicle.speed), StrafeSpeed: get("StrafeSpeed", 0),
      TurnRate: get("TurnRate", vehicle.turnRate), PitchRate: get("PitchRate", 1),
      MovingTurnOnly: get("MovingTurnOnly", 0) !== 0,
    });
  }
  classViews.set(vehicle.def, view);
  return view;
}

// A seat's weapons as the AI's weapon code reads an arsenal: weapons, the
// selected one per channel (a seat's channel c is its weapon c).
function seatArsenal(weapons, lease) {
  weapons.forEach((w, i) => { if (w.channel === undefined || w.channel === null) w.channel = i; });
  const channels = [weapons[0] ? [weapons[0]] : [], weapons[1] ? [weapons[1]] : []];
  return {
    weapons,
    channels,
    selected: [0, 0],
    // Classic attaches its shared generator after the board command returns.
    // Owned mounted weapons receive that scalar before any later publication.
    setRandom(draw) {
      for (let i = 0; i < weapons.length; i++) weapons[i].random = draw;
      lease.unit.bindMountedRandom(lease,draw);
    },
    current(channel = 0) { return channels[channel ?? 0]?.[0] || null; },
    select(weapon) { return weapons.includes(weapon); },
  };
}

// The seat as the AI drives it: the original game gives every occupied control
// (a vehicle's pilot seat, a gunner's or passenger's slot, a turret) its
// own controller with its own state machine (UnitMobileController,
// UnitFlyController, UnitStationaryController). A SeatUnit is that control
// in the units interface (docs/ai.md): the pilot seat classifies as the
// vehicle (2 walker, 5 flyer, 6 hover), any other seat and a turret as 7
// (mounted turret / passenger slot), with zero nominal speed. It has its
// own intent, which the seat plays once a controller drives it. One per
// lease and seat: a bot moved to another seat gets a new one (the
// original game attaches the pilot to the new control afresh).
export class SeatUnit {
  constructor(lease) {this.initialize(lease);}
  // Native births bind identity before F2 decodes lease references. The same
  // cold reader setup then fills that existing facade without replacing it.
  initialize(lease) {
    this.lease = lease;
    this.seat = lease;
    this.vehicle = lease.vehicle;
    this.turret = lease.turret;
    this.index = lease.index;
    this.occupant = lease.unit;
    this.id = lease.unit?.id ?? 0;
    this.generation = 1;
    this.human = false;
    this.hero = Boolean(lease.unit?.hero);
    this.walker = false;
    this.removed = false;
    this.targetable = false; // perception targets the vehicle or turret
    this.controller = null;
    this.intent = blankSeatIntent();
    this.className = lease.vehicle?.name ?? lease.turret?.desc?.class ?? "turret";
    const pilot = Boolean(lease.vehicle) && lease.index === 0;
    this.kind = pilot ? vehicleKind(lease.vehicle.base) : KIND.mounted;
    const view = lease.vehicle ? vehicleClassView(lease.vehicle) : { base: "armedbuilding", MaxHealth: lease.turret.maxHealth };
    // Only the pilot seat moves: every other control has zero nominal speed.
    this.classDef = pilot ? view : { ...view, MaxSpeed: 0, ForwardSpeed: 0, StrafeSpeed: 0, TurnRate: turretRate(lease), PitchRate: turretRate(lease) };
    if (lease.vehicle) {
      this.navRadius = Math.hypot(lease.vehicle.size.x, lease.vehicle.size.z) * (this.kind === KIND.hover ? 0.5 : 0.25);
    }
    const weapons = lease.vehicle ? lease.vehicle.seats[lease.index].weapons.map((w) => w.state) : lease.turret.weapon ? [lease.turret.weapon] : [];
    const unit = this;
    const arsenal = seatArsenal(weapons,lease);
    this.combat = {
      arsenal,
      mounted: true,
      aimOverride: null,
      get team() { return unit.team; },
      get health() { return unit.vehicle ? unit.vehicle.health : unit.turret.health; },
      get maxHealth() { return unit.vehicle ? unit.vehicle.maxHealth : unit.turret.maxHealth; },
      get dead() { return !unit.alive; },
      explode() {},
    };
    const speed = pilot ? (view.ForwardSpeed ?? view.MaxSpeed ?? 0) : 0;
    this.soldier = {
      get position() { return unit.position; },
      get velocity() { return unit.velocity; },
      get aimYaw() { return unit.aimYaw; },
      get aimPitch() { return unit.aimPitch; },
      maxSpeed: speed,
      maxStrafe: pilot ? (view.StrafeSpeed ?? 0) : 0,
      maxTurn: this.classDef.TurnRate ?? 2,
      maxPitch: this.classDef.PitchRate ?? 1,
      posture: "stand",
      air: null,
      grounded: true,
    };
  }

  // This proxy carries AI planner state, independently of the occupant's
  // canonical body. It never masquerades as a Unit slab reference.
  bindController(controller) { this.controller=controller; }

  get alive() {
    const body = this.vehicle || this.turret;
    return this.lease.live && Boolean(body?.alive) && this.lease.index === this.index;
  }

  get dead() { return !this.alive; }
  get team() { return this.lease.team; }
  get character() { return this.lease.unit?.character ?? null; }
  get position() { return (this.vehicle || this.turret).position; }
  get velocity() { return this.vehicle ? this.vehicle.velocity : ZERO; }
  // The physical body this control belongs to: the hull, or the armed
  // building (the original game's associated physical entity).
  get body() { return this.vehicle || this.turret; }
  get pilot() { return Boolean(this.vehicle) && this.index === 0; }

  get aimYaw() {
    if (this.vehicle) return this.vehicle.seats[this.index].aimYaw;
    return this.turret.facing + this.turret.yaw;
  }

  get aimPitch() {
    if (this.vehicle) return this.vehicle.seats[this.index].pitch;
    return this.turret.pitch;
  }

  // The hull's own heading (the turret drum's for a turret).
  get bodyYaw() {
    return this.vehicle ? this.vehicle.yaw : this.turret.facing + this.turret.yaw;
  }

  get sphere() {
    if (this.vehicle) return this.vehicle.aiTarget.sphere;
    this.turret.aiTarget ||= new TurretTarget(this.turret);
    return this.turret.aiTarget.sphere;
  }

  aimPoint(out) {
    if (this.vehicle) return this.vehicle.aiTarget.aimPoint(out);
    this.turret.sphere(out);
    return out;
  }

  origin(out) { return this.lease.origin(out); }
  forward(out) { return this.lease.forward(out); }
}

function turretRate(lease) {
  if (lease.turret) return lease.turret.turnRate ?? 2;
  const settings = lease.vehicle.seats[lease.index]?.settings || {};
  return numberOf(setting(settings, "TurnRate"), numberOf(setting(lease.vehicle.def.settings, "TurnRate"), lease.vehicle.turnRate));
}

// A turret (armed building) as perception sees it: classifier 8.
export class TurretTarget {
  constructor(turret) {
    this.turret = turret;
    this.id = 0;
    // First life counts as generation 1, like a vehicle spawn and SeatUnit:
    // start-built turrets (Bespin AAA, no unbuilt look) never pass through
    // Turret.built(), the only bump, and the mounted-control lease rejects
    // generation 0, so nobody could ever man them.
    this.generation = 1;
    this.kind = KIND.armedBuilding;
    this.category = 4; // a building (only a built one is a target)
    this.className = turret.desc.class || turret.desc.model || "turret";
    this.classDef = turret.desc;
    this.human = false;
    this.hero = false;
    this.walker = false;
    this.character = null;
    this.controller = null;
    this.seat = null;
    this.bounds = { center: new THREE.Vector3(), radius: 1 };
    const target = this;
    this.combat = {
      get health() { return target.turret.health; },
      get maxHealth() { return target.turret.maxHealth; },
      get dead() { return !target.turret.alive; },
      get team() { return target.team; },
      explode() {},
    };
  }

  get alive() { return this.turret.alive; }
  get dead() { return !this.turret.alive; }
  get team() { return this.turret.lease?.team ?? this.turret.team ?? 0; }
  get position() { return this.turret.position; }
  get velocity() { return ZERO; }

  get sphere() {
    this.bounds.radius = this.turret.sphere(this.bounds.center);
    return this.bounds;
  }

  aimPoint(out) {
    this.turret.sphere(out);
    return out;
  }

  origin(out) { return turretOrigin(this.turret, out); }
  forward(out) { return this.turret.viewDirection(out); }
}
const ZERO = new THREE.Vector3();

// A turret's gun pivot: the gun node, else its fire point, else the drum's
// bounding centre.
export function turretOrigin(turret, out) {
  if (turret.f2Seats) return out.copy(turret.f2Pose.origin);
  const node = turret.gun || turret.firePoint;
  if (node) {
    turret.rig?.root?.updateMatrixWorld?.(true);
    return node.getWorldPosition(out);
  }
  return out.set(0, 1.65, 0.32).applyMatrix4(turret.matrix);
}

// The point a person's aim at a turret meets (the original game's player aim
// target): along `direction` from Turret.aimOrigin, the first wall, prop,
// hull or soldier (`exclude`, him, left out; the turret's own body too)
// within 512 m, else 512 m out. Written to out; the Gunner (src/gunner.js)
// and TurretSeats.drive both aim with it, so the page and the server turn
// the gun to the same point. hit, unitHit: scratch.
const aimFrom = new THREE.Vector3();
const aimUnitHit = makeUnitHit();
// yaw, pitch: the aim's (the turret's own by default; a page passes the one
// it shows).
export function turretAimPoint(turret, firstPerson, direction, collision, units, exclude, out, hit, yaw = turret.yaw, pitch = turret.pitch) {
  turret.aimOrigin(aimFrom, firstPerson, yaw, pitch);
  const wall = collision?.cast(aimFrom, direction, AIM_REACH, hit, turret.bodies?.built || null);
  let far = wall ? wall.distance : AIM_REACH;
  const unit = units?.raycast?.(aimFrom, direction, far, exclude, aimUnitHit);
  if (unit) far = unit.distance;
  return out.copy(aimFrom).addScaledVector(direction, far);
}

// Stationary crew facades and page presentation. The native seat owner installs
// scalar commands at adoption and runs each crew's aim, pose and fire in order.
// This object retains the registry, foreign hooks and replicated lease views.
const AIM_REACH = 512;

const EMPTY_TURRETS = Object.freeze([]);
export class TurretSeats {
  constructor({ live = null, ordnance = null, collision = null, units = null, onFired = null, onBoard = null, onUnboard = null } = {}) {
    this.live = live;
    this.ordnance = ordnance;
    this.collision = collision;
    this.units = null;
    this.onFired = onFired;
    this.onBoard = onBoard;
    this.onUnboard = onUnboard;
    this.leases = [];
    this.time = 0;
    this.ticks = 0;
    this.seatRandom = null; // () => [0, 1): the world's seat stream (seatWeaponPolicy)
    // Everything the crews ask of the rest of the battle (src/seats-world.js).
    this.world = new SeatsWorld(this);
    for (const turret of this.turrets) turret.crew = this;
    if (units) this.attachUnits(units);
  }

  get turrets() {
    return this.live?.turrets || EMPTY_TURRETS;
  }

  // Registers every turret with the units registry as a classifier-8 target.
  attachUnits(units) {
    this.units = units;
    for (const turret of this.turrets) {
      turret.aiTarget ||= new TurretTarget(turret);
      if (units.byId(turret.aiTarget.id) !== turret.aiTarget) units.add(turret.aiTarget);
    }
  }

  seatOf(unit) {
    const lease = unit?.seat;
    return lease?.turret && this.leases.includes(lease) ? lease : null;
  }

  // Whether the unit may take this turret now: a living soldier (not a
  // droideka) on foot, a built turret nobody mans. The approach and the
  // decision are the AI's.
  canMan(unit, turret) {
    return Boolean(unit && turret && unit.alive && !unit.dead && (!unit.human || unit.networkHuman) && !unit.seat && !unit.walker
      && Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0) === 0
      && turret.alive && !turret.gunner && !turret.lease);
  }

  dispose() {
    for (const lease of [...this.leases]) this.vacate(lease, { placement: false });
    if (this.units) for (const turret of this.turrets) if (turret.aiTarget) this.units.remove(turret.aiTarget);
  }
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}
