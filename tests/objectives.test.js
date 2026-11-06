// Destructible command posts (docs/objectives.md): Hoth's shield generator,
// Geonosis' Techno Union ships and forward command center, Endor's bunker
// control panel. The building (src/props.js ObjectivePost) takes damage
// from anyone and dies for good; its post (src/commandposts.js) then leaves
// the battle; its KillRegion goes off once (src/objectives.js); the Info
// voice-over asks for its line while it stands (rust/audio-control/src/director.rs).
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

import {ObjectiveTarget} from "../src/objectives.js";
import { ObjectivePost } from "../src/props.js";

const IDENTITY = new THREE.Matrix4().toArray();

// A building as LiveObjects makes one, without a scene: looks are bare rigs.
function building({ health = 50000, animated = false } = {}) {
  const rig = () => ({ root: new THREE.Group(), bones: [], byName: {}, model: { box: [[-5, 0, -5], [5, 10, 5]] } });
  const looks = { built: rig(), destroyed: animated ? null : rig(), unbuilt: null };
  const desc = {
    class: "hoth_bldg_shieldgenerator", base: "commandpost", model: "hoth_bldg_shieldgenerator", post: "shield", matrix: IDENTITY, team: 2,
    settings: { MaxHealth: String(health) }, ...(animated ? { fx: { dying: { span: 2, effects: [], interval: 0.4, final: [] } } } : {}),
  };
  const b = new ObjectivePost(desc, looks, {}, null, null);
  b.show(b.state);
  return b;
}


test("bots: an enemy objective on the front is drawn three times as often, ranks first for those sent to it, and is no repair job", async () => {
  const { selectObjective, OBJECTIVE_ATTACK_BIAS } = await import("../src/ai/classic/goals.js");
  const { targetPriority, OBJECTIVE_PRIORITY, matchupPriority } = await import("../src/ai/classic/perception.js");
  const { createServiceCandidates } = await import("../src/ai/services.js");
  const ai = { atk: [10, 10, 10, 10, 10, 10], def: [10, 10, 10, 10, 10, 10], spawnPath: null, classSpawnPoints: 0, bans: 0 };
  const b = building();
  const shield = { name: "shield", team: 2, active: true, position: new THREE.Vector3(), ai, building: b };
  const cp5 = { name: "cp5", team: 2, active: true, position: new THREE.Vector3(100, 0, 0), ai };
  // Team 1 spawns at `base`.
  const base = { name: "base", team: 1, active: true, position: new THREE.Vector3(), ai: { ...ai, spawnPath: { present: true } } };
  const relation = (a, c) => (a === c ? 1 : -1);
  // Draws spread evenly over [0, total).
  const draw = (at) => {
    base.position.set(at, 0, 0);
    const picks = { shield: 0, cp5: 0 };
    for (let i = 0; i < 400; i++) {
      const random = { uniform: () => (i + 0.5) / 400, int: () => 0, float: (lo, hi) => lo + ((i + 0.5) / 400) * (hi - lo) };
      const post = selectObjective({ posts: [shield, cp5, base], team: 1, faction: 2, random, relation });
      if (post) picks[post.name] += 1;
    }
    return picks;
  };
  assert.equal(OBJECTIVE_ATTACK_BIAS, 3);
  // The shield nearest the team's spawn: it takes 3 of every 4.
  let picks = draw(-50);
  assert.ok(picks.shield > picks.cp5 * 2, JSON.stringify(picks));
  // cp5 in front of it: the authored weights, one each.
  picks = draw(150);
  assert.equal(picks.shield, picks.cp5, JSON.stringify(picks));
  // No post to spawn at: no front either.
  base.active = false;
  picks = draw(-50);
  assert.equal(picks.shield, picks.cp5, JSON.stringify(picks));
  b.post = shield;
  const target = new ObjectiveTarget(b);
  const soldier = { kind: 0 };
  assert.equal(targetPriority(soldier, target, { objective: shield }), OBJECTIVE_PRIORITY);
  assert.equal(targetPriority(soldier, target, { objective: cp5 }), matchupPriority(0, 9));
  const candidates = createServiceCandidates({ all: () => [] }, { buildings: [b] }, null);
  assert.deepEqual(candidates(new THREE.Vector3(), 1000), []);
});
