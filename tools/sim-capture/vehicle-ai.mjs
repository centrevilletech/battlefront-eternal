// The vehicle AI's frames (tools/sim-capture/specs.mjs `vehicleAI`): the
// Classic AI's vehicle service (src/ai/classic/vehicles/index.js VehicleAI)
// called from the controllers, agents, combat and scheduler, its seat
// steering, attack patterns, notifier and decay, and the boarding and flyer
// agents. rust/sim/tests/replay_vehicle_ai.rs reads them back.
//
// Every call the vehicle AI makes outside its own state is a boundary:
// the other packages' functions it reaches through src/ai/classic/vehicles/
// world.js (the weapon choice, the pilot's stick, a unit's sphere, ...) and
// the methods of live objects (the collision world, the registry, a seat's
// eye, navigation's controls and helpers, the controllers, agents and
// trackers). Navigation's graph and barriers are the map's content, which a
// replay loads with the map.
//
// A frame's state is what the vehicle AI reads and writes of the battle, as
// views: the hulls, turrets, seat leases and the units they name (a frame
// that asks the registry, every unit), the controllers and agents of its
// arguments, the service's own state. At a frame's end only what the
// vehicle AI writes is recorded again: its own state and what it keeps on
// other objects (a control's steering bag and intent, a hull's decay, the
// controllers', agents' and patterns' fields it sets).
//
// Services the controllers call every tick for every bot (the control a
// Character drives, whether a controller sits, ...) record only the first
// few calls of each sampled tick (CAPPED), through a forwarding method this
// file puts on the service: the battle runs exactly as it would.
import { NavControl } from '../../src/ai/classic/nav/control.js';
import { AIPath, PathPlanner } from '../../src/ai/classic/nav/planner.js';
import { FlyGotoHelper as NavFlyGotoHelper, GroundGotoHelper } from '../../src/ai/classic/nav/goto.js';
import { traversalMask, traversalRadius } from '../../src/ai/classic/nav/units.js';
import { Agent } from '../../src/ai/classic/agents.js';
import { UnitController } from '../../src/ai/classic/controller.js';
import { InterruptHelper } from '../../src/ai/classic/interrupts.js';
import { TargetTracker } from '../../src/ai/classic/perception.js';
import { AttackPatterns } from '../../src/ai/classic/combat.js';
import { UnitBoardAgent, UnitFlyAgent, UnitUnBoardAgent } from '../../src/ai/classic/vehicles/agents.js';
import { StationaryHelper } from '../../src/ai/classic/vehicles/index.js';
import { FlyGotoHelper } from '../../src/ai/classic/vehicles/fly.js';
import { _test as patternsTest } from '../../src/ai/classic/vehicles/patterns.js';
import { SeatLease, SeatUnit } from '../../src/seats.js';
import { Vehicle } from '../../src/vehicles.js';
import { Turret } from '../../src/props.js';

// The first calls of each sampled tick recorded, per capped service.
const CAPPED = {
  currentControllable: 2, boarded: 2, lowTickets: 1, boardedVehicle: 4, boardedVehicleOf: 4, boardedVehicleOfCharacter: 4,
  isMountedTurret: 4, emptySeats: 4, crewControllers: 4, seatsFull: 4, pendingReservations: 4, targetReserved: 4,
  reserveTarget: 4, mountReach: 6, sharedTarget: 3, unreachableFromMount: 4, createHelper: 6, mountedState: 6,
  agentFor: 6, adaptController: 6, canEnter: 8, boardPolicy: 8, isAtAt: 4,
};
// Services every call of which is recorded.
const SERVICES = ['analogs', 'seatInput', 'updateDecay', 'notify', 'enterPattern', 'tickPattern', 'exitPattern',
  'sidestep', 'reserveSeat', 'holdReservation', 'boardNow', 'boardPathClear', 'patrolMounted', 'supportPoint', 'requestExit',
  'abandonPad', 'disembarkPassengers', 'destroyStuck', 'holdHint', 'closeHint', 'freeSeats', 'reserve'];

const service = (game) => game.ai?.classic?.ai?.world?.vehicles ?? null;
const aiOf = (game) => game.ai?.classic?.ai ?? null;
const v3 = (v) => (v ? v.clone() : null);
const num = (x) => (typeof x === 'number' ? x : x === undefined ? undefined : x);

// ---- Views of what the vehicle AI reads.
function hullView(svc, v) {
  return {
    self: v, entry: v.aiTarget, alive: v.alive, name: v.name, flying: Boolean(v.flying), walking: Boolean(v.walking),
    grounded: v.grounded, flightState: v.flightState ?? null, gear: v.gear, speed: v.speed,
    maxSpeed: v.def?.settings?.MaxSpeed, forwardSpeed: v.def?.settings?.ForwardSpeed,
    label: v.def?.label ?? v.def?.settings?.Label ?? null,
    position: v3(v.position), velocity: v3(v.velocity), yaw: v.yaw, quaternion: v.quaternion.clone(), matrix: v.matrix.clone(),
    boundsMin: v3(v.bounds.min), boundsMax: v3(v.bounds.max), boundRadius: v.boundRadius, size: v3(v.size), body: v.body,
    seats: v.seats.map((s) => ({ lease: s.lease ?? null, weapons: s.weapons.length, yawLimits: s.settings?.YawLimits, pitchLimits: s.settings?.PitchLimits })),
    occupants: v.occupants, driver: Boolean(v.driver), freeSeats: v.freeSeats, occupied: v.occupied, controllingTeam: v.controllingTeam,
    towTarget: v.towTarget, slotPost: Boolean(v.slot?.desc?.post), decayTime: v.slot?.desc?.decayTime, maxHealth: v.maxHealth,
    ownSystem: v.system === svc.system, decay: v.decay ? { idle: v.decay.idle, sample: v.decay.sample } : undefined,
    padLeft: svc.padLeft.get(v),
  };
}
function turretView(svc, t) {
  return {
    self: t, entry: t.aiTarget ?? null, alive: t.alive, gunner: Boolean(t.gunner), lease: t.lease ?? null, facing: t.facing,
    yawLimitsSetting: t.desc?.settings?.YawLimits, pitchLimitsSetting: t.desc?.settings?.PitchLimits,
    pitchLimits: Array.isArray(t.pitchLimits) ? [...t.pitchLimits] : null, yawLimits: t.yawLimits ?? null,
    holdYaw: typeof t.holdYaw === 'function', padLeft: svc.padLeft.get(t),
  };
}
const leaseView = (l) => ({ self: l, released: l.released, human: l.human, live: l.live, unit: l.unit ?? null, controllable: l.controllable ?? null,
  vehicle: l.vehicle ?? null, turret: l.turret ?? null, index: l.index, team: l.team });
const steerView = (s) => (s ? Object.fromEntries(Object.entries(s).map(([k, v]) => [k, v && typeof v === 'object' && v.isVector3 ? v.clone() : k === 'orbit' && v ? { target: v.target ? { unit: v.target.unit, generation: v.target.generation } : null, offset: v.offset.clone(), radius: v.radius, factor: v.factor } : k === 'stuckAt' && v ? { x: v.x, z: v.z } : v])) : undefined);
const intentView = (i) => (i ? { up: i.up, down: i.down, brake: i.brake, trick: i.trick, immelmann: i.immelmann, boost: i.boost,
  forward: i.forward, right: i.right, turn: i.turn, pitch: i.pitch } : undefined);
// A unit's class as the vehicle AI reads it: a control's class view (made
// for it by src/seats.js), else only the fields read of a soldier's class
// (a hull's or a turret's entry carries its def, which nothing here reads).
function classView(u) {
  const c = u.classDef;
  if (!c || typeof c !== 'object') return null;
  if (u instanceof SeatUnit) return c;
  if (u.vehicle || u.turret) return null;
  return { NoEnterVehicles: c.NoEnterVehicles, role: c.role, UnitType: c.UnitType, unitType: c.unitType };
}
function unitView(u) {
  const c = classView(u);
  const body = u.vehicle ? null : (u.def ?? u.classDef);
  return {
    self: u, kind: u.kind, id: u.id, generation: u.generation, alive: Boolean(u.alive), team: u.team,
    position: v3(u.position), velocity: v3(u.velocity), bodyYaw: u.bodyYaw, aimYaw: u.soldier?.aimYaw,
    classDef: c, className: typeof u.className === 'string' ? u.className : null,
    soldier: Boolean(u.soldier), soldierMaxSpeed: u.soldier?.maxSpeed, walker: Boolean(u.walker), human: Boolean(u.human),
    networkHuman: Boolean(u.networkHuman), targetable: u.targetable !== false, seat: u.seat ?? null, lease: u.lease ?? null,
    vehicle: u.vehicle ?? null, turret: u.turret ?? null, controller: u.controller ?? null, character: u.character ?? null,
    upY: u.upY, ownLabel: body?.label ?? body?.settings?.Label ?? null, ownName: typeof u.name === 'string' ? u.name : null,
    steer: steerView(u.steer), hasSteer: Boolean(u.steer), intent: u instanceof SeatUnit ? intentView(u.intent) : undefined,
    weapons: u instanceof SeatUnit ? (u.combat?.arsenal?.weapons ?? []).map((w) => ({ base: w.def?.base ?? null, channel: w.channel })) : undefined,
  };
}
function controllerView(c) {
  const helper = c.agent?.helper ?? null;
  return {
    self: c, unit: c.unit ?? null, navControl: Boolean(c.navControl), executor: Boolean(c.navControl?.executor), executorStatus: c.executorStatus,
    target: Boolean(c.target), agent: c.agent ?? null, helper: Boolean(helper), helperHasPath: Boolean(helper?.hasPath),
    helperState: helper?.state, helperDestination: v3(helper?.destination), isUnitController: Boolean(c.isUnitController),
    fireChannel: c.perception?.fire?.channel,
    // What the vehicle AI writes.
    reboardAt: c.reboardAt, fireA: c.fireA, requestValid: c.request?.valid, exitRequest: c.exitRequest,
    policy: c.policy?.name ?? null, policyAdapted: Boolean(c.policyAdapted), homeFacing: c.homeFacing,
  };
}
const characterView = (ch) => ({ self: ch, unit: ch.unit ?? null, controller: ch.controller ?? null, objective: ch.objective ?? null });
function helperView(h) {
  if (!h) return null;
  return { self: h, kind: h instanceof FlyGotoHelper ? 'fly' : h instanceof StationaryHelper ? 'stationary' : h instanceof GroundGotoHelper ? 'ground' : 'other',
    controller: h.controller ?? null, arrived: Boolean(h.arrived), failed: Boolean(h.failed), state: h.state, pending: h.pending,
    destination: v3(h.destination) };
}
function agentView(a) {
  if (!a) return null;
  return {
    self: a, class: a.constructor?.name ?? null, state: a.state, pending: a.pending, unit: a.unit ?? null, controller: a.controller ?? null,
    helper: helperView(a.helper), support: Boolean(a.support), supportHolds: a.supportHolds,
    vehicle: a.vehicle ? { unit: a.vehicle.unit, generation: a.vehicle.generation } : a.vehicle,
    hint: a.hint ?? null, searchAt: a.searchAt, flag: a.flag, deadline: a.deadline, post: a.post ?? null,
    tracker: a.tracker ? { point: v3(a.tracker.point), freshTarget: a.tracker.freshTarget ?? null, target: a.tracker.target ?? null } : null,
  };
}
const patternsView = (p) => ({ self: p, agent: agentView(p.agent), controller: p.controller ?? null, vehicle: p.vehicle ? { ...p.vehicle, entry: v3(p.vehicle.entry), destination: v3(p.vehicle.destination) } : undefined });
const patrolView = (p) => ({ self: p, agent: agentView(p.agent), post: p.post ?? null, expires: p.expires, state: p.state, pending: p.pending });

// A movement request as the steering reads it (a control's MovementRequest
// is a class instance the capture would only name).
const requestView = (r) => (r && typeof r === 'object' && r.steeringTarget ? { valid: r.valid, steeringTarget: v3(r.steeringTarget),
  waypoint: v3(r.waypoint), segmentDirection: v3(r.segmentDirection), mode: r.mode, lower: r.lower, upper: r.upper, blend: r.blend, throttle: r.throttle } : null);

// The objects a frame's arguments (and its receiver) name, by what they are.
function argViews(args, out, self) {
  if (self instanceof Agent) out.agent = agentView(self);
  else if (self instanceof FlyGotoHelper || self instanceof StationaryHelper) { out.helper = helperView(self); if (self.controller) out.controllers.add(self.controller); }
  for (const a of args) {
    if (!a || typeof a !== 'object') continue;
    if (a instanceof UnitController) out.controllers.add(a);
    else if (a instanceof AttackPatterns) out.patterns = patternsView(a);
    else if (a.constructor?.name === 'PatrolHelper') out.patrol = patrolView(a);
    else if (a instanceof Agent) out.agent = agentView(a);
    else if (a.constructor?.name === 'Character') out.characters.add(a);
  }
}

// The objects a frame's state takes in, by kind (see enterState).
function collect(game, args, self, profile) {
  const svc = service(game), ai = aiOf(game);
  const units = new Set(), controllers = new Set(), characters = new Set(), leases = new Set(), hulls = new Set(), turrets = new Set();
  const extra = { controllers: new Set(), characters: new Set(), patterns: null, patrol: null, agent: null, helper: null };
  argViews(args, extra, self);
  const queue = [];
  const add = (x) => { if (x && typeof x === 'object') queue.push(x); };
  for (const a of args) add(a);
  add(self);
  for (const c of extra.controllers) add(c);
  for (const ch of extra.characters) add(ch);
  const agentOf = extra.agent ?? extra.patterns?.agent ?? extra.patrol?.agent ?? null;
  const agentObject = agentOf?.self ?? null;
  if (profile !== 'args') {
    for (const r of svc.reservations.records) { add(r.candidate.unit); add(r.owner.unit); }
    if (agentObject) { add(agentObject.controller); add(agentObject.unit); add(agentObject.tracker?.freshTarget); add(agentObject.tracker?.target); add(agentObject.post); }
  }
  if (profile === 'hulls' || profile === 'normal' || profile === 'all') for (const v of svc.system?.vehicles ?? []) add(v);
  if (profile === 'normal' || profile === 'all') {
    for (const t of svc.turrets?.turrets ?? []) add(t);
    for (const u of ai.humanUnits()) add(u);
  }
  if (profile === 'all') for (const u of game.units.all()) add(u);
  const deep = profile !== 'args';
  while (queue.length) {
    const x = queue.pop();
    if (x instanceof Vehicle) {
      if (hulls.has(x)) continue;
      hulls.add(x); if (deep) { add(x.aiTarget); for (const seat of x.seats) add(seat.lease); }
    } else if (x instanceof Turret) {
      if (turrets.has(x)) continue;
      turrets.add(x); if (deep) { add(x.aiTarget); add(x.lease); }
    } else if (x instanceof SeatLease) {
      if (leases.has(x)) continue;
      leases.add(x); if (deep) { add(x.unit); add(x.controllable); add(x.vehicle); add(x.turret); }
    } else if (x instanceof UnitController) {
      if (controllers.has(x)) continue;
      controllers.add(x); if (deep) add(x.unit);
    } else if (x.constructor?.name === 'Character') {
      if (characters.has(x)) continue;
      characters.add(x); if (deep) { add(x.unit); add(x.controller); }
    } else if ('kind' in x && 'generation' in x && !(x instanceof Agent)) {
      if (units.has(x)) continue;
      units.add(x);
      if (deep) {
        add(x.seat); add(x.lease); add(x.vehicle); add(x.turret); add(x.controller); add(x.character);
        const orbit = x.steer?.orbit;
        if (orbit?.target?.unit) add(orbit.target.unit);
      }
    }
  }
  return { units, controllers, characters, leases, hulls, turrets, extra, agentOf };
}

// The battle's views a frame starts from: what its arguments (and its
// receiver) name and what those reach (a unit's seat, hull, controller and
// Character; a lease's unit and control; a hull's entry and seats' leases;
// a turret's; the units the service's reservations name; an orbit's and a
// tracker's targets). profile 'lean' stops there, 'hulls' adds every hull
// (a walker's contacts), 'normal' every hull and turret and the living
// people (the decay, the notifier's boardables), 'all' every member of the
// registry (a query's finds); 'args' takes only the objects the arguments
// name (a seat's input step, whose replay starts from its analogs frame's).
function enterState(game, args, sets, cache) {
  const svc = service(game), ai = aiOf(game);
  const { units, controllers, characters, leases, hulls, turrets, extra, agentOf: agent } = sets;
  return {
    time: game.ai?.classic?.ai?.world?.time ?? 0,
    settings: { controls: ai.controls(), cycle: ai.schedulerNumbers?.()?.cycle ?? (ai.isHard?.() ? 3 : 15), hasPlanner: Boolean(svc.world.nav?.planner),
      hasGraph: Boolean(svc.world.nav?.graph), reinforcements: svc.world.battle?.reinforcements ?? null, humanUnits: ai.humanUnits(),
      posts: (svc.world.posts?.posts ?? []).map((p) => ({ self: p, position: v3(p.position), team: p.team })),
      hulls: svc.system?.vehicles ?? [], turrets: svc.turrets?.turrets ?? [] },
    service: serviceState(svc, cache),
    hulls: [...hulls].map((v) => hullView(svc, v)), turrets: [...turrets].map((t) => turretView(svc, t)),
    leases: [...leases].map(leaseView), units: [...units].map(unitView), controllers: [...controllers].map(controllerView),
    characters: [...characters].map(characterView),
    agent, patterns: extra.patterns, patrol: extra.patrol, helper: extra.helper, request: requestView(args[1]),
    helperMask: args[2] && args[2].controller ? { mask: traversalMask(args[2].controller.unit), radius: traversalRadius(args[2].controller.unit), unit: args[2].controller.unit ?? null } : null,
  };
}
function serviceState(svc, cache = false) {
  return {
    reservations: svc.reservations.records.map((r) => ({ candidate: { unit: r.candidate.unit, generation: r.candidate.generation },
      owner: { unit: r.owner.unit, generation: r.owner.generation }, time: r.time, type: r.type })),
    counter: svc.notifier.counter,
    supportCache: !cache ? null : [...svc.supportCache].map(([post, byKey]) => [post, [...byKey].map(([key, hubs]) => [key, hubs.map((h) => ({ hub: h.hub.index, distance: h.distance }))])]),
    run: { A: patternsTest.run.A, B: patternsTest.run.B },
  };
}
// What the vehicle AI writes, at a frame's end: its own state, and what it
// keeps on the objects the frame took in.
function exitState(game, args, self, cache, sets) {
  const svc = service(game);
  const { units, controllers, characters, hulls, extra } = sets;
  const seats = [...units].filter((u) => u instanceof SeatUnit);
  return {
    service: serviceState(svc, cache),
    hulls: [...hulls].map((v) => ({ self: v, decay: v.decay ? { idle: v.decay.idle, sample: v.decay.sample } : undefined, padLeft: svc.padLeft.get(v) })),
    controls: seats.map((u) => ({ self: u, steer: steerView(u.steer), hasSteer: Boolean(u.steer), intent: intentView(u.intent) })),
    controllers: [...controllers].map((c) => ({ self: c, reboardAt: c.reboardAt, fireA: c.fireA, requestValid: c.request?.valid, exitRequest: c.exitRequest,
      policy: c.policy?.name ?? null, policyAdapted: Boolean(c.policyAdapted), homeFacing: c.homeFacing })),
    characters: [...characters].map((ch) => ({ self: ch, objective: ch.objective ?? null })),
    agent: extra.agent ? agentView(extra.agent.self) : extra.patterns ? agentView(extra.patterns.agent?.self) : extra.patrol ? agentView(extra.patrol.agent?.self) : null,
    patterns: extra.patterns ? patternsView(extra.patterns.self) : null,
    patrol: extra.patrol ? patrolView(extra.patrol.self) : null,
    helper: extra.helper ? helperView(extra.helper.self) : null,
  };
}

// Frames' enter and exit states: the enter state on the way in, the exit
// state on the way out (a frame's state() runs once at each). `profile` is
// a profile, or a function of the frame's arguments giving one.
function frameState(profile, cache = false) {
  const entering = [];
  return (game, args, cap, self) => {
    // A frame's state is asked at its enter, then at its exit; frames nest,
    // so the stack says which. The exit takes in what the enter did.
    const top = entering[entering.length - 1];
    if (top && top.args === args) { entering.pop(); return exitState(game, args, self, cache, top.sets); }
    const sets = collect(game, args, self, typeof profile === 'function' ? profile(args) : profile);
    entering.push({ args, sets });
    return enterState(game, args, sets, cache);
  };
}

// The AI's world object (what the AI sees of the battle) is a plain object
// a record would write out whole (the mission, the sides' class data):
// the dispatch and the weapon choice it is passed to are recorded through
// forwarding methods that take it as read (`frame:dispatch`,
// `call:selectWeapon`). They call straight through.
function forwardWithoutWorld(target, name, prefix, world, at) {
  const inner = `${prefix}:${name}`;
  if (target[inner]) return;
  const base = target[name];
  target[inner] = function (...args) { args.splice(at, 0, world); return base.apply(this, args); };
  target[name] = function (...args) {
    if (args[at] !== world) return base.apply(this, args);
    const rest = args.slice();
    rest.splice(at, 1);
    return this[inner](...rest);
  };
}

// The boundary: [label, target, method].
function boundaries(game) {
  const svc = service(game), ai = aiOf(game);
  if (!svc) return [];
  const port = svc.port, world = ai.world;
  forwardWithoutWorld(port, 'selectWeapon', 'call', world, 4);
  return [
    ['port.selectWeapon', port, 'call:selectWeapon'], ['port.switchWeapon', port, 'switchWeapon'], ['port.pilotInput', port, 'pilotInput'],
    ['port.cpCenter', port, 'cpCenter'], ['port.captureRegion', port, 'captureRegion'], ['port.sphere', port, 'sphere'],
    ['port.turretPivot', port, 'turretPivot'],
    ['collision.cast', game.collision, 'cast'], ['units.query', game.units, 'query'],
    ['world.relation', world, 'relation'], ['world.los', world, 'los'],
    ['seat.origin', SeatUnit.prototype, 'origin'], ['seat.forward', SeatUnit.prototype, 'forward'],
    ['lease.origin', SeatLease.prototype, 'origin'],
    ['vehicle.seatOrigin', Vehicle.prototype, 'seatOrigin'], ['vehicle.firstFreeSeat', Vehicle.prototype, 'firstFreeSeat'],
    ['vehicle.damage', Vehicle.prototype, 'damage'], ['turret.holdYaw', Turret.prototype, 'holdYaw'],
    ['vehicles.eligibleSeat', svc.system, 'eligibleSeat'], ['vehicles.board', svc.system, 'board'],
    ['vehicles.requestExit', svc.system, 'requestExit'], ['vehicles.unboard', svc.system, 'unboard'],
    ['vehicles.humanTarget', svc.system, 'humanTarget'],
    ['turrets.man', svc.turrets, 'man'], ['turrets.requestExit', svc.turrets, 'requestExit'],
    ['navControl.attach', NavControl.prototype, 'attach'], ['navControl.update', NavControl.prototype, 'update'],
    ['navControl.install', NavControl.prototype, 'install'],
    ['planner.cancel', PathPlanner.prototype, 'cancel'], ['planner.path', PathPlanner.prototype, 'path'],
    ['path.prepend', AIPath.prototype, 'prepend'], ['path.release', AIPath.prototype, 'release'],
    ['flyHelper.goto', NavFlyGotoHelper.prototype, 'goto'], ['flyHelper.stop', NavFlyGotoHelper.prototype, 'stop'],
    ['groundHelper.goto', GroundGotoHelper.prototype, 'goto'], ['groundHelper.stop', GroundGotoHelper.prototype, 'stop'],
    ['hints.reserve', world.nav?.hints ?? null, 'reserve'], ['nav.queryHints', ai.nav ?? null, 'queryHints'],
    ['controller.requestObjective', UnitController.prototype, 'requestObjective'],
    ['controller.requestExit', UnitController.prototype, 'requestExit'],
    ['controller.addVehicleNote', UnitController.prototype, 'addVehicleNote'],
    ['agent.updateCombatTarget', Agent.prototype, 'updateCombatTarget'], ['agent.aim', Agent.prototype, 'aim'],
    ['agent.tickHelper', Agent.prototype, 'tickHelper'], ['agent.canEngage', Agent.prototype, 'canEngage'],
    ['interrupts.tick', InterruptHelper.prototype, 'tick'],
    ['tracker.setDelay', TargetTracker.prototype, 'setDelay'], ['tracker.defaultDelay', TargetTracker.prototype, 'defaultDelay'],
    ['patterns.face', AttackPatterns.prototype, 'face'], ['combat.memoryPoint', ai.combat ?? null, 'memoryPoint'],
  ];
}

// A capped service: the first calls of each sampled tick through a method
// the frame records (`frame:NAME`), the rest straight through.
const capCounts = new Map();
let capTick = -1;
function cappedTarget(game, name) {
  const svc = service(game);
  if (!svc) return null;
  const frameName = `frame:${name}`;
  if (!svc[frameName]) {
    const base = svc[name];
    svc[frameName] = function (...args) { return base.apply(this, args); };
    svc[name] = function (...args) {
      const tick = game.worldClock?.tick ?? -1;
      if (tick !== capTick) { capTick = tick; capCounts.clear(); }
      const n = capCounts.get(name) ?? 0;
      if (n >= CAPPED[name]) return base.apply(this, args);
      capCounts.set(name, n + 1);
      return this[frameName](...args);
    };
  }
  return svc;
}

// What each frame's state takes in (enterState's profiles).
const PROFILES = {
  // A walker's analogs read every hull (its contacts); the others what the control reaches.
  analogs: (args) => (args[0]?.kind === 2 ? 'hulls' : 'lean'),
  seatInput: 'args', updateDecay: 'normal', notify: 'all', boardPathClear: 'all', patrolMounted: 'all', contested: 'all',
};
const profileOf = (name) => PROFILES[name] ?? 'lean';
const serviceFrame = (name) => ({ method: `vehicleAI.${name}`, target: service, state: frameState(profileOf(name), name === 'supportPoint'), boundaries });
// dispatch(controller, unit, target, switchAllowed, world, out), recorded without its world.
const dispatchFrame = () => ({ method: 'vehicleAI.frame:dispatch', state: frameState('lean'), boundaries,
  target: (game) => { const svc = service(game); if (svc) forwardWithoutWorld(svc, 'dispatch', 'frame', aiOf(game).world, 4); return svc; } });
const cappedFrame = (name) => ({ method: `vehicleAI.frame:${name}`, target: (game) => cappedTarget(game, name), state: frameState(profileOf(name)), boundaries });
const agentFrame = (cls, label, method) => ({ method: `${label}.${method}`, target: () => cls.prototype, state: frameState(profileOf(method)), boundaries });

export const vehicleAI = {
  frames: [
    ...SERVICES.map((name) => serviceFrame(name)), dispatchFrame(),
    ...Object.keys(CAPPED).map((name) => cappedFrame(name)),
    agentFrame(UnitBoardAgent, 'boardAgent', 'onEnter'), agentFrame(UnitBoardAgent, 'boardAgent', 'behave'),
    agentFrame(UnitBoardAgent, 'boardAgent', 'contested'),
    agentFrame(UnitUnBoardAgent, 'unboardAgent', 'onEnter'), agentFrame(UnitUnBoardAgent, 'unboardAgent', 'behave'),
    agentFrame(UnitFlyAgent, 'flyAgent', 'onEnter'), agentFrame(UnitFlyAgent, 'flyAgent', 'behave'), agentFrame(UnitFlyAgent, 'flyAgent', 'onExit'),
    agentFrame(UnitFlyAgent, 'flyAgent', 'acceptsTarget'),
    agentFrame(FlyGotoHelper, 'flyHelperP14', 'followPath'), agentFrame(FlyGotoHelper, 'flyHelperP14', 'goto'),
    agentFrame(FlyGotoHelper, 'flyHelperP14', 'stop'), agentFrame(FlyGotoHelper, 'flyHelperP14', 'land'),
    agentFrame(StationaryHelper, 'stationaryHelper', 'goto'), agentFrame(StationaryHelper, 'stationaryHelper', 'stop'),
  ],
};

// The views and the service's state as these frames write them, for the AI
// phase's captures that carry the vehicle AI's state (the control core's):
// rust/sim/src/ai/vehicles/replay.rs reads the same shapes.
export { hullView, turretView, leaseView, unitView, controllerView, characterView, agentView, patternsView, steerView, intentView, serviceState };
