// The posts' AI records (src/ai/classic/goals.js attachPostAI): an unnamed
// post (Hoth's probe droid drop, "#5" to the network) takes the record
// standing where it stands, with its own weights, not the defaults.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { attachPostAI, postWeight } from "../src/ai/classic/goals.js";

const record = (name, position, atk, def = atk) => ({ name, position, values: { atk, def }, bans: 0, valueBleed: 10, spawnPath: null, allyPath: null });

test("an unnamed post takes the record at its place, by its world name not its runtime one", () => {
  const posts = [
    { name: "cp5", desc: { name: "cp5" }, position: new THREE.Vector3(1, 0, 0), team: 2 },
    { name: "#1", desc: { name: "" }, position: new THREE.Vector3(251.8, -44.9, 83.5), team: 1 },
  ];
  const mission = { posts: [record("cp5", [1, 0, 0], [100, 10, 10, 10, 10, 10]), record("", [251.8, -44.9, 83.5], [100, 0, 0, 0, 0, 100], [100, 0, 0, 0, 0, 0])] };
  attachPostAI(posts, mission, {});
  assert.equal(postWeight(posts[0], 1, 2), 10);
  assert.equal(postWeight(posts[1], 1, 2), 0, "the Empire is never sent to the probe droid drop");
  assert.equal(postWeight(posts[1], 3, 5), 0);
});
