// Page post presentation derives its view from replicated hull facts.
// Native lifecycle and notices run in the f4-mobile phase fixture.
// Command vehicles as command posts (src/mobile-posts.js): the AT-AT and
// AT-TE are posts while they live, owned by their hull's team, spawn
// soldiers at their class's spawn points as the hull stands now, count for
// the bleed and leave the list when they die. Synthetic hulls throughout.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { MobilePosts, commandSpawnPoint, isCommandClass } from "../src/mobile-posts.js";
import { CommandPosts } from "../src/commandposts.js";

import { friendlySpawn } from "../src/spawn-post-policy.js";
import { choosePost, eligiblePosts, spawnPointAt } from "../src/human-spawn.js";
import { entityPostNames, entitySpawnPosts } from "../src/runtime/entity-spawn-posts.js";

const UP = new THREE.Vector3(0, 1, 0);
// The AT-AT's six (SpawnPointCount 6, SpawnPointLocation x y z yawDeg), as
// ai-mission.json commandClasses carries them.
const ATAT_POINTS = [[8, 0, 0, 5], [8, 0, 4, 5], [8, 0, -4, 5], [-8, 0, 0, 355], [-8, 0, 4, 355], [-8, 0, -4, 355]];
const CLASSES = {
  imp_walk_atat: { base: "commandwalker", valueBleed: 10, values: { atk: [100, 10, 10, 10, 10, 0], def: [100, 0, 0, 0, 0, 0] }, spawnPoints: ATAT_POINTS },
};

// A hull as src/vehicles.js Vehicle keeps one: yaw-only matrix at its feet.
function hull({ name = "imp_walk_atat", base = "commandwalker", team = 1, yaw = 0, at = [0, 0, 0], seats = 2 } = {}) {
  const v = {
    name, label: name === "imp_walk_atat" ? "AT-AT" : name, team, alive: true, yaw,
    def: { base, settings: { ValueBleed: "10" } },
    position: new THREE.Vector3(...at), matrix: new THREE.Matrix4(),
    center: new THREE.Vector3(0, 10, 2.9), size: new THREE.Vector3(8.1, 20, 20.9), radius: 10.5, speed: 2,
    seats: Array.from({ length: seats }, () => ({ lease: null })), aiTarget: { generation: 1 }, body: { moving: true },
    get controllingTeam() {
      for (const s of this.seats) if (s.lease) return s.lease.team;
      return this.team;
    },
    place() {
      this.matrix.compose(this.position, new THREE.Quaternion().setFromAxisAngle(UP, this.yaw), new THREE.Vector3(1, 1, 1));
    },
    respawn() {
      this.alive = true;
      this.aiTarget.generation += 1;
      for (const s of this.seats) s.lease = null;
    },
  };
  v.place();
  return v;
}

// Vehicles' slots: one pad per hull, the hull its only variant.
function system(vehicles, { collision = null } = {}) {
  const slots = vehicles.map((v) => ({ desc: { classes: { [v.team]: v.name } }, variants: new Map([[v.name, v]]), vehicle: v }));
  vehicles.forEach((v, i) => (v.slot = slots[i]));
  return { slots, vehicles, collision, data: { classes: Object.fromEntries(vehicles.map((v) => [v.name, v.def])) } };
}

const relation = (a, b) => (a === 0 || b === 0 ? 0 : a === b ? 1 : -1);
const near = (a, b, eps = 1e-6) => a.every((x, i) => Math.abs(x - b[i]) < eps);

// A placed post as map.json gives one.
const placed = (name, team, x = 0) => ({
  name, label: name, displayName: name, team, matrix: new THREE.Matrix4().makeTranslation(x, 0, 0).toArray(), hologram: [x, 0, 0],
  capture: { type: "sphere", size: [5, 5, 5], matrix: new THREE.Matrix4().makeTranslation(x, 0, 0).toArray() },
  spawnPath: `${name}Spawn`, spawnPoints: [[x, 0, 1, 0]], valueBleed: 10,
});

test("only the command classes are posts", () => {
  assert.equal(isCommandClass({ base: "commandwalker" }), true);
  assert.equal(isCommandClass({ base: "commandhover" }), true);
  assert.equal(isCommandClass({ base: "walker" }), false, "the AT-ST is no post");
  assert.equal(isCommandClass({ base: "hover" }), false);
  const atat = hull({ at: [10, 0, 0] });
  const tank = hull({ name: "imp_hover_fightertank", base: "hover" });
  const mobile = new MobilePosts(system([tank, atat]), CLASSES);
  assert.deepEqual(mobile.posts.map((p) => p.name), ["transport-1"], "named by its pad, as the registry reserves");
  const post = mobile.posts[0];
  assert.equal(post.label, "AT-AT");
  assert.equal(post.team, 1);
  assert.equal(post.active, true);
  assert.equal(post.position, atat.position, "where the hull is now");
  assert.equal(post.ai.classSpawnPoints, 6);
  assert.equal(post.ai.valueBleed, 10);
  assert.equal(post.desc.valueBleed, 10);
  assert.deepEqual(post.ai.def, [100, 0, 0, 0, 0, 0]);
  assert.equal(post.ai.capture, null, "no capture region: it never changes hands by capture");
  assert.ok(post.speed > 0, "a post that moves (the AI's wider arrival)");
  assert.ok(Math.abs(post.radius - atat.size.length() / 2) < 1e-9);
});

test("a dead hull is no post; its pad's next one is", () => {
  const atat = hull({ at: [10, 0, 5] });
  const mobile = new MobilePosts(system([atat]), CLASSES);
  const post = mobile.posts[0];
  atat.alive = false;
  mobile.presentEvents();
  assert.equal(post.active, false);
  assert.deepEqual(post.position.toArray(), [10, 0, 5], "it stays where its hull died");
  assert.deepEqual(post.ai.classSpawnWorld, []);
  assert.equal(friendlySpawn(post, 1, relation), false);
  atat.position.set(0, 0, 0);
  atat.respawn();
  mobile.presentEvents();
  assert.equal(post.active, true);
  assert.equal(friendlySpawn(post, 1, relation), true);
});

test("its owner is whoever took a seat last, and stays his when he leaves", () => {
  const atat = hull();
  const mobile = new MobilePosts(system([atat]), CLASSES);
  const post = mobile.posts[0];
  atat.seats[0].lease = { team: 2, released: false };
  mobile.presentEvents();
  assert.equal(post.team, 2, "the occupant's team");
  atat.seats[0].lease = null;
  mobile.presentEvents();
  assert.equal(post.team, 2, "a command class keeps it when it empties");
  assert.equal(friendlySpawn(post, 2, relation), true);
  assert.equal(friendlySpawn(post, 1, relation), false);
  atat.alive = false;
  mobile.presentEvents();
  atat.respawn();
  mobile.presentEvents();
  assert.equal(post.team, 1, "a new hull has its pad's team");
});

test("spawn points turn and move with the hull", () => {
  // Hoth's second AT-AT pad faces -x (yaw -90 degrees).
  const atat = hull({ yaw: -Math.PI / 2, at: [158.172, -5.636, 119.584] });
  const mobile = new MobilePosts(system([atat]), CLASSES);
  const points = mobile.posts[0].ai.classSpawnWorld;
  assert.equal(points.length, 6);
  // Local +x is the hull's right: x = cos(yaw), z = -sin(yaw).
  assert.ok(near(points[0], [158.172, -5.636, 127.584, -Math.PI / 2 + (5 * Math.PI) / 180]));
  assert.ok(near(points[1], [154.172, -5.636, 127.584, -Math.PI / 2 + (5 * Math.PI) / 180]));
  assert.ok(near(points[3], [158.172, -5.636, 111.584, -Math.PI / 2 + (355 * Math.PI) / 180]));
  // The hull walks on: so do its points.
  atat.position.x -= 20;
  atat.yaw = 0;
  atat.place();
  mobile.presentEvents();
  assert.ok(near(mobile.posts[0].ai.classSpawnWorld[0], [146.172, -5.636, 119.584, (5 * Math.PI) / 180]));
});

test("a spawn point comes back toward the hull to the first static surface", () => {
  const calls = [];
  const collision = {
    cast(from, direction, far, out, ignore, staticOnly) {
      calls.push({ from: from.toArray(), direction: direction.toArray(), far, ignore, staticOnly });
      // A snowbank 1 m up the side the point is on: the ray from the hull's
      // centre meets it three quarters of the way.
      out.point.copy(from).addScaledVector(direction, far * 0.75);
      return out;
    },
  };
  const atat = hull({ at: [0, 0, 0] });
  const [x, y, z] = commandSpawnPoint(atat, [8, 0, 0, 5], collision);
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].from, [0, 10, 2.9], "from the hull's collision centre");
  assert.equal(calls[0].ignore, atat.body, "the hull itself left out");
  assert.equal(calls[0].staticOnly, true, "terrain, buildings and props (mask 0x48), no hulls");
  assert.ok(Math.abs(calls[0].far - Math.hypot(8, 10, 2.9)) < 1e-9);
  assert.ok(near([x, y, z], [6, 2.5, 0.725]));
  // Nothing in the way: the point itself.
  const clear = { cast: () => null };
  assert.ok(near(commandSpawnPoint(atat, [8, 0, 0, 5], clear).slice(0, 3), [8, 0, 0]));
});

test("events: a post lost with its hull, a post taken with a seat", () => {
  const a = hull({ at: [0, 0, 0] });
  const b = hull({ at: [50, 0, 0] });
  const mobile = new MobilePosts(system([a, b]), CLASSES);
  assert.deepEqual(mobile.presentEvents(), [], "nothing at the start");
  b.seats[1].lease = { team: 2, released: false };
  a.alive = false;
  const events = mobile.presentEvents();
  assert.deepEqual(events.map((e) => [e.kind, e.post.name, e.from, e.to]), [["lost", "transport-0", 1, undefined], ["owner", "transport-1", 1, 2]]);
  assert.equal(events[0].vehicle, a);
  assert.deepEqual(mobile.presentEvents(), [], "each change once");
});

test("the human's spawn screen offers it, keeps it, and gives it up when it dies", () => {
  const atat = hull({ at: [200, 0, 0] });
  const map = { commandPosts: [placed("cp6", 1, 0), placed("cp5", 2, -100)] };
  const posts = new CommandPosts(null, map, { logos: new Map(), textures: {}, playerTeam: 1, headless: true });
  const vehicles = system([atat]);
  posts.attachVehicles(vehicles, CLASSES);
  const game = { posts, vehicles };
  assert.deepEqual(eligiblePosts(entitySpawnPosts(game), 1, relation).map((p) => p.name), ["cp6", "transport-0"]);
  assert.deepEqual(eligiblePosts(entitySpawnPosts(game), 2, relation).map((p) => p.name), ["cp5"]);
  assert.ok(entityPostNames(game).includes("transport-0"), "the registry already reserves the pad's name");
  const post = posts.all()[2];
  assert.equal(choosePost(entitySpawnPosts(game), 1, post, relation), post, "an eligible pick is kept");
  const [x, y, z, yaw] = spawnPointAt(post, 1, () => 0.5);
  assert.ok(near([x, y, z, yaw], post.ai.classSpawnWorld[3]), "a class spawn point: floor(0.5 x 6)");
  atat.alive = false;
  posts.presentMobileEvents();
  const choices = entitySpawnPosts(game);
  assert.equal(choices.includes(post), false, "a dead hull is not offered");
  assert.equal(choosePost(choices, 1, choices.find((p) => p.name === post.name) || null, relation).name, "cp6", "the front line instead");
});
