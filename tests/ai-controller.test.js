import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import { Units } from "../src/registry.js";
import { SimEvents } from "../src/sim-events.js";
import {
  COMBAT_MODE,
  COMMAND_CAPACITY,
  CommandQueue,
  DONE,
  Fsm,
  GOAL,
  ROLE,
  STATE,
  SourceGoal,
  UnitController,
  goalState,
  roleOf,
} from "../src/ai/classic/controller.js";
import {
  Agent,
  UnitFollowAgent,
  UnitHoldAgent,
  UnitRandomAgent,
  UnitRepairAgent,
  UnitScoutAgent,
  UnitTrooperAgent,
  UnitWaitAgent,
  rotateY,
  tableSinCos,
} from "../src/ai/classic/agents.js";
import { InterruptHelper, PRIORITY } from "../src/ai/classic/interrupts.js";
import { PatrolHelper, arrivedAtObjective, chooseHint, cpOffset, pickHintStance, queryPatrolHints } from "../src/ai/classic/patrol.js";
import { COMMAND, SquadCommand, countFollowers, distributeCommand, sendCommand, voEntry } from "../src/ai/classic/squads.js";
import { ClassicScheduler, cameraAccepts, candidateBudget } from "../src/ai/classic/scheduler.js";
import { ClassicAI, createClassicAI } from "../src/ai/classic/index.js";

const FIXTURES = new URL("../extract/ai-research/round8/navigation/", import.meta.url);
function nativeFixture(name) {
  try {
    return JSON.parse(fs.readFileSync(new URL(name, FIXTURES), "utf8"));
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Synthetic world.

function blankIntent() {
  return {
    forward: 0, right: 0, turn: 0, pitch: 0,
    jump: false, crouch: false, prone: false, use: false,
    buttons: [0, 0], reload: false, nextPrimary: false, nextSecondary: false, zoom: false, aim: null,
  };
}

function makeUnit(units, team, x, z, { kind = 0, unitType = "trooper", speed = 6.5, hero = false, human = false } = {}) {
  const position = new THREE.Vector3(x, 0, z);
  const unit = {
    team,
    kind,
    hero,
    human,
    alive: true,
    dead: false,
    generation: 1,
    character: null,
    controller: null,
    classDef: { UnitType: unitType, MaxSpeed: speed },
    soldier: { maxSpeed: speed, maxStrafe: 4.5, maxTurn: 4, aimYaw: 0, aimPitch: 0, posture: "stand", mode: 0, rolled: false },
    position,
    velocity: new THREE.Vector3(),
    sphere: { center: position, radius: 0.5 },
    intent: blankIntent(),
    forward(out) {
      return out.set(Math.sin(this.soldier.aimYaw), 0, Math.cos(this.soldier.aimYaw));
    },
    aimPoint(out) {
      return out.copy(this.position).setY(1);
    },
    origin(out) {
      return out.copy(this.position).setY(1.6);
    },
    hitSpheres() {
      return [{ center: position, radius: 0.5 }];
    },
  };
  units.add(unit);
  return unit;
}

function makeCharacter(index, team, human = false) {
  return { index, team, human, unit: null, controller: null, objective: null };
}

function makePost(name, team, x, z, { size = [4, 2, 4], spawnPath = null } = {}) {
  const position = new THREE.Vector3(x, 0, z);
  const matrix = new THREE.Matrix4().makeTranslation(x, 0, z).toArray();
  const inverse = new THREE.Matrix4().fromArray(matrix).invert();
  return {
    name,
    team,
    position,
    desc: { capture: { type: "box", size, matrix }, spawnPath },
    contains(point, scratch = new THREE.Vector3()) {
      scratch.copy(point).applyMatrix4(inverse);
      return Math.abs(scratch.x) < size[0] && Math.abs(scratch.y) < size[1] && Math.abs(scratch.z) < size[2];
    },
  };
}

// A recording movement helper (the Nav interface the agents use).
class FakeHelper {
  constructor(controller) {
    this.controller = controller;
    this.calls = [];
    this.arrived = false;
    this.status = 0;
    this.dest = new THREE.Vector3();
  }
  goto(point, options = {}) {
    this.calls.push(["goto", point.clone(), options.mode ?? "route"]);
    this.dest.copy(point);
  }
  stop() {
    this.calls.push(["stop"]);
  }
  tick() {
    this.calls.push(["tick"]);
  }
  terminate() {
    this.calls.push(["terminate"]);
  }
  destinationPoint(out) {
    return out.copy(this.dest);
  }
}

function makeWorld({ seed = 1, seedB = 2, hard = false } = {}) {
  const units = new Units();
  const world = {
    time: 0,
    random: new ClassicRandom(seed),
    randomB: new ClassicRandom(seedB),
    units,
    events: new SimEvents(),
    battle: { characters: [] },
    difficulty: hard ? { cycle: 3, losBudget: 12 } : { cycle: 15, losBudget: 6 },
    nav: { hints: [] },
  };
  return world;
}

function makeAI(world, options = {}) {
  const ai = new ClassicAI(world, { quiet: true, createHelper: (agent, controller) => new FakeHelper(controller), ...options });
  ai.bind(options.modules ?? {}, { warn: false });
  return ai;
}

function spawnBot(ai, world, team, x, z, unitOptions = {}) {
  const character = makeCharacter(world.battle.characters.length, team);
  world.battle.characters.push(character);
  const unit = makeUnit(world.units, team, x, z, unitOptions);
  unit.character = character;
  character.unit = unit;
  ai.attach(character);
  return { character, unit, controller: character.controller };
}

// ---------------------------------------------------------------------------
// The generic FSM driver.

class Probe extends Fsm {
  constructor(script) {
    super();
    this.log = [];
    this.script = script;
  }
  onEnter() {
    this.log.push(`enter${this.state}`);
    const next = this.script.enter?.[this.state];
    if (next) this.requestState(next);
  }
  onExit() {
    this.log.push(`exit${this.state}`);
  }
  onTick() {
    this.log.push(`tick${this.state}`);
    const next = this.script.tick?.[this.state];
    if (next) {
      delete this.script.tick[this.state];
      this.requestState(next);
    }
  }
}

test("FSM drains entry chains and tick requests within one update", () => {
  const fsm = new Probe({ enter: { 1: 2, 2: 3 }, tick: { 3: 4 } });
  fsm.requestState(1);
  fsm.update(0.1, false);
  assert.deepEqual(fsm.log, ["exit0", "enter1", "exit1", "enter2", "exit2", "enter3", "tick3", "exit3", "enter4", "tick4"]);
  assert.equal(fsm.state, 4);
  assert.equal(fsm.pending, 0);
});

test("FSM alternate mode only ticks; requests wait for a normal update", () => {
  const fsm = new Probe({});
  fsm.requestState(1);
  fsm.update(0.1, false);
  fsm.log.length = 0;
  fsm.script.tick = { 1: 2 };
  fsm.update(0.1, true);
  assert.deepEqual(fsm.log, ["tick1"]);
  assert.equal(fsm.state, 1);
  assert.equal(fsm.pending, 2);
  fsm.update(0.1, false);
  assert.equal(fsm.state, 2);
  // Nothing to tick at state 0 in alternate mode.
  const idle = new Probe({});
  idle.update(0.1, true);
  assert.deepEqual(idle.log, []);
});

test("FSM terminate exits to 100 without an entry; flush applies without ticking", () => {
  const fsm = new Probe({});
  fsm.requestState(3);
  fsm.flush();
  assert.deepEqual(fsm.log, ["exit0", "enter3"]);
  fsm.terminate();
  assert.equal(fsm.state, DONE);
  assert.ok(fsm.done);
  assert.deepEqual(fsm.log, ["exit0", "enter3", "exit3"]);
});

// ---------------------------------------------------------------------------
// The CommandFsm records.

const goalOf = (kind) => SourceGoal.of(kind);
const kinds = (q) => q.queue.map((g) => g.kind);

test("command queue: append is capped at five and sets no latch", () => {
  const q = new CommandQueue();
  for (let i = 1; i <= 7; i++) q.append(goalOf(i));
  assert.equal(COMMAND_CAPACITY, 5);
  assert.deepEqual(kinds(q), [1, 2, 3, 4, 5]);
  assert.equal(q.latch, false);
  assert.equal(q.needsNextGoal(), false);
  q.currentDone = true;
  assert.equal(q.needsNextGoal(), true);
});

test("command queue: prepends set the latch, are refused while it is set, and drop the tail when full", () => {
  const q = new CommandQueue();
  for (let i = 1; i <= 5; i++) q.append(goalOf(i));
  assert.ok(q.prependOne(goalOf(8)));
  assert.deepEqual(kinds(q), [8, 1, 2, 3, 4]);
  assert.ok(q.latch);
  assert.equal(q.prependOne(goalOf(9)), false);
  assert.equal(q.prependTwo(goalOf(3), goalOf(2)), false);
  q.latch = false;
  // The new goal first, then the saved one.
  assert.ok(q.prependTwo(goalOf(3), goalOf(2)));
  assert.deepEqual(kinds(q), [2, 3, 8, 1, 2]);
  assert.ok(q.needsNextGoal());
});

test("command queue: overwrite replaces everything; popFront starts the front goal", () => {
  const q = new CommandQueue();
  q.append(goalOf(4));
  q.append(goalOf(5));
  q.overwrite(SourceGoal.objective({ name: "cp1" }));
  assert.deepEqual(kinds(q), [GOAL.OBJECTIVE]);
  assert.ok(q.latch);
  q.currentDone = true;
  const goal = q.popFront();
  assert.equal(goal.kind, GOAL.OBJECTIVE);
  assert.equal(goal.post.name, "cp1");
  assert.equal(q.count, 0);
  assert.equal(q.currentDone, false);
});

test("command queue: removing a kind compacts survivors and stops at an empty record", () => {
  const q = new CommandQueue();
  [GOAL.FOLLOW, GOAL.OBJECTIVE, GOAL.HOLD, GOAL.COMBAT].forEach((k) => q.append(goalOf(k)));
  q.removeKind(GOAL.FOLLOW);
  q.removeKind(GOAL.HOLD);
  assert.deepEqual(kinds(q), [GOAL.OBJECTIVE, GOAL.COMBAT]);
  const r = new CommandQueue();
  [GOAL.COMBAT, GOAL.NONE, GOAL.OBJECTIVE].forEach((k) => r.append(goalOf(k)));
  r.removeKind(GOAL.HOLD);
  assert.deepEqual(kinds(r), [GOAL.COMBAT]);
});

test("source goals map to controller states; roles pick the objective agent", () => {
  assert.equal(goalState(GOAL.WAIT), STATE.WAIT);
  assert.equal(goalState(GOAL.COMBAT), STATE.COMBAT);
  assert.equal(goalState(GOAL.OBJECTIVE), STATE.OBJECTIVE);
  assert.equal(goalState(GOAL.WAIT_SECONDARY), 4);
  assert.equal(goalState(GOAL.BOARD), 5);
  assert.equal(goalState(GOAL.UNBOARD), 6);
  assert.equal(goalState(GOAL.DROID_REPAIR), 0);
  assert.equal(goalState(GOAL.RECOVERY), STATE.RANDOM);
  assert.equal(goalState(GOAL.FOLLOW), STATE.FOLLOW);
  assert.equal(goalState(GOAL.HOLD), STATE.HOLD);
  assert.equal(roleOf({ UnitType: "Scout" }), ROLE.SCOUT);
  assert.equal(roleOf({ UnitType: "anti-armor" }), ROLE.ANTI_ARMOR);
  assert.equal(roleOf({ UnitType: "REPAIR" }), ROLE.SUPPORT);
  assert.equal(roleOf({ UnitType: "special" }), ROLE.SUPPORT);
  assert.equal(roleOf({ UnitType: "pilot" }), ROLE.PILOT);
  assert.equal(roleOf({ UnitType: "trooper" }), ROLE.TROOPER);
  assert.equal(roleOf({ UnitType: "sharpshooter" }), ROLE.OTHER);
  assert.equal(roleOf({}), ROLE.OTHER);

  const world = makeWorld();
  const ai = makeAI(world);
  const post = makePost("cp1", 2, 50, 0);
  const pick = (type) => {
    const { character, controller } = spawnBot(ai, world, 1, 0, 0, { unitType: type });
    character.objective = post;
    controller.publishObjective(post);
    controller.update(0.016, 1);
    return controller.agent;
  };
  assert.ok(pick("scout") instanceof UnitScoutAgent);
  assert.ok(pick("pilot") instanceof UnitRepairAgent);
  assert.ok(pick("support") instanceof UnitRepairAgent);
  const trooper = pick("assault");
  assert.ok(trooper instanceof UnitTrooperAgent && !(trooper instanceof UnitScoutAgent) && !(trooper instanceof UnitRepairAgent));
});

// ---------------------------------------------------------------------------
// Controller phases.

test("phase 0 ticks without starting goals and clears alerts; phase 1 starts the goal", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const { controller } = spawnBot(ai, world, 1, 0, 0);
  controller.commands.overwrite(SourceGoal.of(GOAL.WAIT));
  controller.fireA = true;
  controller.facing = true;
  controller.danger.pending = true;
  controller.shot.pending = true;
  controller.shot.known = true;
  controller.command.pending = true;
  controller.addAcoustic({ unit: null, generation: 0 });
  controller.update(0.016, 0);
  assert.equal(controller.state, 0, "phase 0 does not start pending goals");
  assert.equal(controller.agent, null);
  assert.equal(controller.fireA, false);
  assert.equal(controller.facing, false);
  assert.equal(controller.danger.pending, false);
  assert.equal(controller.shot.pending, false);
  assert.equal(controller.shot.known, false);
  assert.equal(controller.command.pending, false);
  assert.equal(controller.acoustic.length, 0);
  controller.update(0.016, 1);
  assert.equal(controller.state, STATE.WAIT);
  assert.ok(controller.agent instanceof UnitWaitAgent);
  assert.equal(controller.agent.state, 2, "the agent's own entry drained in phase 1");
  assert.equal(controller.commands.latch, false);
});

test("a finished agent starts the next queued goal in phase 1, several in one update", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const { controller } = spawnBot(ai, world, 1, 0, 0);
  // Combat is unbound: its stand-in finishes at once, so the queue drains to
  // the wait goal behind it in the same phase-1 update.
  controller.commands.overwrite(SourceGoal.combat(COMBAT_MODE.LOCATION, { point: new THREE.Vector3(1, 0, 1) }));
  controller.commands.append(SourceGoal.of(GOAL.WAIT));
  controller.update(0.016, 1);
  assert.equal(controller.state, STATE.WAIT);
  assert.equal(controller.commands.current.kind, GOAL.WAIT);
});

test("binding a new body resets the controller, draws its roll from generator A and republishes the objective", () => {
  const world = makeWorld({ seed: 77 });
  const ai = makeAI(world);
  const post = makePost("cp", 2, 30, 0);
  const character = makeCharacter(0, 1);
  character.objective = post;
  world.battle.characters.push(character);
  const unit = makeUnit(world.units, 1, 0, 0);
  unit.character = character;
  character.unit = unit;
  const expected = new ClassicRandom(77).int(0, 100);
  const controller = ai.attach(character);
  assert.equal(controller.roll, expected);
  assert.equal(controller.unit, unit);
  assert.deepEqual(kinds(controller.commands), [GOAL.OBJECTIVE]);
  assert.ok(controller.commands.latch);
  assert.deepEqual(ai.controllers, [controller]);
  unit.alive = false;
  ai.sync();
  assert.equal(controller.unit, null);
  assert.deepEqual(ai.controllers, []);
});

// ---------------------------------------------------------------------------
// Interrupts.

class OpenAgent extends Agent {
  constructor(controller, goal, saves = true) {
    super(controller, goal);
    this.saves = saves;
  }
  savesOnInterrupt() {
    return this.saves;
  }
}

function agentFixture({ saves = true } = {}) {
  const world = makeWorld();
  const ai = makeAI(world);
  const bot = spawnBot(ai, world, 1, 0, 0);
  const { controller } = bot;
  controller.commands.popFront();
  controller.commands.latch = false;
  controller.commands.current = SourceGoal.objective({ name: "cp1" });
  const agent = new OpenAgent(controller, controller.commands.current, saves);
  controller.agent = agent;
  return { world, ai, controller, agent, ...bot };
}

test("interrupts fire in strict priority order: danger first", () => {
  const { controller, agent } = agentFixture();
  controller.notifyDanger(new THREE.Vector3(3, 0, 4));
  controller.receiveCommand({ source: null, kind: COMMAND.HOLD, time: 0, target: null });
  assert.ok(agent.interrupts.tick());
  assert.equal(agent.interrupts.last, PRIORITY.DANGER);
  // Saved pair: the new goal, then a copy of the current one.
  assert.deepEqual(kinds(controller.commands), [GOAL.COMBAT, GOAL.OBJECTIVE]);
  const [goal, saved] = controller.commands.queue;
  assert.equal(goal.mode, COMBAT_MODE.EVADE);
  assert.deepEqual(goal.point.toArray(), [3, 0, 4]);
  assert.equal(saved.post.name, "cp1");
  assert.ok(controller.commands.latch);
});

test("interrupts: an agent that does not save pushes the goal alone; the latch blocks a second push", () => {
  const { controller, agent } = agentFixture({ saves: false });
  controller.notifyDanger(new THREE.Vector3(1, 0, 0));
  assert.ok(agent.interrupts.tick());
  assert.deepEqual(kinds(controller.commands), [GOAL.COMBAT]);
  controller.notifyDanger(new THREE.Vector3(2, 0, 0));
  assert.ok(agent.interrupts.tick(), "the check still succeeds");
  assert.deepEqual(kinds(controller.commands), [GOAL.COMBAT], "but the latch refuses the insertion");
});

test("interrupts: danger needs a soldier owner and the agent's acceptance", () => {
  const { controller, agent, unit } = agentFixture();
  controller.notifyDanger(new THREE.Vector3(1, 0, 0));
  agent.acceptsHazard = () => false;
  assert.equal(agent.interrupts.tick(), false);
  agent.acceptsHazard = () => true;
  unit.kind = 3;
  controller.notifyDanger(new THREE.Vector3(1, 0, 0));
  assert.equal(agent.interrupts.danger({ goal: null }), false);
});

test("interrupts: a shot alert in range responds at the source's current position; out of range a soldier evades with generator B", () => {
  const { world, ai, controller, agent } = agentFixture();
  const shooterCharacter = makeCharacter(9, 2);
  const shooter = makeUnit(world.units, 2, 10, 0);
  shooterCharacter.unit = shooter;
  controller.notifyShot(shooterCharacter, true, new THREE.Vector3(99, 0, 99));
  ai.combat = { sensorRange: () => 50, canEngage: () => true };
  let out = { goal: null };
  assert.ok(agent.interrupts.shot(out));
  assert.equal(out.goal.mode, COMBAT_MODE.LOCATION);
  assert.deepEqual(out.goal.point.toArray(), [10, 0, 0], "current position, not the stored alert point");
  ai.combat = { sensorRange: () => 5, canEngage: () => true };
  const b = new ClassicRandom(world.randomB.seed);
  const z = Math.fround(b.uniform() - 0.5) * 2;
  const y = Math.fround(b.uniform() - 0.5) * 2;
  const x = Math.fround(b.uniform() - 0.5) * 2;
  out = { goal: null };
  assert.ok(agent.interrupts.shot(out));
  assert.equal(out.goal.mode, COMBAT_MODE.EVADE);
  assert.deepEqual(out.goal.point.toArray(), [x, y, z]);
});

test("interrupts: a droideka takes a scanner target only within 10 m", () => {
  const { world, ai, agent, unit } = agentFixture();
  const target = makeUnit(world.units, 2, 10, 0);
  ai.combat = { acquiredTarget: () => target, canEngage: () => true };
  unit.kind = 3;
  assert.ok(agent.interrupts.target({ goal: null }), "exactly 10 m passes");
  target.position.x = 10.01;
  assert.equal(agent.interrupts.target({ goal: null }), false);
  unit.kind = 0;
  const out = { goal: null };
  assert.ok(agent.interrupts.target(out));
  assert.equal(out.goal.mode, COMBAT_MODE.TARGET);
  assert.equal(out.goal.refA.unit, target);
});

test("interrupts: acoustic contacts within the horizontal hearing range (half for a droideka) and the heard-enemy VO", () => {
  const { world, ai, controller, agent, unit } = agentFixture();
  const source = makeUnit(world.units, 2, 30, 0);
  source.position.y = 50; // horizontal distance only
  ai.combat = { acousticRange: () => 40, canEngage: () => true };
  const seen = [];
  world.events.listen((r) => seen.push(r));
  controller.addAcoustic({ unit: source, generation: source.generation });
  const out = { goal: null };
  assert.ok(agent.interrupts.acoustic(out));
  assert.equal(out.goal.mode, COMBAT_MODE.LOCATION);
  assert.ok(seen.some((r) => r.kind === "vo" && r.event === "heardEnemy"));
  unit.kind = 3;
  assert.equal(agent.interrupts.acoustic({ goal: null }), false, "20 m half range");
});

test("interrupts: recovery needs a stuck executor, a soldier or droideka, and an accepting agent", () => {
  const { controller, agent, unit } = agentFixture();
  const helper = agent.helper;
  helper.status = 0;
  assert.equal(agent.interrupts.recovery({ goal: null }), false);
  helper.status = 1;
  const out = { goal: null };
  assert.ok(agent.interrupts.recovery(out));
  assert.equal(out.goal.kind, GOAL.RECOVERY);
  unit.kind = 6;
  assert.equal(agent.interrupts.recovery({ goal: null }), false);
  unit.kind = 0;
  const random = new UnitRandomAgent(controller, SourceGoal.of(GOAL.RECOVERY));
  random.helper.status = 2;
  assert.equal(random.interrupts.recovery({ goal: null }), false, "the random agent refuses recovery");
});

// ---------------------------------------------------------------------------
// Squads.

function squadFixture() {
  const world = makeWorld();
  const ai = makeAI(world);
  const leader = makeCharacter(0, 1, true);
  const leaderUnit = makeUnit(world.units, 1, 0, 0, { human: true });
  leader.unit = leaderUnit;
  leaderUnit.character = leader;
  world.battle.characters.push(leader);
  return { world, ai, leader, leaderUnit };
}

test("follower counts return followers plus one", () => {
  const { world, ai, leaderUnit } = squadFixture();
  const a = spawnBot(ai, world, 1, 2, 0);
  const b = spawnBot(ai, world, 1, 4, 0);
  assert.equal(countFollowers(ai, leaderUnit), 1);
  a.controller.leader = { unit: leaderUnit, generation: leaderUnit.generation };
  b.controller.leader = { unit: leaderUnit, generation: leaderUnit.generation };
  assert.equal(countFollowers(ai, leaderUnit), 3);
  leaderUnit.generation += 1; // stale references stop counting
  assert.equal(countFollowers(ai, leaderUnit), 1);
});

test("follow acceptance: recipient under 2, leader under 3; the yes comes first either way", () => {
  const { world, ai, leader, leaderUnit } = squadFixture();
  const leaderRef = { unit: leaderUnit, generation: leaderUnit.generation };
  const followers = [spawnBot(ai, world, 1, 2, 0), spawnBot(ai, world, 1, 3, 0)];
  const recipient = spawnBot(ai, world, 1, 5, 0);
  const agent = new OpenAgent(recipient.controller, SourceGoal.objective(null));
  recipient.controller.agent = agent;
  const vo = [];
  world.events.listen((r) => r.kind === "vo" && vo.push(r));
  const follow = () => {
    recipient.controller.receiveCommand(new SquadCommand(leader, COMMAND.FOLLOW, 0, leaderRef));
    const out = { goal: null };
    return [agent.interrupts.command(out), out.goal];
  };
  followers[0].controller.leader = leaderRef;
  let [ok, goal] = follow();
  assert.ok(ok, "one existing follower: accepted");
  assert.equal(goal.kind, GOAL.FOLLOW);
  assert.equal(goal.refA.unit, leaderUnit);
  followers[1].controller.leader = leaderRef;
  [ok] = follow();
  assert.equal(ok, false, "two existing followers: refused");
  assert.equal(vo.filter((r) => r.event === "tac_yes").length, 2, "yes was said both times");
  followers[1].controller.leader = null;
  // A recipient with a follower of its own refuses.
  followers[1].controller.leader = { unit: recipient.unit, generation: recipient.unit.generation };
  [ok] = follow();
  assert.equal(ok, false);
});

test("hold, board, unboard and move-out commands", () => {
  const { world, ai, leader, leaderUnit } = squadFixture();
  const bot = spawnBot(ai, world, 1, 2, 0);
  const controller = bot.controller;
  const agent = new OpenAgent(controller, SourceGoal.of(GOAL.HOLD));
  controller.agent = agent;
  const run = (kind, target = null) => {
    controller.receiveCommand(new SquadCommand(leader, kind, 0, target));
    const out = { goal: null };
    return [agent.interrupts.command(out), out.goal];
  };
  let [ok, goal] = run(COMMAND.HOLD);
  assert.ok(ok);
  assert.equal(goal.kind, GOAL.HOLD);
  world.time = 7;
  [ok, goal] = run(COMMAND.UNBOARD);
  assert.ok(ok);
  assert.equal(goal.kind, GOAL.UNBOARD);
  assert.equal(controller.reboardAt, 27);
  // Move out from the same issuer ends a hold.
  controller.commands.current = SourceGoal.of(GOAL.HOLD);
  controller.lastCommand = { source: leader, kind: COMMAND.HOLD };
  [ok] = run(COMMAND.MOVE_OUT);
  assert.equal(ok, false, "move out never pushes a goal");
  assert.equal(agent.pending, DONE);
  // Queued follow/hold goals are removed by any command.
  controller.commands.append(SourceGoal.of(GOAL.FOLLOW));
  controller.commands.append(SourceGoal.of(GOAL.OBJECTIVE));
  run(COMMAND.AT_EASE);
  assert.deepEqual(kinds(controller.commands), [GOAL.OBJECTIVE]);
  void leaderUnit;
});

test("command VO mapping by context, with a separate bank for the local player", () => {
  assert.deepEqual(voEntry(COMMAND.MOVE_OUT, 0), { offset: 0x00, property: "AISCFieldMoveOutSound", suffix: "tac_moveout" });
  assert.deepEqual(voEntry(COMMAND.AT_EASE, 0), { offset: 0x00, property: "AISCFieldMoveOutSound", suffix: "tac_moveout" });
  assert.equal(voEntry(COMMAND.FOLLOW, 0).suffix, "tac_follow");
  assert.equal(voEntry(COMMAND.HOLD, 0, true).offset, 0x08 + 0x68);
  assert.equal(voEntry(COMMAND.HOLD, 0, true).property, "SCFieldHoldSound");
  assert.equal(voEntry(COMMAND.BOARD, 1).suffix, "tac_needgunner");
  assert.equal(voEntry(COMMAND.UNBOARD, 1).suffix, "tac_moveout");
  assert.equal(voEntry(COMMAND.MOVE_OUT, 1), null);
  assert.equal(voEntry(COMMAND.MOVE_OUT, 2).suffix, "tac_spreadout");
  assert.equal(voEntry(COMMAND.HOLD, 2).suffix, "tac_stop");
  assert.equal(voEntry(COMMAND.MOVE_OUT, 3).suffix, "tac_allclear");
  assert.equal(voEntry(COMMAND.HOLD, 3).property, "AISCGunnerSteadySound");
  assert.equal(voEntry(COMMAND.FOLLOW, 3), null);
});

test("delivery: humans' orders reach nearby friendly AI (45 m move out, 30 m otherwise); AI orders 0..3 are chatter only", () => {
  const { world, ai, leader } = squadFixture();
  const near = spawnBot(ai, world, 1, 30, 0);
  const mid = spawnBot(ai, world, 1, 44, 0);
  const enemy = spawnBot(ai, world, 2, 5, 0);
  let got = distributeCommand(ai, new SquadCommand(leader, COMMAND.MOVE_OUT, 0));
  assert.deepEqual(got, [near.controller, mid.controller]);
  got = distributeCommand(ai, new SquadCommand(leader, COMMAND.HOLD, 0));
  assert.deepEqual(got, [near.controller], "30 m inclusive for hold");
  void enemy;
  const vo = [];
  world.events.listen((r) => r.kind === "vo" && vo.push(r));
  got = distributeCommand(ai, new SquadCommand(near.character, COMMAND.FOLLOW, 0, null));
  assert.deepEqual(got, [], "an AI's follow is not delivered");
  assert.equal(vo.at(-1).event, "tac_follow", "but it is voiced");
});

test("sendCommand draws generator B once and sends at or under the probability", () => {
  const { world, ai, leader } = squadFixture();
  spawnBot(ai, world, 1, 5, 0);
  const reference = new ClassicRandom(world.randomB.seed);
  const u = reference.uniform();
  const sent = sendCommand(ai, new SquadCommand(leader, COMMAND.HOLD, 0), u);
  assert.equal(sent, true);
  assert.equal(world.randomB.seed, reference.seed);
  const u2 = reference.uniform();
  assert.equal(sendCommand(ai, new SquadCommand(leader, COMMAND.HOLD, 0), u2 - 1e-6), false);
});

// ---------------------------------------------------------------------------
// The scheduler.

test("candidate budgets: floor(N/C) plus the remainder at the end of the cycle; the final step drains", () => {
  assert.deepEqual([1, 2, 3].map((t) => candidateBudget(14, 3, t)), [4, 5, 14]);
  assert.deepEqual([1, 2, 3].map((t) => candidateBudget(2, 3, t)), [0, 1, 2]);
  const fixture = nativeFixture("scheduler-fixtures.json");
  if (!fixture) return;
  for (const { cycle, count, budgets } of fixture.native_candidate_budgets) {
    assert.deepEqual(budgets.map((_, i) => candidateBudget(count, cycle, i + 1)), budgets, `C=${cycle} N=${count}`);
  }
});

function stubController(id) {
  return {
    id,
    processed: false,
    lodSkipped: false,
    live: true,
    unit: { id, position: new THREE.Vector3(), velocity: new THREE.Vector3(20, 0, 0) },
    updates: [],
    update(dt, phase) {
      this.updates.push(phase);
    },
  };
}

test("scheduler services each controller once per cycle in budgeted slices, skipping processed ones", () => {
  for (const [hard, N] of [[false, 22], [true, 7], [false, 3]]) {
    const world = makeWorld({ hard });
    const ai = makeAI(world);
    const controllers = Array.from({ length: N }, (_, i) => stubController(i + 1));
    ai.controllers.push(...controllers);
    const C = hard ? 3 : 15;
    const perStep = [];
    for (let t = 1; t <= C; t++) {
      ai.scheduler.update(0.016);
      perStep.push(ai.scheduler.stats.serviced);
      assert.equal(ai.scheduler.counter, t);
    }
    const expected = Array.from({ length: C }, (_, i) => {
      const t = i + 1;
      return t === C ? N - Array.from({ length: C - 1 }, (_, j) => candidateBudget(N, C, j + 1)).reduce((a, b) => a + b, 0) : candidateBudget(N, C, t);
    });
    assert.deepEqual(perStep, expected);
    for (const c of controllers) assert.deepEqual(c.updates, [0, 1], "phase 0 then phase 1, once");
    ai.scheduler.update(0.016); // a new cycle clears the processed flags
    assert.equal(ai.scheduler.counter, 1);
  }
  // A controller already processed is skipped without spending budget.
  const world = makeWorld({ hard: true });
  const ai = makeAI(world);
  const controllers = Array.from({ length: 6 }, (_, i) => stubController(i + 1));
  ai.controllers.push(...controllers);
  ai.scheduler.update(0.016); // t=1, budget 2: controllers 1, 2
  controllers[2].processed = true;
  ai.scheduler.update(0.016); // t=2, budget 2: 4, 5 (3 skipped)
  assert.deepEqual(controllers.map((c) => c.updates.length / 2), [1, 1, 0, 1, 1, 0]);
});

test("phase wrappers: an invalid owner gets no update, but phase 1 still clears the candidate", () => {
  const world = makeWorld();
  const events = [];
  const ai = makeAI(world);
  ai.combat = { clearCandidate: () => events.push("clear") };
  const fixture = nativeFixture("phases.json") ?? [
    { valid: 0, phase: 0, events: [] },
    { valid: 0, phase: 1, events: [["clear"]] },
    { valid: 1, phase: 0, events: [["update"]] },
    { valid: 1, phase: 1, events: [["update"], ["clear"]] },
  ];
  for (const valid of [0, 1]) {
    events.length = 0;
    const c = stubController(1);
    c.live = Boolean(valid);
    c.update = (dt, phase) => events.push(`update${phase}`);
    ai.scheduler.service(c, 0.016);
    const want = [];
    for (const row of fixture.filter((r) => r.valid === valid)) {
      for (const [kind] of row.events) want.push(kind === "update" ? `update${row.phase}` : "clear");
    }
    assert.deepEqual(events, want);
  }
});

test("LOD gate truth table: skip only when not skipped last time, identity parity matches and unseen", () => {
  const world = makeWorld();
  const ai = makeAI(world, { identity: (unit) => unit.id });
  const fixture = nativeFixture("scheduler-fixtures.json")?.native_lod_policy_with_injected_predicates;
  const rows = fixture ?? [];
  if (!rows.length) {
    for (const prior of [0, 1]) for (const id of [0, 1, 2, 3]) for (const parity of [0, 1]) for (const visible of [0, 1]) {
      const got = Number(!prior && ((~id) & 1) === parity && !visible);
      rows.push([prior, id, parity, visible, got, Number(!prior && ((~id) & 1) === parity)]);
    }
  }
  for (const [prior, id, parity, visible, got, consulted] of rows) {
    const c = stubController(id);
    c.unit.id = id;
    c.lodSkipped = Boolean(prior);
    ai.scheduler.parity = parity;
    let calls = 0;
    ai.scheduler.visibleOrNear = () => {
      calls += 1;
      return Boolean(visible);
    };
    assert.equal(Number(ai.scheduler.lodSkip(c)), got, JSON.stringify([prior, id, parity, visible]));
    assert.equal(Number(c.lodSkipped), got);
    assert.equal(calls, consulted);
  }
});

test("a skipped controller is serviced on its next consideration (no consecutive skips)", () => {
  const world = makeWorld({ hard: true });
  const ai = makeAI(world, { identity: () => 1 });
  ai.scheduler.visibleOrNear = () => false;
  // Parity always matching and never visible: skip, run, skip, run...
  const c = stubController(1);
  const outcomes = [];
  for (let i = 0; i < 6; i++) {
    ai.scheduler.parity = 0; // (~1) & 1
    outcomes.push(ai.scheduler.lodSkip(c) ? "skip" : "run");
  }
  assert.deepEqual(outcomes, ["skip", "run", "skip", "run", "skip", "run"]);
  // Through the scheduler: one controller, Hard (budgets 0, 0, 1): the
  // parity bit flips every cycle, so services and skips alternate per cycle.
  const d = stubController(2);
  d.unit.velocity.set(0, 0, 0);
  ai.controllers.push(d);
  const perCycle = [];
  for (let cycle = 0; cycle < 6; cycle++) {
    const before = d.updates.length;
    for (let t = 0; t < 3; t++) ai.scheduler.update(0.016);
    perCycle.push(d.updates.length > before ? "run" : "skip");
  }
  for (let i = 1; i < perCycle.length; i++) assert.ok(!(perCycle[i] === "skip" && perCycle[i - 1] === "skip"), perCycle.join(","));
  assert.ok(perCycle.includes("skip") && perCycle.includes("run"));
});

test("the visibility test: fast or near the first camera, or inside a frustum within min(far, 8 zoom + 70)", () => {
  const camera = new THREE.PerspectiveCamera(60, 1.5, 0.5, 1000);
  camera.position.set(0, 0, 0);
  camera.lookAt(0, 0, -1);
  camera.updateMatrixWorld(true);
  assert.ok(cameraAccepts(camera, new THREE.Vector3(0, 0, -50)));
  assert.ok(cameraAccepts(camera, new THREE.Vector3(0, 0, -78)), "depth limit 78 at zoom 1");
  assert.equal(cameraAccepts(camera, new THREE.Vector3(0, 0, -78.5)), false);
  assert.equal(cameraAccepts(camera, new THREE.Vector3(0, 0, 50)), false, "behind");
  assert.equal(cameraAccepts(camera, new THREE.Vector3(0, 0, -0.2)), false, "nearer than near");
  const tanV = Math.tan(Math.PI / 6);
  assert.ok(cameraAccepts(camera, new THREE.Vector3(0, tanV * 40 - 0.01, -40)));
  assert.equal(cameraAccepts(camera, new THREE.Vector3(0, tanV * 40 + 0.01, -40)), false);
  assert.ok(cameraAccepts(camera, new THREE.Vector3(tanV * 1.5 * 40 - 0.01, 0, -40)));
  camera.zoom = 4;
  assert.ok(cameraAccepts(camera, new THREE.Vector3(0, 0, -100)), "zoom lengthens the depth limit");
  assert.equal(cameraAccepts(camera, new THREE.Vector3(0, tanV * 40 / 4 + 0.01, -40)), false, "and narrows the slopes");

  const world = makeWorld();
  world.camera = new THREE.PerspectiveCamera(60, 1.5, 0.5, 1000);
  world.camera.lookAt(0, 0, -1);
  world.camera.updateMatrixWorld(true);
  const ai = makeAI(world);
  const unit = { position: new THREE.Vector3(0, 0, 200), velocity: new THREE.Vector3() };
  assert.equal(ai.scheduler.visibleOrNear(unit), false);
  unit.velocity.set(10, 0, 0);
  assert.ok(ai.scheduler.visibleOrNear(unit), "speed squared 100");
  unit.velocity.set(0, 0, 0);
  unit.position.set(0, 0, 45);
  assert.ok(ai.scheduler.visibleOrNear(unit), "45 m from camera 0");
});

test("AI input step: a skipped step carries its dt into the next, which always runs", () => {
  const world = makeWorld();
  const ai = makeAI(world, { identity: () => 0 });
  const { controller } = spawnBot(ai, world, 1, 0, 0);
  ai.scheduler.visibleOrNear = () => false;
  const steps = [];
  ai.combat = { consumeFire: (c, u, dt) => steps.push(dt), markSkip: () => {} };
  ai.scheduler.inputParity = 0; // toggled to 1 = (~0)&1: the first step skips
  ai.scheduler.executeInputs(0.01);
  assert.deepEqual(steps, []);
  assert.ok(controller.inputSkip);
  ai.scheduler.executeInputs(0.02);
  assert.equal(steps.length, 1);
  assert.ok(Math.abs(steps[0] - 0.03) < 1e-12);
  assert.equal(controller.inputSkip, false);
});

test("the input step writes posture toggles from the stance request", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const { controller, unit } = spawnBot(ai, world, 1, 0, 0);
  controller.setStance(1);
  ai.scheduler.executeInput(controller, 0.016);
  assert.equal(unit.intent.crouch, true);
  unit.intent.crouch = false;
  unit.soldier.posture = "crouch";
  controller.setStance(0);
  ai.scheduler.executeInput(controller, 0.016);
  assert.equal(unit.intent.crouch, true, "a second toggle stands it up");
  unit.intent.crouch = false;
  unit.soldier.posture = "prone";
  ai.scheduler.executeInput(controller, 0.016);
  assert.equal(unit.intent.prone, true);
});

// ---------------------------------------------------------------------------
// Patrol.

function patrolFixture({ hints = [], unitOptions = {} } = {}) {
  const world = makeWorld({ seed: 1234 });
  world.nav.hints = hints;
  const ai = makeAI(world);
  const post = makePost("cp1", 1, 0, 0);
  const bot = spawnBot(ai, world, 1, 2, 2, unitOptions);
  bot.character.objective = post;
  const agent = new UnitTrooperAgent(bot.controller, SourceGoal.objective(post));
  bot.controller.agent = agent;
  return { world, ai, post, agent, patrol: agent.patrol, ...bot };
}

function hint(name, post, x, z, extra = {}) {
  return { name, bit: 2, position: new THREE.Vector3(x, 0, z), forward: new THREE.Vector3(1, 0, 0), post, ...extra };
}

test("patrol at a friendly post: draw 1..4; a soldier drawing 2..4 picks a hint by random permutation", () => {
  const post = makePost("cp1", 1, 0, 0);
  const other = makePost("cp2", 1, 100, 0);
  const hints = [hint("h0", post, 5, 0), hint("h1", other, 6, 0), hint("h2", post, 7, 0), hint("h3", post, 400, 0)];
  const f = patrolFixture({ hints });
  f.patrol.post = post;
  const reference = new ClassicRandom(f.world.random.seed);
  const draw = reference.int(1, 4);
  const candidates = queryPatrolHints(f.ai, post, f.controller);
  assert.deepEqual(candidates.map((h) => h.name), ["h0", "h2"], "associated with the post, inside 300");
  f.patrol.start(post);
  if (draw === 1) {
    assert.equal(f.patrol.state, 4);
    return;
  }
  // The permutation's first draw picks among two.
  const i = reference.int(0, 1);
  const expected = ["h0", "h2"][i];
  assert.equal(f.patrol.hint.name, expected);
  assert.equal(f.patrol.state, 3);
  const expires = reference.int(30, 45);
  assert.equal(f.patrol.expires, expires);
  assert.equal(f.helper ?? f.agent.helper.calls.at(-1)[0], "goto");
  assert.equal(f.world.random.seed, reference.seed, "no other draws");
});

test("hint choice skips the previous hint and hints marked in the last 4 s", () => {
  const world = makeWorld({ seed: 5 });
  const ai = makeAI(world);
  const post = makePost("cp1", 1, 0, 0);
  const a = hint("a", post, 1, 0);
  const b = hint("b", post, 2, 0);
  world.time = 10;
  a.reservedAt = 6.5; // 10 < 6.5 + 4: unavailable
  assert.equal(chooseHint(ai, [a, b], null, world.time), b);
  assert.equal(chooseHint(ai, [a, b], b, world.time), null);
  a.reservedAt = 6; // equality passes
  const got = chooseHint(ai, [a, b], b, world.time);
  assert.equal(got, a);
});

test("hint stance: integer draws 0..2 until an allowed one", () => {
  const random = new ClassicRandom(9);
  const reference = new ClassicRandom(9);
  let expected;
  do expected = reference.int(0, 2);
  while (!(0b100 & (1 << expected)));
  assert.equal(pickHintStance({ stanceMask: 0b100 }, random), 2);
  assert.equal(expected, 2);
  assert.equal(random.seed, reference.seed);
  assert.equal(pickHintStance({ stanceMask: 0 }, new ClassicRandom(1)), 0, "an empty mask stands without looping");
});

test("patrol roaming: 20..30 s and a flat cube point scaled 8..14 around the post (generator A)", () => {
  const f = patrolFixture();
  f.patrol.post = f.post;
  f.patrol.state = 1;
  const reference = new ClassicRandom(f.world.random.seed);
  f.patrol.requestState(4);
  f.patrol.flush();
  const expires = reference.int(20, 30);
  const z = Math.fround(reference.uniform() - 0.5) * 2;
  void Math.fround(reference.uniform() - 0.5);
  const x = Math.fround(reference.uniform() - 0.5) * 2;
  const r = reference.range(8, 14);
  assert.equal(f.patrol.expires, expires);
  const [kind, point, mode] = f.agent.helper.calls.at(-1);
  assert.equal(kind, "goto");
  assert.equal(mode, "direct");
  assert.ok(Math.abs(point.x - x * r) < 1e-5 && point.y === 0 && Math.abs(point.z - z * r) < 1e-5);
});

test("patrol timers: hint and roam states end strictly after their expiry; arrival settles", () => {
  const post = makePost("cp1", 1, 0, 0);
  const f = patrolFixture({ hints: [hint("h0", post, 5, 0, { stanceMask: 1 })] });
  f.patrol.post = post;
  f.patrol.hint = f.world.nav.hints[0];
  f.patrol.startTime = 1e8;
  f.patrol.state = 3;
  f.patrol.pending = 0;
  f.patrol.expires = 40;
  f.world.time = 40;
  f.patrol.tick(0.1);
  assert.equal(f.patrol.state, 3, "equality keeps the state");
  assert.equal(f.world.nav.hints[0].reservedAt, 40, "marked while approaching");
  f.agent.helper.arrived = true;
  f.patrol.tick(0.1);
  assert.equal(f.patrol.state, 5);
  assert.ok(f.patrol.look);
  assert.deepEqual(f.patrol.lookPoint.toArray(), [10, 0, 0], "10 along the hint's forward from the destination");
  f.agent.helper.arrived = false;
  f.world.time = 40.0001;
  f.patrol.expires = 40;
  const before = f.world.random.seed;
  f.patrol.tick(0.1);
  assert.ok(f.patrol.state !== 5, "expired: chooses again");
  assert.notEqual(f.world.random.seed, before);
});

test("patrol revalidates once, after 5 s: a post its team can spawn at sends for a new objective", () => {
  const f = patrolFixture();
  const requests = [];
  f.ai.population = { requestObjective: (character, exclude, previous) => requests.push([character, exclude, previous]), friendlySpawnEligible: () => true };
  f.patrol.post = f.post;
  f.patrol.state = 6;
  f.patrol.pending = 0;
  f.patrol.expires = 1e9;
  f.patrol.startTime = 0;
  f.world.time = 5;
  f.patrol.tick(0.1);
  assert.equal(requests.length, 0, "not at exactly 5 s");
  f.world.time = 5.01;
  f.patrol.tick(0.1);
  assert.equal(requests.length, 1);
  assert.equal(requests[0][1], true);
  assert.equal(f.patrol.state, DONE);
  assert.equal(f.patrol.startTime, 1e8);
});

test("command-post offset draws z then x across the capture region (generator A)", () => {
  const world = makeWorld({ seed: 31 });
  const ai = makeAI(world);
  const post = makePost("cp", 1, 0, 0, { size: [6, 2, 2] });
  const reference = new ClassicRandom(31);
  const z = reference.range(-2, 2);
  const x = reference.range(-6, 6);
  const offset = cpOffset(ai, post);
  assert.deepEqual(offset.toArray(), [x, 0, z]);
});

test("objective arrival: inside the capture region, else within half the nominal speed", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const post = makePost("cp", 1, 0, 0, { size: [2, 2, 2] });
  const unit = makeUnit(world.units, 1, 1, 1, { speed: 6 });
  assert.ok(arrivedAtObjective(ai, unit, post));
  unit.position.set(2.9, 0, 0);
  assert.ok(arrivedAtObjective(ai, unit, post), "2.9 < 3");
  unit.position.set(3, 0, 0);
  assert.equal(arrivedAtObjective(ai, unit, post), false, "strict");
});

// ---------------------------------------------------------------------------
// Agents.

// Run an agent the way its controller does: phase 0 ticks behavior only
// (alternate mode), phase 1 applies requested states (normal mode).
function phase(agent, p, dt = 0.016) {
  agent.controller.phase = p;
  return agent.tick(dt, p);
}

test("the trooper walks to an unreached objective (with its chatter roll) and patrols when it arrives", () => {
  const world = makeWorld({ seed: 3, seedB: 4 });
  const ai = makeAI(world);
  const post = makePost("cp1", 2, 60, 0);
  const { controller, character } = spawnBot(ai, world, 1, 0, 0);
  character.objective = post;
  const b = new ClassicRandom(4);
  b.uniform(); // the 30% "follow me" roll
  controller.publishObjective(post);
  controller.update(0.016, 1);
  const agent = controller.agent;
  assert.ok(agent instanceof UnitTrooperAgent);
  assert.equal(agent.state, 3);
  assert.equal(world.randomB.seed, b.seed, "one generator-B draw");
  const [kind, point] = agent.helper.calls.find((c) => c[0] === "goto");
  assert.equal(kind, "goto");
  assert.ok(Math.abs(point.x - 60) <= 4 && Math.abs(point.z) <= 4);
  // Arrival: patrol, and a move-out (always sent; chatter only from an AI).
  controller.unit.position.copy(post.position);
  const vo = [];
  world.events.listen((r) => r.kind === "vo" && vo.push(r));
  controller.update(0.016, 0);
  assert.equal(agent.pending, 2);
  assert.equal(vo.at(-1)?.event, "tac_moveout");
  controller.update(0.016, 1);
  assert.equal(agent.state, 2);
});

test("trooper state 3 charges: three allied AI soldiers fighting within 7 m mute alerts and shout ChargeSound", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const post = makePost("cp1", 2, 60, 0);
  const bot = spawnBot(ai, world, 1, 0, 0);
  bot.character.objective = post;
  bot.controller.publishObjective(post);
  bot.controller.update(0.016, 1);
  const agent = bot.controller.agent;
  for (let i = 0; i < 3; i++) {
    const friend = spawnBot(ai, world, 1, 2 + i, 0);
    friend.controller.state = STATE.COMBAT;
  }
  const vo = [];
  world.events.listen((r) => r.kind === "vo" && vo.push(r));
  world.time = 1;
  assert.equal(agent.acceptsHazard(), false);
  assert.equal(vo.filter((r) => r.event === "charge").length, 1);
  assert.equal(agent.acceptsAlert(false, null), false, "cached for the same time");
  assert.equal(vo.filter((r) => r.event === "charge").length, 1);
});

test("random recovery: done beyond 10 m (2 m droideka) and clear; retries after more than 3 s", () => {
  const world = makeWorld({ seedB: 8 });
  const ai = makeAI(world);
  const bot = spawnBot(ai, world, 1, 0, 0);
  const agent = new UnitRandomAgent(bot.controller, SourceGoal.of(GOAL.RECOVERY));
  bot.controller.agent = agent;
  bot.controller.navControl = { executor: {}, status: 0, avoidSide: 0 };
  agent.update(0.016, false); // entry: first random point (3 draws of B)
  assert.equal(agent.state, 2);
  const draws = agent.helper.calls.filter((c) => c[0] === "goto");
  assert.equal(draws.length, 1);
  assert.equal(draws[0][2], "straight");
  bot.unit.position.set(10, 0, 0);
  agent.update(0.016, true);
  assert.equal(agent.pending, 0, "exactly 10 m is not enough");
  bot.unit.position.set(10.01, 0, 0);
  agent.update(0.016, true);
  assert.equal(agent.pending, DONE);
  // Droideka threshold and the 3 s retry.
  const d = spawnBot(ai, world, 1, 50, 0, { kind: 3 });
  d.controller.navControl = { executor: {}, status: 0, avoidSide: 0 };
  const droid = new UnitRandomAgent(d.controller, SourceGoal.of(GOAL.RECOVERY));
  world.time = 0;
  droid.update(0.016, false);
  d.unit.position.set(51.5, 0, 0);
  world.time = 3;
  droid.update(0.016, true);
  assert.equal(droid.pending, 0);
  assert.equal(droid.helper.calls.filter((c) => c[0] === "goto").length, 1, "no retry at exactly 3 s");
  world.time = 3.01;
  droid.update(0.016, true);
  assert.equal(droid.helper.calls.filter((c) => c[0] === "goto").length, 2, "retry after 3 s");
  assert.equal(d.controller.wantedMode, 1, "a recovering droideka deploys");
  d.unit.position.set(52.01, 0, 0);
  droid.update(0.016, true);
  assert.equal(droid.pending, DONE);
});

test("follow: stop under 5, resume over 9, and the 0.05 countdown when farther than 20", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const leaderUnit = makeUnit(world.units, 1, 0, 0);
  const bot = spawnBot(ai, world, 1, 30, 0);
  const goal = SourceGoal.follow({ unit: leaderUnit, generation: leaderUnit.generation }, null);
  const agent = new UnitFollowAgent(bot.controller, goal);
  bot.controller.agent = agent;
  bot.controller.navControl = { executor: {}, status: 0, avoidSide: 0 };
  phase(agent, 1);
  assert.equal(agent.state, 2);
  assert.equal(bot.controller.leader.unit, leaderUnit);
  assert.equal(countFollowers(ai, leaderUnit), 2);
  for (let i = 0; i < 19; i++) {
    phase(agent, 0);
    phase(agent, 1);
  }
  assert.ok(Math.abs(agent.countdown - 0.05) < 1e-6, String(agent.countdown));
  assert.equal(agent.state, 2);
  phase(agent, 0);
  assert.equal(agent.pending, DONE, "twenty far callbacks end it (phase 0 only)");
  phase(agent, 1);
  assert.equal(agent.state, DONE);
  assert.equal(bot.controller.leader, null, "the terminal state drops the leader");
  // Hysteresis.
  const near = spawnBot(ai, world, 1, 0.1, 0);
  const f = new UnitFollowAgent(near.controller, goal);
  near.controller.navControl = { executor: {}, status: 0, avoidSide: 0 };
  phase(f, 1);
  phase(f, 0); // within 0.2 of the leader point: state 3
  phase(f, 1);
  assert.equal(f.state, 3);
  near.unit.position.set(5, 0, 0);
  phase(f, 0);
  assert.equal(f.pending, 0, "5 is not under 5");
  near.unit.position.set(4.99, 0, 0);
  phase(f, 0);
  assert.equal(f.pending, 4);
  phase(f, 1);
  near.unit.position.set(9, 0, 0);
  phase(f, 0);
  assert.equal(f.pending, 0, "9 is not over 9");
  near.unit.position.set(9.01, 0, 0);
  phase(f, 0);
  assert.equal(f.pending, 3);
});

test("hold keeps fighting stance and ends after 20 far callbacks from its issuer", () => {
  const world = makeWorld();
  const ai = makeAI(world);
  const issuer = makeCharacter(5, 1, true);
  issuer.unit = makeUnit(world.units, 1, 0, 0, { human: true });
  const bot = spawnBot(ai, world, 1, 25, 0);
  bot.controller.lastCommand = { source: issuer, kind: COMMAND.HOLD };
  const agent = new UnitHoldAgent(bot.controller, SourceGoal.of(GOAL.HOLD));
  phase(agent, 1);
  assert.equal(agent.state, 2);
  for (let i = 0; i < 19; i++) {
    phase(agent, 0);
    phase(agent, 1);
  }
  assert.equal(agent.pending, 0);
  assert.equal(agent.state, 2);
  phase(agent, 0);
  assert.equal(agent.pending, DONE);
  assert.equal(agent.savesOnInterrupt(SourceGoal.of(GOAL.COMBAT)), true);
  assert.equal(agent.savesOnInterrupt(SourceGoal.of(GOAL.FOLLOW)), false);
});

test("the table sine/cosine and the Y rotation the wait/hold look points use", () => {
  const { sin, cos } = tableSinCos(0);
  assert.equal(sin, 0);
  assert.equal(cos, 1);
  const q = tableSinCos(Math.PI / 2);
  assert.ok(Math.abs(q.sin - 1) < 1e-5 && Math.abs(q.cos) < 1e-5);
  const v = rotateY(new THREE.Vector3(), new THREE.Vector3(0, 0, 1), Math.PI / 2);
  assert.ok(Math.abs(v.x + 1) < 1e-5 && Math.abs(v.z) < 1e-5);
});

// ---------------------------------------------------------------------------
// The entry point.

test("createClassicAI attaches AI characters, ignores humans, and runs a step", () => {
  const world = makeWorld();
  const api = createClassicAI(world, { modules: {}, quiet: true, createHelper: (agent, controller) => new FakeHelper(controller) });
  const human = makeCharacter(0, 1, true);
  human.unit = makeUnit(world.units, 1, 0, 0, { human: true });
  assert.equal(api.attach(human), null);
  const post = makePost("cp1", 2, 40, 0);
  const bot = makeCharacter(1, 1);
  bot.objective = post;
  bot.unit = makeUnit(world.units, 1, 5, 0);
  world.battle.characters.push(human, bot);
  api.attach(bot);
  for (let i = 0; i < 20; i++) {
    world.time += 1 / 60;
    api.update(1 / 60);
  }
  const snapshot = api.debug.snapshot();
  assert.equal(snapshot.controllers.length, 1);
  assert.equal(snapshot.controllers[0].state, STATE.OBJECTIVE);
  assert.equal(api.controllers[0].agent.state, 3);
  assert.equal(typeof bot.unit.intent.forward, "number");
});

test("with the sibling modules that exist bound, steps run and bots head for their objective", async () => {
  const { loadClassicModules } = await import("../src/ai/classic/index.js");
  const modules = await loadClassicModules();
  const navModule = modules.nav;
  if (!navModule?.BarrierSet || !navModule?.PathPlanner) return; // Nav not landed
  const world = makeWorld({ seed: 21, seedB: 22 });
  const barriers = new navModule.BarrierSet([]);
  world.nav = { barriers, planner: new navModule.PathPlanner(barriers), graph: null, hints: [] };
  world.relation = (a, b) => world.units.relation(a, b);
  world.los = () => true;
  const api = createClassicAI(world, { modules, quiet: true });
  await api.ready;
  const post = makePost("cp1", 2, 30, 0);
  for (let i = 0; i < 3; i++) {
    const character = makeCharacter(i, 1);
    character.objective = post;
    character.unit = makeUnit(world.units, 1, i * 2, 0);
    character.unit.combat = { arsenal: { weapons: [], current: () => null } };
    character.unit.character = character;
    world.battle.characters.push(character);
    api.attach(character);
  }
  const h = 1 / 60;
  for (let s = 0; s < 240; s++) {
    world.time += h;
    api.update(h);
    for (const c of world.battle.characters) {
      const u = c.unit;
      const st = u.soldier;
      st.aimYaw -= u.intent.turn * st.maxTurn * h;
      const fx = Math.sin(st.aimYaw), fz = Math.cos(st.aimYaw);
      u.position.x += (fx * u.intent.forward * st.maxSpeed - Math.cos(st.aimYaw) * u.intent.right * st.maxStrafe) * h;
      u.position.z += (fz * u.intent.forward * st.maxSpeed + Math.sin(st.aimYaw) * u.intent.right * st.maxStrafe) * h;
    }
  }
  for (const c of world.battle.characters) {
    assert.ok(c.controller.agent instanceof UnitTrooperAgent);
    assert.ok(c.unit.position.distanceTo(post.position) < 30 - 5, `moved toward the post: ${c.unit.position.toArray()}`);
  }
});

// ---------------------------------------------------------------------------
// Regression: the objective agents delegate their alert policies to the
// embedded helper of the current state (patrol, snipe, mine); every helper
// must answer every policy, and a patrolling bot must survive every
// interrupt priority.

const POLICIES = ["acceptsHazard", "acceptsAlert", "acceptsTarget", "acceptsAcoustic", "acceptsBoard", "acceptsSecondaryWait"];
// crewWait: the trooper asks its helper (patrol: no); the scout and repair
// agents' crew-wait slot answers yes in every state.
const OBJECTIVE_AGENTS = [
  { name: "trooper", unitType: "trooper", Class: UnitTrooperAgent, patrol: 2, snipe: 4, mine: 5, approach: 3, crewWait: false },
  { name: "scout", unitType: "scout", Class: UnitScoutAgent, patrol: 2, snipe: 4, mine: 5, approach: 3, crewWait: true },
  { name: "repair", unitType: "pilot", Class: UnitRepairAgent, patrol: 4, snipe: 5, mine: 6, approach: 2, crewWait: true },
];

// A bot standing in its friendly post: its objective agent arrives at once
// and patrols.
function patrolling(spec, { seed = 41 } = {}) {
  const world = makeWorld({ seed, seedB: seed + 1 });
  const ai = makeAI(world);
  const post = makePost("cp1", 1, 0, 0);
  const bot = spawnBot(ai, world, 1, 0.5, 0.5, { unitType: spec.unitType });
  bot.character.objective = post;
  bot.controller.publishObjective(post);
  bot.controller.update(0.016, 1);
  const agent = bot.controller.agent;
  assert.ok(agent instanceof spec.Class, `${spec.name} agent`);
  assert.equal(agent.state, spec.patrol, `${spec.name} patrols`);
  const enemyCharacter = makeCharacter(9, 2);
  const enemy = makeUnit(world.units, 2, 12, 0);
  enemy.character = enemyCharacter;
  enemyCharacter.unit = enemy;
  return { world, ai, post, agent, enemy, enemyCharacter, ...bot };
}

test("every objective agent answers every alert policy in every state, with patrol's own answers", () => {
  for (const spec of OBJECTIVE_AGENTS) {
    const f = patrolling(spec);
    const { agent, enemy } = f;
    let engaged = 0;
    f.ai.combat = { canEngage: () => (engaged += 1) > 0 };
    const vehicle = makeUnit(f.world.units, 1, 3, 0, { speed: 10 });
    for (const state of [1, spec.approach, spec.patrol, spec.snipe, spec.mine]) {
      agent.state = state;
      for (const name of POLICIES) {
        const result = agent[name](enemy.position && name === "acceptsAlert" ? true : name === "acceptsBoard" ? vehicle : enemy, enemy.position);
        assert.equal(typeof result, "boolean", `${spec.name} state ${state} ${name}`);
      }
    }
    // Every embedded helper answers every policy itself (no base fallback).
    for (const helper of [agent.patrol, agent.snipe, agent.mine]) {
      for (const name of POLICIES) assert.equal(typeof helper[name], "function", `${spec.name}: ${helper.constructor.name}.${name}`);
    }
    assert.deepEqual([...f.ai.warned].filter((key) => key.startsWith("policy:")), [], "no fallback used");
    agent.state = spec.patrol;
    assert.equal(agent.acceptsHazard(), true);
    assert.equal(agent.acceptsAlert(true, enemy.position), true, "known alerts accepted while patrolling");
    assert.equal(agent.acceptsAlert(false, enemy.position), true, "and unknown ones");
    engaged = 0;
    assert.equal(agent.acceptsTarget(enemy), true);
    assert.equal(agent.acceptsAcoustic(enemy), true);
    assert.equal(engaged, 2, "target and acoustic use the base weapon-eligibility policy");
    f.ai.combat = { canEngage: () => false };
    assert.equal(agent.acceptsTarget(enemy), false);
    assert.equal(agent.acceptsAcoustic(enemy), false);
    assert.equal(agent.acceptsBoard(vehicle), false, "no boarding while patrolling");
    assert.equal(agent.acceptsSecondaryWait(), spec.crewWait, `${spec.name} crew wait while patrolling`);
  }
});

test("a helper missing a policy falls back to the agent's base policy instead of failing", () => {
  const f = patrolling(OBJECTIVE_AGENTS[0]);
  f.agent.patrol.acceptsAcoustic = undefined;
  f.ai.combat = { canEngage: () => true };
  assert.equal(f.agent.acceptsAcoustic(f.enemy), true);
  assert.ok(f.ai.warned.has("policy:PatrolHelper.acceptsAcoustic"), "and says so");
});

function resetQueue(controller) {
  const commands = controller.commands;
  commands.queue.length = 0;
  commands.latch = false;
  controller.danger.pending = false;
  controller.shot.pending = false;
  controller.shot.known = false;
  controller.shot.source = null;
  controller.acoustic.length = 0;
  controller.command.pending = false;
  controller.vehicleNotes.length = 0;
  controller.secondaryWaitUntil = 0;
  controller.navControl = { executor: {}, status: 0, avoidSide: 0 };
}

test("patrolling troopers, scouts and repair bots go through every interrupt priority", () => {
  for (const spec of OBJECTIVE_AGENTS) {
    const f = patrolling(spec);
    const { world, ai, agent, controller, enemy, enemyCharacter } = f;
    const services = { hasWeapon: () => true, canEngage: () => true, sensorRange: () => 100, acousticRange: () => 100, acquiredTarget: () => null };
    ai.combat = services;
    const human = makeCharacter(7, 1, true);
    human.unit = makeUnit(world.units, 1, 1, 0, { human: true });
    const vehicle = makeUnit(world.units, 1, 3, 0, { kind: 6, speed: 10 });
    ai.vehicles = { canEnter: () => true, freeSeats: () => 1, reserve: () => {} };
    const run = (label, setup, expectFired, check = null) => {
      resetQueue(controller);
      agent.state = spec.patrol;
      setup();
      let fired;
      assert.doesNotThrow(() => {
        fired = agent.interrupts.tick();
      }, `${spec.name}: ${label}`);
      assert.equal(fired, expectFired, `${spec.name}: ${label}`);
      if (fired) {
        assert.equal(agent.interrupts.last, check.priority, `${spec.name}: ${label} priority`);
        const goal = controller.commands.queue[0];
        assert.equal(goal.kind, check.kind, `${spec.name}: ${label} goal`);
        if (check.mode !== undefined) assert.equal(goal.mode, check.mode, `${spec.name}: ${label} mode`);
        // A patrolling objective agent saves its goal to resume.
        assert.equal(controller.commands.queue[1]?.kind, GOAL.OBJECTIVE, `${spec.name}: ${label} saved`);
      }
    };
    run("1 danger", () => controller.notifyDanger(new THREE.Vector3(2, 0, 2)), true, { priority: PRIORITY.DANGER, kind: GOAL.COMBAT, mode: COMBAT_MODE.EVADE });
    run("2 shot", () => controller.notifyShot(enemyCharacter, true, new THREE.Vector3(12, 0, 0)), true, { priority: PRIORITY.SHOT, kind: GOAL.COMBAT, mode: COMBAT_MODE.LOCATION });
    run("2 unknown shot", () => controller.notifyShot(enemyCharacter, false), true, { priority: PRIORITY.SHOT, kind: GOAL.COMBAT, mode: COMBAT_MODE.LOCATION });
    run("3 target", () => (services.acquiredTarget = () => enemy), true, { priority: PRIORITY.TARGET, kind: GOAL.COMBAT, mode: COMBAT_MODE.TARGET });
    services.acquiredTarget = () => null;
    run("4 acoustic", () => controller.addAcoustic({ unit: enemy, generation: enemy.generation }), true, { priority: PRIORITY.ACOUSTIC, kind: GOAL.COMBAT, mode: COMBAT_MODE.LOCATION });
    run("5 command", () => controller.receiveCommand(new SquadCommand(human, COMMAND.HOLD, 0)), true, { priority: PRIORITY.COMMAND, kind: GOAL.HOLD });
    run("6 boarding (patrol refuses)", () => controller.addVehicleNote({ unit: vehicle, generation: vehicle.generation }), false);
    run("7 crew wait", () => (controller.secondaryWaitUntil = world.time + 4), spec.crewWait, { priority: PRIORITY.WAIT_SECONDARY, kind: GOAL.WAIT_SECONDARY });
    run("8 recovery", () => (controller.navControl.status = 1), true, { priority: PRIORITY.RECOVERY, kind: GOAL.RECOVERY });
  }
});

test("Combat's own alert responses reach the patrolling agents' policies", async () => {
  let P;
  try {
    P = await import("../src/ai/classic/perception.js");
  } catch {
    return; // Combat not landed
  }
  if (!P.hazardResponse || !P.alertResponse || !P.acousticResponse) return;
  for (const spec of OBJECTIVE_AGENTS) {
    const f = patrolling(spec);
    const { world, ai, agent, controller, enemy, enemyCharacter } = f;
    const weapon = { def: { class: "test_rifle", ai: { minRange: 0, optimalRange: 30, maxRange: 60, implicitRange: false } } };
    controller.unit.combat = { arsenal: { weapons: [weapon], current: () => weapon } };
    const dispatch = () => ({ pattern: 2, aux: true });
    ai.combat = {
      hazardResponse: (c, a) => P.hazardResponse(c, world, a),
      alertResponse: (c, a) => P.alertResponse(c, world, a, dispatch),
      acousticResponse: (c, a) => P.acousticResponse(c, world, a),
      canEngage: () => true,
    };
    const cases = [
      ["danger", () => controller.notifyDanger(new THREE.Vector3(2, 0, 2)), PRIORITY.DANGER],
      ["shot", () => controller.notifyShot(enemyCharacter, true, new THREE.Vector3(12, 0, 0)), PRIORITY.SHOT],
      ["acoustic", () => controller.addAcoustic({ unit: enemy, generation: enemy.generation }), PRIORITY.ACOUSTIC],
    ];
    for (const [label, setup, priority] of cases) {
      resetQueue(controller);
      agent.state = spec.patrol;
      setup();
      let fired;
      assert.doesNotThrow(() => {
        fired = agent.interrupts.tick();
      }, `${spec.name}: ${label}`);
      assert.ok(fired, `${spec.name}: ${label} fires`);
      assert.equal(agent.interrupts.last, priority);
      assert.equal(controller.commands.queue[0].kind, GOAL.COMBAT);
    }
  }
});
