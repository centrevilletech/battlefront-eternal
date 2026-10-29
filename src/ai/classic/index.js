import { ClassicRandom } from "../random.js";
import { roleOf, ROLE, STATE, UnitController } from "./controller.js";
import { DEFAULT_AGENTS, DirectHelper, FOLLOW_RADII, MineHelper, SnipeHelper, isFlyer } from "./agents.js";
import { ClassicScheduler } from "./scheduler.js";
import { COMMAND, playerCommand } from "./squads.js";

// The Classic AI entry point (docs/ai.md, docs/ai-controller.md).
//
//   const modules = await loadClassicModules();   // or import the siblings yourself
//   const ai = createClassicAI(world, { modules });
//   ai.attach(character)        // once per AI Character (humans are ignored)
//   ai.spawned(character)       // right after its unit spawns (else noticed lazily)
//   ai.died(character)          // when its unit dies (else noticed lazily)
//   ai.update(h)                // every simulation step, before units tick
//
// update(h) runs the decision scheduler (budgeted controller services, the
// deferred-LOS and path-planner budgets), then the AI input step for every
// AI unit, which fills unit.intent. The other modules plug in through the
// adapters below; a missing one degrades with a console warning instead of
// failing.

// Static imports keep every Classic service in the production bundle.
import * as module_nav from "./nav/index.js";
import * as module_navGoto from "./nav/goto.js";
import * as module_navUnits from "./nav/units.js";
import * as module_combat from "./combat.js";
import * as module_perception from "./perception.js";
import * as module_aim from "./aim.js";
import * as module_weaponsAi from "./weapons-ai.js";
import * as module_difficulty from "./difficulty.js";
import * as module_spawning from "./spawning.js";
import * as module_goals from "./goals.js";
import * as module_vehicles from "./vehicles/index.js";
import * as module_classes from "./classes.js";

export async function loadClassicModules() {
  return {
    nav: module_nav,
    navGoto: module_navGoto,
    navUnits: module_navUnits,
    combat: module_combat,
    perception: module_perception,
    aim: module_aim,
    weaponsAi: module_weaponsAi,
    difficulty: module_difficulty,
    spawning: module_spawning,
    goals: module_goals,
    vehicles: module_vehicles,
    classes: module_classes,
  };
}

const resolveRef = (ref) => {
  const u = ref?.unit;
  return u && u.alive && u.generation === ref.generation ? u : null;
};

// Pick the first function named `name` among the sources (bound to its owner).
function pick(name, ...sources) {
  for (const source of sources) {
    const fn = source?.[name];
    if (typeof fn === "function") return fn.bind(source);
  }
  return undefined;
}

function compact(object) {
  const out = {};
  let any = false;
  for (const [key, value] of Object.entries(object)) {
    if (value !== undefined && value !== null) {
      out[key] = value;
      any = true;
    }
  }
  return any ? out : null;
}

// -- sibling adapters ---------------------------------------------------------------------
//
// Each turns a sibling's published API into the service names the controller
// code calls. An explicit object in options (options.nav, options.combat,
// options.population, options.vehicles, options.classes) wins per name; then
// a sibling's `classicBinding(ai)` export; then these mappings.

function navAdapter(ai, modules, explicit) {
  const world = ai.world;
  const runtime = () => world.nav ?? null;
  const units = modules.navUnits ?? modules.nav;
  const binding = typeof modules.nav?.classicBinding === "function" ? modules.nav.classicBinding(ai) : null;
  return compact({
    plannerIdle: pick("plannerIdle", explicit, binding) ?? (() => {
      const nav = runtime();
      if (typeof nav?.plannerIdle === "function") return nav.plannerIdle();
      return nav?.planner ? Boolean(nav.planner.idle) : true;
    }),
    service: pick("service", explicit, binding) ?? ((budget) => {
      const nav = runtime();
      if (typeof nav?.service === "function") return nav.service(budget);
      return nav?.planner?.service?.(budget);
    }),
    pointClear: pick("pointClear", explicit, binding) ?? ((p, r, mask) => {
      const nav = runtime();
      const barriers = nav?.barriers;
      if (barriers?.pointClear) return barriers.pointClear(p, r, mask);
      if (typeof nav?.pointClear === "function") return nav.pointClear(p, r, mask);
      return true;
    }),
    clipSegment: pick("clipSegment", explicit, binding) ?? ((s, e, r, mask) => runtime()?.barriers?.obstruct?.(s, e, r, mask) ?? false),
    traversalRadius: pick("traversalRadius", explicit, binding, units),
    traversalMask: pick("traversalMask", explicit, binding, units),
    inDangerRegion: pick("inDangerRegion", explicit, binding) ?? ((p, pad) => runtime()?.inDangerRegion?.(p, pad) ?? false),
    queryHints: pick("queryHints", explicit, binding) ?? (runtime()?.queryHints ? (q) => runtime().queryHints(q) : undefined),
    hintAvailable: pick("hintAvailable", explicit, binding),
    markHint: pick("markHint", explicit, binding),
    analogs: pick("analogs", explicit, binding, modules.nav),
    navigationInput: pick("navigationInput", explicit, binding, modules.nav),
    get hints() {
      return explicit?.hints ?? runtime()?.hints ?? [];
    },
  });
}

function combatAdapter(ai, modules, explicit) {
  const { perception: P, weaponsAi: W, aim: A, combat: C } = modules;
  if (!P && !W && !A && !C && !explicit) return null;
  const world = ai.world;
  const binding = typeof C?.classicBinding === "function" ? C.classicBinding(ai) : null;
  const dispatch = binding?.dispatch ?? C?.dispatch ?? null;
  // The global deferred line-of-sight queue (one per match).
  let los = world.deferredLos ?? null;
  if (!los && P?.DeferredLos) los = world.deferredLos = new P.DeferredLos();
  const sounds = ai.options.weaponSounds ?? null;
  const engage = dispatch
    ? (controller, target) => {
        const unit = controller.unit;
        if (!unit?.combat?.arsenal?.weapons?.length || !target) return false;
        return Boolean(dispatch(controller, unit, target, false, world)?.pattern);
      }
    : undefined;
  const services = {
    createPerception: P?.Perception ? (controller) => new P.Perception(controller) : undefined,
    initAgent: P?.TargetTracker
      ? (agent) => {
          agent.tracker ??= new P.TargetTracker(agent.controller);
        }
      : undefined,
    scan: P?.scan && dispatch ? (controller) => P.scan(controller, world, dispatch) : undefined,
    dropLosRequests: los ? (controller) => los.drain(controller) : undefined,
    serviceLos: los ? (_world, budget) => los.service(world, budget) : undefined,
    broadcastAcoustic: P?.broadcastAcoustic ? (unit) => P.broadcastAcoustic(unit, world, sounds) : undefined,
    clearCandidate: P?.clearCandidate,
    rankTargets: P?.thin && los ? (agent) => P.thin(agent.controller, world, agent.tracker?.ref ?? null, los) : undefined,
    updateTarget: P?.TargetTracker ? (agent) => agent.tracker?.update(world, dispatch) : undefined,
    updateTargetSelection: P?.TargetTracker
      ? (agent) => {
          agent.tracker?.select(world);
          agent.tracker?.assignWeaponTarget(world);
        }
      : undefined,
    gateFire: P?.TargetTracker ? (agent) => agent.tracker?.fireControls(world) : undefined,
    currentTarget: P?.TargetTracker ? (agent) => agent.tracker?.freshTarget ?? null : undefined,
    memoryPoint: P?.TargetTracker ? (agent) => (agent.tracker?.target ? agent.tracker.point : null) : undefined,
    acquiredTarget: (agent) => resolveRef(agent.controller.perception?.candidate),
    canEngage: engage,
    hasWeapon: W?.selectedWeapon ? (controller) => Boolean(W.selectedWeapon(controller.unit, controller)) : undefined,
    sensorRange: P?.sightRange ? (controller, target) => P.sightRange(controller.unit, target) : undefined,
    acousticRange: W?.sensorRange ? (controller) => W.sensorRange(controller.unit, controller) : undefined,
    consumeFire: W?.consumeFire ? (controller, unit) => W.consumeFire(controller, unit, world) : undefined,
    aimPoint: A?.aimPointFor ? (controller, unit, dt, out) => A.aimPointFor(controller, unit, dt, out) : undefined,
    hazardResponse: P?.hazardResponse ? (controller, agent) => P.hazardResponse(controller, world, agent) : undefined,
    alertResponse: P?.alertResponse && dispatch ? (controller, agent) => P.alertResponse(controller, world, agent, dispatch) : undefined,
    acousticResponse: P?.acousticResponse ? (controller, agent) => P.acousticResponse(controller, world, agent) : undefined,
  };
  // Explicit and binding entries win.
  for (const source of [binding, explicit]) {
    if (!source) continue;
    for (const [key, value] of Object.entries(source)) if (typeof value === "function") services[key] = value.bind(source);
  }
  return compact(services);
}

function populationAdapter(ai, modules, explicit) {
  const { goals: G, difficulty: D, spawning: S } = modules;
  const binding = typeof G?.classicBinding === "function" ? G.classicBinding(ai) : null;
  return compact({
    requestObjective: pick("requestObjective", explicit, binding),
    friendlySpawnEligible: pick("friendlySpawnEligible", explicit, binding),
    curvesFor: pick("curvesFor", explicit, D, S),
    isHard: pick("isHard", explicit, D),
    postAI: pick("postAI", explicit, G),
    objectiveCenter: pick("objectiveCenter", explicit, G),
    objectiveOffset: pick("objectiveOffset", explicit, G),
    objectiveArrived: pick("objectiveArrived", explicit, G),
  });
}

const VEHICLE_SERVICES = [
  "createHelper", "notify", "requestExit", "boarded", "boardedVehicle", "boardedVehicleOf", "boardedVehicleOfCharacter",
  "currentControllable", "isMountedTurret", "emptySeats", "crewControllers", "seatsFull", "pendingReservations",
  "lowTickets", "pilotKeepsSeat", "alertsBlocked", "mountReach", "refusesHold", "canEnter", "freeSeats", "reserve",
  "reserveSeat", "boardTick", "boardPolicy", "targetReserved", "reserveTarget", "patrolMounted", "isAtAt", "supportPoint",
];

const CLASS_SERVICES = ["selectSnipe", "canMine", "selectMine", "findRepairJob", "startRepairService", "repairService"];

function namedAdapter(names, ai, explicit, ...modules) {
  const sources = [explicit];
  for (const mod of modules) {
    if (!mod) continue;
    if (typeof mod.classicBinding === "function") sources.push(mod.classicBinding(ai));
    sources.push(mod);
  }
  const out = {};
  for (const name of names) out[name] = pick(name, ...sources);
  return compact(out);
}

// ---------------------------------------------------------------------------------------

export class ClassicAI {
  constructor(world, options = {}) {
    this.world = world;
    this.options = options;
    this.warned = new Set();
    // Two generators: A (integer, range and most draws, world.random) and B
    // (the command-send roll, the alert/recovery/follow cube offsets and the
    // jet retry; world.randomB).
    this.randomA = options.randomA ?? world.randomA ?? world.random ?? new ClassicRandom(0);
    this.randomB = options.randomB ?? world.randomB ?? world.gameRandom ?? null;
    if (!this.randomB) {
      this.randomB = this.randomA;
      this.warnOnce("randomB", "no second generator (world.randomB / world.gameRandom): its draws share world.random, which shifts the original's sequences");
    }
    this.tracing = options.trace ?? false;
    this.fallbackSensorRange = options.fallbackSensorRange ?? 75;
    this.controllers = []; // live controllers, in spawn order
    this.byCharacter = new Map();
    this.scheduler = new ClassicScheduler(this);
    this.agents = { ...DEFAULT_AGENTS };
    this.helperTypes = { snipe: SnipeHelper, mine: MineHelper };
    this.nav = null;
    this.combat = null;
    this.population = null;
    this.vehicles = null;
    this.classes = null;
    this.analogs = null;
    this.helperClasses = { ground: null, fly: null };
    this.bound = false;
  }

  // -- sibling bindings -------------------------------------------------------------

  bind(modules = {}, { warn = true } = {}) {
    const options = this.options;
    this.modules = modules;
    const say = (key, message) => warn && this.warnOnce(key, message);
    // Nav.
    this.nav = navAdapter(this, modules, options.nav);
    this.analogs = options.analogs ?? this.nav?.analogs ?? null;
    this.navigationInput = options.navigationInput ?? this.nav?.navigationInput ?? null;
    const goto = modules.navGoto ?? modules.nav;
    this.helperClasses.ground = options.GroundGotoHelper ?? goto?.GroundGotoHelper ?? null;
    this.helperClasses.fly = options.FlyGotoHelper ?? goto?.FlyGotoHelper ?? null;
    this.navHelperFactory = options.createHelper ?? null;
    if (!this.helperClasses.ground && !this.navHelperFactory) say("nav", "Nav GroundGotoHelper is not bound: bots steer straight at their goals (not Classic)");
    if (!this.analogs && !this.navigationInput) say("analogs", "Nav navigationInput()/analogs() are not bound: using the degraded request-to-input conversion");
    // Combat.
    this.combat = combatAdapter(this, modules, options.combat);
    const combatAgent = options.UnitCombatAgent ?? modules.combat?.UnitCombatAgent ?? null;
    if (combatAgent) this.agents.combat = combatAgent;
    if (!this.combat) say("combat", "Combat modules are not bound: no perception, aim or firing");
    else if (!this.combat.scan) say("dispatch", "Combat dispatch() is not bound: no sight scan, so bots acquire no targets by sight");
    if (!combatAgent) say("combatAgent", "UnitCombatAgent is not bound: combat goals finish at once");
    // Population.
    this.population = populationAdapter(this, modules, options.population);
    if (!this.population?.requestObjective) say("population", "Population goals are not bound: objectives come only from character.objective");
    // Vehicles and class behaviors (later waves).
    this.vehicles = namedAdapter(VEHICLE_SERVICES, this, options.vehicles, modules.vehicles);
    this.classes = namedAdapter(CLASS_SERVICES, this, options.classes, modules.classes);
    const vehicleAgents = modules.vehicles?.agents ?? {};
    for (const key of ["board", "unboard", "waitSecondary"]) if (vehicleAgents[key]) this.agents[key] = vehicleAgents[key];
    const classHelpers = modules.classes?.helpers ?? {};
    if (classHelpers.snipe) this.helperTypes.snipe = classHelpers.snipe;
    if (classHelpers.mine) this.helperTypes.mine = classHelpers.mine;
    for (const [key, cls] of Object.entries(options.agents ?? {})) if (cls) this.agents[key] = cls;
    for (const [key, cls] of Object.entries(options.helpers ?? {})) if (cls) this.helperTypes[key] = cls;
    this.bound = true;
    return this;
  }

  warnOnce(key, message) {
    if (this.warned.has(key)) return;
    this.warned.add(key);
    if (this.options.quiet) return;
    console.warn(`[classic-ai] ${message}`);
  }

  // -- world adapters ------------------------------------------------------------------

  // Every Character, in roster order: the match's ClassicSpawning roster
  // (Population) when there is one, else the battle's.
  characters() {
    return this.options.characters?.() ?? this.world.spawning?.characters ?? this.world.battle?.characters ?? [];
  }

  relation(a, b) {
    const world = this.world;
    if (world.relation) return world.relation(a, b);
    if (world.units?.relation) return world.units.relation(a, b);
    if (a === b) return a ? 1 : 0;
    return !a || !b ? 0 : -1;
  }

  // The identity whose parity drives the visibility skips.
  identity(unit) {
    return this.options.identity ? this.options.identity(unit) : (unit?.id ?? 0);
  }

  cameras() {
    const world = this.world;
    if (Array.isArray(world.cameras)) return world.cameras;
    return world.camera ? [world.camera] : [];
  }

  lodDisabled() {
    return Boolean(this.world.lodDisabled || this.world.network);
  }

  // Cycle length and deferred-LOS budget: a ClassicDifficulty carries them
  // (15/6 Easy and Medium, 3/12 Hard); otherwise by tier.
  schedulerNumbers() {
    const d = this.world.difficulty;
    if (this.options.hard === undefined && Number.isFinite(d?.cycle) && Number.isFinite(d?.losBudget)) {
      return { cycle: d.cycle, losBudget: d.losBudget };
    }
    return this.isHard() ? { cycle: 3, losBudget: 12 } : { cycle: 15, losBudget: 6 };
  }

  isHard() {
    if (this.options.hard !== undefined) return Boolean(this.options.hard);
    const d = this.world.difficulty;
    const fromPopulation = this.population?.isHard?.(d);
    if (fromPopulation !== undefined) return Boolean(fromPopulation);
    if (d == null) {
      this.warnOnce("difficulty", "world.difficulty is not set: scheduling as Easy/Medium (cycle 15)");
      return false;
    }
    if (typeof d.hard === "boolean") return d.hard;
    if (typeof d.isHard === "function") return Boolean(d.isHard());
    if (Number.isFinite(d.cycle)) return d.cycle === 3;
    if (d.tier !== undefined) return d.tier === 3 || d.tier === "hard";
    if (d.level !== undefined) return d.level === 3 || d.level === "hard";
    return false;
  }

  curvesFor(team) {
    const d = this.world.difficulty;
    return this.population?.curvesFor?.(d, team) ?? d?.curves?.(team) ?? null;
  }

  // Mission controls (Population's missionControls() result, when the
  // integrator stores it on the world).
  controls() {
    const world = this.world;
    return world.missionControls ?? world.mission?.controls ?? null;
  }

  get followRadii() {
    return { ...FOLLOW_RADII, ...(this.controls()?.formation ?? {}), ...(this.options.followRadii ?? {}) };
  }

  stayInTurrets() {
    return Boolean(this.controls()?.stayInTurrets);
  }

  stayDelay() {
    return this.controls()?.stationaryWait ?? 10;
  }

  secondaryNetworkWorld() {
    return false;
  }

  humanUnits() {
    const out = [];
    for (const character of this.characters()) if (character?.human && character.unit?.alive) out.push(character.unit);
    return out;
  }

  // Living allied units within `radius` of `unit` (excluding it), at most `cap`.
  queryAllies(unit, radius, cap) {
    const units = this.world.units;
    if (!units?.query) return [];
    const team = unit.team;
    const found = units.query(unit.position, radius, [], (other) => other !== unit && this.relation(team, other.team) > 0);
    if (found.length > cap) found.length = cap;
    return found;
  }

  // -- agents and helpers ---------------------------------------------------------------

  // The agent factory for a controller state (the state-entry table).
  agentFor(state, controller) {
    const agents = this.agents;
    let Agent = null;
    switch (state) {
      case STATE.WAIT:
        Agent = agents.wait;
        break;
      case STATE.COMBAT:
        Agent = agents.combat;
        break;
      case STATE.OBJECTIVE: {
        const role = roleOf(controller.unit?.classDef);
        if (role === ROLE.SCOUT) Agent = agents.scout;
        else if (role === ROLE.SUPPORT || role === ROLE.PILOT) Agent = agents.repair;
        else Agent = agents.trooper;
        break;
      }
      case STATE.WAIT_SECONDARY:
        Agent = agents.waitSecondary;
        break;
      case STATE.BOARD:
        Agent = agents.board;
        break;
      case STATE.UNBOARD:
        Agent = agents.unboard;
        break;
      case STATE.RANDOM:
        Agent = agents.random;
        break;
      case STATE.FOLLOW:
        Agent = agents.follow;
        break;
      case STATE.HOLD:
        Agent = agents.hold;
        break;
      default:
        return null;
    }
    return Agent ? (c, goal) => new Agent(c, goal) : null;
  }

  // The agent's movement helper: FlyGotoHelper for flyers, else
  // GroundGotoHelper; the degraded straight-line helper without Nav.
  createHelper(agent) {
    const controller = agent.controller;
    const vehicleHelper = this.vehicles?.createHelper?.(agent);
    if (vehicleHelper) return vehicleHelper;
    if (this.navHelperFactory) {
      const helper = this.navHelperFactory(agent, controller, this.world);
      if (helper) return helper;
    }
    const nav = this.world.nav;
    if (typeof nav?.helper === "function") return nav.helper(controller, this.world, { agent });
    const Helper = isFlyer(controller.unit) ? (this.helperClasses.fly ?? this.helperClasses.ground) : this.helperClasses.ground;
    if (Helper) return new Helper(controller, this.world, { agent });
    return new DirectHelper(agent);
  }

  // -- characters -------------------------------------------------------------------------

  attach(character) {
    if (!character || character.human) return null;
    let controller = this.byCharacter.get(character);
    if (!controller) {
      controller = new UnitController(this, character);
      this.byCharacter.set(character, controller);
      character.controller = controller;
    }
    if (character.unit?.alive && controller.unit !== character.unit) this.spawned(character);
    return controller;
  }

  detach(character) {
    const controller = this.byCharacter.get(character);
    if (!controller) return;
    this.died(character);
    this.byCharacter.delete(character);
    if (character.controller === controller) character.controller = null;
  }

  // A Character's new body: reset its controller as the native builds a new
  // one per spawned entity (drawing its roll) and republish its objective.
  // Call right after the spawn for the original RNG order.
  spawned(character) {
    const controller = this.byCharacter.get(character) ?? this.attach(character);
    const unit = character?.unit;
    if (!controller || !unit?.alive) return null;
    if (controller.unit === unit && controller.generation === unit.generation) return controller;
    if (controller.unit) this.removeLive(controller);
    controller.bind(unit);
    this.controllers.push(controller);
    return controller;
  }

  died(character) {
    const controller = this.byCharacter.get(character);
    if (!controller?.unit) return;
    this.removeLive(controller);
    controller.unbind();
  }

  removeLive(controller) {
    const i = this.controllers.indexOf(controller);
    if (i >= 0) this.controllers.splice(i, 1);
  }

  // Notice spawns and deaths the integrator did not report.
  sync() {
    for (const [character, controller] of this.byCharacter) {
      const unit = character.unit;
      if (controller.unit && (!unit || unit !== controller.unit || unit.generation !== controller.generation || !unit.alive)) this.died(character);
      if (!controller.unit && unit?.alive) this.spawned(character);
    }
  }

  // -- the step ----------------------------------------------------------------------------

  decide(dt) {
    // Combat turns the step's shot and damage events into controller alerts.
    this.combat?.ingest?.();
    this.sync();
    this.scheduler.update(dt);
  }

  execute(dt) {
    this.scheduler.executeInputs(dt);
  }

  update(dt) {
    this.decide(dt);
    this.execute(dt);
  }

  // -- debug ----------------------------------------------------------------------------------

  controllerOfUnit(unitId) {
    for (const controller of this.byCharacter.values()) if (controller.unit?.id === unitId) return controller;
    return null;
  }

  snapshot() {
    return {
      time: this.world.time ?? 0,
      cycle: this.scheduler.counter,
      parity: this.scheduler.parity,
      hard: this.isHard(),
      scheduler: { ...this.scheduler.stats, skips: this.scheduler.skips },
      controllers: this.controllers.map((c) => c.snapshot()),
    };
  }
}

export function createClassicAI(world, options = {}) {
  const ai = new ClassicAI(world, options);
  let ready;
  if (options.modules) {
    ai.bind(options.modules);
    ready = Promise.resolve(ai);
  } else {
    ai.bind({}, { warn: false });
    ready = loadClassicModules().then((modules) => ai.bind(modules));
  }
  return {
    ai,
    ready,
    update: (dt) => ai.update(dt),
    decide: (dt) => ai.decide(dt),
    execute: (dt) => ai.execute(dt),
    attach: (character) => ai.attach(character),
    detach: (character) => ai.detach(character),
    spawned: (character) => ai.spawned(character),
    died: (character) => ai.died(character),
    playerCommand: (character, kind) => playerCommand(ai, character, kind),
    COMMAND,
    get controllers() {
      return ai.controllers;
    },
    scheduler: ai.scheduler,
    debug: {
      trace(unitId) {
        return ai.controllerOfUnit(unitId)?.traceLog.slice() ?? [];
      },
      snapshot: () => ai.snapshot(),
      tracing(on = true) {
        ai.tracing = on;
      },
    },
  };
}

export { COMMAND };
