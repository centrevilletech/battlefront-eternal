import './fixtures/native-anim.js';
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Vehicles } from "../src/vehicles.js";
import { stepPrediction } from './helpers/hull-prediction.js';
const stepFlight = (v, dt, input) => stepPrediction(v.system, dt, v, input);
import { fixture, FIXTURE_SPEEDS } from "./flight-fixture.js";

const DT = 1 / 60;
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

test("a takeoff under a deck lifts until the deck holds it, and goes on by itself once the way is clear", () => {
  const roofed = () => {
    const collision = solid(FLOOR), geometry = new THREE.BoxGeometry(40, 1, 60).translate(0, 3.5, 4);
    const deck = collision.addBody(geometry.attributes.position.array, geometry.index.array, new Uint32Array(geometry.index.count / 3));
    deck.enabled = true;
    const f = fixture(undefined, 'ship', {}, undefined, collision); f.deck = deck;
    // The pad is below the deck. Construction's top-down spawn would place
    // the craft on its roof, so establish the below-deck birth pose first.
    f.v.position.y = 0; f.v.floorY = 0; f.v.syncBody();
    stepFlight(f.v, DT, {});
    return f;
  };
  // Held human input exercises prediction through the same native hull lift
  // used by bot intent. Pilot decision cases live in the native flight tests.
  const held = roofed();
  const tick = () => stepFlight(held.v, DT, { takeoff: true });
  for (let i = 0; i < 90; i++) tick();
  assert.equal(held.v.flightState, "TakingOff");
  assert.equal(held.v.flightWarning, "Takeoff path blocked");
  assert.ok(held.v.position.y + held.v.bounds.max.y <= 3 + 1e-6, `held under the deck (${held.v.position.y})`);
  held.deck.enabled = false;
  for (let i = 0; i < 600 && held.v.flightState !== "Flying"; i++) tick();
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
