import * as THREE from "three";
import { DONE, ROLE, roleOf } from "../controller.js";
import { forwardSpeed } from "../weapons-ai.js";
import { fallbackAnalogs } from "../scheduler.js";
import { UnitBoardAgent, UnitFlyAgent, UnitUnBoardAgent } from "./agents.js";
import { controllerPolicy, FLY_POLICY, MOBILE_POLICY, STATIONARY_POLICY } from "./controllers.js";
import { FlyGotoHelper } from "./fly.js";
import { VehicleNotifier } from "./notify.js";
import { enterPattern, exitPattern, isAtAt, sidestep, tickPattern, vehicleDispatch } from "./patterns.js";
import { freeCapacity, Reservations, SEAT } from "./reservations.js";
import { seatAnalogs, steerState, walkerContact } from "./steering.js";
import { registerService, serviceOf } from "./service.js";
import { outOfReach, pickSupport, REACH_MARGIN, supportHubs } from "./support.js";
import { captureRegion, cpCenter } from "../patrol.js";

// Classic vehicle AI: the services the Classic controller, agents, combat
// and scheduler call for anything to do with vehicles and turrets
// (docs/ai-vehicles.md). createClassicAI binds it through classicBinding();
// Combat reads its pattern hooks at world.vehicles.
//
// The gameplay side it drives: world.vehicleSystem, the Vehicles of
// src/vehicles.js (seats, boarding, the vehicles' registry entries), and
// world.turretSeats, the TurretSeats of src/seats.js (bots at turrets).
// It reaches them only through those objects (AI modules import three and
// each other only).
//
// While a bot sits, the control it occupies is a unit of its own, a seat
// unit (src/seats.js SeatUnit: unit.seat.controllable): the 2004 game
// gives every occupied control its own controller, and the classifier, the
// nominal speed, the weapons and the aim the AI reads are the control's.
// currentControllable(character) names it; the controller policies say how
// its controller maps goals to states (controllers.js).

export const agents = { board: UnitBoardAgent, unboard: UnitUnBoardAgent };
export { UnitBoardAgent, UnitUnBoardAgent, UnitFlyAgent, FlyGotoHelper, controllerPolicy, MOBILE_POLICY, STATIONARY_POLICY, FLY_POLICY };

export { serviceOf };

export function classicBinding(ai) {
  return serviceOf(ai) ?? registerService(ai, new VehicleAI(ai));
}

// The goal kind a mobile controller's state came from (the inverse of its
// map), for the adapter below.
const MOBILE_GOAL = new Map([[1, 1], [2, 2], [3, 3], [4, 4], [5, 5], [6, 6], [9, 8], [10, 9], [11, 10]]);

// A thin adapter, until the controller code consults these
// services itself (docs/ai-vehicles.md "Integration", hooks C1-C5): on one
// ClassicAI instance it
//   - makes a Character's controller drive its current control: the seat
//     unit while its body sits (a fresh bind, as the 2004 game builds a
//     new controller for the control), its body again once out;
//   - gives a seat's controller its mounted policy: goal-to-state mapping,
//     the flyer agent for state 7, the stationary idle-look flag;
//   - converts a seat's movement request with the vehicle families' input
//     consumers instead of the soldier's.
// Instance-level only: no sibling file or class is changed. Idempotent.
export function adaptClassicAI(ai) {
  if (ai.vehicleAdapter) return ai;
  const service = classicBinding(ai);
  ai.vehicleAdapter = true;
  const controllable = (character) => service.currentControllable(character) ?? character?.unit ?? null;
  // C1: follow the current control.
  ai.sync = function sync() {
    for (const [character, controller] of this.byCharacter) {
      const unit = controllable(character);
      if (controller.unit && (!unit || unit !== controller.unit || unit.generation !== controller.generation || !unit.alive)) this.died(character);
      if (!controller.unit && unit?.alive) this.spawned(character);
    }
  };
  ai.spawned = function spawned(character) {
    const controller = this.byCharacter.get(character) ?? this.attach(character);
    const unit = controllable(character);
    if (!controller || !unit?.alive) return null;
    if (controller.unit === unit && controller.generation === unit.generation) return controller;
    if (controller.unit) this.removeLive(controller);
    controller.bind(unit);
    adaptController(controller, unit);
    this.controllers.push(controller);
    return controller;
  };
  // C3: the agent for a mounted policy's own states.
  const agentFor = ai.agentFor.bind(ai);
  ai.agentFor = (state, controller) => controller.policy?.agentFor?.(state, controller) ?? agentFor(state, controller);
  // C5: the seat's input conversion, in both of the scheduler's paths:
  // Nav's navigationInput (executor step plus analogs) and the plain
  // analog conversion.
  const analogs = ai.analogs;
  ai.analogs = (unit, request, aim, dt, out, options = {}) => {
    if (unit?.lease?.controllable === unit) return service.analogs(unit, request, aim, dt, out, options);
    if (analogs) return analogs(unit, request, aim, dt, out, options);
    return fallbackAnalogs(unit, request, aim, dt, out, options.facing);
  };
  const navigationInput = ai.navigationInput;
  if (navigationInput) {
    const seatOut = { forward: 0, right: 0, turn: 0, pitch: 0 };
    ai.navigationInput = (control, unit, intent, dt, aim = null, options = {}) => {
      if (unit?.lease?.controllable !== unit) return navigationInput(control, unit, intent, dt, aim, options);
      control.attach(unit);
      control.update(dt);
      service.analogs(unit, control.request, aim, dt, seatOut, { ...options, controller: unit.controller, stepped: true });
      intent.forward = seatOut.forward;
      intent.right = seatOut.right;
      intent.turn = seatOut.turn;
      intent.pitch = seatOut.pitch;
      return intent;
    };
  }
  for (const controller of ai.controllers) adaptController(controller, controller.unit);
  return ai;
}

// C2: a controller bound to a seat maps goals by its mounted policy. Its
// own state requests come from goal starts (the mobile map), so they are
// translated back to the goal kind and through the policy.
function adaptController(controller, unit) {
  const policy = controllerPolicy(unit);
  controller.policy = policy;
  policy?.bind?.(controller, unit);
  if (controller.policyAdapted) return;
  controller.policyAdapted = true;
  const request = controller.requestState.bind(controller);
  controller.requestState = (state) => {
    const p = controller.policy;
    if (!p || state === DONE || state === 0) return request(state);
    const goal = MOBILE_GOAL.get(state);
    return request(goal === undefined ? state : p.goalState(goal));
  };
}

const UP = new THREE.Vector3(0, 1, 0);
const _p = new THREE.Vector3();
const _q = new THREE.Vector3();
const _o = new THREE.Vector3();
const _ground = { height: 0, normal: new THREE.Vector3() };
const _near = [];
// Stands of 20 to 30 s a pilot makes at a support point before it asks for
// another post (patrolMounted).
const SUPPORT_HOLDS = 3;

export class VehicleAI {
  constructor(ai) {
    this.ai = ai;
    this.world = ai.world;
    this.isVehicleAI = true;
    this.reservations = new Reservations(() => this.world.time ?? 0);
    this.notifier = new VehicleNotifier();
    this.padLeft = new WeakMap(); // hull -> until when no bot boards it (abandonPad)
    this.supportCache = new Map(); // post -> "mask/radius" -> support hubs (supportPoint)
    this.legacySystem = null;
    // Combat's pattern hooks read world.vehicles. A gameplay Vehicles left
    // there is kept as the system (prefer world.vehicleSystem).
    const prior = this.world.vehicles;
    if (prior && !prior.isVehicleAI) {
      if (typeof prior.board === "function" && Array.isArray(prior.vehicles)) this.legacySystem = prior;
    }
    if (!prior?.isVehicleAI) this.world.vehicles = this;
  }

  get system() {
    return this.world.vehicleSystem ?? this.legacySystem ?? null;
  }

  get turrets() {
    return this.world.turretSeats ?? null;
  }

  now() {
    return this.world.time ?? 0;
  }

  // -- the bodies ------------------------------------------------------------------------------

  // A registry entry, a seat unit or a gameplay body, to its body.
  bodyOf(x) {
    if (!x) return null;
    if (x.vehicle) return x.vehicle;
    if (x.turret) return x.turret;
    if (Array.isArray(x.seats) || x.drum !== undefined || x.weapon !== undefined) return x;
    return null;
  }

  entryOf(body) {
    if (!body) return null;
    return body.aiTarget ?? null;
  }

  // The live bot lease a controller's unit (a seat unit, or a seated
  // soldier) holds.
  leaseOf(x) {
    if (!x) return null;
    const lease = x.lease && x.lease.controllable === x ? x.lease : x.seat;
    if (!lease || lease.released || lease.human) return null;
    return lease;
  }

  controllerLease(controller) {
    return this.leaseOf(controller?.unit);
  }

  seatCount(entry) {
    const body = this.bodyOf(entry);
    if (!body) return 0;
    return Array.isArray(body.seats) ? body.seats.length : 1;
  }

  occupiedSeats(entry) {
    const body = this.bodyOf(entry);
    if (!body) return 0;
    if (Array.isArray(body.seats)) return body.occupants + (body.driver && !body.seats.some((s) => s.lease?.human) ? 1 : 0);
    return body.lease || body.gunner ? 1 : 0;
  }

  humanOccupants(entry) {
    const body = this.bodyOf(entry);
    if (!body) return 0;
    if (Array.isArray(body.seats)) return body.seats.filter((s) => s.lease?.human).length || (body.driver ? 1 : 0);
    return body.lease?.human || (body.gunner && !body.lease) ? 1 : 0;
  }

  humanAboard(entry) {
    return this.humanOccupants(entry) > 0;
  }

  // Everything a bot may be invited into: built turrets, then vehicles.
  *boardables() {
    for (const turret of this.turrets?.turrets ?? []) if (turret.alive && turret.aiTarget) yield turret.aiTarget;
    for (const v of this.system?.vehicles ?? []) if (v.alive) yield v.aiTarget;
  }

  // -- clocks and mission settings ---------------------------------------------------------------

  // The AI cycles per second the 2004 game keeps (30 / cycle length: 2 on
  // Easy and Medium, 10 on Hard).
  cyclesPerSecond() {
    const cycle = this.ai.schedulerNumbers?.()?.cycle ?? (this.ai.isHard?.() ? 3 : 15);
    return Math.trunc(30 / cycle) || 1;
  }

  controls() {
    return this.ai.controls?.() ?? this.world.missionControls ?? this.world.mission?.controls ?? null;
  }

  notifyRadius() {
    return this.controls()?.vehicleNotifyRadius ?? 50;
  }

  dense() {
    const d = this.controls()?.dense;
    return Boolean(d?.value ?? d);
  }

  flyHeights() {
    const h = this.controls()?.flyHeight ?? {};
    return { min: h.min ?? -200, max: h.max ?? 200 };
  }

  // -- the controller and scheduler hooks ---------------------------------------------------------

  // At the scheduler's cycle wrap.
  notify() {
    return this.notifier.run(this);
  }

  // The control a Character's controller should drive: the seat its body
  // sits in (a bot), else null (its own unit).
  currentControllable(character) {
    const lease = character?.unit?.seat;
    if (!lease || lease.human || lease.released) return null;
    const seat = lease.controllable;
    return seat?.alive ? seat : null;
  }

  controllerPolicy(unit) {
    return controllerPolicy(unit);
  }

  // The AI input step's analog conversion for a seat (in place of the
  // soldier's).
  analogs(unit, request, aim, dt, out, options = {}) {
    const controller = options.controller ?? unit.controller;
    // The path executor's step, unless the caller ran it (Nav's
    // navigationInput does; the scheduler's plain conversion does not).
    if (!options.stepped && controller?.navControl) {
      controller.navControl.attach?.(unit);
      controller.navControl.update?.(dt);
      request = controller.navControl.request ?? request;
    }
    const world = this.world;
    const collision = world.collision ?? null;
    const water = world.water ?? this.system?.water ?? null;
    const heights = this.flyHeights();
    const env = {
      now: this.now(),
      nav: world.nav ?? null,
      collision,
      hasPath: Boolean(controller?.navControl?.executor ?? controller?.agent?.helper?.hasPath),
      status: controller?.executorStatus ?? 0,
      hasTarget: Boolean(controller?.target),
      minFlyHeight: heights.min,
      maxFlyHeight: heights.max,
      height: collision?.ground ? (x, z) => (collision.ground(x, z, 10000, _ground) ? _ground.height : -Infinity) : null,
      water: water ? (x, z) => (water.over?.(x, z) ? water.level : null) : null,
      dangerCrosses: (a, b, r) => this.dangerCrosses(a, b, r),
      // A walker its recovery cannot free plans its trip again, or asks for
      // another post (steering.js).
      replan: (another) => {
        if (another) {
          controller?.requestObjective?.(true);
          return;
        }
        const helper = controller?.agent?.helper;
        if (helper?.state === 6 && helper.destination) helper.goto(helper.destination.clone());
      },
    };
    if (unit.kind === 2) this.walkerContacts(unit);
    return seatAnalogs(unit, request, aim, dt, out, env);
  }

  // The walker's collision yield: a contact with
  // another walker or hover it should give way to holds its travel. The
  // port's hulls report no contacts, so their footprints touching stands in
  // for the collision callback (provisional): the two collision boxes, seen
  // from above at their headings, within CONTACT_MARGIN of each other,
  // counted only while this hull moves into the other (a walker held beside
  // a larger hull at rest, a parked one say, would otherwise renew its hold
  // every step and never walk again). Touching bounding spheres stood in
  // before; they take in the hull's height, so an AT-AT's reached 15 m to
  // every side of a body 4 m wide, and an AT-ST escorting one 20 m off was
  // held for most of its walk.
  walkerContacts(unit) {
    const self = unit.vehicle;
    const v = self.velocity;
    for (const other of this.system?.vehicles ?? []) {
      if (other === self || !other.alive) continue;
      const reach = self.boundRadius + other.boundRadius;
      if (self.position.distanceToSquared(other.position) > reach * reach) continue;
      if (!(v.x * (other.position.x - self.position.x) + v.z * (other.position.z - self.position.z) > 0)) continue;
      if (!footprintsTouch(self, other, CONTACT_MARGIN)) continue;
      const entry = other.aiTarget;
      walkerContact(unit, { kind: entry.kind, position: other.position, velocity: other.velocity, classDef: { bounds: boundsOf(other) } }, this.now());
    }
  }

  // A danger box (expanded by r) crossed by the segment a..b:
  // Nav's danger regions when it has them.
  dangerCrosses(a, b, r) {
    const nav = this.world.nav;
    if (typeof nav?.dangerSegment === "function") return nav.dangerSegment(a, b, r);
    if (typeof nav?.inDangerRegion === "function") return Boolean(nav.inDangerRegion(b, r));
    return false;
  }

  createHelper(agent) {
    const unit = agent.controller?.unit;
    const lease = this.leaseOf(unit);
    if (!lease || unit !== lease.controllable) return null;
    if (unit.kind === 7) return new StationaryHelper(agent);
    if (unit.kind === 5 && this.world.nav?.planner) return new FlyGotoHelper(agent.controller, this.world, { agent });
    return null;
  }

  // -- seats ------------------------------------------------------------------------------------

  boarded(controller) {
    return Boolean(this.controllerLease(controller));
  }

  boardedVehicle(controller) {
    return this.entryOf(this.bodyOf(this.controllerLease(controller)));
  }

  boardedVehicleOf(unit) {
    return this.entryOf(this.bodyOf(this.leaseOf(unit)));
  }

  boardedVehicleOfCharacter(character) {
    return this.entryOf(this.bodyOf(this.leaseOf(character?.unit)));
  }

  isMountedTurret(sender) {
    return Boolean(sender?.turret ?? sender?.lease?.turret ?? sender?.seat?.turret);
  }

  emptySeats(sender) {
    const body = this.bodyOf(this.leaseOf(sender)) ?? this.bodyOf(sender);
    if (!body || !Array.isArray(body.seats)) return 0;
    return body.freeSeats;
  }

  crewControllers(entry, source) {
    const body = this.bodyOf(entry);
    const out = [];
    if (!body || !Array.isArray(body.seats)) return out;
    for (const seat of body.seats) {
      const lease = seat.lease;
      if (!lease || lease.human || lease.unit?.character === source) continue;
      const controller = lease.controllable?.controller ?? lease.unit?.controller ?? null;
      if (controller) out.push(controller);
    }
    return out;
  }

  seatsFull(entry) {
    return this.occupiedSeats(entry) >= this.seatCount(entry);
  }

  pendingReservations(entry, team) {
    return this.reservations.count(entry, team, SEAT) > 0;
  }

  // Exit at the next input batch; post: the command post the Character
  // takes up once out (the 2004 game's saved post).
  requestExit(controller, post = null) {
    const lease = this.controllerLease(controller);
    if (!lease) return false;
    const character = lease.unit?.character;
    if (post && character) character.objective = post;
    if (lease.vehicle) return Boolean(this.system?.requestExit?.(lease.unit, { post }));
    if (lease.turret) return Boolean(this.turrets?.requestExit?.(lease.unit, { post }));
    return false;
  }

  // Teams 1 and 2 with 25 reinforcements or fewer.
  lowTickets(team) {
    if (team !== 1 && team !== 2) return false;
    const left = this.world.battle?.reinforcements?.[team];
    return Number.isFinite(left) && left <= 25;
  }

  // Every stock vehicle and turret control has PilotType 2 (the
  // Controllable class default; docs/ai-vehicles.md), which the low-ticket
  // exit applies to.
  pilotKeepsSeat() {
    return false;
  }

  // No class flag muting priorities 1-4 is known for vehicles.
  alertsBlocked() {
    return false;
  }

  // The 2004 game accepts a hold command in every control (an unmapped
  // goal leaves the controller in its state).
  refusesHold() {
    return false;
  }

  // -- boarding ---------------------------------------------------------------------------------

  // The soldier's boarding eligibility: a
  // living soldier on foot; the candidate alive; a candidate that belongs to
  // a team takes that team's bots only; and a seat free (bots never take a
  // seat another bot holds). A hull is judged by the rules every boarder
  // follows, the player and network humans too (Vehicles.eligibleSeat, src/
  // boarding.js): none boards a hull an enemy holds in any seat.
  canEnter(controller, candidate) {
    const unit = controller?.unit;
    if (!unit || unit.seat || !unit.soldier || unit.walker || unit.kind !== 0 || !unit.alive) return false;
    if (Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0) !== 0) return false;
    const body = this.bodyOf(candidate);
    if (!body?.alive) return false;
    if ((this.padLeft.get(body) ?? -Infinity) > this.now()) return false;
    if (Array.isArray(body.seats) && body.system && body.system === this.system && this.system.eligibleSeat) return this.system.eligibleSeat(unit, body) >= 0;
    const team = candidate.team ?? 0;
    if (team > 0 && team !== unit.team) return false;
    if (Array.isArray(body.seats)) return body.firstFreeSeat() >= 0;
    return !body.gunner && !body.lease;
  }

  freeSeats(candidate, team, type = SEAT) {
    return freeCapacity(this.reservations, candidate, team, type, this.seatCount(candidate), this.humanOccupants(candidate));
  }

  reserve(candidate, unit, type = SEAT) {
    return this.reservations.reserve(candidate, unit, type);
  }

  // The boarding agent's entry: its own reservation, or capacity it then
  // reserves.
  reserveSeat(controller, candidate) {
    const unit = controller?.unit;
    if (!unit || !candidate) return false;
    if (this.reservations.has(candidate, unit, SEAT)) return true;
    if (this.freeSeats(candidate, unit.team, SEAT) > 0) {
      this.reservations.reserve(candidate, unit, SEAT);
      return true;
    }
    return false;
  }

  // The boarding agent's tick: its reservation must still hold; renewed.
  holdReservation(controller, candidate) {
    const unit = controller?.unit;
    if (!unit || !this.reservations.has(candidate, unit, SEAT)) return false;
    this.reservations.reserve(candidate, unit, SEAT);
    return true;
  }

  // The board request, carried out: the first free seat now.
  boardNow(controller, candidate) {
    const unit = controller?.unit;
    const body = this.bodyOf(candidate);
    if (!unit || !body) return false;
    let ok = false;
    if (Array.isArray(body.seats)) ok = Boolean(this.system?.board?.(unit, body));
    else ok = Boolean(this.turrets?.man?.(unit, body));
    if (ok) this.reservations.release(candidate, unit, SEAT);
    return ok;
  }

  // The Controller's stub agent's tick (unused once agents.board is bound).
  boardTick(agent) {
    const vehicle = agent.vehicle?.unit ?? null;
    if (!vehicle || !this.canEnter(agent.controller, vehicle) || !this.holdReservation(agent.controller, vehicle)) {
      agent.requestState?.(DONE);
      return;
    }
    const unit = agent.controller.unit;
    const a = unit.sphere;
    const b = vehicle.sphere;
    const reach = a.radius + b.radius + 0.5;
    if (a.center.distanceToSquared(b.center) < reach * reach) this.boardNow(agent.controller, vehicle);
  }

  // No eligible enemy within 8 m of the vehicle (the board agent
  // then refuses alerts).
  boardPathClear(controller, candidate) {
    const units = this.world.units;
    const unit = controller?.unit;
    if (!units?.query || !unit) return true;
    const found = units.query(candidate.position, 8, _near, (u) => u !== unit && u.alive && this.relation(unit.team, u.team) < 0 && u.targetable !== false);
    if (found.length > 30) found.length = 30;
    return found.length === 0;
  }

  relation(a, b) {
    return this.ai.relation?.(a, b) ?? (a === b ? 1 : !a || !b ? 0 : -1);
  }

  // The base boarding acceptance: a candidate that moves always;
  // a stationary one (a turret) only with a remembered target it could fire
  // at, or, with StayInTurrets, for any role but the repair roles.
  boardPolicy(agent, candidate) {
    const body = this.bodyOf(candidate);
    if (this.nominalSpeed(candidate) > 0) return true;
    if (!this.ai.stayInTurrets?.()) {
      const memory = this.ai.combat?.memoryPoint?.(agent) ?? null;
      if (!memory) return false;
      return this.bodyReach(body, memory);
    }
    const role = roleOf(agent.controller?.unit?.classDef);
    return role !== ROLE.SUPPORT && role !== ROLE.PILOT;
  }

  // A candidate's nominal forward speed (its class data, not its velocity):
  // a hover's ForwardSpeed, a walker's or flyer's MaxSpeed, a turret's 0.
  nominalSpeed(candidate) {
    const body = this.bodyOf(candidate);
    if (!body || !Array.isArray(body.seats)) return 0;
    if (body.flying || body.walking) return parseSpeed(body.def.settings, "MaxSpeed", body.speed);
    return parseSpeed(body.def.settings, "ForwardSpeed", body.speed);
  }

  // Flyer target reservations (priority 3, type 2): one flyer of a team on
  // a target at a time.
  targetReserved(target, team) {
    return this.reservations.count(target, team, 2) > 0;
  }

  reserveTarget(target, unit) {
    return this.reservations.reserve(target, unit, 2);
  }

  // -- mounted reach and perception ----------------------------------------------------------------

  // One of the control's mounts can bring its weapon to
  // bear on `point` by angle alone (no line of sight): the point's yaw
  // atan2(x, z) and pitch asin(y / length) in the body's frame, from the
  // mount's pivot, inside the mount's yaw and pitch limits (inclusive).
  mountReach(unit, point) {
    const lease = this.leaseOf(unit) ?? unit?.lease ?? null;
    if (!lease) return true;
    return mountAngles(lease, point);
  }

  // Any mount of a vehicle or turret (a boarding candidate) reaches it.
  bodyReach(body, point) {
    if (!body) return false;
    if (!Array.isArray(body.seats)) return mountAngles({ turret: body, index: 0, origin: (out) => turretPivot(body, out) }, point);
    for (let i = 0; i < body.seats.length; i++) {
      if (!body.seats[i].weapons.length) continue;
      if (mountAngles({ vehicle: body, index: i, origin: (out) => body.seatOrigin(i, out) }, point)) return true;
    }
    return false;
  }

  // Perception's mounted exit (StayInTurrets off is checked by the caller):
  // a zero-speed control that cannot turn to the point.
  unreachableFromMount(controller, point) {
    const unit = controller?.unit;
    if (!unit?.lease || forwardSpeed(unit) !== 0) return false;
    return !this.mountReach(unit, point);
  }

  // The shared target: an AI gun in a walker or hover a player
  // drives fires at the player's target when its mount can reach it. The
  // player's target comes from the gameplay system (humanTarget(vehicle),
  // the entity under his reticle), when the integrator provides one.
  sharedTarget(controller) {
    const unit = controller?.unit;
    const lease = this.leaseOf(unit);
    const body = lease?.vehicle;
    if (!body || body.flying) return null;
    const pilot = body.seats[0]?.lease;
    if (!pilot?.human || lease === pilot) return null;
    const target = this.system?.humanTarget?.(body) ?? null;
    if (!target?.alive || !(this.relation(unit.team, target.team) < 0)) return null;
    return mountAngles(lease, target.position) ? target : null;
  }

  // -- combat hooks (world.vehicles) -------------------------------------------------------------

  dispatch(controller, unit, target, switchAllowed, world, out) {
    return vehicleDispatch(controller, unit, target, switchAllowed, world, out, { dense: this.dense() });
  }

  enterPattern(patterns, state) {
    const h = this.flyHeights();
    return enterPattern(patterns, state, { minFlyHeight: h.min, maxFlyHeight: h.max });
  }

  tickPattern(patterns, state, dt) {
    const h = this.flyHeights();
    return tickPattern(patterns, state, dt, { minFlyHeight: h.min, maxFlyHeight: h.max });
  }

  exitPattern(patterns, state) {
    return exitPattern(patterns, state);
  }

  sidestep(patterns) {
    return sidestep(patterns, this.cyclesPerSecond());
  }

  isAtAt(unit) {
    return isAtAt(unit);
  }

  // A walker's yielding contact: hold its travel.
  walkerContact(unit, other) {
    walkerContact(unit, other, this.now());
  }

  // -- patrol, flyer objective ------------------------------------------------------------------

  // Patrol's mounted branch (state 1): a slow control (nominal
  // speed under 12.5) with an enemy that has a Character within 30 m (their
  // bounds added) keeps its seat and asks for a post other than this one
  // (a new objective request excluding the current post); otherwise, and
  // for anything faster, it gets out and may not board again for 5 s.
  patrolMounted(patrol) {
    const controller = patrol.agent.controller;
    const unit = controller.unit;
    // At a support point for a post out of its hull's reach (port;
    // support.js): stand and fight for it while it is not the team's, a
    // while at a time; then, or once it is, on to another post, still
    // aboard.
    if (patrol.agent.support) {
      const agent = patrol.agent;
      const held = unit && patrol.post && this.relation(unit.team, patrol.post.team) > 0;
      if (!held && (agent.supportHolds = (agent.supportHolds ?? 0) + 1) <= SUPPORT_HOLDS) {
        patrol.helper?.stop?.();
        patrol.expires = this.now() + this.ai.randomA.int(20, 30);
        patrol.requestState(6);
        return true;
      }
      controller.requestObjective?.(true);
      patrol.requestState(DONE);
      return true;
    }
    if (forwardSpeed(unit) < 12.5) {
      const units = this.world.units;
      const found = units?.query
        ? units.query(unit.position, 30, _near, (u) => u !== unit && u.alive && this.relation(unit.team, u.team) < 0)
        : [];
      if (found.length > 10) found.length = 10;
      if (found.some((u) => characterOf(u))) {
        controller.requestObjective?.(true);
        patrol.requestState(DONE);
        return true;
      }
    }
    controller.requestExit();
    controller.reboardAt = this.now() + 5;
    patrol.requestState(DONE);
    return false;
  }

  // Where a pilot fights for a post its hull cannot stand on (port;
  // support.js): a routable hub of its hull's that sees the post, or null
  // when the post is in reach (or for a soldier, a flyer, a turret).
  // helper: the agent's movement helper (its radius and traversal mask).
  supportPoint(unit, post, helper, out = new THREE.Vector3()) {
    const nav = this.world.nav;
    if (!unit || !post || !nav?.graph || !helper || !(helper.mask > 0) || !(helper.radius >= 0)) return null;
    // Walkers and hovers only (KIND: 2 walker, 6 hover).
    if (!unit.seat?.vehicle || (unit.kind !== 2 && unit.kind !== 6)) return null;
    const ai = this.ai;
    const center = cpCenter(ai, post, _p);
    const region = captureRegion(ai, post);
    const reach = Math.max(region?.sizeX ?? 3, region?.sizeZ ?? 3) + REACH_MARGIN;
    if (!outOfReach(nav.barriers, center, reach, helper.radius, helper.mask)) return null;
    let byHull = this.supportCache.get(post);
    if (!byHull) this.supportCache.set(post, (byHull = new Map()));
    const key = `${helper.mask}/${helper.radius.toFixed(1)}`;
    let hubs = byHull.get(key);
    if (!hubs) {
      unit.origin?.(_q);
      const eye = unit.origin && Number.isFinite(_q.y) ? Math.max(1, _q.y - unit.position.y) : 3;
      const los = this.world.los ? (a, b) => this.world.los(a, b) : null;
      hubs = supportHubs(nav, los, center, helper.radius, helper.mask, eye);
      byHull.set(key, hubs);
    }
    const hub = pickSupport(nav.graph, hubs, unit.position, helper.mask, ai.randomA);
    return hub ? out.set(hub.x, hub.y, hub.z) : null;
  }

  holdHint(hint, controller) {
    const hints = this.world.nav?.hints;
    if (hints?.reserve) hints.reserve(hint, controller, this.now());
    else hint.reservedAt = this.now();
  }

  // Close a hint until `until`: the hint set treats a hint as
  // taken for 4 s after its reservation time.
  closeHint(hint, until) {
    let h = hint;
    do {
      h.reservedAt = until - 4;
      h = h.link;
    } while (h && h !== hint);
  }

  // A landed transport's passengers: every AI aboard but the
  // pilot gets out (placed, the last bot moving up), taking up `post`;
  // players stay.
  disembarkPassengers(controller, post) {
    const lease = this.controllerLease(controller);
    const body = lease?.vehicle;
    if (!body) return 0;
    let n = 0;
    for (let i = 1; i < body.seats.length; i++) {
      const other = body.seats[i].lease;
      if (!other || other.human) continue;
      const character = other.unit?.character;
      if (post && character) character.objective = post;
      if (this.system?.unboard?.(other.unit, { placement: true, promote: true, post })) n += 1;
    }
    return n;
  }

  // A stranded flyer is destroyed.
  destroyStuck(controller) {
    const lease = this.controllerLease(controller);
    lease?.vehicle?.damage?.(Infinity);
  }

  // A craft its pilot cannot lift off its pad (stranded while still landed;
  // src/flight/ai.js tries a blocked takeoff again every second): every bot
  // aboard gets out by the boarding rules everyone follows (Vehicles
  // unboard and requestExit, src/boarding.js mayLeave), nobody moving up to
  // the controls, and goes on foot; none boards again for 15 s, and no bot
  // is invited into this hull for 60 s. People aboard stay. (Port: the
  // 2004 game destroys a stranded flyer, which suits one stuck in the air;
  // one that never left its pad blew up with its crew, came back on the same
  // pad and did it again.)
  abandonPad(controller) {
    const lease = this.controllerLease(controller);
    const body = lease?.vehicle;
    if (!body?.seats) return 0;
    const now = this.now();
    this.padLeft.set(body, now + 60);
    let n = 0;
    for (let i = body.seats.length - 1; i >= 0; i--) {
      const other = body.seats[i].lease;
      if (!other || other === lease || other.human || other.unit?.networkHuman) continue;
      const crew = other.unit?.character?.controller ?? other.controllable?.controller;
      if (crew) crew.reboardAt = now + 15;
      if (this.system?.unboard?.(other.unit, { placement: true, promote: false })) n += 1;
    }
    // The pilot, whose agent is running: at this update's seat batch.
    controller.reboardAt = now + 15;
    if (this.system?.requestExit?.(lease.unit, { promote: false })) n += 1;
    return n;
  }

  // -- VehicleSpawn decay ------------------------------------------------------------------------

  // The unattended-vehicle service, once per frame or step: a
  // hull outside the map's boundary, or one that has lost its team (team 0)
  // on a pad linked to a post, and with nobody aboard, gathers idle time,
  // one sample a second; a player who could board it within its radius and
  // his plus 10 m pauses the count. Past 20 samples it loses MaxHealth /
  // DecayTime (10 by default) a sample. Anyone aboard clears the count.
  updateDecay(dt) {
    const system = this.system;
    if (!system) return;
    const nav = this.world.nav;
    for (const v of system.vehicles) {
      if (!v.alive) continue;
      const st = (v.decay ||= { idle: 0, sample: 0 });
      const inside = !(nav?.outside?.(v.position) ?? false);
      if (inside && (!v.slot?.desc?.post || v.controllingTeam !== 0)) {
        st.idle = 0;
        continue;
      }
      if (v.occupied) {
        st.idle = 0;
        continue;
      }
      st.sample -= dt;
      if (!(st.sample < 0)) continue;
      st.sample += 1;
      if (this.playerNear(v)) continue;
      st.idle += 1;
      if (st.idle > 20) v.damage(v.maxHealth / (v.slot?.desc?.decayTime ?? 10));
    }
  }

  playerNear(v) {
    for (const unit of this.ai.humanUnits?.() ?? []) {
      if (!unit?.alive) continue;
      const team = v.controllingTeam;
      if (team > 0 && !(this.relation(team, unit.team) > 0)) continue;
      const reach = v.boundRadius + (unit.sphere?.radius ?? 1) + 10;
      if (unit.position.distanceToSquared(v.position) <= reach * reach) return true;
    }
    return false;
  }

  // The Classic steering state of a seat (tests, debugging).
  steerState(unit) {
    return steerState(unit);
  }

  // Can soldiers reach where the vehicle sits (the notifier's barrier
  // tests: a 2 m clearance, then the vehicle's own).
  reachable(entry) {
    const barriers = this.world.nav?.barriers;
    if (!barriers?.pointClear) return true;
    const p = entry.position;
    const at = { x: p.x, y: p.y, z: p.z };
    if (!barriers.pointClear(at, 2, 1)) return false;
    const r = entry.vehicle ? Math.hypot(entry.vehicle.size.x, entry.vehicle.size.z) * (entry.kind === 6 ? 0.5 : 0.25) : 0;
    return r <= 0 || barriers.pointClear(at, r, 1);
  }
}

// A movement helper for controls that cannot move (gunner seats, turrets):
// every goto is accepted and goes nowhere; the request stays empty.
export class StationaryHelper {
  constructor(agent) {
    this.agent = agent;
    this.controller = agent.controller;
    this.arrived = true;
    this.failed = false;
    this.status = 0;
    this.hasPath = false;
    this.stopped = true;
  }

  goto() {
    this.controller.request.valid = false;
  }

  follow() {}
  stop() {
    this.controller.request.valid = false;
  }

  tick() {}
  stance() {}
  aimAt() {}
  aimAtTarget() {}
  aimAlongPath() {}
  destinationPoint(out) {
    return out.copy(this.controller.unit?.position ?? out);
  }
}

// How near two hulls' footprints count as touching (m, provisional).
const CONTACT_MARGIN = 0.5;
const _fa = { cx: 0, cz: 0, fx: 0, fz: 0, hx: 0, hz: 0 };
const _fb = { cx: 0, cz: 0, fx: 0, fz: 0, hx: 0, hz: 0 };

// A hull's collision box from above: its centre, forward (+Z in the model)
// and half extents across (x) and along (z).
function footprint(v, out) {
  const b = v.bounds;
  const yaw = v.yaw ?? 0;
  const fx = Math.sin(yaw), fz = Math.cos(yaw);
  const lx = (b.min.x + b.max.x) / 2, lz = (b.min.z + b.max.z) / 2;
  // The model's +X in the world at this yaw: (cos yaw, -sin yaw).
  out.cx = v.position.x + fz * lx + fx * lz;
  out.cz = v.position.z - fx * lx + fz * lz;
  out.fx = fx;
  out.fz = fz;
  out.hx = (b.max.x - b.min.x) / 2;
  out.hz = (b.max.z - b.min.z) / 2;
  return out;
}

// Two oriented rectangles within `margin` of each other (separating axes).
function footprintsTouch(a, b, margin) {
  const A = footprint(a, _fa), B = footprint(b, _fb);
  const dx = B.cx - A.cx, dz = B.cz - A.cz;
  for (const [ax, az] of [[A.fx, A.fz], [A.fz, -A.fx], [B.fx, B.fz], [B.fz, -B.fx]]) {
    const ra = A.hz * Math.abs(A.fx * ax + A.fz * az) + A.hx * Math.abs(A.fz * ax - A.fx * az);
    const rb = B.hz * Math.abs(B.fx * ax + B.fz * az) + B.hx * Math.abs(B.fz * ax - B.fx * az);
    if (Math.abs(dx * ax + dz * az) > ra + rb + margin) return false;
  }
  return true;
}

function boundsOf(v) {
  const b = v.bounds;
  return [[b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]];
}

function characterOf(u) {
  if (u.character) return u.character;
  for (const seat of u.vehicle?.seats ?? []) if (seat.lease?.unit?.character) return seat.lease.unit.character;
  return u.turret?.lease?.unit?.character ?? null;
}

function parseSpeed(settings, name, fallback) {
  const n = Number.parseFloat(settings?.[name]);
  return Number.isFinite(n) ? n : fallback;
}

// The angular reach of one mount. lease: { vehicle | turret, index,
// origin(out) }.
function mountAngles(lease, point) {
  const body = lease.vehicle ?? lease.turret;
  if (!body) return true;
  const settings = lease.vehicle ? lease.vehicle.seats[lease.index]?.settings ?? {} : body.desc?.settings ?? {};
  const yawLimits = degrees(settings.YawLimits, [-180, 180]);
  const pitchLimits = degrees(settings.PitchLimits, lease.vehicle ? [-90, 90] : [-45, 45]);
  lease.origin(_o);
  _q.subVectors(point, _o);
  const length = _q.length();
  if (length < 1e-9) return true;
  const yaw = lease.vehicle ? body.yaw : body.facing;
  // The body's frame: the 2004 game's right row and forward.
  const x = _q.x * Math.cos(yaw) - _q.z * Math.sin(yaw);
  const z = _q.x * Math.sin(yaw) + _q.z * Math.cos(yaw);
  const a = Math.atan2(x, z);
  const p = Math.asin(Math.max(-1, Math.min(1, _q.y / length)));
  // A placed turret reaches what its aim can take (Turret.pitchLimits, up
  // positive, and its yaw arc), and its gun what its aim can (the port
  // widens the gun to the view, src/props.js), so a bot engages exactly
  // what it can hit: the Yavin tower's gunner 35 degrees down, not 35 up.
  if (lease.turret && Array.isArray(body.pitchLimits)) {
    const arc = body.yawLimits ?? null;
    const held = arc && typeof body.holdYaw === "function" ? Math.abs(Math.atan2(Math.sin(body.holdYaw(a) - a), Math.cos(body.holdYaw(a) - a))) < 1e-6 : true;
    return held && p >= body.pitchLimits[0] - 1e-6 && p <= body.pitchLimits[1] + 1e-6;
  }
  // As the 2004 game's reach test has it: the yaw, -pi..pi,
  // against YawLimits as written, so an arc past 180 degrees (the AT-TE's
  // and the snowspeeder's rear guns) reaches only up to 180, its left half;
  // and the pitch, up positive, against PitchLimits, which a turret's own
  // aim reads down positive. Both are the 2004 game's, kept for vehicles.
  return a >= yawLimits[0] && a <= yawLimits[1] && p >= pitchLimits[0] && p <= pitchLimits[1];
}

function degrees(value, fallback) {
  const parts = String(value ?? "").split(/\s+/).map(Number).filter(Number.isFinite);
  const pair = parts.length >= 2 ? parts : fallback;
  return [(pair[0] * Math.PI) / 180, (pair[1] * Math.PI) / 180];
}

function turretPivot(turret, out) {
  const node = turret.gun || turret.firePoint;
  if (node) {
    turret.rig?.root?.updateMatrixWorld?.(true);
    return node.getWorldPosition(out);
  }
  return out.set(0, 1.65, 0.32).applyMatrix4(turret.matrix);
}

export { UP, _p };
