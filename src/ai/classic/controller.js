import * as THREE from "three";

// The Classic unit controller: the per-bot state machine the 2004 game
// runs for every AI Character, its queue of source goals (the CommandFsm),
// and the request fields that its agents write and the AI input step reads.
// See docs/ai-controller.md.
//
// Two state machines nest here. The controller's own FSM has states 1..11;
// entering a state builds the agent that runs it (Wait, Combat, the role
// agent for objectives, Follow, ...). Each agent is itself an FSM. Both use
// the same generic driver (Fsm below): in its normal mode it calls the old
// state's exit, copies the pending state to current, calls the new state's
// entry, keeps doing that while entries request more states, then ticks the
// current state, and if that tick requested another state the whole round
// repeats in the same update. Its alternate mode only ticks the current
// state, leaving any requested state pending.
//
// The scheduler calls a controller twice per service: phase 0 then phase 1
// (scheduler.js). Phase 0 clears the request fields, runs only the current
// state's tick (the agents make their decisions and their interrupt checks
// here), then clears the one-update alert inputs. Phase 1 starts any source
// goal that became pending (an interrupt's or a newly published objective),
// draining the state changes and their entries, and gives the agents their
// target-ranking pass. A decision taken in phase 0 therefore takes effect in
// phase 1 of the same service.

export const DONE = 100; // the terminal state every FSM here uses

// Source-goal kinds (the first field of a CommandFsm record).
export const GOAL = Object.freeze({
  NONE: 0,
  WAIT: 1,
  COMBAT: 2,
  OBJECTIVE: 3, // a command post to take or hold
  WAIT_SECONDARY: 4, // a vehicle driver waiting for crew
  BOARD: 5,
  UNBOARD: 6,
  DROID_REPAIR: 7, // only the droid controller handles it
  RECOVERY: 8, // stagnation recovery: random movement
  FOLLOW: 9,
  HOLD: 10,
});

// Controller states. A mobile unit controller maps source goals 1..6 to the
// same numbers, 8 to 9, 9 to 10 and 10 to 11; goal 7 has no branch.
export const STATE = Object.freeze({
  NONE: 0,
  WAIT: 1,
  COMBAT: 2,
  OBJECTIVE: 3,
  WAIT_SECONDARY: 4,
  BOARD: 5,
  UNBOARD: 6,
  RANDOM: 9,
  FOLLOW: 10,
  HOLD: 11,
  DONE,
});

const GOAL_TO_STATE = new Map([
  [GOAL.WAIT, STATE.WAIT],
  [GOAL.COMBAT, STATE.COMBAT],
  [GOAL.OBJECTIVE, STATE.OBJECTIVE],
  [GOAL.WAIT_SECONDARY, STATE.WAIT_SECONDARY],
  [GOAL.BOARD, STATE.BOARD],
  [GOAL.UNBOARD, STATE.UNBOARD],
  [GOAL.RECOVERY, STATE.RANDOM],
  [GOAL.FOLLOW, STATE.FOLLOW],
  [GOAL.HOLD, STATE.HOLD],
]);

export function goalState(kind) {
  return GOAL_TO_STATE.get(kind) ?? 0;
}

// Combat source-goal sub-modes (the record's mode field). The combat agent maps
// 0 to its state 2, 1 to 4, 2 to 5 and 3 to 6.
export const COMBAT_MODE = Object.freeze({
  LOCATION: 0, // respond to a source's position (shot/damage alert in range, acoustic contact)
  TARGET: 1, // the scanner's acquired target
  EVADE: 2, // grenade/danger point, or the random fallback near itself
  MODE3: 3,
});

// UnitType roles, as the soldier class parser stores them. Source goal 3
// gives role 0 the scout agent, roles 3 and 4 the repair agent, every other
// role the trooper agent.
export const ROLE = Object.freeze({ SCOUT: 0, ANTI_ARMOR: 1, ASSAULT: 2, SUPPORT: 3, PILOT: 4, TROOPER: 5, OTHER: 6 });

const ROLE_NAMES = new Map([
  ["scout", ROLE.SCOUT],
  ["anti-armor", ROLE.ANTI_ARMOR],
  ["assault", ROLE.ASSAULT],
  ["support", ROLE.SUPPORT],
  ["repair", ROLE.SUPPORT],
  ["special", ROLE.SUPPORT],
  ["pilot", ROLE.PILOT],
  ["trooper", ROLE.TROOPER],
]);

// The role of a class record. The parser compares the authored string's
// case-folded hash, so case does not matter; anything unrecognized (or no
// UnitType at all) is 6. A numeric `role` (from ai.json) wins when present.
export function roleOf(classDef) {
  if (!classDef) return ROLE.OTHER;
  if (Number.isInteger(classDef.role)) return classDef.role;
  const type = classDef.UnitType ?? classDef.unitType;
  if (typeof type !== "string") return ROLE.OTHER;
  return ROLE_NAMES.get(type.trim().toLowerCase()) ?? ROLE.OTHER;
}

// ---------------------------------------------------------------------------
// Source goals: the CommandFsm's 32-byte records, as objects. Entity
// references are {unit, generation} pairs (registry refs). `point` stands in
// for the one-point AIPath payload combat goals carry.

export class SourceGoal {
  constructor(kind = GOAL.NONE) {
    this.kind = kind;
    this.refA = null;
    this.refB = null;
    this.post = null;
    this.point = null;
    this.mode = 0;
  }

  static objective(post) {
    const goal = new SourceGoal(GOAL.OBJECTIVE);
    goal.post = post ?? null;
    return goal;
  }

  // mode: COMBAT_MODE; point (Vector3) for the location/evade modes, target
  // ref for TARGET.
  static combat(mode, { point = null, target = null } = {}) {
    const goal = new SourceGoal(GOAL.COMBAT);
    goal.mode = mode;
    if (point) goal.point = point.clone();
    goal.refA = target ? { unit: target.unit, generation: target.generation } : null;
    return goal;
  }

  // refA: the requested leader (the command's target); refB: the issuing
  // Character's current entity. Both are the same soldier for an ordinary
  // infantry follow, but they stay separate fields.
  static follow(leaderRef, sourceRef) {
    const goal = new SourceGoal(GOAL.FOLLOW);
    goal.refA = leaderRef ?? null;
    goal.refB = sourceRef ?? null;
    return goal;
  }

  static board(vehicleRef) {
    const goal = new SourceGoal(GOAL.BOARD);
    goal.refA = vehicleRef ?? null;
    return goal;
  }

  static droidRepair(targetRef, ownerRef) {
    const goal = new SourceGoal(GOAL.DROID_REPAIR);
    goal.refA = targetRef ?? null;
    goal.refB = ownerRef ?? null;
    return goal;
  }

  static of(kind) {
    return new SourceGoal(kind);
  }

  copy() {
    const goal = new SourceGoal(this.kind);
    goal.refA = this.refA;
    goal.refB = this.refB;
    goal.post = this.post;
    goal.point = this.point ? this.point.clone() : null;
    goal.mode = this.mode;
    return goal;
  }
}

// ---------------------------------------------------------------------------
// The CommandFsm's source-goal storage: the goal being executed, up to five
// queued records, the insertion latch and the "current agent finished" flag.

export const COMMAND_CAPACITY = 5;

export class CommandQueue {
  constructor() {
    this.current = new SourceGoal(GOAL.NONE);
    this.queue = [];
    this.latch = false; // set by overwrite/prepend; a set latch starts the front goal next
    this.currentDone = false; // the running agent reached its terminal state
  }

  get count() {
    return this.queue.length;
  }

  // A goal should start: something was inserted at the front, or the current
  // agent finished and another goal is queued.
  needsNextGoal() {
    return this.latch || (this.currentDone && this.queue.length > 0);
  }

  // Replace the whole queue with one goal and mark it to start. This is the
  // mutator the objective publisher and the droid-repair producer use.
  overwrite(goal) {
    this.latch = true;
    this.queue.length = 0;
    this.queue.push(goal.copy());
  }

  // Append while below capacity; no latch.
  append(goal) {
    if (this.queue.length >= COMMAND_CAPACITY) return false;
    this.queue.push(goal.copy());
    return true;
  }

  // Insert one goal at the front unless an insertion is already pending.
  // A full queue drops its last record.
  prependOne(goal) {
    if (this.latch) return false;
    this.queue.unshift(goal.copy());
    if (this.queue.length > COMMAND_CAPACITY) this.queue.length = COMMAND_CAPACITY;
    this.latch = true;
    return true;
  }

  // Insert a pair at the front: `goal` first, then `saved` (the interrupted
  // command, so it resumes when the interrupt's agent finishes).
  prependTwo(saved, goal) {
    if (this.latch) return false;
    this.queue.unshift(goal.copy(), saved.copy());
    if (this.queue.length > COMMAND_CAPACITY) this.queue.length = COMMAND_CAPACITY;
    this.latch = true;
    return true;
  }

  // Start the front goal: it becomes current and leaves the queue.
  popFront() {
    const goal = this.queue.shift();
    if (goal) this.current = goal;
    this.currentDone = false;
    return this.current;
  }

  // Drop queued goals of one kind, keeping the others in order. As the
  // 2004 game compacts them, a record of kind 0 ends the scan and drops
  // everything after it.
  removeKind(kind) {
    const kept = [];
    for (const goal of this.queue) {
      if (goal.kind === GOAL.NONE) break;
      if (goal.kind !== kind) kept.push(goal);
    }
    this.queue = kept;
  }

  // A copy of the current goal (an empty record if its kind is out of range).
  currentCopy() {
    const kind = this.current.kind;
    return kind >= 0 && kind <= 10 ? this.current.copy() : new SourceGoal(GOAL.NONE);
  }
}

// ---------------------------------------------------------------------------
// The generic FSM driver.

// Bound on transitions in one update. The 2004 game has none (a cycle of
// states requesting each other would hang it); the port stops and warns.
const DRAIN_LIMIT = 64;

export class Fsm {
  constructor() {
    this.state = 0;
    this.pending = 0;
  }

  get done() {
    return this.state === DONE;
  }

  requestState(state) {
    this.pending = state;
  }

  // Subclasses switch on this.state in these.
  onEnter() {}
  onExit() {}
  onTick(_dt) {}

  update(dt, alternate = false) {
    if (alternate) {
      if (this.state !== 0) this.onTick(dt);
      return;
    }
    let rounds = 0;
    let next = this.pending;
    while (true) {
      while (next !== 0) {
        if (++rounds > DRAIN_LIMIT) return this.runaway();
        this.onExit();
        this.state = this.pending;
        this.pending = 0;
        this.onEnter();
        next = this.pending;
      }
      if (this.state === 0) return;
      this.onTick(dt);
      next = this.pending;
      if (next === 0) return;
    }
  }

  // Apply pending transitions without ticking.
  flush() {
    let rounds = 0;
    while (this.pending !== 0) {
      if (++rounds > DRAIN_LIMIT) return this.runaway();
      this.onExit();
      this.state = this.pending;
      this.pending = 0;
      this.onEnter();
    }
  }

  // Leave the current state for the terminal one (no entry runs).
  terminate() {
    this.onExit();
    this.state = DONE;
  }

  runaway() {
    console.warn(`[classic-ai] ${this.constructor.name}: state transitions did not settle; stopped at ${this.state}`);
    this.pending = 0;
  }
}

// ---------------------------------------------------------------------------
// Weak references.

export function refOf(unit) {
  return unit ? { unit, generation: unit.generation } : null;
}

export function resolveRef(ref) {
  const unit = ref?.unit;
  if (!unit || unit.generation !== ref.generation || !unit.alive) return null;
  return unit;
}

export function sameRef(a, b) {
  if (!a || !b) return a === b || (!resolveRef(a) && !resolveRef(b));
  return a.unit === b.unit && a.generation === b.generation;
}

// ---------------------------------------------------------------------------
// The unit controller.

const ALERT_ACOUSTIC_CAP = 20;
const VEHICLE_NOTE_CAP = 10;
const TRACE_SIZE = 64;

export class UnitController extends Fsm {
  // ai: the Classic AI context (index.js): world, random streams, sibling
  // bindings, agent registry.
  constructor(ai, character) {
    super();
    this.ai = ai;
    this.world = ai.world;
    this.character = character;
    this.unit = null;
    this.generation = -1;
    this.agent = null;
    this.commands = new CommandQueue();
    this.phase = 0;
    this.published = false; // an objective was published during this phase 1

    // Movement request, written by the movement helper (Nav).
    this.request = {
      valid: false,
      steeringTarget: new THREE.Vector3(),
      waypoint: new THREE.Vector3(),
      segmentDirection: new THREE.Vector3(),
      mode: 0,
      lower: -1,
      upper: 1,
    };
    // Aim request: 0 aim at aimTarget (with lead), 1 at aimPoint, 2 along
    // the steering target.
    this.aimState = 2;
    this.aimPoint = new THREE.Vector3();
    this.aimTarget = null; // ref for aimState 0
    this.target = null; // firing target ref (Combat writes it)
    this.fireA = false; // direct fire request
    this.fireB = false; // validated fire request
    this.facing = false; // transient enhanced-facing request (yaw clamp +-2)
    this.facingAux = false; // the byte beside it, cleared with it
    this.stance = 0; // requested posture: 0 stand, 1 crouch, 2 prone
    this.rangeScale = 1; // effective weapon-range scale (sniping sets 2)
    this.jet = { pending: false, vector: new THREE.Vector3() };
    this.useTarget = null; // a request reference cleared every phase 0 (purpose unknown)
    this.wantedMode = 0; // droideka: 1 deploy, 2 roll (0 none)
    this.exitRequest = false; // leave the occupied vehicle/turret (Vehicles consume it)
    this.leader = null; // follow leader ref (what the follower count reads)

    // Alert inputs (Combat's producers call the notify methods); cleared
    // after each phase 0.
    this.danger = { pending: false, point: new THREE.Vector3() };
    this.shot = { pending: false, known: false, point: new THREE.Vector3(), source: null };
    this.acoustic = []; // refs, at most 20 per update
    this.vehicleNotes = []; // boarding candidate refs, at most 10
    this.command = { pending: false, record: null }; // a squad command addressed to this bot
    this.lastCommand = null; // the command last handled (compared by move-out/at-ease)

    this.secondaryWaitUntil = 0; // crew-wait window (vehicles)
    this.reboardAt = -Infinity; // the Character's boarding cooldown
    this.roll = 0; // integer 0..100 drawn per spawn; read by weapon/attack-pattern choice (Combat)
    this.homeFacing = false; // stationary controllers only
    this.homeDirection = new THREE.Vector3(0, 0, 1);

    // Scheduler bookkeeping.
    this.processed = false;
    this.lodSkipped = false;
    this.inputSkip = false;
    this.inputCarry = 0;

    this.navControl = null; // Nav's per-unit movement state (its helpers create it)
    this.perception = null; // Combat's per-controller state
    this.difficulty = null;
    this.traceLog = [];
  }

  // -- lifetime ---------------------------------------------------------------

  // A new body for this Character: the 2004 game builds a fresh controller
  // for every spawned entity. Reset everything the constructor sets, draw the
  // per-controller roll, then republish the Character's existing objective.
  bind(unit) {
    if (this.agent) this.disposeAgent();
    this.unit = unit;
    this.generation = unit.generation;
    unit.controller = this;
    this.state = 0;
    this.pending = 0;
    this.commands = new CommandQueue();
    this.phase = 0;
    this.clearRequests();
    this.aimState = 2;
    this.aimTarget = null;
    this.stance = 0;
    this.wantedMode = 0;
    this.exitRequest = false;
    this.leader = null;
    this.danger.pending = false;
    this.shot.pending = false;
    this.shot.known = false;
    this.shot.source = null;
    this.acoustic.length = 0;
    this.vehicleNotes.length = 0;
    this.command.pending = false;
    this.command.record = null;
    this.lastCommand = null;
    this.processed = false;
    this.lodSkipped = false;
    this.inputSkip = false;
    this.inputCarry = 0;
    // Walkers, hovers and flyers open a four-second crew-wait window.
    this.secondaryWaitUntil = unit.kind === 2 || unit.kind === 5 || unit.kind === 6 ? this.now() + 4 : 0;
    this.homeFacing = false;
    this.roll = this.ai.randomA.int(0, 100);
    // The body's facing at creation (its transform's forward row).
    const yaw = unit.soldier?.aimYaw ?? 0;
    this.homeDirection.set(Math.sin(yaw), 0, Math.cos(yaw));
    this.difficulty = this.ai.curvesFor(unit.team);
    this.perception = this.ai.combat?.createPerception?.(this) ?? this.perception ?? null;
    this.ai.combat?.attachController?.(this);
    this.trace("bind", { unit: unit.id, generation: unit.generation });
    const post = this.character?.objective;
    if (post) this.publishObjective(post);
  }

  unbind() {
    if (this.agent) this.disposeAgent();
    // The life's path work ends with it: the planner would otherwise go on
    // searching for a body that is gone, on the budget the living wait for.
    this.navControl?.reset?.();
    if (this.unit && this.unit.controller === this) this.unit.controller = null;
    this.ai.combat?.detachController?.(this);
    this.unit = null;
    this.generation = -1;
    this.state = 0;
    this.pending = 0;
    this.leader = null;
    this.trace("unbind");
  }

  get isUnitController() {
    return true;
  }

  get live() {
    const unit = this.unit;
    return Boolean(unit && unit.alive && unit.generation === this.generation);
  }

  now() {
    return this.world.time ?? 0;
  }

  // -- the per-service update --------------------------------------------------

  update(dt, phase) {
    this.phase = phase;
    const restricted = phase === 0;
    if (restricted) this.clearRequests();
    this.driveCommands(dt, restricted);
    if (restricted) {
      this.shot.pending = false;
      this.shot.known = false;
      this.danger.pending = false;
      this.vehicleNotes.length = 0;
      this.command.pending = false;
      this.useTarget = null;
      this.clearAcoustic();
    }
    this.lowTicketExit();
  }

  // The CommandFsm's own update: start pending goals, then run the state
  // machine (normal mode), until no goal is pending; in phase 0 only tick
  // the current state, and only when no goal is waiting to start.
  driveCommands(dt, restricted) {
    const commands = this.commands;
    if (restricted) {
      if (!commands.needsNextGoal()) super.update(dt, true);
      return;
    }
    let rounds = 0;
    this.published = false;
    do {
      if (commands.needsNextGoal()) {
        if (commands.count) {
          const goal = commands.popFront();
          commands.latch = false;
          this.trace("goal", { kind: goal.kind, mode: goal.mode, post: goal.post?.name ?? null });
          const state = goalState(goal.kind);
          if (state) this.requestState(state);
        } else {
          // A latch over an emptied queue (an insertion later filtered out):
          // the 2004 game would start a stale record; nothing starts here.
          commands.latch = false;
        }
      }
      super.update(dt, false);
      if (++rounds > DRAIN_LIMIT) return this.runaway();
      // An objective published while goals start here (a mounted patrol
      // asking for a post as it begins) starts at the next phase 1, as
      // publication always does. Starting it now could publish again from
      // the same start, round and round, and the bot would never act.
    } while (commands.needsNextGoal() && !this.published);
  }

  // Map the current goal to a state. A droid controller would map 2, 3 and
  // 7 differently; Classic infantry use the mobile mapping.
  goalToState() {
    const state = goalState(this.commands.current.kind);
    if (state) this.requestState(state);
  }

  onEnter() {
    const factory = this.ai.agentFor(this.state, this);
    this.agent = factory ? factory(this, this.commands.current) : null;
    this.trace("enter", { state: this.state, agent: this.agent?.constructor.name ?? null });
  }

  onExit() {
    if (this.agent) {
      this.agent.exit();
      this.disposeAgent();
    }
  }

  onTick(dt) {
    if (this.agent) this.commands.currentDone = Boolean(this.agent.tick(dt, this.phase));
  }

  disposeAgent() {
    const agent = this.agent;
    this.agent = null;
    agent?.dispose?.();
  }

  // -- requests (the Controllable's request fields) ---------------------------

  // What phase 0 clears before the state machine runs.
  clearRequests() {
    this.jet.pending = false;
    this.jet.vector.set(0, 0, 0);
    this.fireA = false;
    this.fireB = false;
    this.target = null;
    this.facing = false;
    this.facingAux = false;
    this.rangeScale = 1;
  }

  lookAt(point) {
    this.aimState = 1;
    this.aimPoint.copy(point);
  }

  aimAt(ref) {
    this.aimState = 0;
    this.aimTarget = ref ?? null;
  }

  lookAlongPath() {
    this.aimState = 2;
  }

  // Posture request (0 stand, 1 crouch, 2 prone). Nav's per-unit control
  // keeps the same field when it exists.
  setStance(stance) {
    this.stance = stance;
    if (this.navControl) this.navControl.stance = stance;
  }

  // Transient enhanced facing (soldiers and droidekas only).
  requestFacing() {
    const kind = this.unit?.kind;
    if (kind === 0 || kind === 3) this.facing = true;
  }

  // A jet launch request (zero vector: the soldier's ordinary direction).
  requestJet(vector) {
    const jet = this.navControl?.jet ?? this.jet;
    jet.vector.set(vector.x, vector.y, vector.z);
    jet.pending = true;
  }

  // The movement executor (Nav's per-unit control): its progress status
  // (0, 1 no improvement, 2 stationary), whether a path is being followed,
  // and the remembered contact-sidestep side.
  get executorStatus() {
    return this.navControl?.status ?? this.agent?.helper?.status ?? 0;
  }

  get hasExecutor() {
    const control = this.navControl;
    if (control) return Boolean(control.executor);
    return Boolean(this.agent?.helper?.hasPath);
  }

  get avoidSide() {
    return this.navControl?.avoidSide ?? 0;
  }

  requestExit() {
    this.exitRequest = true;
    this.ai.vehicles?.requestExit?.(this);
  }

  // -- alert intake -----------------------------------------------------------

  // Grenade/sticky danger at a point.
  notifyDanger(point) {
    this.danger.pending = true;
    this.danger.point.copy(point);
  }

  // A targeted shot or damage from `source` (a Character). A known position
  // is remembered (the response itself uses the source's current position);
  // an unknown one leaves an earlier known point in place.
  notifyShot(source, known = false, point = null) {
    if (!source) return;
    this.shot.pending = true;
    this.shot.source = source;
    if (known && point) {
      this.shot.known = true;
      this.shot.point.copy(point);
    }
  }

  // A moving or weapon-active enemy nearby (the acoustic broadcast).
  addAcoustic(ref) {
    if (this.acoustic.length < ALERT_ACOUSTIC_CAP) this.acoustic.push(ref);
  }

  clearAcoustic() {
    this.acoustic.length = 0;
  }

  addVehicleNote(ref) {
    if (this.vehicleNotes.length < VEHICLE_NOTE_CAP) this.vehicleNotes.push(ref);
  }

  // A squad command addressed to this bot (squads.js delivers it).
  receiveCommand(record) {
    this.command.pending = true;
    this.command.record = { ...record };
  }

  // -- objectives ---------------------------------------------------------------

  get objective() {
    return this.character?.objective ?? null;
  }

  // Publishing an objective to a live controller replaces its queue with the
  // command-post goal and starts it at the next phase 1.
  publishObjective(post) {
    if (!post) return;
    this.commands.overwrite(SourceGoal.objective(post));
    this.published = true;
    this.trace("objective", { post: post.name ?? null });
  }

  // Ask Population for a new objective (excluding the current one when told).
  requestObjective(excludePrevious = false) {
    const ai = this.ai;
    const request = ai.population?.requestObjective;
    if (!request) {
      ai.warnOnce("requestObjective", "Population goals.js is not bound: objectives are not reassigned");
      return null;
    }
    return request(this.character, excludePrevious, this.objective, ai.world);
  }

  // Low reinforcements make AI vehicle pilots leave.
  lowTicketExit() {
    const vehicles = this.ai.vehicles;
    if (!vehicles?.boarded?.(this)) return;
    if (vehicles.lowTickets?.(this.unit.team) && !vehicles.pilotKeepsSeat?.(this)) this.requestExit();
  }

  // -- debug ----------------------------------------------------------------------

  trace(kind, fields = {}) {
    if (!this.ai.tracing) return;
    const log = this.traceLog;
    log.push({ time: this.now(), kind, ...fields });
    if (log.length > TRACE_SIZE) log.shift();
  }

  snapshot() {
    const goal = this.commands.current;
    return {
      character: this.character?.index ?? null,
      team: this.character?.team ?? this.unit?.team ?? null,
      unit: this.unit?.id ?? null,
      state: this.state,
      pending: this.pending,
      agent: this.agent?.constructor.name ?? null,
      agentState: this.agent?.state ?? null,
      goal: { kind: goal.kind, mode: goal.mode, post: goal.post?.name ?? null },
      queue: this.commands.queue.map((g) => g.kind),
      latch: this.commands.latch,
      objective: this.objective?.name ?? null,
      aimState: this.aimState,
      stance: this.stance,
      alerts: {
        danger: this.danger.pending,
        shot: this.shot.pending,
        acoustic: this.acoustic.length,
        command: this.command.pending,
      },
      processed: this.processed,
      lodSkipped: this.lodSkipped,
    };
  }
}
