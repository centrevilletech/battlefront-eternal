// Bots that stop moving: one test per root cause found by
// tools/ai-stuck-probe.mjs (docs/ai.md "Stuck bots"). Synthetic fixtures.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Arsenal } from "../src/weapons.js";
import { Units } from "../src/registry.js";
import { ClassicRandom } from "../src/ai/random.js";
import { RepairServices } from "../src/ai/classic/classes.js";
import { DONE, GOAL, SourceGoal, refOf } from "../src/ai/classic/controller.js";
import { Agent, UnitRandomAgent } from "../src/ai/classic/agents.js";
import { InterruptHelper } from "../src/ai/classic/interrupts.js";
import { AttackPatterns } from "../src/ai/classic/combat.js";
import { TargetTracker } from "../src/ai/classic/perception.js";
import { PatrolHelper } from "../src/ai/classic/patrol.js";
import { ClassicAI } from "../src/ai/classic/index.js";
import { ClassicNav, GroundGotoHelper, NavControl, analogs, navigationInput, KIND, EXEC } from "../src/ai/classic/nav/index.js";
import { VehicleAI, UnitBoardAgent } from "../src/ai/classic/vehicles/index.js";
import { registerService } from "../src/ai/classic/vehicles/service.js";
import { STARVED } from "../src/ai/classic/nav/planner.js";
import { hoverInput, walkerInput, steerState } from "../src/ai/classic/vehicles/steering.js";

const v = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z);
const DT = 1 / 60;

// ---------------------------------------------------------------------------
// Pilots and their repair jobs (classes.js RepairServices).

// A fusion cutter as the sides' data has it: no authored ranges (the
// engine's 60 m), a 3 m lock.
const cutter = { class: "cutter", base: "repair", channel: 0, RoundsPerClip: -1, ammo: 1, BuildingBuild: 30, LockOnRange: 3,
  ai: { minRange: 0, optimalRange: 30, maxRange: 60, implicitRange: true, extremeRange: 3, relationMask: 6, categoryMask: 0xffffff9c } };

// A turret's build zone, as the service candidates wrap a building: not
// alive, health type 6 (unbuilt), no vehicle.
function wreck(x, z = 0) {
  const building = { team: 0 };
  return { building, kind: 8, alive: false, healthType: 6, team: 0, generation: 1, health: 0, maxHealth: 300, classDef: {},
    position: v(x, 0, z), velocity: v(), sphere: { center: v(x, 0, z), radius: 0.7 } };
}

function pilot() {
  const units = new Units();
  const owner = { position: v(), velocity: v(), sphere: { center: v(), radius: 0.5 }, alive: true, generation: 1, team: 1, kind: 0,
    classDef: {}, origin: (out) => out.set(owner.position.x, 1.7, owner.position.z), forward: (out) => out.set(0, 0, 1) };
  owner.combat = { arsenal: new Arsenal([cutter], { player: false }), health: 100, maxHealth: 100, repairTarget: null };
  units.add(owner);
  const world = { units, time: 1, battle: { reinforcements: { 1: 100, 2: 100 } }, items: [], missionControls: {} };
  const ai = { world, randomA: { int: (lo) => lo, range: (lo) => lo }, combat: {}, warnOnce() {} };
  const helper = {
    calls: [],
    status: 0,
    goto(p, options = {}) { this.calls.push(["goto", p.clone(), options.mode ?? "route"]); },
    stop() { this.calls.push(["stop"]); },
  };
  const controller = { ai, world, unit: owner, perception: { fire: { channel: 0, burst: {} } }, rangeScale: 1,
    lookAt(p) { this.look = p.clone(); }, aimAt(ref) { this.aim = ref; }, setStance() {} };
  const agent = { ai, world, unit: owner, helper, controller };
  return { world, owner, helper, controller, agent, services: new RepairServices(ai) };
}

function startJob(s, target) {
  s.agent.job = { target: refOf(target), kind: "health" };
  s.services.startRepairService(s.agent);
  s.helper.calls.length = 0;
}

// One service, after phase 0 has cleared the controller's requests.
function serve(s) {
  s.controller.target = null;
  s.controller.fireA = false;
  return s.services.repairService(s.agent);
}

test("a pilot walks to a wreck 40 m off: one planned walk, not a new one every service", () => {
  const s = pilot();
  const target = wreck(40);
  startJob(s, target);
  for (let i = 0; i < 12; i++) {
    s.world.time += 0.25;
    const result = serve(s);
    // With no lock on it, the job is not "close" inside the tool's 60 m:
    // the empty lock no longer matches the wreck's missing vehicle.
    assert.equal(result.near, false);
  }
  assert.deepEqual(s.helper.calls.map((c) => c[0]), ["goto"], "goto once: each goto starts the plan over and stops the bot");
  assert.ok(s.helper.calls[0][1].distanceTo(target.position) < 1e-9);
});

test("a pilot close by the 5 m rule but not locked on walks the last metres straight in, then stops to work", () => {
  const s = pilot();
  const target = wreck(4);
  startJob(s, target);
  serve(s);
  serve(s);
  assert.equal(s.helper.calls.length, 1, "once");
  const [kind, point, mode] = s.helper.calls[0];
  assert.deepEqual([kind, mode], ["goto", "straight"]);
  assert.ok(Math.abs(point.distanceTo(target.position) - (0.7 + 1.5)) < 1e-6, "its bounds and 1.5 m short of the center");
  s.owner.combat.repairTarget = target.building;
  serve(s);
  assert.deepEqual(s.helper.calls.at(-1), ["stop"], "locked: stand and work");
  assert.equal(s.controller.fireA, true);
});

test("a job worked 8 s for nothing is given up and kept off the pilot's list for 30 s", () => {
  const s = pilot();
  const target = wreck(3);
  s.world.serviceCandidates = () => [target];
  s.agent.job = s.services.findRepairJob(s.agent);
  assert.equal(s.agent.job.target.unit, target);
  s.services.startRepairService(s.agent);
  s.owner.combat.repairTarget = target.building;
  assert.ok(serve(s));
  s.world.time += 8.01;
  assert.equal(serve(s), null, "no health gained in 8 s");
  assert.equal(s.services.findRepairJob(s.agent), null, "not the same job again at the next search");
  s.world.time += 30;
  assert.equal(s.services.findRepairJob(s.agent)?.target.unit, target, "after 30 s it may be tried again");
});

test("a tool that never comes to bear gives up after 8 s instead of standing for good", () => {
  const s = pilot();
  const target = wreck(0, 3);
  s.owner.forward = (out) => out.set(0, 0, -1); // facing away, and it cannot turn
  s.controller.perception.aimer = { direction: v(0, 0, -1) };
  s.owner.combat.repairTarget = target.building;
  startJob(s, target);
  assert.ok(serve(s));
  assert.equal(s.controller.fireA, false, "no work starts");
  s.world.time += 8.01;
  assert.equal(serve(s), null);
});

// ---------------------------------------------------------------------------
// A goal whose own start publishes an objective (controller.js driveCommands).

function makeUnit(units, team, x) {
  const position = v(x, 0, 0);
  const unit = { team, kind: 0, alive: true, dead: false, generation: 1, character: null, controller: null,
    classDef: { UnitType: "trooper", MaxSpeed: 6 }, soldier: { maxSpeed: 6, aimYaw: 0, aimPitch: 0, posture: "stand" },
    position, velocity: v(), sphere: { center: position, radius: 0.5 }, intent: { buttons: [0, 0] },
    forward: (out) => out.set(0, 0, 1), aimPoint: (out) => out.copy(position), origin: (out) => out.copy(position).setY(1.6) };
  units.add(unit);
  return unit;
}

test("an objective published as a goal starts begins at the next phase 1, not round and round in this one", () => {
  // A mounted patrol that asks for a post as it begins did this:
  // 64 agents and objective draws a service, then a warning, and the bot
  // never acted.
  let made = 0;
  class Publisher extends Agent {
    constructor(controller, goal) {
      super(controller, goal);
      made += 1;
      this.requestState(1);
    }
    onEnter() {
      if (this.state === 1) this.controller.publishObjective(this.goal.post);
    }
  }
  const units = new Units();
  const world = { time: 0, random: new ClassicRandom(1), randomB: new ClassicRandom(2), units, battle: { characters: [] },
    difficulty: { cycle: 15, losBudget: 6 }, nav: { hints: [] } };
  const helper = { goto() {}, stop() {}, tick() {}, terminate() {} };
  const ai = new ClassicAI(world, { quiet: true, createHelper: () => helper, agents: { trooper: Publisher } });
  ai.bind({}, { warn: false });
  const post = { name: "cp1", team: 2, position: v(50, 0, 0) };
  const character = { index: 0, team: 1, human: false, unit: null, controller: null, objective: post };
  world.battle.characters.push(character);
  character.unit = makeUnit(units, 1, 0);
  const controller = ai.attach(character);
  const warn = console.warn;
  const warnings = [];
  console.warn = (...args) => warnings.push(args.join(" "));
  try {
    controller.update(DT, 1);
    assert.equal(made, 1);
    assert.equal(controller.commands.latch, true, "the new objective waits for the next phase 1");
    controller.update(DT, 0);
    controller.update(DT, 1);
    assert.equal(made, 2);
  } finally {
    console.warn = warn;
  }
  assert.deepEqual(warnings, []);
});

test("a slow mounted patrol with an enemy near asks for a post other than its own", () => {
  const units = new Units();
  const world = { time: 0, units };
  const service = new VehicleAI({ world, relation: (a, b) => (a === b ? 1 : -1) });
  const enemy = makeUnit(units, 2, 10);
  enemy.character = { team: 2 };
  const calls = [];
  const controller = { unit: { team: 1, position: v(), classDef: { ForwardSpeed: 5 } }, requestObjective: (exclude) => calls.push(exclude), requestExit: () => calls.push("exit") };
  const patrol = { agent: { controller }, requestState(state) { this.state = state; } };
  assert.equal(service.patrolMounted(patrol), true);
  assert.deepEqual(calls, [true], "an objective request with the exclude flag and the current post");
});

// ---------------------------------------------------------------------------
// Navigation: Droideka start, stranded routes, planner (nav/*.js).

function kinematic(position, { kind = KIND.soldier, yaw = 0, classDef = {} } = {}) {
  const unit = {
    id: 1, kind, generation: 1, alive: true, position: v(...position), velocity: v(),
    classDef: { MaxSpeed: 6.5, MaxStrafeSpeed: 4.5, MaxTurnSpeed: 4, MaxPitchSpeed: 0.5, ...classDef },
    soldier: { aimYaw: yaw, aimPitch: 0, posture: "stand", air: null, lock: 0, fuel: 0, avoidance: v(), jet: { minBorder: 0.1, recharge: 0 } },
    origin: (out) => out.set(unit.position.x, unit.position.y + 1.7, unit.position.z),
    forward: (out) => out.set(Math.sin(unit.soldier.aimYaw), 0, Math.cos(unit.soldier.aimYaw)),
  };
  return unit;
}

function move(unit, intent, dt) {
  const s = unit.soldier;
  s.aimYaw -= unit.classDef.MaxTurnSpeed * intent.turn * dt;
  const sin = Math.sin(s.aimYaw);
  const cos = Math.cos(s.aimYaw);
  const vf = unit.classDef.MaxSpeed * intent.forward;
  const vr = unit.classDef.MaxStrafeSpeed * intent.right;
  unit.velocity.set(sin * vf - cos * vr, 0, cos * vf + sin * vr);
  unit.position.addScaledVector(unit.velocity, dt);
}

const blankIntent = () => ({ forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false });
const record = (center, hx, hz, flags = 1) => ({ name: "b", xfrm: [1, 0, 0, 0, 1, 0, 0, 0, -1, center[0], center[1], -center[2]], size: [hx, 0, hz], flags, properties: [] });

function navWorld(barriers = [], options = {}) {
  const json = { schema: 1, graph: null, hints: [], barriers, boundary: { configs: [], references: [] }, paths: [], regions: [], warnings: [] };
  return { nav: new ClassicNav(json, null, options), random: new ClassicRandom(1), gameRandom: new ClassicRandom(2), time: 0 };
}

function drive(w, helper, unit, controller, seconds, until = () => false) {
  const intent = blankIntent();
  for (let i = 0; i < seconds * 60; i++) {
    w.time += DT;
    helper.tick(DT);
    if (!w.nav.planner.idle) w.nav.service(1);
    navigationInput(controller.navControl, unit, intent, DT, null);
    move(unit, intent, DT);
    if (until()) return true;
  }
  return false;
}

test("a Droideka at rest starts along its path (its 0.2 m lookahead is not an arrival)", () => {
  const nav = ClassicNav.empty();
  const unit = kinematic([0, 0, 0], { kind: KIND.droideka, classDef: { MaxSpeed: 8, MaxStrafeSpeed: 0, MaxTurnSpeed: 3 } });
  const control = new NavControl(nav, { unit });
  control.attach(unit);
  control.install(nav.planner.segment(unit.position, v(0, 0, 10)));
  const intent = blankIntent();
  navigationInput(control, unit, intent, DT, null);
  assert.ok(intent.forward > 0.9, `forward ${intent.forward}: the old stop never let it start`);
  // Within 0.2 m of the point it steers for, at the waypoint itself, it
  // still stops.
  const request = { valid: true, steeringTarget: v(0, 0, 0.15), waypoint: v(0, 0, 0.15), segmentDirection: v(0, 0, 1), mode: 2, lower: -1, upper: 1 };
  const out = blankIntent();
  analogs(unit, request, null, DT, out, { executor: control.executor });
  assert.equal(out.forward, 0);
});

// A destination walled in: a ring of barriers wider than the search strays.
function walledIn() {
  const walls = [];
  for (let i = 0; i < 4; i++) {
    const a = (i * Math.PI) / 2;
    walls.push(record([30 + 8 * Math.cos(a), 0, 8 * Math.sin(a)], i % 2 ? 9 : 1, i % 2 ? 1 : 9));
  }
  return walls;
}

test("a soldier whose route fails reports no progress after the executor's 5 s, and takes stagnation recovery", () => {
  const w = navWorld(walledIn());
  const unit = kinematic([0, 0, 0]);
  const controller = { unit, get executorStatus() { return this.navControl?.status ?? 0; } };
  const helper = new GroundGotoHelper(controller, w);
  helper.goto(v(30, 0, 0));
  assert.ok(drive(w, helper, unit, controller, 20, () => helper.failed));
  const control = controller.navControl;
  assert.equal(control.stranded, true);
  assert.equal(control.status, 0, "not yet");
  drive(w, helper, unit, controller, EXEC.noProgress + 0.1);
  assert.equal(control.status, 1);
  // Priority 8 now fires (it needs a nonzero status; without one the bot
  // stood where its route failed until something else moved it).
  const agent = { controller, ai: {}, now: () => w.time, acceptsRecovery: () => true };
  const out = { goal: null };
  assert.equal(new InterruptHelper(agent).recovery(out), true);
  assert.equal(out.goal.kind, GOAL.RECOVERY);
  // New orders clear it.
  helper.goto(v(-10, 0, 0));
  assert.equal(control.stranded, false);
  assert.equal(control.status, 0);
});

test("stranded again where it stood after a look round, the time already spent there counts on; a walk away starts it over", () => {
  const w = navWorld(walledIn());
  const unit = kinematic([0, 0, 0]);
  const controller = { unit };
  const helper = new GroundGotoHelper(controller, w);
  const control = () => controller.navControl;
  const strandAt = () => {
    helper.goto(v(30, 0, 0));
    assert.ok(drive(w, helper, unit, controller, 20, () => helper.failed));
    assert.equal(control().stranded, true);
  };
  strandAt();
  const spot = unit.position.clone();
  drive(w, helper, unit, controller, 3);
  // A fight's look round stops it, then its orders send it the same way.
  helper.stop();
  assert.equal(control().status, 0);
  drive(w, helper, unit, controller, 1);
  strandAt();
  assert.ok(unit.position.distanceTo(spot) < 2);
  drive(w, helper, unit, controller, 2.2);
  assert.equal(control().status, 1, "3 s and 2.2 s at one spot; each alone is short of 5 s, and the bot stood there for good");
  // Somewhere else it starts over.
  helper.stop();
  unit.position.x -= 5;
  drive(w, helper, unit, controller, 0.1);
  strandAt();
  drive(w, helper, unit, controller, 2.2);
  assert.equal(control().status, 0);
});

test("only soldiers and droidekas report a stranded route (others have their own stuck handling)", () => {
  const w = navWorld(walledIn());
  const unit = kinematic([0, 0, 0], { kind: KIND.hover });
  const controller = { unit };
  const helper = new GroundGotoHelper(controller, w);
  helper.goto(v(30, 0, 0));
  assert.ok(drive(w, helper, unit, controller, 20, () => helper.failed));
  drive(w, helper, unit, controller, EXEC.noProgress + 1);
  assert.equal(controller.navControl.stranded, true);
  assert.equal(controller.navControl.status, 0);
});

test("a full planner queue answers the request as a failure, not with a wait for good", () => {
  const w = navWorld([record([10, 0, 0], 1, 6)], { pools: { requests: 0 } });
  const unit = kinematic([0, 0, 0]);
  const controller = { unit };
  const helper = new GroundGotoHelper(controller, w);
  helper.goto(v(20, 0, 0));
  helper.tick(DT);
  assert.equal(helper.failed, true);
  assert.equal(helper.state, 7);
  assert.equal(controller.navControl.stranded, true);
});

test("following a target that keeps still past a barrier strands the follower once its cut-short path runs out", () => {
  const w = navWorld([record([10, 0, 0], 1, 30)]);
  const target = kinematic([20, 0, 0]);
  const unit = kinematic([0, 0, 0], { yaw: Math.PI / 2 });
  const controller = { unit };
  const helper = new GroundGotoHelper(controller, w);
  helper.follow(refOf(target));
  assert.equal(helper.adjusted, true, "cut 1 m short of the barrier");
  assert.ok(drive(w, helper, unit, controller, 10, () => !controller.navControl.executor));
  drive(w, helper, unit, controller, EXEC.noProgress + 0.2);
  assert.equal(controller.navControl.status, 1);
});

test("a dead bot's queued path search leaves the planner with its life", () => {
  const units = new Units();
  const nav = navWorld([record([10, 0, 0], 1, 6)]).nav;
  const world = { time: 0, random: new ClassicRandom(1), randomB: new ClassicRandom(2), units, battle: { characters: [] },
    difficulty: { cycle: 15, losBudget: 6 }, nav };
  const ai = new ClassicAI(world, { quiet: true });
  ai.bind({}, { warn: false });
  const character = { index: 0, team: 1, human: false, unit: makeUnit(units, 1, 0), controller: null, objective: null };
  world.battle.characters.push(character);
  const controller = ai.attach(character);
  const control = nav.control(controller);
  control.attach(controller.unit);
  nav.planner.request(control, v(0, 0, 0), v(20, 0, 0), 0.5, 1, 0);
  assert.equal(nav.planner.queue.length, 1);
  character.unit.alive = false;
  ai.died(character);
  assert.equal(nav.planner.queue.length, 0, "the planner would have searched on for a body that is gone");
});

// ---------------------------------------------------------------------------
// Walkers (vehicles/steering.js, vehicles/index.js).

function walker(position, yaw = 0) {
  const unit = {
    kind: 2, position: v(...position), velocity: v(), bodyYaw: yaw,
    classDef: { MaxSpeed: 5, MaxTurnSpeed: 2, MaxPitchSpeed: 0.5, bounds: [[-1, -1, -1], [1, 1, 1]] },
    origin: (out) => out.set(unit.position.x, unit.position.y + 2, unit.position.z),
    forward: (out) => out.set(Math.sin(unit.bodyYaw), 0, Math.cos(unit.bodyYaw)),
  };
  return unit;
}

test("a walker turns to a waypoint on its way that is near but behind it", () => {
  const unit = walker([0, 0, 0]);
  const request = { valid: true, steeringTarget: v(-2, 0, -1), waypoint: v(-2, 0, -1), segmentDirection: v(-1, 0, 0), mode: 2, lower: -1, upper: 1 };
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  walkerInput(unit, request, null, DT, { now: 0, hasPath: true }, out);
  assert.notEqual(out.turn, 0, "it only walks where it faces: facing straight on, it stood within 4 m of the waypoint for good");
  // Arrived (inside the radius with no path left): it faces its aim.
  walkerInput(unit, request, null, DT, { now: 0, hasPath: false }, out);
  assert.equal(out.turn, 0);
  assert.equal(out.forward, 0);
});

test("a hover or walker stopped on the spot to stand and shoot turns to its target", () => {
  // The stop is written where the hull stands; the target is 50 degrees
  // off its nose. A fixed gun only fires once the hull faces it.
  const aim = v(Math.sin(0.87) * 40, 1, Math.cos(0.87) * 40);
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  const hover = walker([0, 0, 0]);
  hover.kind = 6;
  hover.classDef = { ForwardSpeed: 40, StrafeSpeed: 0, TurnRate: 2, PitchRate: 1 };
  const stop = { valid: true, steeringTarget: v(0, 0, 0), waypoint: v(0, 0, 0), segmentDirection: v(0, 0, 1), mode: 0, lower: -1, upper: 1 };
  hoverInput(hover, stop, aim, DT, { now: 0, hasPath: false, status: 0 }, out);
  assert.equal(out.forward, 0);
  assert.ok(out.turn < -0.5, `turn ${out.turn}: it faced the point under it and never fired`);
  const atst = walker([0, 0, 0]);
  walkerInput(atst, stop, aim, DT, { now: 0, hasPath: false }, out);
  assert.equal(out.forward, 0);
  assert.ok(out.turn < -0.5, `turn ${out.turn}`);
});

test("a walker yields to a larger hull it walks into, not to one it merely rests beside", () => {
  const self = { position: v(0, 0, 0), velocity: v(), boundRadius: 3, alive: true, yaw: 0, bounds: { min: v(-1, 0, -1), max: v(1, 4, 1) } };
  const parked = { position: v(4, 0, 0), velocity: v(), boundRadius: 5, alive: true, aiTarget: { kind: 6 },
    bounds: { min: v(-5, -2, -5), max: v(5, 2, 5) } };
  const world = { time: 10, vehicleSystem: { vehicles: [self, parked] } };
  const service = new VehicleAI({ world, relation: () => 1 });
  const unit = walker([0, 0, 0]);
  unit.vehicle = self;
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, -100, "at rest: no collision, no hold");
  self.velocity.set(-1, 0, 0);
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, -100, "moving away: none either");
  self.velocity.set(1, 0, 0);
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, 10, "walking into it: the smaller hull yields");
});

test("a walker beside a tall hull's bounding sphere but clear of its body walks on", () => {
  // An AT-AT (8 x 20 x 21 m box): its bounding sphere reaches 15 m to every
  // side of a body 4 m wide. An AT-ST walking 12 m off its flank, toward
  // it, is nowhere near touching it.
  const atst = { position: v(12, 0, 0), velocity: v(-1, 0, 0), boundRadius: 5.6, alive: true, yaw: 0, bounds: { min: v(-2.5, 0, -2.8), max: v(2.5, 8.7, 2.8) } };
  const atat = { position: v(0, 0, 0), velocity: v(0, 0, 2), boundRadius: 15, alive: true, yaw: 0, aiTarget: { kind: 2 },
    bounds: { min: v(-4, 0, -10.5), max: v(4, 20, 10.5) } };
  const world = { time: 10, vehicleSystem: { vehicles: [atst, atat] } };
  const service = new VehicleAI({ world, relation: () => 1 });
  const unit = walker([12, 0, 0]);
  unit.vehicle = atst;
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, -100, "held by a sphere 8 m short of the hull");
  // Its footprint against the AT-AT's flank: held.
  atst.position.set(6.6, 0, 0);
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, 10);
  // Turned across, the AT-AT's flank is 10.5 m out: 12 m off is clear.
  atat.yaw = Math.PI / 2;
  atst.position.set(14, 0, 0);
  steerState(unit).holdSince = -100;
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, -100);
  atst.position.set(13, 0, 0);
  service.walkerContacts(unit);
  assert.equal(steerState(unit).holdSince, 10, "nose to its side");
});

test("a bot whose walk to a vehicle ends out of reach of its seat gives it up instead of standing there", () => {
  const units = new Units();
  const world = { time: 3, random: new ClassicRandom(1), randomB: new ClassicRandom(2), units, battle: { characters: [] },
    difficulty: { cycle: 15, losBudget: 6 }, nav: { hints: [] } };
  const helper = { arrived: false, failed: false, goto() {}, stop() {}, tick() {}, terminate() {} };
  const ai = new ClassicAI(world, { quiet: true, createHelper: () => helper });
  ai.bind({}, { warn: false });
  registerService(ai, { reserveSeat: () => true, canEnter: () => true, holdReservation: () => true });
  const character = { index: 0, team: 1, human: false, unit: makeUnit(units, 1, 0), controller: null, objective: null };
  const controller = ai.attach(character);
  // A turret on the deck 22 m above the point its route ends under.
  const turret = { alive: true, generation: 1, position: v(0, 22, 0), velocity: v(), sphere: { center: v(0, 23, 0), radius: 3 } };
  const agent = new UnitBoardAgent(controller, SourceGoal.board(refOf(turret)));
  agent.update(DT, false);
  assert.equal(agent.state, 2);
  agent.behave(DT);
  assert.equal(agent.pending, 0, "still walking: it stays");
  helper.arrived = true;
  agent.behave(DT);
  assert.equal(agent.pending, 100, "arrived out of reach: done");
  assert.equal(controller.reboardAt, world.time + 15);
});

test("a walker pressing on and going nowhere, or turning a hull that will not turn, backs out turning, as a hover does; one whose hull turns does not", () => {
  const ahead = { valid: true, steeringTarget: v(0, 0, 30), waypoint: v(0, 0, 30), segmentDirection: v(0, 0, 1), mode: 2, lower: -1, upper: 1 };
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  // Pressing on for the executor's 3 s stationary window first: the status
  // is then its own.
  const stuck = walker([0, 0, 0]);
  walkerInput(stuck, ahead, null, DT, { now: 10, hasPath: true, status: 2 }, out);
  assert.equal(out.forward, 1, "status 2 at the moment it sets off is not its walking's");
  walkerInput(stuck, ahead, null, DT, { now: 13.1, hasPath: true, status: 2 }, out);
  assert.deepEqual([out.forward, out.turn], [-1, 0.5]);
  // Behind it: a walker turns before it walks (forward 0), and its
  // stationary executor is no reason to back up.
  const behind = { ...ahead, steeringTarget: v(0, 0, -30), waypoint: v(0, 0, -30) };
  const turning = walker([0, 0, 0]);
  walkerInput(turning, behind, null, DT, { now: 10, hasPath: true, status: 2 }, out);
  assert.equal(out.forward, 0);
  assert.notEqual(out.turn, 0.5);
  assert.equal(steerState(turning).recovery, 0);
  // Its hull turning, it never backs out; a hull that has not turned in
  // 2 s (a tauntaun wedged by a wall, every turn swinging it in) does.
  for (let t = 10; t < 13; t += 0.5) {
    turning.bodyYaw += 0.2;
    walkerInput(turning, behind, null, DT, { now: t, hasPath: true, status: 2 }, out);
    assert.equal(steerState(turning).recovery, 0);
  }
  const wedged = walker([0, 0, 0]);
  walkerInput(wedged, behind, null, DT, { now: 10, hasPath: true, status: 2 }, out);
  walkerInput(wedged, behind, null, DT, { now: 11.9, hasPath: true, status: 2 }, out);
  assert.equal(steerState(wedged).recovery, 0);
  walkerInput(wedged, behind, null, DT, { now: 12.1, hasPath: true, status: 2 }, out);
  assert.deepEqual([out.forward, out.turn], [-1, 0.5], "it stood facing away from its path for good");
});

test("a slow walker that turned on the spot for longer than 3 s walks off rather than backing up", () => {
  // Geonosis: the executor's stationary count runs while a walker turns on
  // the spot, so a slow turner set off with status 2 already up. Taken as
  // stuck, it backed up turning, turned back, and backed up again, and
  // drifted off the map instead of walking to its waypoint.
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  const unit = walker([0, 0, 0]);
  unit.classDef.MaxTurnSpeed = 0.6;
  const request = { valid: true, steeringTarget: v(0, 0, -30), waypoint: v(0, 0, -30), segmentDirection: v(0, 0, -1), mode: 2, lower: -1, upper: 1 };
  let t = 10;
  for (;; t += DT) {
    walkerInput(unit, request, null, DT, { now: t, hasPath: true, status: t > 13 ? 2 : 0 }, out);
    if (out.forward !== 0) break;
    unit.bodyYaw += 0.6 * DT;
  }
  assert.ok(t > 14, "the half turn outlasted the stationary window");
  for (let k = 0; k < 60; k++, t += DT) {
    walkerInput(unit, request, null, DT, { now: t, hasPath: true, status: 2 }, out);
    assert.ok(out.forward > 0.9, "it backed up as it set off");
  }
  assert.equal(steerState(unit).recovery, 0);
});

test("a walker whose recoveries leave it where it was plans its trip again", () => {
  // A spider walker on Kashyyyk pressed into one quay kerb for six minutes:
  // every recovery backed it out and it walked into the same spot again.
  const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
  const unit = walker([0, 0, 0]);
  const ahead = { valid: true, steeringTarget: v(0, 0, 30), waypoint: v(0, 0, 30), segmentDirection: v(0, 0, 1), mode: 2, lower: -1, upper: 1 };
  let replans = 0;
  const asked = [];
  const env = (now) => ({ now, hasPath: true, status: 2, replan: (another) => { replans += 1; asked.push(another); } });
  let t = 10;
  let ended = 0;
  for (; t < 120 && replans === 0; t += DT) {
    const was = steerState(unit).recovery;
    walkerInput(unit, ahead, null, DT, env(t), out);
    if (was !== 0 && steerState(unit).recovery === 0) ended += 1;
    unit.bodyYaw -= out.turn * 2 * DT; // the hull turns; it goes nowhere
  }
  assert.equal(replans, 1);
  assert.equal(ended, 2, "planned again after its second recovery in the same place");
  for (; t < 300 && replans < 2; t += DT) {
    walkerInput(unit, ahead, null, DT, env(t), out);
    unit.bodyYaw -= out.turn * 2 * DT;
  }
  assert.deepEqual(asked, [false, true], "another post after the fourth");
  // One that got clear (its recoveries 10 m or more apart) does not.
  const moving = walker([0, 0, 0]);
  replans = 0;
  for (let k = 0; t < 400 && k < 4; t += DT) {
    const was = steerState(moving).recovery;
    walkerInput(moving, ahead, null, DT, env(t), out);
    moving.bodyYaw -= out.turn * 2 * DT;
    if (was !== 0 && steerState(moving).recovery === 0) { k += 1; moving.position.x += 12; }
  }
  assert.equal(replans, 0);
});

test("a path request passed over by 20 later searches is served next, whatever its priority", () => {
  const nav = navWorld([record([10, 0, 0], 1, 6)]).nav;
  const planner = nav.planner;
  // A route's first leg (priority 1) behind a stream of fighters' asks
  // (priority 0), two more each round, so one always waits ahead of it.
  const leg = new NavControl(nav, null);
  let at = null;
  const publish = leg.publish.bind(leg);
  leg.publish = (path, ready) => {
    if (ready && at === null) at = planner.served;
    publish(path, ready);
  };
  planner.request(leg, v(0, 0, 0), v(20, 0, 0), 0.5, 1, 1);
  for (let round = 0; round < 3 * STARVED && at === null; round++) {
    for (let k = 0; k < 2; k++) planner.request(new NavControl(nav, null), v(0, 0, round * 0.1 + k * 0.05), v(20, 0, round * 0.1), 0.5, 1, 0);
    const before = planner.served;
    for (let k = 0; k < 1000 && planner.served === before; k++) planner.service(1);
  }
  assert.notEqual(at, null, "strict priority order kept it waiting for as long as the asks came");
  assert.equal(at, STARVED, "taken once 20 later searches had finished");
});

test("a soldier in stagnation recovery that has not got a metre in 3 s jumps as it tries again", () => {
  const units = new Units();
  const world = { time: 0, random: new ClassicRandom(1), randomB: new ClassicRandom(2), units, battle: { characters: [] },
    difficulty: { cycle: 15, losBudget: 6 }, nav: { hints: [] } };
  const helper = { goto() {}, stop() {}, tick() {}, terminate() {} };
  const ai = new ClassicAI(world, { quiet: true, createHelper: () => helper });
  ai.bind({}, { warn: false });
  const character = { index: 0, team: 1, human: false, unit: makeUnit(units, 1, 0), controller: null, objective: null };
  const controller = ai.attach(character);
  const agent = new UnitRandomAgent(controller, SourceGoal.of(GOAL.RECOVERY));
  agent.update(DT, false);
  assert.equal(controller.jet.pending, false, "the first try walks");
  world.time = 3.5;
  agent.behave(DT);
  assert.equal(controller.jet.pending, true, "3 s on, still in the hollow: it jumps");
  controller.jet.pending = false;
  character.unit.position.x += 4;
  world.time = 7;
  agent.behave(DT);
  assert.equal(controller.jet.pending, false, "it got somewhere: no jump");
});

test("a bot that reaches an enemy post outside its capture region walks on to the center, once, then takes another objective", () => {
  // A round region of radius 10 whose box offset can land at a corner,
  // 14 m out: there the bot took no part in the capture and, with no timer
  // in this state, stood for as long as the post held.
  const post = { name: "cp1", team: 2, position: v(), desc: { capture: { size: [10, 2, 10], matrix: new THREE.Matrix4().toArray() } },
    contains: (p) => Math.hypot(p.x, p.z) < 10 };
  const world = { time: 0 };
  const calls = [];
  const asked = [];
  let next = { name: "cp2" };
  const helper = { arrived: false, failed: false, goto(p) { calls.push([p.x, p.z]); this.arrived = false; }, stop() {} };
  const ai = { world, randomA: { int: (lo) => lo, range: (lo) => lo, uniform: () => 0.5 }, relation: (a, b) => (a === b ? 1 : -1), nav: { hints: [] } };
  const agentAt = (x, z) => ({ ai, unit: { team: 1, kind: 0, position: v(x, 0, z) }, helper, controller: { character: { team: 1 }, requestObjective(exclude) { asked.push(exclude); return next; } }, now: () => world.time });
  const patrol = new PatrolHelper(agentAt(-10, -10));
  patrol.start(post);
  assert.equal(patrol.state, 2);
  assert.deepEqual(calls, [[-10, -10]], "center plus the box offset");
  helper.arrived = true;
  patrol.tick(DT);
  helper.arrived = true;
  patrol.tick(DT);
  assert.deepEqual(calls, [[-10, -10], [0, 0]], "on to the center, once");
  // Still outside there (Yavin's temple region, 36 m above where routes
  // end): 20 s on it asks for an objective other than this post.
  world.time = 20;
  patrol.tick(DT);
  assert.equal(calls.length, 2);
  assert.deepEqual(asked, []);
  world.time = 20.1;
  patrol.tick(DT);
  assert.deepEqual(asked, [true]);
  assert.equal(patrol.state, DONE);
  // No other post to give: it chooses about this one again.
  next = null;
  calls.length = 0;
  const only = new PatrolHelper(agentAt(-10, -10));
  only.start(post);
  helper.arrived = true;
  only.tick(DT);
  helper.arrived = true;
  only.tick(DT);
  only.tick(DT);
  world.time = 40.2;
  only.tick(DT);
  assert.equal(only.state, 2);
  assert.deepEqual(calls, [[-10, -10], [0, 0], [-10, -10]], "a fresh offset");
  // Arrived inside the region: it stays there to capture.
  calls.length = 0;
  const inside = new PatrolHelper(agentAt(2, 2));
  inside.start(post);
  inside.tick(DT);
  assert.equal(calls.length, 1);
});

// ---------------------------------------------------------------------------
// Fighting (combat.js): cover's hold on the target.

test("a fighter that leaves cover for another pattern lets go of the target cover held", () => {
  const world = { time: 10, random: new ClassicRandom(1), randomB: new ClassicRandom(2), nav: { hints: [] } };
  const unit = kinematic([0, 0, 0]);
  const controller = { unit, roll: 0 };
  const helper = { arrived: false, goto() {}, stop() {} };
  const agent = { world, controller, helper, aim() {} };
  agent.tracker = new TargetTracker(controller);
  const patterns = new AttackPatterns(agent);
  // Pattern 9: on the way to a cover hint, the target is held (frozen).
  patterns.coverHint = { position: v(5, 0, 0), forward: v(0, 0, 1), primaryMask: 1, secondaryMask: 1, sideMask: 0 };
  patterns.init(9, 1);
  assert.equal(patterns.cover.state, 2);
  assert.equal(agent.tracker.frozen, true);
  // A new dispatch (another target, another pattern) leaves cover for
  // stand-and-shoot.
  patterns.init(2, 1);
  assert.equal(patterns.state, 2);
  assert.equal(agent.tracker.frozen, false, "held for the rest of the fight: it stood facing where its target was last seen");
  agent.tracker.startCountdown(world.time);
  assert.equal(agent.tracker.deadline, 13, "an unseen target is let go 3 s on");
});
