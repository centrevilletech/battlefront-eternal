// Flyers on their pads: an AI pilot tries a blocked takeoff again, a craft
// whose takeoff keeps failing is left by its crew rather than destroyed, and
// a takeoff lifts a hull out of a pad structure it already overlaps at rest.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CollisionWorld } from "../src/collision.js";
import { Vehicles } from "../src/vehicles.js";
import { stepFlight } from "../src/flight/model.js";
import { pilotInput } from "../src/flight/ai.js";
import { fixture, FIXTURE_SPEEDS } from "./flight-fixture.js";
import { Units } from "../src/registry.js";
import { ClassicRandom } from "../src/ai/random.js";
import { ClassicAI } from "../src/ai/classic/index.js";
import { DONE, GOAL, SourceGoal } from "../src/ai/classic/controller.js";
import { VehicleAI } from "../src/ai/classic/vehicles/index.js";
import { UnitFlyAgent } from "../src/ai/classic/vehicles/agents.js";
import { registerService } from "../src/ai/classic/vehicles/service.js";

const DT = 1 / 60;
const v = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z);
const FLOOR = [0, -1, 0, 20000, 2, 20000];

// Static boxes [x, y, z, w, h, d] as a collision world.
function solid(...boxes) {
  const pos = [], idx = [];
  for (const [x, y, z, w, h, d] of boxes) {
    const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z), base = pos.length / 3;
    pos.push(...g.attributes.position.array);
    idx.push(...[...g.index.array].map((i) => i + base));
  }
  return new CollisionWorld(new Float32Array(pos), new Uint32Array(idx), new Uint32Array(idx.length / 3));
}

// The flight fixture's ship, 6 m tall and 8 m long (a gunship's build, and
// its lift: 10 m up at TakeoffSpeed 5, over to flight in 1.5 s), at rest on
// its pad in `world`.
function tallShip(world) {
  const g = new THREE.BoxGeometry(4, 6, 8).translate(0, 3, 0);
  const hull = { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
  const lift = { TakeoffHeight: "10.0", TakeoffSpeed: "5.0", TakeoffTime: "1.5" };
  const d = { model: "ship", label: "Tall ship", base: "flyer", settings: { ...FIXTURE_SPEEDS, ...lift },
    seats: [{ name: "BODY", settings: { EyePointOffset: "0 1 1", TrackCenter: "0 2 -4", TrackOffset: "0 2 6" }, weapons: [] }], explosion: null };
  const data = { models: { ship: { bones: [], parts: [] } }, collision: { ship: hull }, classes: { ship: d },
    spawns: [{ name: "pad", matrix: new THREE.Matrix4().toArray(), team: 1, post: "cp", respawnTime: 2, classes: { 1: "ship" } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision: world, texture: () => null,
    camera: new THREE.PerspectiveCamera(50, 16 / 9, 0.1, 2000), posts: { posts: [{ desc: { name: "cp" }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() { return null; } }, flightEnvironment: {} });
  const ship = system.vehicles[0];
  stepFlight(ship, DT, {});
  return ship;
}

// The AI pilot's takeoff key for this tick, steering for a point ahead.
const aiKey = (craft) => Boolean(pilotInput({ vehicle: craft }, { valid: true, steeringTarget: v(0, 60, 200) }, null, DT, {}, {}).up);

test("an AI pilot on the ground holds takeoff for half of each second of its time there", () => {
  const f = fixture();
  stepFlight(f.v, DT, {});
  const press = (t) => {
    f.v.stateTime = t;
    return aiKey(f.v);
  };
  assert.deepEqual([0, 0.4, 0.6, 0.9, 1.1, 2.7].map(press), [true, true, false, false, true, false]);
});

test("a takeoff under a deck lifts until the deck holds it, and goes on by itself once the way is clear", () => {
  const roofed = () => {
    const f = fixture();
    stepFlight(f.v, DT, {});
    f.system.collision = solid(FLOOR, [0, 3.5, 4, 40, 1, 60]); // a deck over the pad
    return f;
  };
  // An AI pilot's key, and a held one: either starts the lift, as the 2004
  // game's key does whatever is overhead.
  const ai = roofed(), held = roofed();
  const tick = () => {
    stepFlight(ai.v, DT, { takeoff: aiKey(ai.v) });
    stepFlight(held.v, DT, { takeoff: true });
  };
  for (let i = 0; i < 90; i++) tick();
  for (const f of [ai, held]) {
    assert.equal(f.v.flightState, "TakingOff");
    assert.equal(f.v.flightWarning, "Takeoff path blocked");
    assert.ok(f.v.position.y + f.v.bounds.max.y <= 3 + 1e-6, `held under the deck (${f.v.position.y})`);
  }
  // What was over the pad moves off.
  ai.system.collision = held.system.collision = solid(FLOOR);
  for (let i = 0; i < 600 && (ai.v.flightState !== "Flying" || held.v.flightState !== "Flying"); i++) tick();
  assert.equal(ai.v.flightState, "Flying");
  assert.equal(held.v.flightState, "Flying");
});

test("a hull parked with a pad structure inside its box lifts out of it; a roof above the box still holds it", () => {
  // A rim over one corner, inside the box the parked hull fills (on Bespin's
  // gunship pad _16 the pad's structure reaches into a corner of the box).
  const under = tallShip(solid(FLOOR, [2.5, 3.5, 4, 2, 1, 2]));
  const start = under.position.y;
  stepFlight(under, DT, { takeoff: true });
  assert.equal(under.flightState, "TakingOff", "the rim it already overlaps does not hold it down");
  // Nor does it on the way up: the lift met the rim at once and hung there,
  // half off the ground, until the stranded check blew it up.
  for (let i = 0; i < 600 && under.flightState === "TakingOff"; i++) stepFlight(under, DT, {});
  assert.equal(under.flightState, "Flying");
  assert.ok(under.position.y > start + 10, `lifted ${(under.position.y - start).toFixed(2)} m`);
  const roofed = tallShip(solid(FLOOR, [0, 7.5, 0, 30, 1, 40]));
  const parked = roofed.position.y;
  stepFlight(roofed, DT, { takeoff: true });
  assert.equal(roofed.flightState, "TakingOff");
  for (let i = 0; i < 240; i++) stepFlight(roofed, DT, {});
  assert.equal(roofed.flightState, "TakingOff");
  assert.equal(roofed.flightWarning, "Takeoff path blocked");
  assert.ok(roofed.position.y + roofed.bounds.max.y <= 7 + 1e-6 && roofed.position.y >= parked, "held under the roof");
});

function flyerPilot(units, grounded) {
  const position = v();
  const unit = { team: 1, kind: 5, alive: true, dead: false, generation: 1, character: null, controller: null,
    classDef: { MaxSpeed: 60 }, soldier: { maxSpeed: 60, aimYaw: 0, aimPitch: 0, posture: "stand" }, vehicle: { grounded },
    position, velocity: v(), sphere: { center: position, radius: 5 }, intent: { buttons: [0, 0] },
    forward: (out) => out.set(0, 0, 1), aimPoint: (out) => out.copy(position), origin: (out) => out.copy(position) };
  units.add(unit);
  return unit;
}

test("a stranded flyer still on its pad is left by its crew, not destroyed; one in the air is destroyed, as Classic does", () => {
  const strand = (grounded) => {
    const units = new Units();
    const world = { time: 10, random: new ClassicRandom(1), randomB: new ClassicRandom(2), units, battle: { characters: [] },
      difficulty: { cycle: 15, losBudget: 6 }, nav: { hints: [] }, posts: { posts: [] } };
    const helper = { arrived: false, failed: false, status: 2, goto() {}, stop() {}, tick() {}, terminate() {}, followPath() {}, land() {} };
    const ai = new ClassicAI(world, { quiet: true, createHelper: () => helper });
    ai.bind({}, { warn: false });
    const calls = [];
    registerService(ai, { abandonPad: () => calls.push("abandon"), destroyStuck: () => calls.push("destroy") });
    const character = { index: 0, team: 1, human: false, unit: flyerPilot(units, grounded), controller: null, objective: null };
    const controller = ai.attach(character);
    Object.defineProperty(controller, "executorStatus", { get: () => 2 });
    const agent = new UnitFlyAgent(controller, SourceGoal.of(GOAL.OBJECTIVE));
    agent.behave(DT);
    return { calls, agent };
  };
  const pad = strand(true);
  assert.deepEqual(pad.calls, ["abandon"]);
  assert.equal(pad.agent.pending, DONE, "the agent ends: the crew go on foot");
  const air = strand(false);
  assert.deepEqual(air.calls, ["destroy"]);
});

test("a craft left on its pad: the bots get out, nobody moving up to the controls, the pilot at the seat batch; no bot boards it for 60 s", () => {
  const calls = [];
  const system = {
    unboard(unit, options) { calls.push(["unboard", unit.name, options.promote]); return true; },
    requestExit(unit, options) { calls.push(["exit", unit.name, options.promote]); return true; },
  };
  const world = { time: 100, vehicleSystem: system };
  const service = new VehicleAI({ world, relation: () => 1 });
  const hull = { alive: true, team: 1, seats: [], firstFreeSeat: () => 3 };
  const crew = (name, index, human = false) => {
    const lease = { vehicle: hull, index, released: false, human, unit: { name, character: { controller: { reboardAt: -Infinity } } } };
    lease.controllable = { lease, vehicle: hull };
    hull.seats[index] = { lease };
    return lease;
  };
  const pilot = crew("pilot", 0), gunner = crew("gunner", 1);
  crew("player", 2, true);
  hull.seats[3] = { lease: null };
  const controller = { unit: pilot.controllable, reboardAt: -Infinity };
  service.abandonPad(controller);
  assert.deepEqual(calls, [["unboard", "gunner", false], ["exit", "pilot", false]], "the player stays aboard");
  assert.equal(controller.reboardAt, 115);
  assert.equal(gunner.unit.character.controller.reboardAt, 115);
  const walker = { unit: { kind: 0, alive: true, team: 1, soldier: {}, classDef: {} } };
  assert.equal(service.canEnter(walker, hull), false);
  world.time = 159;
  assert.equal(service.canEnter(walker, hull), false);
  world.time = 161;
  assert.equal(service.canEnter(walker, hull), true);
});
