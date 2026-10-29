import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Units, segmentBox, segmentCylinder, segmentSphere, unitsClear } from "../src/registry.js";
import { SimEvents } from "../src/sim-events.js";
import { Battle } from "../src/battle.js";
import { ClassicRandom } from "../src/ai/random.js";

// A stand-in unit: one hit sphere at a point.
function fakeUnit(team, x, y, z, r = 0.6) {
  const center = new THREE.Vector3(x, y, z);
  return {
    team,
    alive: true,
    generation: 0,
    position: center,
    sphere: { center, radius: r },
    hitShapes: () => [{ type: "sphere", part: "body", center, radius: r }],
  };
}

test("registry ids, generations and stale references", () => {
  const units = new Units();
  const a = units.add(fakeUnit(1, 0, 1, 0));
  const b = units.add(fakeUnit(2, 5, 1, 0));
  assert.equal(a.id, 1);
  assert.equal(b.id, 2);
  assert.equal(units.byId(2), b);
  const ref = units.ref(a);
  assert.equal(units.resolve(ref), a);
  a.generation += 1; // respawned
  assert.equal(units.resolve(ref), null);
  b.alive = false;
  assert.equal(units.resolve(units.ref(b)), null);
});

test("relations: same team friendly, neutral zero, others hostile, overridable", () => {
  const units = new Units();
  assert.equal(units.relation(1, 1), 1);
  assert.equal(units.relation(1, 2), -1);
  assert.equal(units.relation(0, 2), 0);
  assert.equal(units.relation(1, 3), -1);
  units.setRelation(3, 1, 1);
  assert.equal(units.relation(1, 3), 1);
});

test("segment-sphere entry distance", () => {
  const o = new THREE.Vector3(0, 0, 0);
  const d = new THREE.Vector3(0, 0, 1);
  assert.equal(segmentSphere(o, d, 100, new THREE.Vector3(0, 0, 10), 1), 9);
  assert.equal(segmentSphere(o, d, 5, new THREE.Vector3(0, 0, 10), 1), null);
  assert.equal(segmentSphere(o, d, 100, new THREE.Vector3(0, 0, -10), 1), null);
  assert.equal(segmentSphere(o, d, 100, new THREE.Vector3(0.5, 0, 0), 1), 0); // starts inside
});

test("raycast finds the nearest unit, skipping the excluded and filtered", () => {
  const units = new Units();
  const shooter = units.add(fakeUnit(1, 0, 1, 0));
  const friend = units.add(fakeUnit(1, 0, 1, 5));
  const enemy = units.add(fakeUnit(2, 0, 1, 10));
  const o = new THREE.Vector3(0, 1, 0);
  const d = new THREE.Vector3(0, 0, 1);
  assert.equal(units.raycast(o, d, 50, shooter).unit, friend);
  const hostile = (u) => units.relation(1, u.team) < 0;
  assert.equal(units.raycast(o, d, 50, shooter, undefined, hostile).unit, enemy);
  assert.equal(units.raycast(o, d, 50, [shooter, friend]).unit, enemy);
  assert.equal(units.raycast(o, d, 8, [shooter, friend]), null);
  const hit = units.raycast(o, d, 50, [shooter, friend]);
  assert.ok(Math.abs(hit.distance - 9.4) < 1e-9);
  assert.ok(unitsClear(units, o, new THREE.Vector3(0, 1, 20), [shooter, friend, enemy]));
  assert.ok(!unitsClear(units, o, new THREE.Vector3(0, 1, 20), [shooter, friend]));
});

test("query returns living units whose sphere meets the ball", () => {
  const units = new Units();
  const a = units.add(fakeUnit(1, 0, 0, 0));
  const b = units.add(fakeUnit(2, 10, 0, 0));
  units.add(fakeUnit(2, 30, 0, 0));
  b.alive = true;
  const got = units.query(new THREE.Vector3(0, 0, 0), 9.5);
  assert.deepEqual(got.map((u) => u.id), [a.id, b.id]);
  a.alive = false;
  assert.deepEqual(units.query(new THREE.Vector3(0, 0, 0), 9.5).map((u) => u.id), [b.id]);
});

test("event journal: snapshots, independent readers, overflow accounting", () => {
  const journal = new SimEvents(4);
  const early = journal.reader();
  const p = new THREE.Vector3(1, 2, 3);
  journal.emit("shot", { position: p, team: 1 });
  p.set(9, 9, 9);
  const seen = [];
  early.read((e) => seen.push(e));
  assert.deepEqual(seen[0].position, [1, 2, 3]);
  const late = journal.reader();
  for (let i = 0; i < 6; i++) journal.emit("damage", { amount: i });
  const got = [];
  early.read((e) => got.push(e.amount));
  assert.deepEqual(got, [2, 3, 4, 5]);
  assert.equal(early.dropped, 2);
  const lateGot = [];
  late.read((e) => lateGot.push(e.amount));
  assert.deepEqual(lateGot, [2, 3, 4, 5]);
});

test("battle: characters include the human; bleed and loss", () => {
  const posts = { posts: [{ team: 1, desc: { valueBleed: 10 } }], owned: (t) => posts.posts.filter((p) => p.team === t) };
  const teams = { 1: { units: 3, reinforcements: 5, bleed: [[31, 0], [1, 4]] }, 2: { units: 2, reinforcements: 1, bleed: [[1, 4]] } };
  let lost = null;
  const battle = new Battle({ teams, posts, own: 1, foe: 2, onOver: (t) => (lost = t) });
  const characters = battle.populate(1);
  assert.equal(characters.length, 5);
  assert.equal(characters.filter((c) => c.human).length, 1);
  assert.equal(battle.human.team, 1);
  battle.spend(2);
  battle.update(0.1);
  assert.equal(lost, 2);
  assert.ok(battle.over);
});

test("classic random: the double LCG step and float construction", () => {
  const r = new ClassicRandom(0);
  // Two steps of x*1664525+1013904223 from 0.
  const a = 1013904223 >>> 0;
  const b = (Math.imul(a, 1664525) + 1013904223) >>> 0;
  assert.equal(r.next(), ((b & 0xffff0000) | (a >>> 16)) >>> 0);
  assert.equal(r.state, b);
  const u = new ClassicRandom(12345);
  for (let i = 0; i < 1000; i++) {
    const x = u.uniform();
    assert.ok(x >= 0 && x < 1);
    assert.equal(Math.fround(x), x);
  }
  const ints = new ClassicRandom(7);
  for (let i = 0; i < 1000; i++) {
    const n = ints.int(3, 8);
    assert.ok(Number.isInteger(n) && n >= 3 && n <= 8);
  }
  assert.equal(new ClassicRandom(1).int(5, 5), 5);
  assert.equal(new ClassicRandom(1).int(5, 2), 5);
});

test("battle team update: one debit per update, no-post countdown pauses, resets, expires", () => {
  const posts = { posts: [{ team: 2, desc: { valueBleed: 10 } }, { team: 0, desc: {} }], owned: (t) => posts.posts.filter((p) => p.team === t) };
  const teams = { 1: { units: 1, reinforcements: 100, bleed: [[1, 3], [31, 0]] }, 2: { units: 1, reinforcements: 100, bleed: [[31, 0]] }, 3: { units: 2 } };
  const battle = new Battle({ teams, posts, own: 1, foe: 2 });
  assert.equal(battle.reinforcements[3], 2147483647);
  // Team 1 holds nothing; one neutral post scores 1, rate 3: a big step debits once.
  battle.update(10);
  assert.equal(battle.reinforcements[1], 99);
  assert.ok(battle.bleedClock[1] >= 4); // the rest of the debt stays
  // A neutral post pauses the no-post count.
  assert.equal(battle.noPost[1], 20);
  posts.posts[1].team = 2; // no neutral left: team 1 counts down
  battle.update(1);
  assert.equal(battle.noPost[1], 19);
  assert.ok(battle.losing);
  assert.equal(battle.countdown, 19);
  posts.posts[1].team = 1; // holding a post resets it
  battle.update(0.5);
  assert.equal(battle.noPost[1], 20);
  posts.posts[1].team = 2;
  battle.update(20); // expires at <= 0: tickets and the starting count go
  assert.equal(battle.reinforcements[1], 0);
  assert.equal(battle.start[1], 0);
  assert.ok(battle.over);
  assert.equal(battle.loser, 1);
});

test("cylinder and box entry distances", () => {
  const o = new THREE.Vector3(0, 1, -5);
  const d = new THREE.Vector3(0, 0, 1);
  // Upright cylinder r 0.35, 1.55 tall, centred at y 0.8.
  const c = new THREE.Vector3(0, 0.8, 0);
  assert.ok(Math.abs(segmentCylinder(o, d, 10, c, 0.35, 0.775) - 4.65) < 1e-9);
  assert.equal(segmentCylinder(new THREE.Vector3(0, 2, -5), d, 10, c, 0.35, 0.775), null); // over its top
  assert.equal(segmentCylinder(o, d, 4, c, 0.35, 0.775), null); // short of it
  const down = new THREE.Vector3(0, -1, 0);
  assert.ok(Math.abs(segmentCylinder(new THREE.Vector3(0.1, 5, 0), down, 10, c, 0.35, 0.775) - (5 - 1.575)) < 1e-9); // through the cap
  // A prone box turned a quarter: its long axis along x.
  const half = new THREE.Vector3(0.25, 0.2, 0.87);
  const center = new THREE.Vector3(0, 0.2, 0);
  const across = new THREE.Vector3(-5, 0.2, 0);
  const east = new THREE.Vector3(1, 0, 0);
  assert.ok(Math.abs(segmentBox(across, east, 10, center, half, Math.PI / 2) - (5 - 0.87)) < 1e-9);
  assert.ok(Math.abs(segmentBox(across, east, 10, center, half, 0) - (5 - 0.25)) < 1e-9);
  assert.equal(segmentBox(new THREE.Vector3(-5, 1, 0), east, 10, center, half, 0), null);
});
