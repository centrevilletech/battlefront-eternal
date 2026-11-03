// The Classic AI control core's capture spec (plan-claude row P15): the
// AI's whole decide-and-execute step (ClassicAI.update), a controller's
// bind and unbind, and the objective reactions to post events. State is
// hand-built values: controllers and agents have no store key and no home
// list (rust/sim/schema/ai.mjs declares the controller's shell for the
// flip; agents are eleven state machines and two other packages'), so
// this writes what rust/sim/src/ai/control holds, in its shapes.
// Everything outside the core crosses labelled boundary calls mirroring
// src/ai/control/world.rs's AiWorld: the service bags (combat, vehicles,
// nav), the movement helpers, the foreign agents (combat, board,
// unboard, the flyer), the registry query, the repair candidates, the
// mine ground probe and the voice journal. Population and the class jobs
// are the core's own and run natively in the replay; their weapon reads
// cross at world.weapons (src/game.js).
import { Vector3 } from 'three';
import { UnitController } from '../../src/ai/classic/controller.js';
import { ClassicScheduler } from '../../src/ai/classic/scheduler.js';
import { postAI } from '../../src/ai/classic/goals.js';
import { repairServicesOf } from '../../src/ai/classic/classes.js';
import { UnitCombatAgent } from '../../src/ai/classic/combat.js';
import { UnitBoardAgent, UnitUnBoardAgent } from '../../src/ai/classic/vehicles/agents.js';
import { GroundGotoHelper, FlyGotoHelper as NavFlyGotoHelper } from '../../src/ai/classic/nav/goto.js';
import { FlyGotoHelper as VehicleFlyGotoHelper } from '../../src/ai/classic/vehicles/fly.js';
import { StationaryHelper } from '../../src/ai/classic/vehicles/index.js';

const HEALTH_TYPES = ["Person", "Animal", "Droid", "Vehicle", "Building", "BuildingRebuild", "BuildingBuild"];

let current = null; // the controller whose update/service/input is running
let acousticLens = new Map(); // controller -> acoustic count last recorded

const v3 = (p) => (p ? [p.x, p.y, p.z] : null);
const num = (v) => (typeof v === 'number' ? v : null);
const own = (o, k) => {
  const d = o ? Object.getOwnPropertyDescriptor(o, k) : null;
  return d && 'value' in d ? d.value : undefined;
};

// ---- the boundary-routing shims spec code cannot do without ------------
// Installed here (specs' boundaries() may prepare the game, as
// tools/sim-capture/units.mjs replaces collision.gather):
// - `current` tracking on the controller-facing entry points, so an
//   extra can name the controller a call was for;
// - Units.query's filter answers (a closure cannot cross: the calls
//   and their answers are recorded, the replay re-runs its own filter
//   against them);
// - foreign agents: agentFor's factories for the combat, board,
//   unboard and fly agents are wrapped so their construction, ticks
//   and exits run through `foreignHooks` (the boundary/frame targets
//   below). The instances created before this runs are documented by
//   their state alone.
const foreignHooks = {
  create(kind, make) { return make(); },
  tick(agent, dt, phase) { return agent.__aiTick(dt, phase); },
  exit(agent) { agent.__aiExit(); },
};
const queryFilterLog = [];
let prepared = false;

function prepare(game) {
  if (prepared) return;
  prepared = true;
  const ai = game.ai?.classic?.ai;
  if (!ai) return;
  const withCurrent = (controller, fn, thisArg, args) => {
    const before = current;
    current = controller;
    try { return fn.apply(thisArg, args); } finally { current = before; }
  };
  const ctlUpdate = UnitController.prototype.update;
  UnitController.prototype.update = function (...args) {
    return withCurrent(this, ctlUpdate, this, args);
  };
  const service = ClassicScheduler.prototype.service;
  ClassicScheduler.prototype.service = function (controller, ...args) {
    return withCurrent(controller, service, this, [controller, ...args]);
  };
  const executeInput = ClassicScheduler.prototype.executeInput;
  ClassicScheduler.prototype.executeInput = function (controller, ...args) {
    return withCurrent(controller, executeInput, this, [controller, ...args]);
  };
  const unitsProto = Object.getPrototypeOf(game.units);
  const query = unitsProto.query;
  unitsProto.query = function (center, radius, out = [], filter = null) {
    if (!filter) return query.call(this, center, radius, out, filter);
    const scope = [];
    const result = query.call(this, center, radius, out, (u) => {
      const answer = filter(u);
      scope.push([u, Boolean(answer)]);
      return answer;
    });
    queryFilterLog.push(scope);
    if (queryFilterLog.length > 64) queryFilterLog.shift();
    return result;
  };
  // Foreign agents through the hooks: agentFor is the (possibly
  // vehicle-adapted) instance method; wrap what it returns when the
  // bind put a foreign class on the state (combat 2, board 5,
  // unboard 6; the flyer's state 7 comes only from a mounted policy).
  const wrapFactory = (kind, factory) => (controller, goal) => {
    const agent = foreignHooks.create(kind, () => factory(controller, goal));
    return wrapAgent(kind, agent, controller);
  };
  const wrapAgent = (kind, agent, controller) => {
    if (!agent || agent.__aiForeignKind) return agent;
    const tick = agent.tick, exit = agent.exit;
    Object.defineProperty(agent, '__aiForeignKind', { value: kind });
    Object.defineProperty(agent, '__aiTick', { value: tick.bind(agent) });
    Object.defineProperty(agent, '__aiExit', { value: typeof exit === 'function' ? exit.bind(agent) : () => {} });
    agent.tick = function (dt, phase) {
      const before = current;
      current = controller;
      try { return foreignHooks.tick(agent, dt, phase); } finally { current = before; }
    };
    agent.exit = function () {
      const before = current;
      current = controller;
      try { foreignHooks.exit(agent); } finally { current = before; }
    };
    return agent;
  };
  for (const controller of ai.controllers ?? []) {
    const a = controller.agent;
    if (!a) continue;
    const kind = a instanceof UnitCombatAgent ? 'combat' : a instanceof UnitBoardAgent ? 'board' : a instanceof UnitUnBoardAgent ? 'unboard' : a.constructor?.name === 'UnitFlyAgent' ? 'fly' : null;
    if (kind) wrapAgent(kind, a, controller);
  }
  const agentFor = ai.agentFor.bind(ai);
  ai.agentFor = (state, controller) => {
    const factory = agentFor(state, controller);
    if (!factory) return factory;
    const kind = foreignKindFor(ai, state);
    return kind ? wrapFactory(kind, factory) : factory;
  };
}

function foreignKindFor(ai, state) {
  if (state === 2 && ai.agents?.combat === UnitCombatAgent) return 'combat';
  if (state === 5 && ai.agents?.board === UnitBoardAgent) return 'board';
  if (state === 6 && ai.agents?.unboard === UnitUnBoardAgent) return 'unboard';
  if (state === 7) return 'fly';
  return null;
}

// ---- snapshots -------------------------------------------------------------

const _v = new Vector3();

function qtySnap(def) {
  if (!def) return { svcH: null, svcA: null, namedH: [], namedA: [], bare: [] };
  const named = (suffix) => HEALTH_TYPES.map((t) => num(own(def, t + suffix) ?? (t + suffix in def ? def[t + suffix] : undefined)) ?? null);
  const svc = (kind) => {
    const arr = def.service?.[kind];
    return arr ? HEALTH_TYPES.map((_, i) => num(arr[i]) ?? null) : null;
  };
  return {
    svcH: svc('health'), svcA: svc('ammo'),
    namedH: named('Health'), namedA: named('Ammo'),
    bare: HEALTH_TYPES.map((t) => num(def[t]) ?? null),
  };
}

function weaponSnap(world, unit, w) {
  const def = w.def ?? {};
  const item = def.item ?? null;
  let ownedCount = 0;
  if (def.base === 'dispenser' && world.items?.ownedBy) {
    ownedCount = world.items.ownedBy(def.class, unit).filter((i) => !i.gone).length;
  }
  return {
    base: def.base ?? null,
    clip: typeof w.clip === 'function' ? w.clip() : num(w.clip),
    reserve: num(w.reserve),
    rounds: num(w.rounds),
    bottomless: Boolean(w.bottomless),
    defAmmo: num(def.ammo),
    defClass: def.class ?? null,
    maxItems: num(def.MaxItems),
    itemBase: item?.base ?? null,
    itemClass: item?.class ?? null,
    itemLifeMissing: !item || item.LifeSpan == null || item.LifeSpan === Infinity || item.LifeSpan === 3.4028234663852886e38,
    qtyDef: qtySnap(def),
    qtyItem: qtySnap(item),
    ownedCount,
  };
}

function classSnap(def) {
  if (!def) return null;
  const size = def.AISizeType;
  return {
    role: num(def.role),
    unitType: typeof def.UnitType === 'string' ? def.UnitType : null,
    sizeText: typeof size === 'string' ? size : null,
    sizeNum: typeof size === 'number' ? size : null,
    bounds: Array.isArray(def.bounds) && def.bounds.length === 2 ? def.bounds : null,
    maxSpeed: num(def.MaxSpeed), maxStrafeSpeed: num(def.MaxStrafeSpeed), strafeSpeed: num(def.StrafeSpeed),
    maxTurnSpeed: num(def.MaxTurnSpeed), maxPitchSpeed: num(def.MaxPitchSpeed),
    forwardSpeed: num(def.ForwardSpeed), turnRate: num(def.TurnRate), pitchRate: num(def.PitchRate),
    maxHealth: num(def.MaxHealth),
    healthNum: num(def.HealthType), healthName: typeof def.HealthType === 'string' ? def.HealthType : null,
    base: def.base ?? null,
  };
}

function soldierSnap(s) {
  if (!s) return null;
  return {
    aimYaw: num(s.aimYaw), aimPitch: num(s.aimPitch),
    maxSpeed: num(s.maxSpeed), maxStrafe: num(s.maxStrafe), maxTurn: num(s.maxTurn), maxPitch: num(s.maxPitch),
    posture: typeof s.posture === 'string' ? s.posture : null,
    mode: num(s.mode), rolled: typeof s.rolled === 'boolean' ? s.rolled : null,
  };
}

function unitSnap(ctx, u) {
  const world = ctx.game.ai.classic.ai.world;
  const snap = {
    self: u,
    id: u.id ?? 0, generation: u.generation ?? 0, kind: u.kind ?? 0, team: u.team ?? 0,
    alive: Boolean(u.alive), removed: Boolean(u.removed), human: Boolean(u.human), hero: Boolean(u.hero),
    targetable: typeof u.targetable === 'boolean' ? u.targetable : null,
    position: v3(u.position),
    velocity: u.velocity ? v3(u.velocity) : null,
    soldier: soldierSnap(u.soldier ?? null),
    classDef: classSnap(u.classDef ?? null),
    className: u.className ?? null,
    weapons: (u.combat?.arsenal?.weapons ?? []).map((w) => weaponSnap(world, u, w)),
    combatHealth: num(u.combat?.health), health: num(own(u, 'health') ?? undefined) ?? num(u.health),
    combatMax: num(u.combat?.maxHealth), maxHealth: num(own(u, 'maxHealth') ?? undefined) ?? num(u.maxHealth),
    // The lazy sphere: its bounds as they stand, and whether they are
    // stale (the replay takes the frame-end bounds then, the value the
    // game's own read computed).
    boundsRadius: num(u.bounds?.radius),
    spheresStale: u.spheresTick !== undefined && u.tickCount !== undefined ? u.spheresTick !== u.tickCount : false,
    radius: num(own(u, 'radius') ?? undefined) ?? num(u.radius),
    navRadius: num(u.navRadius), navMask: num(u.navMask),
    seat: Boolean(u.seat),
    seatVehiclePos: u.seat?.vehicle ? v3(u.seat.vehicle.position) : null,
    hasLease: Boolean(u.lease), leaseIsSelf: Boolean(u.lease && u.lease.controllable === u),
    isSeatUnit: u.constructor?.name === 'SeatUnit',
    hasTurret: Boolean(u.turret),
    mine: Boolean(u.mine),
    character: u.character ? ctx.characterIndex.get(u.character) ?? null : null,
    humanPiloted: Boolean(u.humanPiloted),
    intent: intentSnap(u.intent),
    forward: typeof u.forward === 'function' ? v3(u.forward(_v)) : null,
    origin: typeof u.origin === 'function' ? v3(u.origin(_v)) : null,
    aimPoint: typeof u.aimPoint === 'function' ? v3(u.aimPoint(_v)) : null,
    buildingAlive: u.building ? Boolean(u.building.alive) : null,
  };
  return snap;
}

function intentSnap(intent) {
  if (!intent) return null;
  return {
    forward: intent.forward ?? 0, right: intent.right ?? 0, turn: intent.turn ?? 0, pitch: intent.pitch ?? 0,
    jump: Boolean(intent.jump), crouch: Boolean(intent.crouch), prone: Boolean(intent.prone), use: Boolean(intent.use),
    buttons: intent.buttons ? [intent.buttons[0] ?? 0, intent.buttons[1] ?? 0] : [0, 0],
    reload: Boolean(intent.reload), nextPrimary: Boolean(intent.nextPrimary), previousPrimary: Boolean(intent.previousPrimary),
    nextSecondary: Boolean(intent.nextSecondary), zoom: Boolean(intent.zoom), exit: Boolean(intent.exit),
    aim: intent.aim ? v3(intent.aim) : null,
    jetDirection: intent.jetDirection ? v3(intent.jetDirection) : null,
    brake: intent.brake === undefined ? null : Boolean(intent.brake),
    up: num(intent.up), down: num(intent.down),
  };
}

function refSnap(ctx, ref) {
  if (!ref || !ref.unit) return null;
  return { u: ctx.keyOf(ref.unit), g: ref.generation ?? 0 };
}

function goalSnap(ctx, goal) {
  if (!goal) return null;
  return {
    kind: goal.kind ?? 0,
    refA: refSnap(ctx, goal.refA), refB: refSnap(ctx, goal.refB),
    post: goal.post ? ctx.postIndex.get(goal.post) ?? null : null,
    point: goal.point ? v3(goal.point) : null,
    mode: goal.mode ?? 0,
  };
}

function helperSnap(helper) {
  if (!helper) return { kind: null };
  const name = helper.constructor?.name ?? '';
  const base = {
    kind: name,
    arrived: Boolean(helper.arrived), failed: Boolean(helper.failed),
    status: helper.status ?? 0, hasPath: Boolean(helper.hasPath),
  };
  if (name === 'DirectHelper') {
    return { ...base, target: v3(helper.target), active: Boolean(helper.active), radius: helper.radius ?? 1 };
  }
  return base;
}

function controlSnap(control) {
  if (!control) return null;
  const request = control.request ?? {};
  return {
    status: control.status ?? 0,
    hasExecutor: Boolean(control.executor),
    avoidSide: control.avoidSide ?? 0,
    stance: control.stance ?? 0,
    jet: { pending: Boolean(control.jet?.pending), vector: v3(control.jet?.vector) ?? [0, 0, 0] },
    request: {
      valid: Boolean(request.valid),
      steering: v3(request.steeringTarget) ?? [0, 0, 0],
      waypoint: v3(request.waypoint) ?? [0, 0, 0],
      direction: v3(request.segmentDirection) ?? [0, 0, 0],
      mode: request.mode ?? 0, lower: request.lower ?? -1, upper: request.upper ?? 1,
    },
  };
}

function patrolSnap(ctx, p) {
  return {
    state: p.state ?? 0, pending: p.pending ?? 0,
    post: p.post ? ctx.postIndex.get(p.post) ?? null : null,
    hint: p.hint ? ctx.hintIndex.get(p.hint) ?? null : null,
    startTime: p.startTime ?? 0, look: Boolean(p.look), lookPoint: v3(p.lookPoint) ?? [0, 0, 0],
    stance: p.stance ?? 0, expires: p.expires ?? 0,
    candidates: (p.candidates ?? []).map((h) => ctx.hintIndex.get(h) ?? -1),
    centered: Boolean(p.centered),
  };
}

function snipeSnap(ctx, s, full) {
  return {
    state: s.state ?? 0, pending: s.pending ?? 0, scout: Boolean(s.scout),
    hint: s.hint ? ctx.hintIndex.get(s.hint) ?? null : null,
    stance: s.stance ?? 0, look: Boolean(own(s, 'look') ?? false), lookPoint: v3(s.lookPoint) ?? [0, 0, 0],
    full,
    retryAt: num(s.retryAt) ?? -1,
    revalidateAt: num(s.revalidateAt) ?? 0,
    blockAim: Boolean(own(s, 'blockAim') ?? false),
  };
}

function mineSnap(ctx, m, full) {
  return {
    state: m.state ?? 0, pending: m.pending ?? 0, full,
    hint: m.hint ? ctx.hintIndex.get(m.hint) ?? null : null,
    placement: m.placement ? v3(m.placement) : null,
    aimPoint: m.aimPoint ? v3(m.aimPoint) : null,
    firedAt: num(m.firedAt), retryAt: num(m.retryAt) ?? -1,
  };
}

const AGENT_KIND = {
  UnitWaitAgent: 'wait', UnitWaitSecondaryAgent: 'wait2', UnitRandomAgent: 'random',
  UnitFollowAgent: 'follow', UnitHoldAgent: 'hold',
  UnitTrooperAgent: 'trooper', UnitScoutAgent: 'scout', UnitRepairAgent: 'repair',
  EnhancedTrooperAgent: 'trooper+', EnhancedScoutAgent: 'scout+', EnhancedRepairAgent: 'repair+',
  MissingCombatAgent: 'missing',
};

function agentSnap(ctx, agent, ai) {
  if (!agent) return null;
  const name = agent.constructor?.name ?? '';
  const snap = {
    self: agent,
    kind: AGENT_KIND[name] ?? null,
    foreign: agent.__aiForeignKind ?? (name === 'UnitCombatAgent' ? 'combat' : name === 'UnitFlyAgent' ? 'fly' : null),
    state: agent.state ?? 0, pending: agent.pending ?? 0,
    goal: goalSnap(ctx, agent.goal),
    unit: agent.unit ? ctx.keyOf(agent.unit) : null,
    interrupts: agent.interrupts
      ? { checkedAt: agent.interrupts.checkedAt ?? 0, charge: Boolean(agent.interrupts.charge), last: agent.interrupts.last ?? 0 }
      : { checkedAt: 0, charge: false, last: 0 },
    helper: helperSnap(agent.helper),
  };
  // Board/unboard: the stubs are the core's, the vehicles' are foreign
  // (both board classes share the name; the constructor tells them).
  if (name === 'UnitBoardAgent') {
    snap.kind = 'board';
    snap.foreign = agent.__aiForeignKind ?? (agent instanceof UnitBoardAgent ? 'board' : null);
    if (!snap.foreign) snap.vehicle = refSnap(ctx, agent.vehicle);
  }
  if (name === 'UnitUnBoardAgent') {
    snap.kind = 'unboard';
    snap.foreign = agent.__aiForeignKind ?? (agent instanceof UnitUnBoardAgent ? 'unboard' : null);
  }
  if (snap.kind === 'wait') {
    snap.since = agent.since ?? 0;
    snap.lookPoint = v3(agent.lookPoint) ?? [0, 0, 0];
  } else if (snap.kind === 'random') {
    snap.start = v3(agent.start) ?? [0, 0, 0];
    snap.lastRetry = agent.lastRetry ?? 0;
    snap.retriedAt = v3(agent.retriedAt) ?? [0, 0, 0];
  } else if (snap.kind === 'follow') {
    snap.leader = refSnap(ctx, agent.leader);
    snap.leaderPoint = v3(agent.leaderPoint) ?? [0, 0, 0];
    snap.countdown = agent.countdown ?? 1;
  } else if (snap.kind === 'hold') {
    snap.countdown = agent.countdown ?? 1;
    snap.lookPoint = v3(agent.lookPoint) ?? [0, 0, 0];
  } else if (snap.kind && snap.kind.startsWith('board') && !snap.foreign) {
    snap.vehicle = refSnap(ctx, agent.vehicle);
  } else if (snap.kind && (snap.kind.startsWith('trooper') || snap.kind.startsWith('scout') || snap.kind.startsWith('repair'))) {
    const snipeFull = ai.helperTypes?.snipe === agent.snipe?.constructor && agent.snipe?.constructor?.name === 'SnipeHelper' && ai.modulesBoundClasses === true;
    snap.post = agent.post ? ctx.postIndex.get(agent.post) ?? null : null;
    snap.patrol = patrolSnap(ctx, agent.patrol);
    snap.snipe = snipeSnap(ctx, agent.snipe, agent.snipe?.__full ?? isFullHelper(ai, agent.snipe));
    snap.mine = mineSnap(ctx, agent.mine, isFullHelper(ai, agent.mine));
    snap.startPosition = v3(agent.startPosition) ?? [0, 0, 0];
    snap.center = v3(agent.center) ?? [0, 0, 0];
    snap.offset = v3(agent.offset) ?? [0, 0, 0];
    snap.entryTime = agent.entryTime ?? 0;
    snap.support = Boolean(agent.support);
    snap.mineTries = num(agent.mineTries) ?? 0;
    snap.deadline = num(agent.deadline) ?? 0;
    snap.jobAt = num(agent.jobAt) ?? -1;
    snap.job = agent.job ? { target: refSnap(ctx, agent.job.target), ammo: agent.job.kind === 'ammo' } : null;
    snap.service = agent.service
      ? {
          approachAt: agent.service.approachAt ?? 0, progressAt: agent.service.progressAt ?? Infinity,
          savedHealth: agent.service.savedHealth ?? 0,
          toward: agent.service.toward ? v3(agent.service.toward) : null,
          closing: Boolean(own(agent.service, 'closing') ?? false),
          workable: num(agent.service.workable) ?? null,
        }
      : null;
  }
  return snap;
}

function isFullHelper(_ai, helper) {
  // classes.js's helpers carry retryAt on first select at the latest;
  // their prototypes override onTick (the bases' are Fsm's own).
  if (!helper) return false;
  return Object.prototype.hasOwnProperty.call(Object.getPrototypeOf(helper), 'onTick');
}

function squadSnap(ctx, record) {
  if (!record) return null;
  return {
    source: record.source ? ctx.characterIndex.get(record.source) ?? null : null,
    kind: record.kind ?? 0, time: record.time ?? 0,
    target: refSnap(ctx, record.target),
  };
}

function controllerSnap(ctx, c, ai) {
  const request = c.request ?? {};
  return {
    self: c,
    state: c.state ?? 0, pending: c.pending ?? 0,
    character: ctx.characterIndex.get(c.character) ?? null,
    unit: c.unit ? ctx.keyOf(c.unit) : null,
    generation: c.generation ?? -1,
    commands: {
      current: goalSnap(ctx, c.commands.current),
      queue: (c.commands.queue ?? []).map((g) => goalSnap(ctx, g)),
      latch: Boolean(c.commands.latch), currentDone: Boolean(c.commands.currentDone),
    },
    phase: c.phase ?? 0, published: Boolean(c.published),
    request: {
      valid: Boolean(request.valid), steering: v3(request.steeringTarget) ?? [0, 0, 0],
      waypoint: v3(request.waypoint) ?? [0, 0, 0], direction: v3(request.segmentDirection) ?? [0, 0, 0],
      mode: request.mode ?? 0, lower: request.lower ?? -1, upper: request.upper ?? 1,
    },
    aimState: c.aimState ?? 2, aimPoint: v3(c.aimPoint) ?? [0, 0, 0],
    aimTarget: refSnap(ctx, c.aimTarget), target: refSnap(ctx, c.target),
    fireA: Boolean(c.fireA), fireB: Boolean(c.fireB), facing: Boolean(c.facing), facingAux: Boolean(c.facingAux),
    stance: c.stance ?? 0, rangeScale: c.rangeScale ?? 1,
    jet: { pending: Boolean(c.jet?.pending), vector: v3(c.jet?.vector) ?? [0, 0, 0] },
    wantedMode: c.wantedMode ?? 0, exitRequest: Boolean(c.exitRequest),
    leader: refSnap(ctx, c.leader),
    danger: { pending: Boolean(c.danger?.pending), point: v3(c.danger?.point) ?? [0, 0, 0] },
    shot: {
      pending: Boolean(c.shot?.pending), known: Boolean(c.shot?.known),
      point: v3(c.shot?.point) ?? [0, 0, 0],
      source: c.shot?.source ? ctx.characterIndex.get(c.shot.source) ?? null : null,
    },
    acoustic: (c.acoustic ?? []).map((r) => refSnap(ctx, r)),
    vehicleNotes: (c.vehicleNotes ?? []).map((r) => refSnap(ctx, r)),
    command: { pending: Boolean(c.command?.pending), record: squadSnap(ctx, c.command?.record) },
    lastCommand: squadSnap(ctx, c.lastCommand),
    secondaryWaitUntil: num(c.secondaryWaitUntil) ?? 0, reboardAt: num(c.reboardAt) ?? -Infinity,
    roll: num(c.roll) ?? 0, homeFacing: Boolean(c.homeFacing), homeDirection: v3(c.homeDirection) ?? [0, 0, 1],
    processed: Boolean(c.processed), lodSkipped: Boolean(c.lodSkipped),
    inputSkip: Boolean(c.inputSkip), inputCarry: num(c.inputCarry) ?? 0,
    control: controlSnap(c.navControl),
    difficulty: c.difficulty ? [c.difficulty.A, c.difficulty.D, c.difficulty.B, c.difficulty.S] : null,
    dodge: num(own(c, 'dodge') ?? undefined) ?? null,
    alertAnim: typeof own(c, 'alertAnim') === 'boolean' ? c.alertAnim : null,
    enhanced: own(c, 'enhanced')
      ? {
          lookUntil: c.enhanced.lookUntil ?? -Infinity, lookPoint: v3(c.enhanced.lookPoint) ?? [0, 0, 0],
          heard: [...(c.enhanced.heard?.entries() ?? [])],
        }
      : null,
    policy: policyName(c.policy),
    perceptionFire: Boolean(c.perception?.fire),
    perceptionAimer: Boolean(c.perception?.aimer),
    agent: agentSnap(ctx, c.agent, ai),
  };
}

function policyName(policy) {
  if (!policy) return null;
  return policy.name ?? null;
}

function regionSnap(built) {
  if (!built) return null;
  return {
    type: built.type ?? null,
    half: built.half ? [...built.half] : null,
    radius: built.radius ?? 0,
    center: v3(built.center) ?? [0, 0, 0],
    inverse: built.inverse?.elements ? [...built.inverse.elements] : null,
  };
}

function rawCaptureSnap(capture) {
  if (!capture) return null;
  const m = capture.matrix;
  return {
    center: Array.isArray(capture.center) ? [...capture.center] : null,
    matrix: Array.isArray(m) ? [...m] : m?.isMatrix4 ? [...m.elements] : null,
    size: Array.isArray(capture.size) ? [...capture.size] : null,
    half: Array.isArray(capture.half) ? [...capture.half] : null,
  };
}

function postSnap(post) {
  const ai = postAI(post);
  return {
    self: post,
    name: post.name ?? null,
    team: post.team ?? 0, active: post.active !== false,
    position: v3(post.position),
    buildingAlive: post.building ? Boolean(post.building.alive) : null,
    neutralize: post.neutralize ?? 0,
    radius: post.radius ?? 0, speed: post.speed ?? 0,
    ai: {
      order: num(ai.order),
      atk: [...(ai.atk ?? [])], def: [...(ai.def ?? [])],
      bans: ai.bans ?? 0, valueBleed: ai.valueBleed ?? 0,
      spawnPresent: ai.spawnPath ? Boolean(ai.spawnPath.present) : null,
      allyPresent: ai.allyPath ? Boolean(ai.allyPath.present) : null,
      allyCount: ai.allyCount ?? 0, classSpawnPoints: ai.classSpawnPoints ?? 0,
      capture: ai.capture
        ? { center: [...ai.capture.center], half: [...(ai.capture.half ?? ai.capture.size ?? [3, 3, 3])] }
        : null,
      radius: ai.radius ?? 0, yaw: ai.yaw ?? 0,
    },
    region: regionSnap(post.capture && typeof post.capture === 'object' && 'inverse' in post.capture ? post.capture : null),
    descCapture: rawCaptureSnap(post.desc?.capture ?? (post.capture && !('inverse' in post.capture) ? post.capture : post.capture ?? null)),
    descSpawnPath: Boolean(post.desc?.spawnPath),
    descSpawnPoints: post.desc?.spawnPoints?.length ?? 0,
    descAllyPath: Boolean(post.desc?.allyPath),
  };
}

// The frame state: everything the core reads or writes in a step.
function aiState(game) {
  const classic = game.ai.classic;
  const ai = classic.ai;
  const world = ai.world;
  const spawning = game.ai.spawning;
  const characters = spawning?.characters ?? [];
  const postsSvc = world.posts;
  const posts = (typeof postsSvc?.all === 'function' ? postsSvc.all() : postsSvc?.posts ?? []).filter(Boolean);
  const characterIndex = new Map(characters.map((c, i) => [c, i]));
  const postIndex = new Map(posts.map((p, i) => [p, i]));
  const hintSet = game.ai.nav?.hints;
  const hintList = hintSet?.list ?? [];
  const hintIndex = new Map(hintList.map((h, i) => [h, i]));
  // The unit table: the registry in order, then seat controllables,
  // then anything a goal or alert still names, then the buildings as
  // repair candidates (materialized through the world's own service).
  const table = [];
  const inTable = new Map();
  const keyOf = (u) => {
    if (!u) return null;
    let key = inTable.get(u);
    if (key === undefined) {
      key = table.length;
      inTable.set(u, key);
      table.push(u);
    }
    return key;
  };
  for (const u of game.units.list ?? []) keyOf(u);
  for (const u of [...table]) {
    if (u.lease?.controllable) keyOf(u.lease.controllable);
    if (u.seat?.vehicle?.aiTarget) keyOf(u.seat.vehicle.aiTarget);
  }
  for (const c of characters) if (c.unit) keyOf(c.unit);
  const ctx = { game, characterIndex, postIndex, hintIndex, keyOf };
  const roster = [...ai.byCharacter.keys()];
  const controllersInRoster = roster.map((ch) => ai.byCharacter.get(ch));
  const nameRefs = (goal) => {
    if (!goal) return;
    for (const r of [goal.refA, goal.refB]) if (r?.unit) keyOf(r.unit);
  };
  for (const c of controllersInRoster) {
    if (c.unit) keyOf(c.unit);
    nameRefs(c.commands.current);
    for (const g of c.commands.queue ?? []) nameRefs(g);
    for (const r of [c.aimTarget, c.target, c.leader]) if (r?.unit) keyOf(r.unit);
    for (const r of [...(c.acoustic ?? []), ...(c.vehicleNotes ?? [])]) if (r?.unit) keyOf(r.unit);
    const a = c.agent;
    if (a) {
      if (a.unit) keyOf(a.unit);
      nameRefs(a.goal);
      if (a.leader?.unit) keyOf(a.leader.unit);
      if (a.vehicle?.unit) keyOf(a.vehicle.unit);
      if (a.job?.target?.unit) keyOf(a.job.target.unit);
    }
  }
  // Repair-candidate building wrappers, through the world's service.
  const buildings = game.live?.buildings ?? [];
  const wrapperByBuilding = new Map();
  if (typeof world.serviceCandidates === 'function') {
    for (const b of buildings) {
      const found = world.serviceCandidates(b.position, 0.5);
      for (const cand of found) {
        if (cand && cand.building === b && !wrapperByBuilding.has(b)) {
          wrapperByBuilding.set(b, cand);
          keyOf(cand);
        }
      }
    }
  }
  ctx.wrapperByBuilding = wrapperByBuilding;
  const difficulty = world.difficulty;
  const objectives = spawning?.objectives;
  const teams = spawning?.teams ?? [];
  acousticLens = new Map();
  return {
    order: controllersInRoster,
    time: world.time ?? 0,
    paused: Boolean(world.paused),
    bind: {
      nav: Boolean(ai.nav), combat: Boolean(ai.combat),
      combatAgent: ai.agents?.combat === UnitCombatAgent,
      population: Boolean(ai.population?.requestObjective),
      vehicles: Boolean(ai.vehicles && Object.keys(ai.vehicles).length),
      classes: Boolean(ai.classes?.selectSnipe),
      randomB: ai.randomA !== ai.randomB,
      enhanced: difficulty?.constructor?.name === 'EnhancedDifficulty',
      mineGround: typeof world.mineGroundHeight === 'function',
      vehicleAdapter: Boolean(own(ai, 'vehicleAdapter')),
    },
    fallbackSensorRange: ai.fallbackSensorRange ?? 75,
    lodWorld: Boolean(world.networkViews),
    lodOption: Boolean(ai.options?.lodDisabled),
    hardOption: typeof ai.options?.hard === 'boolean' ? ai.options.hard : null,
    difficulty: difficulty
      ? {
          tier: difficulty.tier, profileTier: difficulty.profileTier,
          playerTeam: difficulty.playerTeam, secondPlayerTeam: difficulty.secondPlayerTeam,
          network: Boolean(difficulty.network), override: Boolean(difficulty.override),
          playerSkill: difficulty.playerSkill, enemySkill: difficulty.enemySkill,
          enhanced: difficulty.constructor?.name === 'EnhancedDifficulty',
        }
      : null,
    controls: world.missionControls
      ? {
          aggressiveness: Object.entries(world.missionControls.aggressiveness ?? {}).map(([k, v]) => [Number(k), v]),
          dense: world.missionControls.dense ? [Boolean(world.missionControls.dense.explicit), Boolean(world.missionControls.dense.value)] : [false, false],
          attackingTeam: world.missionControls.attackingTeam ?? 1,
          snipe: world.missionControls.snipe ?? null,
          stayInTurrets: Boolean(world.missionControls.stayInTurrets),
          stationaryWait: world.missionControls.stationaryWait ?? 10,
          vehicleNotifyRadius: world.missionControls.vehicleNotifyRadius ?? 50,
          formation: world.missionControls.formation ?? null,
          blindJetJumps: world.missionControls.blindJetJumps !== false,
          fly: world.missionControls.flyHeight ?? null,
        }
      : null,
    followOption: ai.options?.followRadii ?? null,
    teams: {
      relations: teams.map((t) => (t ? [...(t.relations ?? [])] : null)),
      factions: teams.map((t) => t?.faction ?? 0),
      alliances: [...(game.units.alliances?.entries() ?? [])].map(([k, v]) => [k, v]),
    },
    tickets: teams.map((t, team) => (t ? spawning.tickets(team) : null)),
    reinforcements: game.battle?.reinforcements ? [0, 1, 2, 3, 4, 5, 6, 7].map((t) => num(game.battle.reinforcements[t]) ?? null) : null,
    scheduler: {
      counter: ai.scheduler.counter, parity: ai.scheduler.parity,
      inputParity: ai.scheduler.inputParity, skips: ai.scheduler.skips,
      stats: { ...ai.scheduler.stats },
    },
    rng: { a: ai.randomA?.state ?? 0, b: ai.randomB?.state ?? 0 },
    cameras: ai.cameras().map((camera) => ({
      elements: [...camera.matrixWorld.elements],
      fov: camera.fov ?? 0, aspect: camera.aspect ?? 0,
      near: camera.near ?? 0, far: camera.far ?? 0, zoom: camera.zoom ?? 1,
    })),
    characters: characters.map((c) => ({
      self: c, team: c.team, human: Boolean(c.human),
      unit: c.unit ? keyOf(c.unit) : null,
      objective: c.objective ? postIndex.get(c.objective) ?? null : null,
    })),
    live: (ai.controllers ?? []).map((c) => controllersInRoster.indexOf(c)),
    roster: roster.map((ch) => characterIndex.get(ch)),
    posts: posts.map((p) => postSnap(p)),
    units: table.map((u) => unitSnap(ctx, u)),
    items: (world.items?.items ?? []).map((item) => ({
      gone: Boolean(item.gone), mine: Boolean(item.mine),
      defBase: item.def?.base ?? null, defClass: item.def?.class ?? null,
      position: v3(item.position), radius: num(item.radius),
      team: item.team ?? 0, owner: item.owner ? keyOf(item.owner) : null,
      qty: qtySnap(item.def),
    })),
    hints: hintList.map((h) => ({
      bit: h.bit ?? 0, position: v3(h.position), forward: v3(h.forward),
      primaryStance: h.primaryStance ?? 1,
      post: h.commandPost ? postIndex.get(h.commandPost) ?? null : null,
      mode: h.mode ?? 0, radius: h.radius ?? -1,
      reservedAt: num(h.reservedAt),
    })),
    controllers: controllersInRoster.map((c) => controllerSnap(ctx, c, ai)),
    repair: repairSnap(ctx, ai),
    objectives: objectives
      ? {
          enhanced: objectives.constructor?.name === 'EnhancedObjectives',
          threatAt: [...(objectives.threatAt?.entries() ?? [])].map(([p, t]) => [postIndex.get(p) ?? -1, t]),
          recruitAt: [...(objectives.recruitAt?.entries() ?? [])],
          pollAt: objectives.pollAt ?? 0,
        }
      : null,
  };
}

function repairSnap(ctx, ai) {
  const repairServices = repairServicesOf.get(ai);
  if (!repairServices) return { reservations: [], futile: [] };
  return {
    reservations: repairServices.reservations.map((r) => ({
      target: refSnap(ctx, r.target), ammo: r.kind === 'ammo', owner: refSnap(ctx, r.owner), time: r.time ?? 0,
    })),
    futile: repairServices.futile.map((r) => ({
      unit: r.owner ? ctx.keyOf(r.owner) : null, generation: r.generation ?? 0,
      target: r.target ? ctx.keyOf(r.target) : null, ammo: r.kind === 'ammo', until: r.until ?? 0,
    })),
  };
}

// ---- extras -------------------------------------------------------------

function writesOf(controller) {
  if (!controller) return null;
  return {
    fireA: Boolean(controller.fireA), fireB: Boolean(controller.fireB),
    // Resolved to a table key in the replay from the handle written.
    target: controller.target ? { u: controller.target.unit, g: controller.target.generation ?? 0 } : null,
    aimState: controller.aimState ?? 2,
    aimPoint: v3(controller.aimPoint) ?? [0, 0, 0],
    stance: controller.stance ?? 0, facing: Boolean(controller.facing),
    wantedMode: controller.wantedMode ?? 0,
  };
}

const afterCurrent = () => ({
  w: writesOf(current),
  c: current ? controlSnap(current.navControl) : null,
  h: current ? helperSnap(current.agent?.helper) : null,
});

const foreignAfter = () => ({
  ...afterCurrent(),
  state: current?.agent?.state ?? 0, pending: current?.agent?.pending ?? 0,
});

function alertExtra(game) {
  const ai = game.ai.classic.ai;
  const characters = game.ai.spawning?.characters ?? [];
  const rows = [];
  const roster = [...ai.byCharacter.keys()];
  const list = roster.map((ch) => ai.byCharacter.get(ch));
  list.forEach((c, i) => {
    rows.push({
      controller: i,
      shotPending: Boolean(c.shot?.pending), shotKnown: Boolean(c.shot?.known),
      shotPoint: v3(c.shot?.point) ?? [0, 0, 0],
      shotSource: c.shot?.source ? characters.indexOf(c.shot.source) : -1,
      dangerPending: Boolean(c.danger?.pending), dangerPoint: v3(c.danger?.point) ?? [0, 0, 0],
      acoustic: (c.acoustic ?? []).map((r) => (r?.unit ? { u: r.unit, g: r.generation ?? 0 } : null)),
      dodge: num(own(c, 'dodge') ?? undefined) ?? null,
      alertAnim: typeof own(c, 'alertAnim') === 'boolean' ? c.alertAnim : null,
    });
    acousticLens.set(c, c.acoustic?.length ?? 0);
  });
  return { alerts: rows };
}

function acousticExtra(game) {
  const ai = game.ai.classic.ai;
  const adds = [];
  const roster = [...ai.byCharacter.keys()];
  const list = roster.map((ch) => ai.byCharacter.get(ch));
  list.forEach((c, i) => {
    const before = acousticLens.get(c) ?? 0;
    const now = c.acoustic?.length ?? 0;
    for (let k = before; k < now; k++) {
      const r = c.acoustic[k];
      if (r?.unit) adds.push([i, { u: r.unit, g: r.generation ?? 0 }]);
    }
    acousticLens.set(c, now);
  });
  return { adds };
}

function notesExtra(game) {
  const ai = game.ai.classic.ai;
  const roster = [...ai.byCharacter.keys()];
  const list = roster.map((ch) => ai.byCharacter.get(ch));
  return {
    notes: list.map((c, i) => [i, (c.vehicleNotes ?? []).map((r) => (r?.unit ? { u: r.unit, g: r.generation ?? 0 } : null))]),
  };
}

function patrolExtra(game) {
  const ai = game.ai.classic.ai;
  const postsSvc = ai.world.posts;
  const posts = (typeof postsSvc?.all === 'function' ? postsSvc.all() : postsSvc?.posts ?? []);
  const postIndex = new Map(posts.map((p, i) => [p, i]));
  const hintList = game.ai.nav?.hints?.list ?? [];
  const hintIndex = new Map(hintList.map((h, i) => [h, i]));
  const ctx = { postIndex, hintIndex };
  const patrol = current?.agent?.patrol;
  return { ...afterCurrent(), patrol: patrol ? patrolSnap(ctx, patrol) : null };
}

function queryExtra() {
  const scope = queryFilterLog[queryFilterLog.length - 1] ?? [];
  return { filter: scope };
}

function responseExtra() {
  const c = current;
  return {
    ...afterCurrent(),
    enhanced: c && own(c, 'enhanced')
      ? { lookUntil: c.enhanced.lookUntil ?? -Infinity, lookPoint: v3(c.enhanced.lookPoint) ?? [0, 0, 0], heard: [...(c.enhanced.heard?.entries() ?? [])] }
      : null,
    secondaryWaitUntil: c ? (num(c.secondaryWaitUntil) ?? 0) : null,
  };
}

// ---- boundaries and frames -----------------------------------------------------

const COMBAT = [
  'createPerception', 'attachController', 'detachController', 'rebuildProtected', 'ingest',
  'initAgent', 'scan', 'dropLosRequests', 'serviceLos', 'broadcastAcoustic', 'clearCandidate',
  'rankTargets', 'updateTarget', 'updateTargetSelection', 'gateFire', 'currentTarget',
  'memoryPoint', 'acquiredTarget', 'canEngage', 'hasWeapon', 'sensorRange', 'acousticRange',
  'consumeFire', 'aimPoint', 'leadPoint', 'markSkip',
  'hazardResponse', 'alertResponse', 'acousticResponse',
];
const VEHICLES = [
  'notify', 'requestExit', 'boarded', 'boardedVehicle', 'boardedVehicleOf', 'boardedVehicleOfCharacter',
  'currentControllable', 'isMountedTurret', 'emptySeats', 'crewControllers', 'seatsFull',
  'pendingReservations', 'lowTickets', 'pilotKeepsSeat', 'alertsBlocked', 'mountReach',
  'refusesHold', 'canEnter', 'freeSeats', 'reserve', 'reserveSeat', 'targetReserved',
  'reserveTarget', 'isAtAt', 'supportPoint', 'boardPolicy', 'patrolMounted', 'boardTick',
  'createHelper', 'vehicleBucket', 'analogs', 'decayLeases',
];
const HELPER_METHODS = ['goto', 'stop', 'terminate', 'tick', 'destinationPoint', 'installPath', 'follow', 'followTrail', 'holdPath', 'setLeader', 'dispose'];

function aiBoundaries(game) {
  prepare(game);
  const classic = game.ai?.classic;
  const ai = classic?.ai;
  if (!ai) return [];
  const world = ai.world;
  const out = [];
  const onBag = (bag, names, prefix, extra) => {
    if (!bag) return;
    for (const name of names) {
      if (typeof bag[name] === 'function') out.push([`${prefix}.${name}`, bag, name, extra]);
    }
  };
  const combatExtra = (name) => (name === 'ingest' ? alertExtra : name === 'broadcastAcoustic' ? acousticExtra
    : name.endsWith('Response') ? responseExtra : () => afterCurrent());
  for (const name of COMBAT) {
    if (typeof ai.combat?.[name] === 'function') out.push([`combat.${name}`, ai.combat, name, combatExtra(name)]);
  }
  for (const name of VEHICLES) {
    if (typeof ai.vehicles?.[name] === 'function') {
      const extra = name === 'notify' ? notesExtra : name === 'patrolMounted' ? patrolExtra : () => afterCurrent();
      out.push([`vehicles.${name}`, ai.vehicles, name, extra]);
    }
  }
  onBag(ai.nav, ['plannerIdle', 'service', 'pointClear', 'clipSegment', 'inDangerRegion', 'queryHints', 'hintAvailable', 'markHint'], 'nav', () => afterCurrent());
  // The world's own seams.
  if (world.weapons) {
    for (const name of ['selectedWeapon', 'switchWeapon', 'effectiveRange', 'firingCone']) {
      out.push([`weapons.${name}`, world.weapons, name, afterCurrent]);
    }
  }
  out.push(['units.query', game.units, 'query', queryExtra]);
  out.push(['world.serviceCandidates', world, 'serviceCandidates']);
  if (typeof world.mineGroundHeight === 'function') out.push(['world.mineGroundHeight', world, 'mineGroundHeight']);
  out.push(['events.emit', world.events, 'emit']);
  // Navigation input and the helper/control seams.
  out.push(['ai.navigationInput', ai, 'navigationInput', afterCurrent]);
  out.push(['ai.analogs', ai, 'analogs']);
  const nav = world.nav;
  if (nav) {
    out.push(['nav.helper', nav, 'helper', afterCurrent]);
    out.push(['nav.control', nav, 'control', afterCurrent]);
    const helperClasses = [
      ['ground', GroundGotoHelper], ['fly', NavFlyGotoHelper],
      ['vfly', VehicleFlyGotoHelper], ['stationary', StationaryHelper],
    ];
    for (const [prefix, cls] of helperClasses) {
      for (const method of HELPER_METHODS) {
        if (typeof cls.prototype[method] === 'function') out.push([`${prefix}.${method}`, cls.prototype, method, afterCurrent]);
      }
    }
    const controlProto = (ai.controllers ?? []).map((c) => c.navControl).find(Boolean);
    if (controlProto) out.push(['control.reset', Object.getPrototypeOf(controlProto), 'reset', afterCurrent]);
  }
  // Foreign agents (create is a boundary inside ai.update and inside
  // bind frames; tick and exit likewise).
  out.push(['foreign.create', foreignHooks, 'create', foreignAfter]);
  out.push(['foreign.tick', foreignHooks, 'tick', foreignAfter]);
  out.push(['foreign.exit', foreignHooks, 'exit', foreignAfter]);
  return out;
}

const aiTarget = (game) => {
  const ai = game.ai?.classic?.ai;
  return ai ? Object.getPrototypeOf(ai) : null;
};
const objectivesProto = (game) => {
  const objectives = game.ai?.spawning?.objectives;
  return objectives ? Object.getPrototypeOf(objectives) : null;
};
const objectivesTickProto = (game) => {
  const proto = objectivesProto(game);
  return proto && typeof proto.tick === 'function' ? proto : null;
};

export const ai = {
  frames: [
    { method: 'ai.update', target: aiTarget, state: (game) => aiState(game), boundaries: aiBoundaries },
    { method: 'controller.bind', target: () => UnitController.prototype, state: (game) => aiState(game), boundaries: aiBoundaries },
    { method: 'controller.unbind', target: () => UnitController.prototype, state: (game) => aiState(game), boundaries: aiBoundaries },
    { method: 'objectives.onOwnerChanged', target: objectivesProto, state: (game) => aiState(game), boundaries: aiBoundaries },
    { method: 'objectives.onDeactivated', target: objectivesProto, state: (game) => aiState(game), boundaries: aiBoundaries },
    { method: 'objectives.tick', target: objectivesTickProto, state: (game) => aiState(game), boundaries: aiBoundaries },
  ],
};


