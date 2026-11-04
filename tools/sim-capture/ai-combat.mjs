// The AI's perception and combat frames (tools/sim-capture/specs.mjs
// `aiCombat`): the combat services the control core calls (classic/index.js
// combatAdapter with combat.js classicBinding, or the Enhanced aim's), the
// combat agent's construction, ticks, exit and policies.
// rust/sim/tests/replay_ai_combat.rs reads them back into
// rust/sim/src/ai/combat.
//
// Every call these make outside their own state is a boundary: a unit's eye,
// aim, aim point and broadphase sphere (forwarded through GEOMETRY so the
// unit is an argument), the registry's query (with the filter's answers),
// the sight line, the vehicle AI's hooks, the navigation's cover and jet
// hints (hints named by their place in the map's list), the agent base's
// interrupts and movement helper (forwarded per helper), the active agent's
// policies (forwarded per agent), a soldier's arsenal selecting, the voice
// journal. A frame's state is what the code reads and writes, as views: the
// controllers, units, agents and the services' own state (the deferred
// line-of-sight queue, cluster timestamps, cover reservations, the aim
// hook, the Enhanced aim's skills).
//
// The forwarders this file installs call straight through: the battle runs
// as it would (tools/sim-capture.mjs checks the fingerprints). Nothing here
// imports src/combat.js or src/weapons.js (capture hazard: seeded draws).
import { clusterState } from '../../src/ai/classic/weapons-ai.js';
import { UnitCombatAgent, coverReservation } from '../../src/ai/classic/combat.js';
import { Agent } from '../../src/ai/classic/agents.js';
import { InterruptHelper } from '../../src/ai/classic/interrupts.js';
import { SeatUnit, VehicleTarget, TurretTarget } from '../../src/seats.js';
import { ObjectiveTarget } from '../../src/objectives.js';

const aiOf = (game) => game.ai?.classic?.ai ?? null;
const clone = (v) => (v && v.isVector3 ? v.clone() : v ? { x: v.x, y: v.y, z: v.z } : null);
const refView = (r) => (r ? { unit: r.unit ?? null, generation: r.generation } : null);

// ---- What the frames' code reads, as views.
const classView = (c) => (c && typeof c === 'object' ? {
  ForwardSpeed: c.ForwardSpeed, MaxSpeed: c.MaxSpeed, MaxHealth: c.MaxHealth, HealthType: c.HealthType, JetJump: c.JetJump,
  JetFuelMinBorder: c.JetFuelMinBorder, UnitType: c.UnitType, role: c.role, aiRole: c.ai?.role,
} : null);
const soldierView = (s) => (s ? {
  air: Boolean(s.air), posture: s.posture, shield: s.shield ? { on: Boolean(s.shield.on), max: s.shield.max, value: s.shield.value } : null,
  jet: Boolean(s.jet), jetJump: s.jet?.jump, jetMinBorder: s.jet?.minBorder, fuel: s.fuel, maxSpeed: s.maxSpeed,
} : null);
const weaponView = (w) => ({
  self: w, def: w.def, levels: w.levels ?? null, channel: w.channel, clip: w.clip, reserve: w.reserve, rounds: w.rounds, bottomless: Boolean(w.bottomless),
  heat: w.heat, overheated: Boolean(w.overheated), charging: Boolean(w.charging), held: w.held, wait: w.wait, limit: w.limit,
  reloading: Boolean(w.reloading), busy: Boolean(w.busy), spreadScale: w.spreadScale, sniperScale: w.sniperScale,
  yawSpreadOverride: w.yawSpreadOverride, kick: w.kick, floor: w.floor, enhancedFloor: Boolean(w.enhancedFloor), random: Boolean(w.random),
});
const arsenalView = (a) => ({
  kind: Object.getPrototypeOf(a) === Object.prototype ? 'seat' : 'soldier',
  weapons: a.weapons.map(weaponView),
  channels: a.channels.map((list) => (list ? list.map((w) => a.weapons.indexOf(w)) : null)),
  selected: [...a.selected],
});
const leaseView = (l) => (l ? {
  self: l, human: Boolean(l.human), released: Boolean(l.released), liveFalse: l.live === false,
  controllableController: l.controllable?.controller ?? null, unitController: l.unit?.controller ?? null,
} : null);
function bodyLeases(u) {
  const body = u.turret ?? u.vehicle ?? null;
  if (!body) return null;
  const leases = Array.isArray(body.seats) ? body.seats.map((s) => s.lease) : [body.lease];
  return leases.map((l) => leaseView(l ?? null));
}
// A unit (a registry member, a seat control, a round's frozen owner). Its
// arsenal only where the frame reads it (its own unit's).
function unitView(u, arsenal = false) {
  const tow = Boolean(u.vehicle) && u.vehicle.towTarget >= 0;
  return {
    self: u, id: u.id, generation: u.generation, alive: Boolean(u.alive), team: u.team, kind: u.kind, human: Boolean(u.human), hero: Boolean(u.hero),
    trap: Boolean(u.trap), targetableFalse: u.targetable === false, collisionCategory: u.collisionCategory,
    category: Number.isInteger(u.category) ? u.category : null,
    position: clone(u.position), velocity: u.velocity ? clone(u.velocity) : null,
    class: classView(u.classDef), combatMaxHealth: u.combat?.maxHealth, maxHealth: u.maxHealth,
    soldier: soldierView(u.soldier ?? null),
    arsenal: u.combat?.arsenal ? (arsenal ? arsenalView(u.combat.arsenal) : 'omitted') : null,
    hasOrigin: typeof u.origin === 'function', controller: u.controller ?? null, character: u.character ?? null,
    pilot: Boolean(u.pilot), vehicle: u.vehicle ?? null, towOn: tow, towed: tow ? (u.vehicle.system?.vehicles?.[u.vehicle.towTarget] ?? null) : null,
    turret: u.turret ?? null, buildingPost: u.building?.post ?? null, seat: u.seat ? leaseView(u.seat) : null, hasLease: Boolean(u.lease),
    bodyLeases: bodyLeases(u), intent: Boolean(u.intent), intentButtons: Boolean(u.intent?.buttons),
    buttons: u.intent?.buttons ? [u.intent.buttons[0], u.intent.buttons[1]] : null,
    aim: u.intent?.aim ? clone(u.intent.aim) : null,
  };
}
const heapView = (h, item) => ({
  count: h.count, dropped: h.dropped, priority: Array.from(h.priority.subarray(0, h.count + 1)), items: h.items.slice(1, h.count + 1).map(item),
});
const fireView = (f) => ({
  channel: f.channel, buttons: f.buttons.map((b) => ({ count: b.count, code: b.code })), lastRelease: f.lastRelease, charging: Boolean(f.charging),
  hold: f.hold, sniperActive: Boolean(f.sniperActive), sniperDeadline: f.sniperDeadline, burst: { ...f.burst },
  weaponTarget: refView(f.weaponTarget), protected: f.protected.map(refView),
});
const enhancedView = (a) => (a ? {
  skill: a.skill, direction: clone(a.direction), started: Boolean(a.started), error: [...a.error], bias: [...a.bias], key: a.key, lastKey: a.lastKey,
  lostAt: a.lostAt, acquiredAt: a.acquiredAt, readyAt: a.readyAt,
} : null);
const perceptionView = (p) => (p ? {
  heap: heapView(p.heap, refView), candidate: refView(p.candidate), candidatePoint: clone(p.candidatePoint),
  aimer: { direction: clone(p.aimer.direction), velocity: clone(p.aimer.velocity), started: Boolean(p.aimer.started), curves: { ...p.aimer.curves } },
  fire: fireView(p.fire), lastScanCount: p.lastScanCount, enhanced: enhancedView(p.enhancedAimer),
} : null);
const curvesView = (d) => (d ? { A: d.A, D: d.D, B: d.B, S: d.S } : null);
function ctlView(c) {
  const jet = c.navControl?.jet ?? c.jet;
  return {
    self: c, unit: c.unit ?? null, generation: c.generation, character: c.character ?? null, objective: c.objective ?? null,
    perception: perceptionView(c.perception), target: refView(c.target), fireA: Boolean(c.fireA), fireB: Boolean(c.fireB),
    roll: c.roll, rangeScale: c.rangeScale, aimState: c.aimState, aimPoint: clone(c.aimPoint), aimTarget: refView(c.aimTarget),
    stance: c.stance, navStance: c.navControl ? c.navControl.stance : undefined, facing: Boolean(c.facing), wantedMode: c.wantedMode,
    dodge: c.dodge, alertAnim: c.alertAnim,
    shot: { pending: Boolean(c.shot.pending), known: Boolean(c.shot.known), point: clone(c.shot.point), source: c.shot.source ?? null },
    danger: { pending: Boolean(c.danger.pending), point: clone(c.danger.point) },
    acoustic: c.acoustic.map(refView), difficulty: curvesView(c.difficulty),
    jet: jet ? { pending: Boolean(jet.pending), vector: clone(jet.vector) } : null,
  };
}
const trackerView = (t) => (t ? {
  controller: t.controller, ref: refView(t.ref), fresh: Boolean(t.fresh), point: clone(t.point), sampleTime: t.sampleTime, cost: t.cost,
  delay: t.delay, frozen: Boolean(t.frozen), counting: Boolean(t.counting), deadline: t.deadline,
} : null);
const reqView = (r) => (r ? { controller: r.controller, target: refView(r.target), generation: r.generation } : null);
const losView = (los) => (los ? { active: heapView(los.active, reqView), scratch: heapView(los.scratch, reqView), overflow: los.overflow, tested: los.tested } : null);

// Hints: plain objects of the map's list, named by their place in it.
let hintList = null;
const hintIndex = (h) => (h ? hintList.indexOf(h) : -1);
const hintView = (h) => (h ? {
  index: hintIndex(h), position: clone(h.position), forward: clone(h.forward), primaryMask: h.primaryMask, secondaryMask: h.secondaryMask,
  sideMask: h.sideMask, siblings: (h.siblings || []).map(hintIndex), reservation: coverReservation(h),
} : null);
const helperView = (h) => (h ? {
  self: h, arrived: Boolean(h.arrived), flying: Boolean(h.flying), follow: typeof h.follow === 'function', terminate: typeof h.terminate === 'function',
} : null);
function agentView(a) {
  if (!a) return null;
  if (!(a instanceof UnitCombatAgent)) return { self: a, combat: false, controller: a.controller, tracker: trackerView(a.tracker), helper: helperView(a.helper) };
  const p = a.patterns, c = p.cover;
  return {
    self: a, combat: true, controller: a.controller, state: a.state, pending: a.pending, tracker: trackerView(a.tracker), helper: helperView(a.helper),
    point: clone(a.point), hazard: clone(a.hazard), alignAt: a.alignAt, looks: a.looks, lookAngle: a.lookAngle, aligned: Boolean(a.aligned),
    hazardUntil: a.hazardUntil, proneUntil: a.proneUntil, pattern: a.pattern, stationary: Boolean(a.stationary),
    patterns: {
      self: p, state: p.state, pending: p.pending, probability: p.probability, coverRetry: p.coverRetry, prior: p.prior, pathActive: Boolean(p.pathActive),
      crouch: p.crouch, paused: Boolean(p.paused), pauseUntil: p.pauseUntil, toward: p.toward, side: p.side, bandCheckAt: p.bandCheckAt,
      stepAt: p.stepAt, facing: Boolean(p.facing), stepped: Boolean(p.stepped), lateralAt: p.lateralAt, lateralFacing: Boolean(p.lateralFacing),
      lastPath: p.lastPath, retreatAt: p.retreatAt, coverHint: hintView(p.coverHint), jetting: Boolean(p.jetting), jetRetry: p.jetRetry,
      jetStart: p.jetStart, jetHint: hintView(p.jetHint), hintJetRetry: p.hintJetRetry, searched: Boolean(p.searched), skipFire: Boolean(p.skipFire),
      cover: { state: c.state, pending: c.pending, hint: hintView(c.hint), posture: c.posture, deadline: c.deadline, checkAt: c.checkAt, reselectAt: c.reselectAt },
    },
  };
}
// The cluster timestamps the frame's code may read (by target: its
// controller's, else its own). Reading resets a stale human's, as the
// game's own read would first (weapons-ai.js clusterState).
function clusterView(u) {
  const s = u ? clusterState(u) : null;
  return s ? { target: u, scanAt: s.scanAt, flaggedAt: s.flaggedAt, generation: s.generation } : null;
}

// ---- The battle's settings the frames read.
let G = null; // the game
let world = null; // the AI's world (aiWorld)
const enhancedSkills = new WeakMap(); // Character -> the Enhanced skill drawn for it
let aimHooked = false;
let ingestFrom = null; // the journal's sequence number the next ingest reads from
function teamsView() {
  const u = G.units;
  return { relations: u.teams?.relations ?? null, alliances: [...u.alliances.entries()] };
}
function difficultyView() {
  const d = world.difficulty;
  return d ? { present: true, enemySkill: d.enemySkill, hard: d.hard, tier: d.tier, dangerRow: typeof d.dangerRow === 'function' ? [...d.dangerRow()] : null } : { present: false };
}
function boundView() {
  const ai = aiOf(G), nav = world.nav;
  return {
    hintsQuery: typeof nav?.hints?.query === 'function', queryHints: typeof ai.nav?.queryHints === 'function', jetHints: Boolean(nav?.jetHints),
    pointClear: typeof nav?.pointClear === 'function', segmentBlocked: typeof nav?.segmentBlocked === 'function',
    directClear: typeof nav?.directClear === 'function', inDangerRegion: typeof nav?.inDangerRegion === 'function',
    dispatchHook: typeof world.vehicles?.dispatch === 'function', sharedTarget: typeof world.vehicles?.sharedTarget === 'function',
    mountReach: typeof (ai.vehicles?.mountReach ?? world.vehicles?.mountReach) === 'function',
    currentControllable: typeof ai.vehicles?.currentControllable === 'function',
    enterPattern: typeof world.vehicles?.enterPattern === 'function',
    mission: { stayInTurrets: Boolean(world.mission?.stayInTurrets), noJetObstacleTest: Boolean(world.mission?.noJetObstacleTest) },
  };
}
function settingsView() {
  const enhanced = Boolean(G.ai?.enhanced);
  return {
    time: world.time ?? 0, teams: teamsView(), difficulty: difficultyView(), bound: boundView(), sides: Object.keys(world.classData ?? {}),
    aimHooked, enhanced: enhanced ? { tier: world.difficulty?.tier ?? null } : null, weaponData,
  };
}

// ---- The open frames of this spec, innermost last: what each wrote, so its
// end state writes the same again.
const opened = [];
function involve(frame, { ctls = [], units = [], agents = [], own = null }) {
  for (const c of ctls) if (c) frame.ctls.add(c);
  for (const u of units) if (u) frame.units.add(u);
  for (const a of agents) if (a) frame.agents.add(a);
  if (own) frame.own.add(own);
}
// The units a controller's state names (its unit, its targets, its
// candidate, its heap, its protected humans, its acoustic contacts, its
// alert source's body).
function ctlUnits(c) {
  const out = [c.unit, c.target?.unit, c.aimTarget?.unit, c.perception?.candidate?.unit, c.perception?.fire?.weaponTarget?.unit];
  if (c.perception) {
    for (let i = 1; i <= c.perception.heap.count; i++) out.push(c.perception.heap.items[i]?.unit);
    for (const r of c.perception.fire.protected) out.push(r.unit);
  }
  for (const r of c.acoustic) out.push(r?.unit);
  const src = c.shot?.source;
  if (src) {
    if (src.alive !== undefined && src.position) out.push(src);
    else {
      // (Which unit the source's control is, to write its view: the
      // capture never calls a function it records.)
      const lease = src.unit?.seat;
      out.push(src.unit, src.remote, lease && !lease.human && !lease.released ? lease.controllable : null);
    }
  }
  return out;
}
function frameState(frame, end) {
  const units = new Set(frame.units);
  for (const c of frame.ctls) for (const u of ctlUnits(c)) if (u) units.add(u);
  for (const a of frame.agents) {
    if (a.tracker?.ref?.unit) units.add(a.tracker.ref.unit);
    if (a.controller) for (const u of ctlUnits(a.controller)) if (u) units.add(u);
  }
  for (const c of frame.ctls) if (c.unit) frame.own.add(c.unit);
  for (const a of frame.agents) if (a.controller?.unit) frame.own.add(a.controller.unit);
  const ctls = new Set(frame.ctls);
  for (const a of frame.agents) if (a.controller) ctls.add(a.controller);
  // The Characters they name (a unit's, a controller's, an alert's source).
  const characters = new Set();
  for (const c of ctls) {
    if (c.character) characters.add(c.character);
    const src = c.shot?.source;
    if (src && !(src.alive !== undefined && src.position)) characters.add(src);
  }
  for (const u of units) if (u.character) characters.add(u.character);
  return {
    ...(end ? { time: world.time ?? 0, aimHooked } : settingsView()),
    ctls: [...ctls].map(ctlView),
    units: [...units].map((u) => unitView(u, frame.own.has(u))),
    agents: [...frame.agents].map(agentView),
    characters: [...characters].map((c) => ({ self: c, unit: c.unit ?? null, remote: c.remote ?? null, team: c.team })),
    clusters: [...units].map(clusterView).filter(Boolean),
    ...(frame.los ? { los: losView(world.deferredLos) } : {}),
    ...(frame.extra ? frame.extra(end) : {}),
  };
}

// ---- Forwarders that make a call's object an argument (GEOMETRY, HELPER,
// POLICY, HINTS): the boundaries are their methods.
const ORIGINAL = new Map(); // prototype -> { method: function }
function original(o, name) {
  for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) {
    const m = ORIGINAL.get(p)?.[name];
    if (m) return m;
  }
  throw Error(`sim-capture aiCombat: no original ${name}`);
}
export const GEOMETRY = {
  origin(u, out) { return original(u, 'origin').call(u, out); },
  forward(u, out) { return original(u, 'forward').call(u, out); },
  aimPoint(u, out) { return original(u, 'aimPoint').call(u, out); },
  sphere(u) { return original(u, 'sphere').call(u); },
};
function forwardGeometry(proto) {
  if (!proto || ORIGINAL.has(proto)) return;
  const own = {};
  for (const name of ['origin', 'forward', 'aimPoint']) {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    if (!d || typeof d.value !== 'function') continue;
    own[name] = d.value;
    Object.defineProperty(proto, name, { ...d, value: { [name](out) { return GEOMETRY[name](this, out); } }[name] });
  }
  const s = Object.getOwnPropertyDescriptor(proto, 'sphere');
  if (s?.get) {
    own.sphere = s.get;
    Object.defineProperty(proto, 'sphere', { ...s, get() { return GEOMETRY.sphere(this); } });
  }
  ORIGINAL.set(proto, own);
}
export const HELPER = {
  goto(h, ...a) { return Object.getPrototypeOf(h).goto.call(h, ...a); },
  stop(h, ...a) { return Object.getPrototypeOf(h).stop.call(h, ...a); },
  tick(h, ...a) { return Object.getPrototypeOf(h).tick.call(h, ...a); },
  follow(h, ...a) { return Object.getPrototypeOf(h).follow.call(h, ...a); },
  terminate(h, ...a) { return Object.getPrototypeOf(h).terminate.call(h, ...a); },
};
const forwardedHelpers = new WeakSet();
function forwardHelper(h) {
  if (!h || forwardedHelpers.has(h)) return;
  forwardedHelpers.add(h);
  for (const name of Object.keys(HELPER)) {
    if (typeof h[name] !== 'function' || Object.prototype.hasOwnProperty.call(h, name)) continue;
    Object.defineProperty(h, name, { value: (...a) => HELPER[name](h, ...a), writable: true, configurable: true, enumerable: false });
  }
}
export const POLICY = {
  acceptsAlert(a, ...x) { return Object.getPrototypeOf(a).acceptsAlert.call(a, ...x); },
  acceptsHazard(a, ...x) { return Object.getPrototypeOf(a).acceptsHazard.call(a, ...x); },
  acceptsAcoustic(a, ...x) { return Object.getPrototypeOf(a).acceptsAcoustic.call(a, ...x); },
};
const forwardedAgents = new WeakSet();
function forwardPolicies(a) {
  if (!a || forwardedAgents.has(a)) return;
  forwardedAgents.add(a);
  for (const name of Object.keys(POLICY)) {
    if (typeof a[name] !== 'function' || Object.prototype.hasOwnProperty.call(a, name)) continue;
    Object.defineProperty(a, name, { value: (...x) => POLICY[name](a, ...x), writable: true, configurable: true, enumerable: false });
  }
}
// The cover and jet hints by their place in the map's list.
export const HINTS = {
  query(set, mask, center, half, cap, owner, now) {
    const found = HINTS.base.call(set, mask, center, half, cap, [], owner, now);
    RESULTS.set('nav.hints.query', found);
    return found.map((h) => set.list.indexOf(h));
  },
};
export const JETS = {
  excluded(c) { return JETS.service.constructor.prototype.excluded.call(JETS.service, c); },
  find(c) { return hintIndex(JETS.service.constructor.prototype.find.call(JETS.service, c)); },
  enter(c, i) { return JETS.service.constructor.prototype.enter.call(JETS.service, c, i < 0 ? null : hintList[i]); },
  exit(c, i) { return JETS.service.constructor.prototype.exit.call(JETS.service, c, i < 0 ? null : hintList[i]); },
  tick(c, i, dt) { return JETS.service.constructor.prototype.tick.call(JETS.service, c, i < 0 ? null : hintList[i], dt); },
};
// The vehicle AI's dispatch without the AI's world (a plain object the
// writer would write whole).
export const VEHICLES = {
  dispatch(controller, unit, target, switchAllowed, out) { return VEHICLES.base.call(world.vehicles, controller, unit, target, switchAllowed, world, out); },
};

// The registry query's filter answers, for the replay to check its filter.
let lastQuery = null;

// ---- The service frames: each service the control core calls goes through
// FRAMES (a frame each), the frequent ones only for every SPREAD-th
// controller of a sampled tick (by its unit id and the tick), the rest
// straight to the service.
const SPREAD = 4;
// (A class's instance, so a frame's record names it rather than writing it.)
class AiCombatFrames {}
export const FRAMES = new AiCombatFrames();
const SERVICES = ['createPerception', 'attachController', 'detachController', 'rebuildProtected', 'scan', 'dropLosRequests', 'serviceLos',
  'broadcastAcoustic', 'clearCandidate', 'rankTargets', 'updateTarget', 'updateTargetSelection', 'gateFire', 'currentTarget', 'memoryPoint',
  'acquiredTarget', 'canEngage', 'hasWeapon', 'sensorRange', 'acousticRange', 'consumeFire', 'aimPoint', 'leadPoint', 'aim', 'ingest',
  'grenadeUpdate', 'hazardResponse', 'alertResponse', 'acousticResponse'];
const SPREAD_SERVICES = new Set(['consumeFire', 'aimPoint', 'aim', 'currentTarget', 'memoryPoint', 'acquiredTarget', 'hasWeapon', 'sensorRange',
  'acousticRange', 'canEngage', 'updateTargetSelection', 'gateFire']);
const ctlOfArgs = (name, args) => {
  const a0 = args[0];
  if (!a0) return null;
  if (a0.agent !== undefined && a0.commands) return a0; // a controller
  if (a0.controller && a0.tracker !== undefined) return a0.controller; // an agent
  return a0.controller ?? null;
};
function spreadPick(name, args) {
  const c = ctlOfArgs(name, args);
  const id = c?.unit?.id ?? 0;
  return (id + (G.worldClock?.tick ?? 0)) % SPREAD === 0;
}

let prepared = false;
let weaponData = [];
function prepare(game) {
  if (prepared) return;
  prepared = true;
  G = game;
  const ai = aiOf(game);
  if (!ai?.combat) return;
  world = ai.world;
  // Named content records are written once by the capture, then referenced.
  // Keep registration order, including shared class names on later sides.
  weaponData = Object.entries(world.classData ?? {}).map(([side, data]) => ({
    class: `aiWeapons:${side}`, base: 'aiWeaponData', weapons: data.weapons ?? {},
  }));
  hintList = world.nav?.hints?.list ?? [];
  // Geometry of every kind of unit the frames meet.
  forwardGeometry(Object.getPrototypeOf(game.playerUnit));
  for (const C of [SeatUnit, VehicleTarget, TurretTarget, ObjectiveTarget]) forwardGeometry(C.prototype);
  for (const u of game.units.all()) forwardGeometry(Object.getPrototypeOf(u));
  // The registry's query, its filter's answers kept.
  const units = game.units, query = units.query;
  units.query = function (center, radius, out = [], filter = null) {
    if (!filter) {
      const r = query.call(this, center, radius, out, filter);
      lastQuery = { tried: null, result: [...r] };
      return r;
    }
    const tried = [];
    const r = query.call(this, center, radius, out, (u) => {
      const answer = filter(u);
      tried.push([u, u.team, Boolean(answer)]);
      return answer;
    });
    lastQuery = { tried, result: [...r] };
    return r;
  };
  // Hints by index.
  const hints = world.nav?.hints;
  if (hints?.query) {
    HINTS.base = hints.query;
    hints.query = function (mask, center, half, cap, out = [], owner = null, now = 0) {
      const found = HINTS.query(this, mask, center, half, cap, owner, now);
      out.length = 0;
      for (const i of found) out.push(this.list[i]);
      return out;
    };
  }
  const jets = world.nav?.jetHints;
  if (jets) {
    JETS.service = jets;
    jets.excluded = (c) => JETS.excluded(c);
    jets.find = (c) => { const i = JETS.find(c); return i < 0 ? null : hintList[i]; };
    jets.enter = (c, h) => JETS.enter(c, hintIndex(h));
    jets.exit = (c, h) => JETS.exit(c, hintIndex(h));
    jets.tick = (c, h, dt) => JETS.tick(c, hintIndex(h), dt);
  }
  if (typeof world.vehicles?.dispatch === 'function') {
    VEHICLES.base = world.vehicles.dispatch;
    world.vehicles.dispatch = function (controller, unit, target, switchAllowed, _world, out) { return VEHICLES.dispatch(controller, unit, target, switchAllowed, out); };
  }
  // The services, through FRAMES; the always-on bookkeeping (the aim hook,
  // the Enhanced skills, the journal's cursor) inside.
  const services = ai.combat;
  for (const name of SERVICES) {
    const base = services[name];
    if (typeof base !== 'function') continue;
    let inner = base;
    if (name === 'createPerception') inner = (...a) => { const r = base(...a); RESULTS.set('createPerception', r); return r; };
    else if (name === 'aim') inner = (...a) => { const r = base(...a); aimHooked = true; noteSkill(a[0]); return r; };
    else if (name === 'consumeFire') inner = (...a) => { const r = base(...a); noteSkill(a[0]); return r; };
    else if (name === 'ingest') inner = (...a) => { const r = base(...a); ingestFrom = world.events?.seq ?? null; return r; };
    FRAMES[name] = inner;
    services[name] = SPREAD_SERVICES.has(name) ? (...a) => (spreadPick(name, a) ? FRAMES[name](...a) : inner(...a)) : (...a) => FRAMES[name](...a);
  }
  // The combat agent's construction.
  FRAMES.createCombatAgent = (controller, goal) => (created = new UnitCombatAgent(controller, goal));
  // Calls whose answer names something the frame then reads: the answer
  // kept for the boundary's record.
  const keep = (target, name, label) => {
    const base = target?.[name];
    if (typeof base !== 'function') return;
    target[name] = function (...a) { const r = base.apply(this, a); RESULTS.set(label, r); return r; };
  };
  keep(world.vehicles, 'sharedTarget', 'vehicles.sharedTarget');
  keep(ai.vehicles, 'currentControllable', 'vehicles.currentControllable');
  keep(ai, 'createHelper', 'ai.createHelper');
  ai.agents.combat = function CombatAgentFactory(controller, goal) { return FRAMES.createCombatAgent(controller, goal); };
  for (const [label, target, name] of [
    ['vehicles.dispatch', VEHICLES, 'dispatch'],
    ...['enterPattern', 'exitPattern', 'tickPattern', 'sidestep'].map((name) => [`vehicles.${name}`, world.vehicles, name]),
    ['interrupts.tick', InterruptHelper.prototype, 'tick'],
    ['arsenal.select', Object.getPrototypeOf(game.playerUnit?.combat?.arsenal ?? {}), 'select'],
    ...Object.keys(HELPER).map((name) => [`helper.${name}`, HELPER, name]),
    ...['find', 'enter', 'exit', 'tick'].map((name) => [`nav.jetHints.${name}`, JETS, name]),
  ]) {
    const base = target?.[name];
    if (typeof base !== 'function') continue;
    target[name] = function (...args) {
      const before = after();
      try { return base.apply(this, args); }
      finally { BEFORE.set(label, before); }
    };
  }
}
function noteSkill(c) {
  const a = c?.perception?.enhancedAimer;
  if (a && c.character) enhancedSkills.set(c.character, a.skill);
}

// ---- Frame states.
// A frame's own state: which controllers, units, agents it names, and
// what else it reads.
function serviceFrame(name, decide) {
  return {
    method: `aiCombat.${name}`,
    target: () => FRAMES,
    state: (game, args, cap) => {
      const end = opened.length && opened[opened.length - 1].args === args;
      let frame;
      if (end) frame = opened.pop();
      else {
        frame = { args, ctls: new Set(), units: new Set(), agents: new Set(), own: new Set(), los: false, extra: null };
        decide(frame, args);
        opened.push(frame);
      }
      return frameState(frame, end);
    },
    boundaries,
  };
}
const agentArg = (frame, a) => {
  if (!a) return;
  involve(frame, { agents: [a] });
  forwardPolicies(a);
  forwardHelper(a.helper);
};
const SERVICE_FRAMES = [
  serviceFrame('createPerception', (f, [c]) => {
    involve(f, { ctls: [c] });
    f.extra = (end) => end ? { createdPerception: perceptionView(RESULTS.get('createPerception')) } : {};
  }),
  serviceFrame('attachController', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('detachController', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('rebuildProtected', (f) => {
    // Every living member, and every controller a query can reach.
    f.extra = (end) => ({ living: end ? null : [...G.units.living()].map((u) => unitView(u)) });
    involve(f, { ctls: aiOf(G).controllers });
  }),
  serviceFrame('scan', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('dropLosRequests', (f, [c]) => { involve(f, { ctls: losControllers(c) }); f.los = true; }),
  serviceFrame('serviceLos', (f) => { involve(f, { ctls: losControllers(null) }); f.los = true; }),
  serviceFrame('broadcastAcoustic', (f, [u]) => { involve(f, { units: [u], own: u }); f.collect = true; }),
  serviceFrame('clearCandidate', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('rankTargets', (f, [a]) => { agentArg(f, a); f.los = true; }),
  serviceFrame('updateTarget', (f, [a]) => agentArg(f, a)),
  serviceFrame('updateTargetSelection', (f, [a]) => agentArg(f, a)),
  serviceFrame('gateFire', (f, [a]) => agentArg(f, a)),
  serviceFrame('currentTarget', (f, [a]) => agentArg(f, a)),
  serviceFrame('memoryPoint', (f, [a]) => agentArg(f, a)),
  serviceFrame('acquiredTarget', (f, [a]) => agentArg(f, a)),
  serviceFrame('canEngage', (f, [c, t]) => involve(f, { ctls: [c], units: [t] })),
  serviceFrame('hasWeapon', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('sensorRange', (f, [c, t]) => involve(f, { ctls: [c], units: [t] })),
  serviceFrame('acousticRange', (f, [c]) => involve(f, { ctls: [c] })),
  serviceFrame('consumeFire', (f, [c, u]) => { involve(f, { ctls: [c], units: [u], own: u }); f.extra = enhancedExtra(c); }),
  serviceFrame('aimPoint', (f, [c, u]) => involve(f, { ctls: [c], units: [u], own: u })),
  serviceFrame('leadPoint', (f, [c, t, dt, out]) => {
    involve(f, { ctls: [c], units: [t] });
    f.extra = () => ({ leadOut: clone(out) });
  }),
  serviceFrame('aim', (f, [c]) => { involve(f, { ctls: [c] }); f.extra = enhancedExtra(c); }),
  serviceFrame('ingest', (f) => {
    const evs = journalEvents();
    for (const e of evs ?? []) for (const k of ['source', 'target', 'attacker']) if (e[k]?.unit) f.units.add(e[k].unit);
    for (const e of evs ?? []) {
      for (const k of ['source', 'target']) {
        const u = e[k]?.unit;
        if (u?.controller) f.ctls.add(u.controller);
        for (const l of u ? (bodyLeases(u) ?? []) : []) if (l?.controllableController) f.ctls.add(l.controllableController);
        if (u?.seat?.controllable?.controller) f.ctls.add(u.seat.controllable.controller);
      }
    }
    f.extra = (end) => (end ? {} : { events: evs ? evs.map(eventView) : null });
  }),
  serviceFrame('grenadeUpdate', (f, [round, position, owner]) => {
    if (owner) f.units.add(owner);
    f.collect = true;
    f.extra = (end) => ({ danger: round.aiDanger ? { firstContact: round.aiDanger.firstContact, lastNotify: round.aiDanger.lastNotify } : null });
  }),
  serviceFrame('hazardResponse', (f, [c, a]) => { involve(f, { ctls: [c] }); agentArg(f, a); }),
  serviceFrame('alertResponse', (f, [c, a]) => { involve(f, { ctls: [c] }); agentArg(f, a); }),
  serviceFrame('acousticResponse', (f, [c, a]) => { involve(f, { ctls: [c] }); agentArg(f, a); }),
];
function enhancedExtra(c) {
  return () => {
    if (!G.ai?.enhanced) return {};
    const character = c?.character ?? null;
    return { skill: character && enhancedSkills.has(character) ? enhancedSkills.get(character) : null };
  };
}
// Every controller a line-of-sight request names, and the asking one.
function losControllers(c) {
  const out = new Set(c ? [c] : []);
  const los = world.deferredLos;
  for (const h of [los.active, los.scratch]) for (let i = 1; i <= h.count; i++) if (h.items[i]?.controller) out.add(h.items[i].controller);
  return [...out];
}
// The journal's events the next ingest reads (src/sim-events.js: the
// reader's cursor is where the last ingest left the journal).
function journalEvents() {
  const j = world.events;
  if (!j || ingestFrom === null) return null;
  const out = [];
  let cursor = ingestFrom;
  if (j.seq - cursor > j.capacity) cursor = j.seq - j.capacity;
  for (; cursor < j.seq; cursor++) {
    const e = j.ring[cursor % j.capacity];
    if (e && (e.kind === 'shot' || e.kind === 'damage')) out.push(e);
  }
  return out;
}
const eventView = (e) => ({
  kind: e.kind, source: refView(e.source), target: refView(e.target), attacker: refView(e.attacker), attackerCharacter: e.attackerCharacter ?? null,
  amount: e.amount, generic: e.generic, typed: e.typed, known: Boolean(e.known), point: e.point ?? null,
});

// The combat agent's frames: its construction, its ticks, its exit, its
// policies (called by the interrupts and the responses).
function agentFrame(method, decide = () => {}) {
  return {
    method: `UnitCombatAgent.${method}`,
    target: () => UnitCombatAgent.prototype,
    state: (game, args, cap, self) => {
      const end = opened.length && opened[opened.length - 1].args === args;
      let frame;
      if (end) frame = opened.pop();
      else {
        frame = { args, ctls: new Set(), units: new Set(), agents: new Set(), own: new Set(), los: method === 'tick' && args[1] !== 0, extra: null };
        agentArg(frame, self);
        decide(frame, args, self);
        opened.push(frame);
      }
      return frameState(frame, end);
    },
    boundaries,
  };
}
const AGENT_FRAMES = [
  agentFrame('tick'),
  agentFrame('exit'),
  agentFrame('acceptsAlert'),
  agentFrame('acceptsHazard'),
  agentFrame('acceptsAcoustic', (f, [source]) => { if (source) f.units.add(source); }),
  agentFrame('acceptsTarget', (f, [target]) => { if (target) f.units.add(target); }),
  agentFrame('acceptsBoard', (f, [candidate]) => { if (candidate) f.units.add(candidate); }),
  agentFrame('acceptsRecovery'),
  agentFrame('savesOnInterrupt'),
];
const CREATE_FRAME = {
  method: 'aiCombat.createCombatAgent',
  target: () => FRAMES,
  state: (game, args) => {
    const end = opened.length && opened[opened.length - 1].args === args;
    let frame;
    if (end) frame = opened.pop();
    else {
      const [c, goal] = args;
      frame = { args, ctls: new Set([c]), units: new Set([goal?.refA?.unit]), agents: new Set(), own: new Set(), los: false,
        extra: (atEnd) => (atEnd ? {} : { goal: goal ? { mode: goal.mode, point: clone(goal.point), refA: refView(goal.refA) } : null }) };
      frame.units.delete(undefined);
      opened.push(frame);
    }
    const s = frameState(frame, end);
    if (end) s.created = agentView(lastCreated());
    return s;
  },
  boundaries,
};
let created = null;
const lastCreated = () => created;

// ---- The boundaries.
const RESULTS = new Map(); // label -> the last answer of a kept call
const BEFORE = new Map(); // label -> the state immediately before that call
const top = () => opened[opened.length - 1] ?? null;
// After a call that can change what the frame holds: the frame's views again.
const after = () => {
  const f = top();
  return f ? frameState(f, true) : {};
};
// The query's answers to the filter, its members' views; for the frames
// whose code writes to the controllers it finds, those controllers too.
const queryAfter = () => {
  const f = top();
  const q = lastQuery;
  lastQuery = null;
  const result = q?.result ?? [];
  if (f?.collect) for (const u of result) if (u.controller) f.ctls.add(u.controller);
  return {
    tried: q?.tried ? q.tried.map(([u, team, answer]) => ({ unit: u, team, answer })) : null,
    views: result.map((u) => unitView(u)),
    ctls: f?.collect ? result.filter((u) => u.controller).map((u) => ctlView(u.controller)) : null,
  };
};
const unitAfter = (label) => () => {
  const u = RESULTS.get(label);
  return { view: u ? unitView(u) : null, after: after() };
};
const hintsAfter = () => ({ hints: (RESULTS.get('nav.hints.query') ?? []).map(hintView) });
const effectsAfter = (label) => () => ({ before: BEFORE.get(label), after: after() });
function boundaries(game) {
  prepare(game);
  const ai = aiOf(game);
  if (!ai?.combat) return [];
  const proto = (o) => (o ? Object.getPrototypeOf(o) : null);
  const vehicles = world.vehicles ?? null, bag = ai.vehicles ?? null;
  return [
    ['unit.origin', GEOMETRY, 'origin'],
    ['unit.forward', GEOMETRY, 'forward'],
    ['unit.aimPoint', GEOMETRY, 'aimPoint'],
    ['unit.sphere', GEOMETRY, 'sphere'],
    ['units.query', game.units, 'query', queryAfter],
    ['world.los', world, 'los'],
    ['vehicles.sharedTarget', vehicles, 'sharedTarget', unitAfter('vehicles.sharedTarget')],
    ['vehicles.dispatch', VEHICLES, 'dispatch', effectsAfter('vehicles.dispatch')],
    ['vehicles.enterPattern', vehicles, 'enterPattern', effectsAfter('vehicles.enterPattern')],
    ['vehicles.exitPattern', vehicles, 'exitPattern', effectsAfter('vehicles.exitPattern')],
    ['vehicles.tickPattern', vehicles, 'tickPattern', effectsAfter('vehicles.tickPattern')],
    ['vehicles.sidestep', vehicles, 'sidestep', effectsAfter('vehicles.sidestep')],
    ['vehicles.mountReach', bag, 'mountReach'],
    ['vehicles.currentControllable', bag, 'currentControllable', unitAfter('vehicles.currentControllable')],
    ['agent.boardPolicy', Agent.prototype, 'boardPolicy'],
    ['nav.hints.query', HINTS, 'query', hintsAfter],
    ['nav.jetHints.excluded', JETS, 'excluded'],
    ['nav.jetHints.find', JETS, 'find', effectsAfter('nav.jetHints.find')],
    ['nav.jetHints.enter', JETS, 'enter', effectsAfter('nav.jetHints.enter')],
    ['nav.jetHints.exit', JETS, 'exit', effectsAfter('nav.jetHints.exit')],
    ['nav.jetHints.tick', JETS, 'tick', effectsAfter('nav.jetHints.tick')],
    ['interrupts.tick', InterruptHelper.prototype, 'tick', effectsAfter('interrupts.tick')],
    ['helper.goto', HELPER, 'goto', effectsAfter('helper.goto')],
    ['helper.stop', HELPER, 'stop', effectsAfter('helper.stop')],
    ['helper.tick', HELPER, 'tick', effectsAfter('helper.tick')],
    ['helper.follow', HELPER, 'follow', effectsAfter('helper.follow')],
    ['helper.terminate', HELPER, 'terminate', effectsAfter('helper.terminate')],
    ['agent.acceptsAlert', POLICY, 'acceptsAlert'],
    ['agent.acceptsHazard', POLICY, 'acceptsHazard'],
    ['agent.acceptsAcoustic', POLICY, 'acceptsAcoustic'],
    ['arsenal.select', proto(game.playerUnit?.combat?.arsenal), 'select', effectsAfter('arsenal.select')],
    ['ai.stayInTurrets', ai, 'stayInTurrets'],
    ['ai.curvesFor', ai, 'curvesFor'],
    ['ai.createHelper', ai, 'createHelper', () => ({ helper: helperView(RESULTS.get('ai.createHelper') ?? null) })],
    ['events.emit', world.events ?? null, 'emit'],
  ];
}

export const aiCombat = {
  frames: [...SERVICE_FRAMES, ...AGENT_FRAMES, CREATE_FRAME],
};
