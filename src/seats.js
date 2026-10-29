import { personLease } from "./boarding.js";
import { makeUnitHit } from "./registry.js";
import { updateWeaponLock, hasMissileLock, seatLockTarget } from "./weapon-targeting.js";
import { bindMountedRound } from "./muzzles.js";
import * as THREE from "three";
import { emitSound } from "./audio/events.js";

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
// and random source to the player's defaults when a human takes the seat;
// the Classic combat binding installs the bot's random source on bind.
export function seatWeaponPolicy(weapon, human) {
  if (!weapon) return;
  weapon.floor = human ? -(weapon.def.SpreadThreshold || 0) : 0;
  if (weapon.kick < weapon.floor) weapon.kick = weapon.floor;
  weapon.spreadScale = 1;
  weapon.sniperScale = 0;
  weapon.yawSpreadOverride = null;
  weapon.random = null;
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
    this.lean = 0; // a visible rider's sway (presentation)
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

  // Freeze the unit's intent for this batch. Held analogs and buttons are
  // copied; edges are taken from the intent (cleared there) so they happen
  // once. Edges a seat has no use for (jump, crouch, prone, weapon cycling,
  // zoom) are dropped rather than left to fire when the unit is on foot.
  latch(lease) {
    const intent = lease.intent;
    this.forward = clamp(intent.forward || 0, -1, 1);
    this.right = clamp(intent.right || 0, -1, 1);
    const aimLimit = lease.turret && lease.unit?.networkHuman ? 16 : 1;
    this.turn = clamp(intent.turn || 0, -aimLimit, aimLimit);
    this.pitch = clamp(intent.pitch || 0, -aimLimit, aimLimit);
    this.brake = Boolean(intent.brake);
    this.trick = Boolean(intent.trick); this.immelmann = Boolean(intent.immelmann); this.boost = Boolean(intent.boost);
    this.up = intent.up ? 1 : 0;
    this.down = intent.down ? 1 : 0;
    const aim = intent.aim;
    this.aiming = Boolean(aim && aim.lengthSq() > 1e-12);
    if (this.aiming) this.aim.copy(aim).normalize();
    const buttons = intent.buttons || ZERO_BUTTONS;
    for (let c = 0; c < 2; c++) {
      const held = buttons[c] > 0;
      this.held[c] = held;
      this.pressed[c] = held && !(lease.was[c] > 0);
      lease.was[c] = buttons[c];
    }
    this.reload = Boolean(intent.reload);
    this.exit = Boolean(intent.use || intent.exit);
    intent.reload = intent.use = intent.exit = false;
    intent.jump = intent.crouch = intent.prone = false;
    intent.nextPrimary = intent.previousPrimary = intent.nextSecondary = intent.zoom = false;
    return this;
  }

  // A trigger press, once.
  take(channel) {
    const pressed = this.pressed[channel];
    this.pressed[channel] = false;
    return pressed;
  }

  takeReload() {
    const reload = this.reload;
    this.reload = false;
    return reload;
  }
}

// The 2004 game's seat exit placement (the soldier's exit callback), for
// Classic bots. Nine candidates around the vehicle in the occupied
// seat's frame, at R = the vehicle's bounding radius + 0.684: behind, left,
// right, the four diagonals (0.7071 R each way), in front, and the vehicle's
// own spot, each 1 m up. The first eight need a clear line from the
// vehicle's bounding centre (the vehicle itself left out of that line).
// A candidate is accepted if any of five support samples (itself, then
// +-0.72 m along the world axes) finds a floor: a 5 m ray up (the sample
// rises to just under what it meets, or 4.9 m), then a 10 m ray down that
// must hit. The soldier goes to the candidate's X/Z at the highest support
// found (the first sample wins ties). If no candidate has support, the
// vehicle's origin. The geometry rays see terrain, buildings, props and
// vehicles, not water; nothing checks for drowning, falls or death
// regions, which is the Classic behaviour (the Enhanced safe exit is a
// different policy). Arithmetic is float32 where the 2004 game's constants
// are, so the candidate layout matches it bit for bit.
//
// frame: { origin, forward, up? } the occupied seat's frame; center/radius:
// the vehicle's bounding sphere; body: its collision body (left out of the
// radial lines only). Returns out = { position, forward, candidate } where
// candidate is the accepted index 0..8, or -1 for the fallback.
const EXIT_EXTRA = f32(0.6840001);
const EXIT_DIAGONAL = f32(0.7071);
const SUPPORT_STEP = f32(0.72);
const SUPPORT_UP = 5;
const SUPPORT_DOWN = 10;

export function exitCandidates(radius, out = []) {
  const r = f32(f32(radius) + EXIT_EXTRA);
  const d = f32(EXIT_DIAGONAL * r);
  const rows = [[0, 1, -r], [-r, 1, 0], [r, 1, 0], [-d, 1, -d], [d, 1, -d], [d, 1, d], [-d, 1, d], [0, 1, r], [0, 1, 0]];
  out.length = 0;
  for (const row of rows) out.push(row);
  return out;
}

export function exitSupportOffsets() {
  const s = SUPPORT_STEP;
  return [[0, 0, 0], [s, 0, s], [s, 0, -s], [-s, 0, s], [-s, 0, -s]];
}

// Of support heights (null where a sample found none), the index of the
// highest; equal heights keep the earlier sample. -1 when none.
export function highestSupport(heights) {
  let best = -1;
  for (let i = 0; i < heights.length; i++) {
    if (heights[i] === null || heights[i] === undefined) continue;
    if (best < 0 || heights[i] > heights[best]) best = i;
  }
  return best;
}

const exitScratch = {
  right: new THREE.Vector3(), up: new THREE.Vector3(), forward: new THREE.Vector3(),
  point: new THREE.Vector3(), sample: new THREE.Vector3(), dir: new THREE.Vector3(),
  hit: { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null },
  candidates: [], heights: [0, 0, 0, 0, 0],
};

export function classicSeatExit(collision, { origin, forward, up = UP, center, radius, body = null }, out = { position: new THREE.Vector3(), forward: new THREE.Vector3(), candidate: -1 }) {
  const s = exitScratch;
  // The orthonormal frame: up, right = up x forward, forward = right x up.
  s.up.copy(up).normalize();
  s.right.crossVectors(s.up, forward);
  if (s.right.lengthSq() < 1e-12) s.right.set(1, 0, 0);
  s.right.normalize();
  s.forward.crossVectors(s.right, s.up);
  out.forward.copy(s.forward);
  out.candidate = -1;
  out.position.copy(origin);
  const offsets = exitSupportOffsets();
  const candidates = exitCandidates(radius, s.candidates);
  for (let k = 0; k < candidates.length; k++) {
    const [x, y, z] = candidates[k];
    s.point.copy(origin).addScaledVector(s.right, x).addScaledVector(s.up, y).addScaledVector(s.forward, z);
    if (k < 8) {
      s.dir.subVectors(s.point, center);
      const span = s.dir.length();
      if (span > 1e-6 && collision.cast(center, s.dir.divideScalar(span), span, s.hit, body)) continue;
    }
    let found = false;
    for (let i = 0; i < offsets.length; i++) {
      const [dx, , dz] = offsets[i];
      s.sample.set(s.point.x + dx, s.point.y, s.point.z + dz);
      const above = collision.cast(s.sample, UP, SUPPORT_UP, s.hit);
      s.sample.y += (above ? s.hit.distance : SUPPORT_UP) - 0.1;
      const below = collision.cast(s.sample, DOWN, SUPPORT_DOWN, s.hit);
      if (below && s.hit.distance < SUPPORT_DOWN) {
        s.heights[i] = s.sample.y - s.hit.distance;
        found = true;
      } else s.heights[i] = null;
    }
    if (!found) continue;
    out.position.set(s.point.x, s.heights[highestSupport(s.heights)], s.point.z);
    out.candidate = k;
    return out;
  }
  return out;
}

// Puts a unit's body down where a seat let it out: standing, at rest
// (the original game zeroes the exit velocity), facing yaw. Its jet fuel is
// kept: leaving a seat is not a respawn. midair: out of a craft in flight,
// it is left where it was put, falling, unless the ground is right there
// (Soldier.spawn would set it on whatever lies below).
export function placeOnFoot(unit, position, yaw, { midair = false } = {}) {
  const s = unit.soldier;
  const fuel = s.fuel;
  s.spawn(position.x, position.y, position.z, yaw);
  if (midair && (!s.grounded || s.position.y < position.y - 0.5)) {
    s.position.copy(position);
    s.previous?.copy(position);
    s.settled?.copy(position);
    s.grounded = false;
    s.setAir?.("fall");
  }
  if (fuel !== undefined) s.fuel = fuel;
  s.velocity?.set(0, 0, 0);
  unit.presentation?.reset?.();
  unit.animator?.gunner?.(null);
  unit.animator?.reset?.(yaw);
  if (unit.actor?.root) {
    unit.actor.root.visible = true;
    unit.actor.root.rotation?.set?.(0, yaw, 0);
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
function seatArsenal(weapons) {
  weapons.forEach((w, i) => { if (w.channel === undefined || w.channel === null) w.channel = i; });
  const channels = [weapons[0] ? [weapons[0]] : [], weapons[1] ? [weapons[1]] : []];
  return {
    weapons,
    channels,
    selected: [0, 0],
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
  constructor(lease) {
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
    const arsenal = seatArsenal(weapons);
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

// The bots who man turrets (props.js armed buildings). The player keeps the
// Gunner adapter (src/gunner.js); this is the same turret driven from a
// unit's intent: the turn and pitch analogs turn the drum at its
// MaxTurnSpeed (as the mouse rates do), the gun follows the point the bot
// aims at (its intent.aim, else the drum's view) at the gun's own rates,
// and the trigger fires the turret's weapon from its fire point along the
// gun, credited to the unit.
//
// One update per rendered frame, after LiveObjects.update (main.js wiring
// in docs/ai-vehicles.md).
const AIM_REACH = 512;

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
    this.exit = { position: new THREE.Vector3(), forward: new THREE.Vector3(), candidate: -1 };
    this.seatPoint = new THREE.Vector3();
    this.dropPoint = new THREE.Vector3();
    this.riderPose = { clip: null, pitch: 0, turn: 0, time: null };
    this.seatTurn = new THREE.Quaternion();
    this.view = new THREE.Vector3();
    this.muzzle = new THREE.Vector3();
    this.gunDir = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    for (const turret of this.turrets) turret.crew = this;
    if (units) this.attachUnits(units);
  }

  get turrets() {
    return this.live?.turrets || [];
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

  man(unit, turret) {
    // A person takes a bot's turret over, as he takes a vehicle's seat
    // (boarding.js claimSeat): the bot is set down beside it first.
    if (unit?.networkHuman && turret?.lease && !turret.lease.released && !personLease(turret.lease)) this.vacate(turret.lease);
    if (!this.canMan(unit, turret)) return false;
    const lease = new SeatLease({ unit, team: unit.team, turret, index: 0, role: GUNNER, time: this.time, tick: this.ticks });
    turret.lease = lease;
    turret.gunner = unit.soldier;
    turret.crew = this;
    turret.yawVel = turret.pitchVel = 0;
    seatWeaponPolicy(turret.weapon, Boolean(unit.networkHuman));
    if (!unit.networkHuman) lease.controllable = new SeatUnit(lease);
    unit.seat = lease;
    seatBody(unit);
    this.leases.push(lease);
    this.pose(lease, 0);
    this.onBoard?.(unit, turret, 0);
    return true;
  }

  // The unit's exit request, taken at the next update (as in the 2004 game).
  requestExit(unit, { post = null, placement = true } = {}) {
    const lease = this.seatOf(unit);
    if (!lease) return false;
    lease.exitRequest = { post, placement };
    return true;
  }

  // Out of the turret now. placement false leaves the body where it is.
  leave(unit, { placement = true, post = null } = {}) {
    const lease = this.seatOf(unit);
    if (!lease) return false;
    this.vacate(lease, { placement, post });
    return true;
  }

  // Out now. kill: the turret was destroyed under him, and he dies,
  // credited to attacker. reason 'killed' (src/boarding.js leaveKilled): he
  // died at the gun. Either way his body falls from where he stood, as a
  // vehicle rider's does (Vehicles.vacate), and shows again if the turret
  // hid it (Turret.hidesGunner).
  vacate(lease, { placement = true, post = null, kill = false, attacker = null, reason = "exit" } = {}) {
    if (lease.released) return;
    const turret = lease.turret;
    const unit = lease.unit;
    const died = kill || reason === "killed";
    const fall = died && unit?.soldier ? turret.riderDrop?.(0, unit, this.dropPoint) ?? null : null;
    let yaw = null;
    if (placement && !died && this.collision && unit?.soldier) {
      const radius = turret.sphere(scratchCenter);
      const facing = turret.facing + turret.yaw;
      this.view.set(Math.sin(facing), 0, Math.cos(facing));
      classicSeatExit(this.collision, { origin: turret.position, forward: this.view, center: scratchCenter, radius, body: turret.bodies?.built || null }, this.exit);
      yaw = Math.atan2(this.exit.forward.x, this.exit.forward.z);
    }
    lease.released = true;
    if (turret.lease === lease) {
      turret.lease = null;
      turret.gunner = null;
    }
    const i = this.leases.indexOf(lease);
    if (i >= 0) this.leases.splice(i, 1);
    if (unit && unit.seat === lease) {
      unit.seat = null;
      unseatBody(unit);
      if (yaw !== null) placeOnFoot(unit, this.exit.position, yaw);
      else if (turret.hidesGunner && unit.actor?.root && !unit.removed && !unit.combat?.blowApart) unit.actor.root.visible = true;
    }
    if (fall) dropBody(unit, fall);
    if (kill && unit && unit.alive && !unit.combat?.dead) unit.combat?.damage?.(Infinity, Infinity, null, attacker);
    if (!kill) this.onUnboard?.(unit, turret, 0, post, reason);
  }

  // A manned turret destroyed: whoever mans it dies, credited to whoever
  // destroyed it (props.js Building.destroyed calls this first).
  destroyed(turret) {
    const lease = turret.lease;
    if (!lease || lease.human) return;
    this.vacate(lease, { placement: false, kill: true, attacker: turret.lastAttacker ?? null });
  }

  update(dt) {
    this.time += dt;
    this.ticks += 1;
    for (const lease of [...this.leases]) {
      if (!lease.turret.alive) {
        this.vacate(lease, { placement: false, kill: true, attacker: lease.turret.lastAttacker ?? null });
        continue;
      }
      if (!lease.live) {
        this.vacate(lease, { placement: false });
        continue;
      }
      const input = lease.input.latch(lease);
      if (input.exit || lease.exitRequest) {
        const request = lease.exitRequest || {};
        this.vacate(lease, { placement: request.placement ?? true, post: request.post ?? null });
        continue;
      }
      this.drive(lease, dt);
    }
  }

  // One frame of a bot or a network human at the gun: the drum, the gun,
  // the body, the trigger.
  drive(lease, dt) {
    const turret = lease.turret;
    const input = lease.input;
    const person = Boolean(lease.unit?.networkHuman);
    // A positive turn analog turns right (yaw down), as for soldiers.
    turret.steer(dt, -input.turn, input.pitch, person ? lease.unit.combat?.zoom || 1 : 1);
    turret.present?.();
    turret.rig.root.updateMatrixWorld(true);
    turret.muzzle(this.muzzle);
    if (input.aiming) this.view.copy(input.aim);
    else turret.viewDirection(this.view);
    // The gun follows what lies under the aim. A person's runs from where
    // his own page aims (Turret.aimOrigin: his eye in first person, else his
    // chase camera's rest brought to the eye's plane), so the point it turns
    // to is the one under his crosshair; a bot's from the gun's fire point.
    if (person && turret.aimOrigin) turretAimPoint(turret, Boolean(lease.unit.combat?.firstPerson), this.view, this.collision, this.units, lease.unit, this.target, this.hit);
    else {
      const hit = this.collision?.cast(this.muzzle, this.view, AIM_REACH, this.hit, turret.bodies?.built || null);
      this.target.copy(this.muzzle).addScaledVector(this.view, hit ? hit.distance : AIM_REACH);
    }
    turret.pose(dt, this.target);
    this.pose(lease, dt);
    const weapon = turret.weapon;
    if (!weapon) return;
    const pressed = input.take(0);
    const trigger = { held: input.held[0] || pressed, pressed };
    if (hasMissileLock(weapon.def)) updateWeaponLock(weapon, dt, { units: this.ordnance?.units, collision: this.collision,
      origin: this.muzzle, dir: this.view, viewer: turret.aiTarget, ignoreBody: turret.bodies?.built,
      target: seatLockTarget(lease) });
    for (const shot of weapon.update(dt, true, trigger, input.takeReload(), { stick: 0, posture: "stand", zoomed: false })) {
      turret.muzzle(this.muzzle, this.gunDir, true);
      turret.fireDirection?.(this.muzzle, this.target, this.gunDir, this.gunDir);
      if (shot.yaw) this.gunDir.applyAxisAngle(UP, shot.yaw);
      if (shot.pitch) this.gunDir.applyAxisAngle(this.view.crossVectors(this.gunDir, UP).normalize(), shot.pitch);
      const round = this.ordnance?.fire(weapon.def.ordnance, this.muzzle, this.gunDir, lease.unit, null, turret.bodies?.built || null, lease.emitter, shot);
      bindMountedRound(round, turret, -1, -1, (turret.barrel - 1) % (turret.firePoints?.length || 1));
      emitSound("turretFire", turret, shot);
      this.onFired?.(turret, 0, shot, lease.unit, this.muzzle, this.gunDir);
    }
  }

  // The body at the drum's seat, turned as the seat node is (facing along
  // the gun, Turret.seatTurn), in the gunner's pose (Turret.riderSample),
  // out of sight where the turret hides its gunner. His hit shapes follow
  // the drum (Unit.updateSpheres) in the same pose and frame.
  pose(lease, dt) {
    const turret = lease.turret;
    const unit = lease.unit;
    const s = unit.soldier;
    if (turret.seatPoint) turret.seatPoint(this.seatPoint);
    else (turret.seat || turret.rig.root).getWorldPosition(this.seatPoint);
    s.position.copy(this.seatPoint);
    s.previous?.copy(this.seatPoint);
    s.settled?.copy(this.seatPoint);
    s.velocity.set(0, 0, 0);
    s.aimYaw = turret.seatYaw ? turret.seatYaw() : turret.facing + turret.yaw;
    s.aimPitch = turret.pitch;
    unit.presentation?.reset?.();
    if (unit.actor?.root) {
      unit.actor.root.position.copy(this.seatPoint);
      unit.actor.root.visible = !turret.hidesGunner;
    }
    const pose = turret.riderSample?.(unit.animator?.clips, this.riderPose);
    unit.animator?.gunner?.(pose?.clip ?? null, pose?.pitch, pose?.turn, pose?.time, turret.seatTurn?.(this.seatTurn) ?? null);
    if (unit.tickCount !== undefined) unit.tickCount += 1;
    return dt;
  }

  // The bots' turrets as a page showing a worker's battle draws them: the
  // drum and gun at the angles the tick gave (replicated with the turret's
  // state; the page never runs drive(), and its leases come with the seat
  // packet, onto the turrets). Once a frame, after the ticks. The player's
  // own is the Gunner's to pose, with his look preview.
  present() {
    for (const turret of this.turrets) if (turret.lease && !turret.lease.human) poseTurret(turret, turret.yaw);
  }

  dispose() {
    for (const lease of [...this.leases]) this.vacate(lease, { placement: false });
    if (this.units) for (const turret of this.turrets) if (turret.aiTarget) this.units.remove(turret.aiTarget);
  }
}

// The drum and gun nodes at a yaw and the gun's own angles, as Turret.pose
// leaves them, without stepping the gun: the page's pose of a turret whose
// aim it was given.
const poseQ = new THREE.Quaternion();
const poseE = new THREE.Euler(0, 0, 0, "YXZ");
export function poseTurret(turret, yaw, pitch = turret.pitch) {
  if (typeof turret.present === "function") { turret.present(yaw, pitch); return; }
  if (turret.drum && turret.drumBind) turret.drum.quaternion.copy(turret.drumBind).multiply(poseQ.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, yaw));
  if (turret.gun && turret.gunBind) {
    poseE.set(-(turret.gunPitch || 0), turret.gunYaw || 0, 0);
    turret.gun.quaternion.copy(turret.gunBind).multiply(poseQ.setFromEuler(poseE));
  }
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}
