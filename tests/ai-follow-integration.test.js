import test from "node:test";
import assert from "node:assert/strict";
import { Vector3 } from "three";
import { ClassicNav } from "../src/ai/classic/nav/index.js";
import { refOf } from "../src/ai/classic/controller.js";
import { ClassicObjectives } from "../src/ai/classic/goals.js";
import { objectiveCrew, objectiveVehicleMask } from "../src/ai/index.js";

const p = (x = 0, z = 0) => new Vector3(x, 0, z);
function setup() {
  const nav = ClassicNav.empty(); let draws = 0;
  const world = { nav, time: 0, randomB: { uniform: () => { draws++; return 0.5; } }, missionControls: { formation: { far: 3 } } };
  const unit = { position: p(), velocity: p(), generation: 1, alive: true, kind: 0, classDef: {} };
  const leader = { position: p(10), generation: 1, alive: true, forward: (out) => out.set(0, 0, 1) };
  const controller = { unit, world, ai: { followRadii: { near: 0, far: 3 } } };
  const helper = nav.helper(controller, world); helper.control.attach(unit);
  return { nav, world, unit, leader, controller, helper, control: helper.control, draws: () => draws };
}

test("Follow records up to30 leader points then resumes their executor without replanning", () => {
  const s = setup(); s.helper.setLeader(refOf(s.leader)); s.helper.followTrail(true);
  for (let i = 0; i < 35; i++) { s.leader.position.x = 10 + i; s.control.update(0.26); }
  assert.equal(s.control.trail.length, 30); assert.equal(s.control.trail.nodes[0].x, 15); assert.equal(s.draws(), 105);
  s.helper.followTrail(false); assert.equal(s.control.trail, null); assert.ok(s.control.executor);
  assert.equal(s.control.executor.final.x, 44);
  s.leader.position.x = 45; s.control.update(0.26); assert.equal(s.control.executor.final.x, 45);
  assert.equal(s.nav.planner.idle, true);
  s.helper.setLeader(null); assert.equal(s.control.executor, null);
  assert.equal(s.nav.planner.pools.paths.used, 0); assert.equal(s.nav.planner.pools.nodes.used, 0);
});

test("Follow sampling preserves strict time/displacement and held paths pause random draws", () => {
  const s = setup(); s.helper.setLeader(refOf(s.leader)); s.helper.followTrail(true);
  s.control.update(0.1); assert.equal(s.control.trail.length, 1);
  s.leader.position.x += 0.25;
  s.control.update(0.25); assert.equal(s.draws(), 3, "deadline equality does not sample");
  s.control.update(0.01); assert.equal(s.draws(), 6); assert.equal(s.control.trail.length, 1, "displacement equality does not append");
  s.leader.position.x += 0.001; s.control.update(0.01); assert.equal(s.control.trail.length, 2);
  s.helper.followTrail(false); s.helper.installPath([p(3), p(3)]); s.helper.holdPath(true);
  const count = s.draws(); const index = s.control.executor.index;
  s.control.update(1); assert.equal(s.draws(), count); assert.equal(s.control.executor.index, index);
  s.helper.holdPath(false); s.control.update(0.26); assert.equal(s.draws(), count + 3);
});

test("Follow completion remains appendable and generation reset releases its recorded path", () => {
  const s = setup(); s.helper.setLeader(refOf(s.leader)); s.helper.installPath([p(), p()]);
  s.control.trailDelay = 10; s.control.update(0.1); s.control.update(0.1);
  assert.ok(s.control.executor); assert.equal(s.control.executor.complete, true);
  s.control.trailDelay = 0; s.control.update(0.1); assert.equal(s.control.executor.complete, false);
  assert.ok(s.control.segments < 2, "completed segment is not consumed twice");
  s.helper.followTrail(true); s.unit.generation++; s.control.attach(s.unit);
  assert.equal(s.control.leader, null); assert.equal(s.control.trail, null); assert.equal(s.control.executor, null);
  assert.equal(s.nav.planner.pools.nodes.used, 0);
});

function character(human = false) { const c = { team: 1, human, objective: null, unit: { alive: true } }; c.unit.character = c; return c; }
function board(c, vehicle, index) { const lease = { vehicle, index, unit: c.unit, live: true }; vehicle.seats[index].lease = lease; c.unit.seat = lease; return lease; }
test("live seat adapter propagates driver objectives, passenger copies and traversal bans", () => {
  const driver = character(); const passenger = character(); const human = character(true);
  const vehicle = { seats: [{}, {}, {}], aiTarget: { classDef: { settings: { AISizeType: "HUGE" } } } };
  board(driver, vehicle, 0); const seat = board(passenger, vehicle, 1); board(human, vehicle, 2);
  const blocked = { name: "blocked", team: 2, active: true, ai: { atk: [100], def: [100], bans: 16 } };
  const allowed = { name: "allowed", team: 2, active: true, ai: { atk: [100], def: [100], bans: 0 } };
  const objectives = new ClassicObjectives({ posts: [blocked, allowed], random: { uniform: () => 0, int: () => 0 },
    faction: () => 0, relation: (a, b) => a === b ? 1 : -1, crew: objectiveCrew, vehicleMask: objectiveVehicleMask });
  assert.equal(objectiveVehicleMask(vehicle), 16);
  assert.equal(objectives.request(driver), allowed); assert.equal(passenger.objective, allowed); assert.equal(human.objective, null);
  driver.objective = blocked; assert.equal(objectives.request(passenger), blocked);
  seat.released = true; assert.equal(objectiveCrew(passenger), null);
});
