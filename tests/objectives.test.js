// Destructible command posts (docs/objectives.md): Hoth's shield generator,
// Geonosis' Techno Union ships and forward command center, Endor's bunker
// control panel. The building (src/props.js ObjectivePost) takes damage
// from anyone and dies for good; its post (src/commandposts.js) then leaves
// the battle; its KillRegion goes off once (src/objectives.js); the Info
// voice-over asks for its line while it stands (src/audio/vo.js).
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CommandPosts } from "../src/commandposts.js";
import { Battle } from "../src/battle.js";
import { ObjectivePost } from "../src/props.js";
import { KILL_REGION_BLASTS, ObjectiveTarget, killRegion } from "../src/objectives.js";
import { VoiceOver } from "../src/audio/vo.js";
import { friendlySpawn } from "../src/ai/classic/goals.js";

const IDENTITY = new THREE.Matrix4().toArray();
const at = (x, y, z) => new THREE.Matrix4().makeTranslation(x, y, z).toArray();

function map() {
  return {
    commandPost: { neutralizeTime: 12, captureTime: 10, holoTurnOnTime: 2.2 },
    mission: { teams: { 1: { name: "Empire" }, 2: { name: "Alliance" } } },
    commandPosts: [
      { name: "cp5", label: "CP5", team: 2, matrix: at(100, 0, 0), hologram: [100, 2, 0], valueBleed: 10, spawnPath: "CP5_SpawnPath",
        capture: { type: "box", size: [5, 5, 5], matrix: at(100, 0, 0) } },
      { name: "shield", label: "CP2", displayName: "Shield Generator", team: 2, matrix: IDENTITY, hologram: [0, 2, 0], valueBleed: 10,
        spawnPath: "CP2_SpawnPath", capture: null, maxHealth: 50000, mapTexture: "hud_dest_icon", mapScale: 3,
        killRegion: { name: "CP2_Death", type: "box", matrix: at(0, 0, 0), size: [10, 5, 10] } },
    ],
  };
}

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

test("a destructible post's building takes damage, never regenerates, and leaves a wreck no one rebuilds", () => {
  const b = building();
  assert.equal(b.alive, true);
  assert.equal(b.healthType, "building");
  b.damage(20000, "player");
  assert.equal(b.health, 30000);
  assert.equal(b.lastAttacker, "player");
  b.regenerate(60);
  assert.equal(b.health, 30000);
  let exploded = 0;
  b.onExplode = () => { exploded += 1; };
  b.damage(Infinity);
  assert.equal(b.alive, false);
  assert.equal(b.state, "destroyed");
  assert.equal(exploded, 0, "no ExplosionName on this one");
  assert.equal(b.healthType, null, "the wreck takes no BuildingRebuild");
  b.regenerate(600);
  assert.equal(b.state, "destroyed");
});

test("an animated one is drawn through its death animation, then gone", () => {
  const b = building({ animated: true });
  b.damage(Infinity);
  assert.equal(b.state, "destroyed");
  assert.equal(b.dying, 2);
  assert.equal(b.looks.built.root.visible, true, "the ship stays up while it dies");
  b.present(1.5);
  assert.equal(b.looks.built.root.visible, true);
  b.present(1);
  assert.equal(b.dying, 0);
  assert.equal(b.looks.built.root.visible, false);
});

test("the post goes inactive with its building: lost once, no capture, no spawns, no bleed, its team kept", () => {
  const posts = new CommandPosts(new THREE.Scene(), map(), { logos: new Map(), textures: {}, playerTeam: 1, headless: true });
  const b = building();
  posts.attachBuildings({ objectives: [b] });
  const shield = posts.posts[1];
  assert.equal(shield.building, b);
  assert.equal(b.post, shield);
  assert.equal(b.team, 2);
  const battle = new Battle({ teams: { 1: { reinforcements: 250 }, 2: { reinforcements: 250 } }, posts, own: 1, foe: 2 });
  assert.equal(battle.score(2), 20);
  b.damage(Infinity);
  posts.update(1 / 60, []);
  const events = posts.events.splice(0);
  assert.deepEqual(events.map((e) => [e.kind, e.post.name, e.from, e.destroyed]), [["lost", "shield", 2, true]]);
  assert.equal(shield.active, false);
  assert.equal(shield.team, 2);
  assert.equal(battle.score(2), 10, "only cp5 counts now");
  assert.deepEqual(posts.owned(2).map((p) => p.name), ["cp5"]);
  assert.equal(friendlySpawn(Object.assign(shield, { ai: { spawnPath: { present: true } } }), 2, () => 1), false);
  posts.update(1 / 60, []);
  assert.equal(posts.events.length, 0, "announced once");
});

test("losing every other post after the objective starts the no-post countdown", () => {
  const posts = new CommandPosts(new THREE.Scene(), map(), { logos: new Map(), textures: {}, playerTeam: 1, headless: true });
  const b = building();
  posts.attachBuildings({ objectives: [b] });
  posts.posts[0].team = 1;
  b.damage(Infinity);
  posts.update(1 / 60, []);
  const battle = new Battle({ teams: { 1: { reinforcements: 250 }, 2: { reinforcements: 250 } }, posts, own: 1, foe: 2 });
  battle.update(1);
  assert.equal(battle.score(2), 0);
  assert.equal(battle.noPost[2], 19);
});

test("the kill region: 15 effects through its box, then everyone inside dies", () => {
  const region = { type: "box", matrix: at(10, 0, 0), size: [4, 2, 4] };
  const played = [];
  let r = 0;
  const random = () => ((r = (r * 9301 + 49297) % 233280) / 233280);
  const killed = [];
  const victim = (name, x) => ({ position: new THREE.Vector3(x, 0, 0), kill: () => killed.push(name) });
  const n = killRegion(region, {
    random, play: (name, point) => played.push([name, point]),
    victims: () => [victim("inside", 11), victim("outside", 20), victim("edge", 13.9)],
  });
  assert.equal(played.length, KILL_REGION_BLASTS);
  for (const [name, point] of played) {
    assert.ok(["explosion", "med_explosion", "small_explosion"].includes(name));
    assert.ok(Math.abs(point.x - 10) <= 4 && Math.abs(point.y) <= 2 && Math.abs(point.z) <= 4);
  }
  assert.deepEqual(killed, ["inside", "edge"]);
  assert.equal(n, 2);
  assert.equal(killRegion(null, { play: () => assert.fail() }), 0);
});

test("the objective is a building target for its enemies while it stands", () => {
  const b = building();
  const target = new ObjectiveTarget(b);
  assert.equal(target.kind, 9);
  assert.equal(target.category, 4);
  assert.equal(target.team, 2);
  assert.equal(target.alive, true);
  assert.equal(target.combat.maxHealth, 50000);
  assert.ok(target.sphere.radius > 5);
  b.damage(Infinity);
  assert.equal(target.alive, false);
});

test("the Info line is asked for while the post stands, by the listener's side and the owner's", () => {
  const queued = [];
  const posts = { posts: [{ desc: { name: "shield" }, team: 2, active: true, building: { alive: true } }] };
  const audio = {
    streams: { queue: (name) => queued.push(name) },
    game: { battle: { over: false, bleed: {}, human: { unit: {} } }, posts },
  };
  const world = { posts: { shield: { vo: { VO_Imp_AllInfo: "imp_impdestroy_shieldgenerator", VO_All_AllInfo: "all_alldefend_shieldgenerator" } } } };
  const vo = new VoiceOver(audio, {}, world, 1, { 1: "Imp", 2: "All" });
  vo.update(1 / 60);
  assert.deepEqual(queued, ["imp_impdestroy_shieldgenerator"]);
  posts.posts[0].active = false;
  vo.update(1 / 60);
  assert.equal(queued.length, 1);
});

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
