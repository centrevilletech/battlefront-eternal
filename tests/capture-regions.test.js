// World regions as the original game builds and tests them (src/regions.js):
// a sphere's radius is the length of its SIZE; a box's SIZE is half extents
// and a cylinder's radius sqrt(x^2 + z^2) with y its half height, both scaled
// by the transform's axis lengths. Tatooine: Dune Sea's HOMESTEAD is a sphere
// sized 0.1 x 8.15 x 0.1, which a first-value radius made a 10 cm target no
// one could stand in; its sarlacc pit (Sarlac01, 2.14 x 6.06 x 8.16) kills
// 8.43 m out, not 2.14.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CommandPosts } from "../src/commandposts.js";
import { killRegion } from "../src/objectives.js";
import { buildRegion, inRegion } from "../src/regions.js";
import { objectiveOffset } from "../src/ai/classic/goals.js";

const at = (x, y, z) => new THREE.Matrix4().makeTranslation(x, y, z).toArray();
const v = (x, y, z) => new THREE.Vector3(x, y, z);

function posts(capture, team = 2) {
  const map = {
    commandPost: { neutralizeTime: 12, captureTime: 10, holoTurnOnTime: 2.2 },
    commandPosts: [{ name: "cp2", label: "CP2", team, matrix: at(0, 0, 0), hologram: [0, 1, 0], valueBleed: 10, capture: { matrix: at(0, 0.4, 0), ...capture } }],
  };
  return new CommandPosts(null, map, { headless: true, playerTeam: 1 });
}

test("a sphere's radius is the length of its SIZE, whatever its transform", () => {
  const [post] = posts({ type: "sphere", size: [0.1, 8.15, 0.1] }).posts;
  const scratch = new THREE.Vector3();
  assert.equal(post.contains(v(1, 0, 1), scratch), true);
  assert.equal(post.contains(v(5, 0, -5), scratch), true);
  assert.equal(post.contains(v(0, 0.4 + 8.1, 0), scratch), true);
  assert.equal(post.contains(v(8.2, 0.4, 0), scratch), false);
  const scaled = buildRegion({ type: "sphere", size: [0.1, 8.15, 0.1], matrix: new THREE.Matrix4().makeScale(3, 3, 3).toArray() });
  assert.equal(scaled.radius.toFixed(3), Math.hypot(0.1, 8.15, 0.1).toFixed(3));
  assert.equal(inRegion(scaled, v(8.2, 0, 0)), false);
});

test("a cylinder's radius is the length of its SIZE's x and z, its half height y", () => {
  const [post] = posts({ type: "cylinder", size: [0.1, 5, 16] }).posts;
  const scratch = new THREE.Vector3();
  assert.equal(post.contains(v(15, 0, 0), scratch), true);
  assert.equal(post.contains(v(0, 5.3, 0), scratch), true);
  assert.equal(post.contains(v(0, 5.5, 0), scratch), false);
  assert.equal(post.contains(v(16.1, 0, 0), scratch), false);
  // Tatooine's sarlacc pit.
  const pit = buildRegion({ type: "cylinder", size: [2.135, 6.056, 8.158], matrix: at(-261, 3.7, 92.6) });
  assert.equal(pit.radius.toFixed(2), "8.43");
  assert.deepEqual(pit.half.map((x) => x.toFixed(2)), ["8.43", "6.06", "8.43"]);
  assert.equal(inRegion(pit, v(-256, 5, 92.6)), true);
  assert.equal(inRegion(pit, v(-251, 5, 92.6)), false);
  assert.equal(inRegion(pit, v(-261, 10, 92.6)), false);
});

test("box and cylinder sizes scale with the transform's axes; the axes are unit for the test", () => {
  const turn = new THREE.Matrix4().makeRotationY(Math.PI / 2);
  const matrix = new THREE.Matrix4().makeTranslation(10, 0, 0).multiply(turn).multiply(new THREE.Matrix4().makeScale(2, 1, 1));
  const box = buildRegion({ type: "box", size: [3, 1, 1], matrix: matrix.toArray() });
  assert.deepEqual(box.half, [6, 1, 1]);
  // Local x (6 m) is world -z after the quarter turn.
  assert.equal(inRegion(box, v(10, 0, 5.5)), true);
  assert.equal(inRegion(box, v(10, 0, 6.5)), false);
  assert.equal(inRegion(box, v(11.5, 0, 0)), false);
  const tilted = new THREE.Matrix4().makeRotationZ(Math.PI / 2).multiply(new THREE.Matrix4().makeScale(1, 2, 1));
  const cylinder = buildRegion({ type: "cylinder", size: [3, 2, 4], matrix: tilted.toArray() });
  assert.deepEqual(cylinder.half, [5, 4, 5]);
  // Its axis is world x after the tilt: 4 m half length, 5 m radius.
  assert.equal(inRegion(cylinder, v(3.9, 4.9, 0)), true);
  assert.equal(inRegion(cylinder, v(4.1, 0, 0)), false);
  assert.equal(inRegion(cylinder, v(0, 5.1, 0)), false);
});

test("a kill region uses the same test", () => {
  const region = { type: "cylinder", size: [0.1, 3, 6], matrix: at(0, 0, 0) };
  const victims = [v(5.9, 0, 0), v(6.1, 0, 0)].map((position) => ({ position, dead: false, kill() { this.dead = true; } }));
  assert.equal(killRegion(region, { random: () => 0.5, victims: () => victims }), 1);
  assert.deepEqual(victims.map((victim) => victim.dead), [true, false]);
});

test("the AI's objective offset spans the built region: a cylinder's radius on both axes", () => {
  const post = { name: "cp1", team: 2, position: v(0, 0, 0), desc: { capture: { type: "cylinder", size: [0.1, 5, 16], matrix: at(0, 0, 0) } } };
  let widest = 0;
  for (let i = 0; i <= 20; i++) widest = Math.max(widest, Math.abs(objectiveOffset(post, { uniform: () => i / 20 }).x));
  assert.ok(widest > 15, `x reached ${widest}`);
});

test("a soldier standing in a hand-sized sphere neutralizes and captures the post", () => {
  const system = posts({ type: "sphere", size: [0.1, 8.15, 0.1] });
  const [post] = system.posts;
  const soldier = { position: v(1, 0, 1), team: 1 };
  const events = [];
  for (let i = 0; i < 60 * 23; i++) {
    system.update(1 / 60, [soldier]);
    events.push(...system.events.splice(0).map((event) => event.kind));
  }
  assert.equal(post.team, 1);
  assert.ok(events.length >= 2, `events: ${events}`);
});
