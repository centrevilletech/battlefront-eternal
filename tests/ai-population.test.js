// Classic population: difficulty, spawning, class choice and objectives.
// Synthetic fixtures throughout; optional reference fixtures measured in the
// original game and the extracted public/maps/*/ai-mission.json are used when
// present (those tests skip otherwise).
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { ClassicRandom } from "../src/ai/random.js";
import {
  ClassicDifficulty,
  EASY,
  MEDIUM,
  HARD,
  curve,
  skillCurves,
  ANCHORS,
  drawRange,
  curvesFor,
  isHard,
} from "../src/ai/classic/difficulty.js";
import {
  ClassicSpawning,
  applyMission,
  appendHeroes,
  makeTeams,
  callsFromMapMission,
  RESPAWN_DELAY,
} from "../src/ai/classic/spawning.js";
import {
  ClassicObjectives,
  selectObjective,
  selectSpawnPost,
  friendlySpawn,
  postWeight,
  humanSpawnPost,
  objectiveOffset,
  objectiveArrived,
  objectiveCenter,
  missionControls,
  factionOf,
  attachPostAI,
  classicBinding,
  TRAVEL,
} from "../src/ai/classic/goals.js";

const { HUGE, HOVER, SOLDIER } = TRAVEL;

const f32 = Math.fround;
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const optional = (path) => {
  const file = ROOT + path;
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
};
const native8 = optional("extract/ai-research/round8/combat/native-fixtures.json");
const supplemental8 = optional("extract/ai-research/round8/combat/supplemental.json");
const spawn9 = optional("extract/ai-research/round9/spawn/results.json");

const clone = (random) => new ClassicRandom(random.state);
const close = (a, b, eps = 5e-7) => Math.abs(a - b) <= eps;

// ---------------------------------------------------------------- difficulty

test("tier skills are asymmetric: Easy 6/4, Medium 4/14, Hard 4/17 (player/enemy)", () => {
  for (const [tier, player, enemy] of [[EASY, 6, 4], [MEDIUM, 4, 14], [HARD, 4, 17]]) {
    const d = new ClassicDifficulty({ tier, playerTeam: 1 });
    assert.equal(d.playerSkill, player);
    assert.equal(d.enemySkill, enemy);
  }
  const odd = new ClassicDifficulty({ tier: 0 });
  assert.deepEqual([odd.playerSkill, odd.enemySkill], [4, 14], "an unknown tier falls through to 4/14");
});

test("curve table for every tier and side, in float32", () => {
  // Skill curves (docs/ai-population.md): rounded table values.
  const table = {
    4: [1.053846, 1.053846, 2.676923, 1.769231],
    6: [1.223077, 1.223077, 2.461538, 1.615385],
    14: [1.9, 1.9, 1.6, 1],
    17: [10.95, 2.15, 1.3, 0.5],
  };
  for (const tier of [EASY, MEDIUM, HARD]) {
    const d = new ClassicDifficulty({ tier, playerTeam: 2 });
    for (const [team, skill] of [[2, d.playerSkill], [1, d.enemySkill]]) {
      const c = d.curves(team);
      const want = table[skill];
      assert.ok(want, `skill ${skill} in table`);
      for (const [i, key] of ["A", "D", "B", "S"].entries()) {
        assert.equal(f32(c[key]), c[key], "float32 value");
        assert.ok(close(c[key], want[i], 1e-5), `tier ${tier} team ${team} ${key}: ${c[key]} vs ${want[i]}`);
      }
    }
  }
});

test("curves round each operation to float32 in the original game's order", () => {
  // At skill 4: t = f32(3/13), (hi - lo) = f32(1.9) - f32(0.8), etc.
  const a = f32(0.8);
  const b = f32(1.9);
  const t = f32(3 / 13);
  const expected = f32(a + f32(t * f32(b - a)));
  assert.equal(curve(ANCHORS.A, 4), expected);
  // Breakpoint: 14 sits on the second interval with t = 0.
  assert.equal(curve(ANCHORS.A, 14), f32(1.9));
  assert.equal(curve(ANCHORS.S, 20), 0);
  assert.equal(curve(ANCHORS.A, 20), 20);
});

test("all twenty skills match the original game's curve outputs bit for bit", { skip: !native8 && "native fixtures absent" }, () => {
  for (const row of native8.allSkillCurves) {
    const c = skillCurves(row.skill);
    assert.deepEqual([c.A, c.D, c.B, c.S], row.C1_C2_C3_C4, `skill ${row.skill}`);
  }
});

test("tier initialization offsets clamp independently; the override keeps skills but refreshes curves", () => {
  const d = new ClassicDifficulty({ tier: HARD });
  d.init(-100, 100);
  assert.deepEqual([d.playerSkill, d.enemySkill], [1, 20]);
  d.init(100, -100);
  assert.deepEqual([d.playerSkill, d.enemySkill], [20, 1]);
  const guarded = new ClassicDifficulty({ tier: EASY, override: true, skills: { player: 9, enemy: 11 } });
  guarded.init(100, -100);
  assert.deepEqual([guarded.playerSkill, guarded.enemySkill], [9, 11]);
  assert.equal(guarded.enemy.A, skillCurves(11).A);
  // The mission setters ignore the guard.
  guarded.apply([{ name: "SetEnemyTeamDifficulty", args: [3] }]);
  assert.equal(guarded.enemySkill, 3);
});

test("tier initializer matches the native cases", { skip: !native8 && "native fixtures absent" }, () => {
  for (const row of native8.networkTierInitializer) {
    const d = new ClassicDifficulty({ tier: row.tier });
    d.init(row.offsets[0], row.offsets[1]);
    assert.deepEqual([d.playerSkill, d.enemySkill], row.playerEnemy);
  }
});

test("mission setters apply in call order: absolute replaces, tier-conditional adds on its tier only", () => {
  const hard = () => new ClassicDifficulty({ tier: HARD });
  const a = hard().apply([
    { name: "SetEnemyTeamDifficulty", args: [10] },
    { name: "SetEnemyTeamDifficultyHard", args: [3] },
  ]);
  assert.equal(a.enemySkill, 13);
  const b = hard().apply([
    { name: "SetEnemyTeamDifficultyHard", args: [3] },
    { name: "SetEnemyTeamDifficulty", args: [10] },
  ]);
  assert.equal(b.enemySkill, 10, "order matters");
  const medium = new ClassicDifficulty({ tier: MEDIUM }).apply([["SetEnemyTeamDifficultyHard", [3]], ["SetPlayerTeamDifficultyEasy", [5]]]);
  assert.deepEqual([medium.playerSkill, medium.enemySkill], [4, 14], "other tiers' setters do nothing");
  const clamped = new ClassicDifficulty({ tier: EASY }).apply([["SetPlayerTeamDifficultyEasy", [14.9]], ["SetEnemyTeamDifficultyEasy", [-3]]]);
  assert.deepEqual([clamped.playerSkill, clamped.enemySkill], [20, 1], "arguments truncate; results clamp to 1..20");
  // Profile tier, not the network tier, gates the conditional setters.
  const net = new ClassicDifficulty({ tier: HARD, profileTier: EASY }).apply([["SetEnemyTeamDifficultyEasy", [2]]]);
  assert.equal(net.enemySkill, 19);
});

test("historical-style setter lists produce the expected table rows", () => {
  // A synthetic script shaped like the first historical row:
  // Easy +1/+4, Medium +2/+3, Hard 0/+3 -> 7/8, 6/17, 4/20.
  const script = [
    ["SetPlayerTeamDifficultyEasy", [1]],
    ["SetEnemyTeamDifficultyEasy", [4]],
    ["SetPlayerTeamDifficultyMedium", [2]],
    ["SetEnemyTeamDifficultyMedium", [3]],
    ["SetPlayerTeamDifficultyHard", [0]],
    ["SetEnemyTeamDifficultyHard", [3]],
  ];
  const rows = { [EASY]: [7, 8], [MEDIUM]: [6, 17], [HARD]: [4, 20] };
  for (const tier of [EASY, MEDIUM, HARD]) {
    const d = new ClassicDifficulty({ tier }).apply(script);
    assert.deepEqual([d.playerSkill, d.enemySkill], rows[tier]);
  }
  // Large offsets clamp: Easy +14 player, -3 enemy -> 20/1.
  const d = new ClassicDifficulty({ tier: EASY }).apply([["SetPlayerTeamDifficultyEasy", [14]], ["SetEnemyTeamDifficultyEasy", [-3]]]);
  assert.deepEqual([d.playerSkill, d.enemySkill], [20, 1]);
});

const PLAN_ROWS = {
  bes2a_h: [[8, 10], [6, 16], [4, 18]],
  geo1r_h: [[8, 10], [6, 10], [4, 17]],
  hot1i_h: [[8, 10], [6, 14], [4, 20]],
  kam1c_h: [[12, 1], [8, 3], [4, 17]],
  nab2c_h: [[8, 10], [4, 14], [4, 17]],
};
for (const [script, rows] of Object.entries(PLAN_ROWS)) {
  const map = script.slice(0, 4);
  const data = optional(`public/maps/${map}/ai-mission.json`);
  test(`extracted ${script} setters reproduce the historical row`, { skip: !data?.scripts?.[script] && "ai-mission.json absent" }, () => {
    for (const [i, tier] of [EASY, MEDIUM, HARD].entries()) {
      const d = new ClassicDifficulty({ tier }).apply(data.scripts[script].calls);
      assert.deepEqual([d.playerSkill, d.enemySkill], rows[i], `tier ${tier}`);
    }
  });
}

test("Advanced Training re-initializes from the tier with the player's team at +4", () => {
  for (const [tier, row] of [[EASY, [10, 4]], [MEDIUM, [8, 14]], [HARD, [8, 17]]]) {
    const d = new ClassicDifficulty({ tier }).apply([["SetPlayerTeamDifficulty", [1]], ["SetEnemyTeamDifficulty", [20]]]);
    d.advancedTraining();
    assert.deepEqual([d.playerSkill, d.enemySkill], row, "earlier mission offsets are overwritten");
  }
  const guarded = new ClassicDifficulty({ tier: EASY, override: true, skills: { player: 5, enemy: 5 } });
  guarded.advancedTraining();
  assert.deepEqual([guarded.playerSkill, guarded.enemySkill], [5, 5]);
});

test("the player's curves go to the local player's team, offline only", () => {
  const d = new ClassicDifficulty({ tier: HARD, playerTeam: 2 });
  assert.equal(d.curves(2), d.player);
  assert.equal(d.curves(1), d.enemy);
  assert.equal(d.curves(3), d.enemy, "allied locals use the enemy curves");
  assert.equal(d.skill(2), 4);
  assert.equal(d.skill(1), 17);
  assert.equal(new ClassicDifficulty({ tier: HARD, playerTeam: 2, network: true }).curves(2).A, skillCurves(17).A);
  const versus = new ClassicDifficulty({ tier: HARD, playerTeam: 2, secondPlayerTeam: 1 });
  assert.equal(versus.isPlayerTeam(2), false, "split-screen players on different teams: nobody");
  const coop = new ClassicDifficulty({ tier: HARD, playerTeam: 2, secondPlayerTeam: 2 });
  assert.equal(coop.isPlayerTeam(2), true);
});

test("scheduler numbers by tier", () => {
  const rows = { [EASY]: [15, 6, 2], [MEDIUM]: [15, 6, 2], [HARD]: [3, 12, 10] };
  for (const tier of [EASY, MEDIUM, HARD]) {
    const d = new ClassicDifficulty({ tier });
    assert.deepEqual([d.cycle, d.losBudget, d.vehicleBucket], rows[tier]);
  }
});

test("grenade danger delay rows follow the enemy skill and draw lo + U(hi - lo) in float32", () => {
  const rows = [[4, [1.1, 2.1]], [13, [1.1, 2.1]], [14, [0.6, 1.3]], [16, [0.6, 1.3]], [17, [0, 0.8]], [20, [0, 0.8]]];
  for (const [skill, row] of rows) {
    const d = new ClassicDifficulty({ tier: MEDIUM }).apply([["SetEnemyTeamDifficulty", [skill]]]);
    assert.deepEqual(d.dangerRow(), row);
    const random = new ClassicRandom(skill * 7919);
    const twin = clone(random);
    for (let i = 0; i < 50; i++) {
      const delay = d.dangerDelay(random);
      const [lo, hi] = row.map(f32);
      assert.equal(delay, f32(f32(twin.uniform() * f32(hi - lo)) + lo));
      assert.ok(delay >= lo && delay < hi);
    }
  }
  const random = new ClassicRandom(5);
  const before = random.state;
  assert.equal(drawRange(random, 2, 2), 2);
  assert.equal(random.state, before, "equal ends draw nothing");
});

// ---------------------------------------------------------------- posts

function post(name, team, opts = {}) {
  const { x = 0, y = 0, z = 0, atk = 10, def = 10, spawn = true, ally = false, bans = 0, allyCount = 0, capture = null, classPoints = 0, active } = opts;
  const values = (v) => (Array.isArray(v) ? v : [100, v, v, v, v, v]);
  const p = {
    name,
    team,
    position: { x, y, z },
    ai: {
      order: 0,
      atk: values(atk),
      def: values(def),
      bans,
      valueBleed: 10,
      spawnPath: spawn ? { name: `${name}Spawn`, present: true, points: [[x, y, z + 1, 0], [x + 1, y, z, 0.5], [x - 1, y, z, 1]] } : null,
      allyPath: ally ? { name: `${name}Ally`, present: true, points: [[x, y, z - 1, 0.25]] } : null,
      allyCount,
      classSpawnPoints: classPoints,
      classSpawnWorld: Array.from({ length: classPoints }, (_, i) => [x + 8, y, z + i, 0]),
      capture,
      radius: 0,
      yaw: 0,
    },
  };
  if (active !== undefined) p.active = active;
  return p;
}

// Team a's view of team b, as the original game's defaults leave it.
const relation = (a, b) => (a === 0 || b === 0 ? 0 : a === b ? 1 : -1);

// Theed's layout: CIS (team 1, faction 4) holds cp3, cp1, cp4; the Republic
// holds cp2 (ATK 30), cp6 and cp5 (10). World-list order as the map has it.
function theed() {
  return [
    post("cp2", 2, { x: 0, atk: [100, 30, 30, 30, 30, 10], def: [100, 50, 50, 50, 50, 10] }),
    post("cp3", 1, { x: 10 }),
    post("cp1", 1, { x: 20 }),
    post("cp6", 2, { x: 30 }),
    post("cp4", 1, { x: 40 }),
    post("cp5", 2, { x: 50 }),
  ];
}

test("friendly-spawn predicate: own post with a path or class points, ally post with an ally path", () => {
  const rel = (a, b) => (a === b ? 1 : a + b === 4 && (a === 1 || a === 3) ? 1 : relation(a, b));
  assert.equal(friendlySpawn(post("a", 1), 1, relation), true);
  assert.equal(friendlySpawn(post("a", 1, { spawn: false }), 1, relation), false);
  assert.equal(friendlySpawn(post("a", 1, { spawn: false, classPoints: 2 }), 1, relation), true);
  assert.equal(friendlySpawn(post("a", 0), 1, relation), false, "neutral never");
  assert.equal(friendlySpawn(post("a", 1, { active: false }), 1, relation), false, "inactive never");
  assert.equal(friendlySpawn(post("a", 2, { ally: true }), 1, relation), false, "enemy's ally path");
  assert.equal(friendlySpawn(post("a", 1, { ally: true }), 3, rel), true, "a friend's ally path");
  assert.equal(friendlySpawn(post("a", 1, { ally: false }), 3, rel), false, "a friend's post without one");
});

test("weights follow the team index, not which team attacks", () => {
  const cp2 = theed()[0];
  const controls = missionControls([{ name: "SetAttackingTeam", args: [2] }]);
  assert.equal(controls.attackingTeam, 2);
  assert.equal(controls.voTeam, 1);
  assert.equal(postWeight(cp2, 1, 4), 30, "team 1 reads ATK for its faction");
  assert.equal(postWeight(cp2, 2, 3), 50, "team 2 reads DEF for its faction");
  assert.equal(postWeight(cp2, 3, 5), 10, "team 3 reads DEF when it has a faction");
  assert.equal(postWeight(cp2, 3, 0), 0, "team 3 without a faction weighs nothing");
  assert.equal(postWeight(cp2, 4, 4), 0, "other teams weigh nothing");
  assert.equal(postWeight(cp2, 1, 0), 100, "faction 0 reads the class default");
});

test("Theed's 30:10:10 roulette with the shared generator", () => {
  const posts = theed();
  const random = new ClassicRandom(20261008);
  const twin = clone(random);
  const counts = { cp2: 0, cp6: 0, cp5: 0 };
  const draws = 20000;
  for (let i = 0; i < draws; i++) {
    const pick = selectObjective({ posts, team: 1, faction: 4, random, relation });
    counts[pick.name]++;
    // The exact mapping: one draw of [0, 50), first running total that reaches it.
    const draw = f32(f32(twin.uniform() * 50) + 0);
    const expected = draw <= 30 ? "cp2" : draw <= 40 ? "cp6" : "cp5";
    assert.equal(pick.name, expected);
  }
  assert.ok(Math.abs(counts.cp2 / draws - 0.6) < 0.02, `cp2 ${counts.cp2}`);
  assert.ok(Math.abs(counts.cp6 / draws - 0.2) < 0.02);
  assert.ok(Math.abs(counts.cp5 / draws - 0.2) < 0.02);
  assert.equal(random.state, twin.state, "one draw per selection");
});

test("the Republic weighs CIS posts by DEF: uniform over its three targets", () => {
  const posts = theed();
  const random = new ClassicRandom(77);
  const counts = {};
  for (let i = 0; i < 9000; i++) {
    const pick = selectObjective({ posts, team: 2, faction: 3, random, relation });
    counts[pick.name] = (counts[pick.name] || 0) + 1;
  }
  assert.deepEqual(Object.keys(counts).sort(), ["cp1", "cp3", "cp4"]);
  for (const n of Object.values(counts)) assert.ok(Math.abs(n / 9000 - 1 / 3) < 0.03);
});

test("previous-post exclusion", () => {
  const posts = theed();
  const random = new ClassicRandom(9);
  for (let i = 0; i < 2000; i++) {
    const pick = selectObjective({ posts, team: 1, faction: 4, random, relation, excludePrevious: true, previous: posts[0] });
    assert.notEqual(pick, posts[0]);
  }
  // Without the flag, the previous post stays a candidate.
  let seen = false;
  for (let i = 0; i < 200; i++) seen ||= selectObjective({ posts, team: 1, faction: 4, random, relation, previous: posts[0] }) === posts[0];
  assert.ok(seen);
});

test("vehicle drivers skip posts banning their traversal category; soldiers on foot don't", () => {
  const posts = [post("own", 1), post("banned", 2, { bans: HUGE, atk: 1000 }), post("open", 2, { atk: 1 })];
  const random = new ClassicRandom(3);
  for (let i = 0; i < 500; i++) {
    const pick = selectObjective({ posts, team: 1, faction: 4, random, relation, driver: true, vehicleMask: HUGE });
    assert.equal(pick.name, "open");
  }
  let banned = 0;
  for (let i = 0; i < 500; i++) banned += selectObjective({ posts, team: 1, faction: 4, random, relation, driver: true, vehicleMask: HOVER }).name === "banned";
  assert.ok(banned > 400, "another category passes");
  banned = 0;
  for (let i = 0; i < 500; i++) banned += selectObjective({ posts, team: 1, faction: 4, random, relation, vehicleMask: HUGE }).name === "banned";
  assert.ok(banned > 400, "not driving: no ban test");
});

test("no positive weight: uniform over every active post, filters ignored, two or more only", () => {
  const posts = [post("own", 1, { atk: 0 }), post("a", 2, { atk: 0 }), post("b", 0, { atk: 0 }), post("gone", 2, { atk: 50, active: false })];
  const random = new ClassicRandom(11);
  const twin = clone(random);
  const seen = new Set();
  for (let i = 0; i < 300; i++) {
    const pick = selectObjective({ posts, team: 1, faction: 4, random, relation });
    assert.equal(pick, posts[twin.int(0, 2)]);
    seen.add(pick.name);
  }
  assert.deepEqual([...seen].sort(), ["a", "b", "own"], "the team's own spawn post is back in");
  // Exclusion redraws until another post comes up.
  for (let i = 0; i < 300; i++) {
    assert.notEqual(selectObjective({ posts, team: 1, faction: 4, random, relation, excludePrevious: true, previous: posts[1] }), posts[1]);
  }
  assert.equal(selectObjective({ posts: [post("only", 2, { atk: 0 })], team: 1, faction: 4, random, relation }), null);
  assert.equal(selectObjective({ posts: [], team: 1, faction: 4, random, relation }), null);
});

test("AI spawn post: uniform among eligible posts, no draw for one, kept while eligible", () => {
  const posts = theed();
  const random = new ClassicRandom(1);
  const twin = clone(random);
  const pick = selectSpawnPost({ posts, team: 1, random, relation });
  assert.equal(pick, [posts[1], posts[2], posts[4]][twin.int(0, 2)]);
  const single = [post("one", 1), post("enemy", 2)];
  const before = random.state;
  assert.equal(selectSpawnPost({ posts: single, team: 1, random, relation }), single[0]);
  assert.equal(random.state, before);
  assert.equal(selectSpawnPost({ posts: [post("enemy", 2)], team: 1, random, relation }), null);
});

test("the human's post: kept while eligible, else the nearest eligible, else nearest an enemy post", () => {
  const posts = [post("west", 1, { x: -100 }), post("mid", 1, { x: 0 }), post("east", 1, { x: 100 }), post("enemy", 2, { x: 130 }), post("neutral", 0, { x: -120 })];
  assert.equal(humanSpawnPost(posts[0], { posts, team: 1, relation }), posts[0]);
  const lost = post("lost", 2, { x: -90 });
  assert.equal(humanSpawnPost(lost, { posts, team: 1, relation }), posts[0], "nearest to the old post");
  assert.equal(humanSpawnPost(null, { posts, team: 1, relation }), posts[2], "nearest an enemy-held post");
  assert.equal(humanSpawnPost(null, { posts: posts.filter((p) => p.team !== 2), team: 1, relation }), null);
});

test("objective destination: capture-region centre plus a world-axis offset, z drawn first", () => {
  const capture = { type: "box", size: [6, 2, 4], matrix: [], center: [10, 0, 20] };
  const p = post("cp", 2, { x: 1, z: 2, capture });
  const center = objectiveCenter(p);
  assert.deepEqual([center.x, center.y, center.z], [10, 0, 20]);
  const random = new ClassicRandom(42);
  const twin = clone(random);
  const offset = objectiveOffset(p, random);
  const z = f32(f32(twin.uniform() * f32(8)) + f32(-4));
  const x = f32(f32(twin.uniform() * f32(12)) + f32(-6));
  assert.deepEqual([offset.x, offset.y, offset.z], [x, 0, z]);
  const bare = objectiveOffset(post("cp", 2), new ClassicRandom(42));
  assert.ok(Math.abs(bare.x) <= 3 && Math.abs(bare.z) <= 3);
  assert.deepEqual(Object.values(objectiveCenter(post("cp", 2, { x: 5, y: 1, z: 7 }))), [5, 1, 7]);
});

test("objective arrival: inside the region or within half the forward speed; else max(10, radii + 5)", () => {
  const capture = { type: "box", size: [6, 2, 4], matrix: [], center: [0, 0, 0] };
  const zero = { x: 0, y: 0, z: 0 };
  const p = post("cp", 2, { capture });
  const actor = (x, speed = 8) => ({ position: { x, y: 0, z: 0 }, forwardSpeed: speed, radius: 0.5 });
  assert.equal(objectiveArrived(p, zero, actor(3.9)), true);
  assert.equal(objectiveArrived(p, zero, actor(4)), false, "strict");
  assert.equal(objectiveArrived(p, zero, actor(40), { inside: () => true }), true);
  const bare = post("cp", 2);
  assert.equal(objectiveArrived(bare, zero, actor(9.99)), true);
  assert.equal(objectiveArrived(bare, zero, actor(10)), false);
  assert.equal(objectiveArrived(bare, zero, actor(12), { postRadius: 7 }), true, "radius 12.5");
  assert.equal(objectiveArrived(bare, zero, actor(29), { postSpeed: 3 }), true, "moving post adds 20");
});

// ---------------------------------------------------------------- objectives

function bots(team, n, extra = {}) {
  return Array.from({ length: n }, (_, i) => ({ team, human: false, unit: null, controller: null, objective: null, name: `b${team}.${i}`, ...extra }));
}

test("drivers hand their objective to AI co-occupants; passengers take an AI driver's", () => {
  const posts = theed();
  const [driver, gunner, rider] = bots(1, 3);
  const human = { team: 1, human: true, objective: null };
  const seats = new Map([
    [driver, { vehicle: "aat", isDriver: true, driver, occupants: [driver, gunner, human] }],
    [gunner, { vehicle: "aat", isDriver: false, driver, occupants: [driver, gunner, human] }],
  ]);
  const objectives = new ClassicObjectives({ posts, random: new ClassicRandom(4), relation, faction: () => 4, crew: (c) => seats.get(c) || null });
  const target = objectives.request(driver);
  assert.ok(target);
  assert.equal(gunner.objective, target);
  assert.equal(human.objective, null, "humans are not ordered");
  driver.objective = posts[3];
  assert.equal(objectives.request(gunner), posts[3], "a passenger copies its AI driver");
  // A human driver: the passenger draws its own.
  seats.set(rider, { vehicle: "x", isDriver: false, driver: human, occupants: [human, rider] });
  const random = new ClassicRandom(4);
  const own = new ClassicObjectives({ posts, random, relation, faction: () => 4, crew: (c) => seats.get(c) || null });
  const twin = clone(random);
  own.request(rider);
  assert.notEqual(twin.state, random.state);
});

test("a published objective reaches the controller only with a body", () => {
  const posts = theed();
  const seen = [];
  const objectives = new ClassicObjectives({ posts, random: new ClassicRandom(1), relation, faction: () => 4 });
  const [bot] = bots(1, 1, { controller: { onObjective: (p) => seen.push(p) } });
  objectives.publish(bot, posts[0]);
  assert.equal(seen.length, 0);
  assert.equal(bot.objective, posts[0]);
  bot.unit = {};
  objectives.republish(bot);
  assert.deepEqual(seen, [posts[0]]);
});

test("ownership changes reassign crossing teams; a friendly post turned neutral recruits five within 60 m", () => {
  const posts = theed();
  const cp3 = posts[1];
  const team1 = bots(1, 3);
  team1.forEach((b) => (b.objective = cp3));
  const team2 = bots(2, 2);
  team2.forEach((b) => (b.objective = cp3));
  // Units near cp3 in registry order: a human, a flyer, a banned walker, then bots.
  const nearby = [];
  const unitFor = (character, extra = {}) => ({ team: character.team, character, controller: {}, human: false, kind: 0, classDef: {}, ...extra });
  const extraBots = bots(1, 8);
  nearby.push({ team: 1, human: true, controller: null, character: null, kind: 0 });
  nearby.push(unitFor(extraBots[0], { kind: 5 }));
  nearby.push(unitFor(extraBots[1], { classDef: { AISizeType: "HUGE" } }));
  for (const b of extraBots.slice(2)) nearby.push(unitFor(b));
  nearby.push(unitFor(bots(2, 1)[0])); // an enemy, filtered by relation
  const units = { query: (center, radius, out, filter) => nearby.filter((u) => !filter || filter(u)) };
  cp3.ai.bans = HUGE;
  const objectives = new ClassicObjectives({ posts, random: new ClassicRandom(8), relation, faction: (t) => (t === 1 ? 4 : 3), units });
  // CIS loses cp3 to neutral: team 1 crosses (1 -> 0), team 2 doesn't (-1 -> 0).
  cp3.team = 0;
  objectives.onOwnerChanged(cp3, 1, 0, [...team1, ...team2]);
  for (const b of team1) assert.notEqual(b.objective, undefined);
  for (const b of team2) assert.equal(b.objective, cp3, "no crossing for team 2");
  const recruited = extraBots.filter((b) => b.objective === cp3);
  // The cap of ten counts the human, the flyer and the walker too: of the
  // first ten allied units, the six bots qualify; five are sent.
  assert.equal(recruited.length, 5);
  assert.equal(extraBots[0].objective, null, "flyers stay");
  assert.equal(extraBots[1].objective, null, "banned class stays");
  // The Republic takes it: team 2 crosses (0 -> 1) and redraws.
  const before = team2.map((b) => b.objective);
  cp3.team = 2;
  objectives.onOwnerChanged(cp3, 0, 2, [...team1, ...team2]);
  team2.forEach((b, i) => assert.ok(b.objective !== cp3 || before[i] !== cp3));
});

test("patrol revalidation replaces an objective that became a spawn point, avoiding it", () => {
  const posts = theed();
  const [bot] = bots(1, 1);
  const objectives = new ClassicObjectives({ posts, random: new ClassicRandom(2), relation, faction: () => 4 });
  bot.objective = posts[0];
  assert.equal(objectives.revalidate(bot), false);
  posts[0].team = 1; // captured: now a CIS spawn point
  assert.equal(objectives.revalidate(bot), true);
  assert.notEqual(bot.objective, posts[0]);
});

test("mission controls: defaults and setters in order", () => {
  const c = missionControls([
    { name: "SetTeamAggressiveness", args: [1, 0.95] },
    { name: "SetDenseEnvironment", args: ["TRUE"] },
    { name: "SetStayInTurrets", args: [1] },
    { name: "SetStayInTurrets", args: [0] },
    { name: "SetDefenderSnipeRange", args: [170] },
    { name: "SetAIVehicleNotifyRadius", args: [64] },
    { name: "SetAllowBlindJetJumps", args: [0] },
  ]);
  assert.equal(c.aggressiveness[1], f32(0.95));
  assert.deepEqual(c.dense, { explicit: true, value: true });
  assert.equal(c.stayInTurrets, false);
  assert.equal(c.stationaryWait, 10000, "turning it off doesn't restore the wait");
  assert.deepEqual(c.snipe, { attacker: 140, defender: 170 });
  assert.equal(c.vehicleNotifyRadius, 64);
  assert.equal(c.blindJetJumps, false);
  assert.equal(missionControls([]).attackingTeam, 1);
});

test("team names parse to factions as the original game's parser does", () => {
  assert.deepEqual(["Alliance", "all", "Empire", "IMP", "Republic", "rep", "CIS", "neutral"].map(factionOf), [1, 1, 2, 2, 3, 3, 4, 0]);
  assert.deepEqual(["locals", "Local", "Naboo Guard", "", null].map(factionOf), [5, 5, 5, 5, 5]);
});

// ---------------------------------------------------------------- roster

const mission = (extra = []) => [
  { name: "SetTeamName", args: [1, "CIS"] },
  { name: "AddUnitClass", args: [1, "droid", 11] },
  { name: "AddUnitClass", args: [1, "assault", 3] },
  { name: "AddUnitClass", args: [1, "pilot", 3] },
  { name: "AddUnitClass", args: [1, "sniper", 3] },
  { name: "AddUnitClass", args: [1, "droideka", 2] },
  { name: "SetHeroClass", args: [1, "dooku"] },
  { name: "SetTeamName", args: [2, "Republic"] },
  { name: "AddUnitClass", args: [2, "trooper", 11] },
  { name: "AddUnitClass", args: [2, "arc", 3] },
  { name: "AddUnitClass", args: [2, "pilot", 3] },
  { name: "AddUnitClass", args: [2, "sharp", 3] },
  { name: "AddUnitClass", args: [2, "jet", 2] },
  { name: "SetUnitCount", args: [1, 22] },
  { name: "SetReinforcementCount", args: [1, 200] },
  { name: "SetUnitCount", args: [2, 22] },
  { name: "SetReinforcementCount", args: [2, 200] },
  { name: "SetSpawnDelay", args: [10, 0.25] },
  ...extra,
];

function counts(characters) {
  const out = {};
  for (const c of characters) out[c.className] = (out[c.className] || 0) + 1;
  return out;
}

test("UnitCount includes the human: 21 bots and the player on his team, 22 bots on the other", () => {
  const sp = new ClassicSpawning({ calls: mission(), posts: theed(), random: new ClassicRandom(1), humans: [{ team: 2 }] });
  assert.equal(sp.team(2).length, 22);
  assert.equal(sp.team(2).filter((c) => !c.human).length, 21);
  assert.equal(sp.team(2)[0].human, true, "humans join first");
  assert.equal(sp.team(1).length, 22);
  assert.equal(sp.characters.length, 44);
  // The deficit fill gives each team exactly its targets (the human counts as class 0).
  assert.deepEqual(counts(sp.team(1)), { droid: 11, assault: 3, pilot: 3, sniper: 3, droideka: 2 });
  assert.deepEqual(counts(sp.team(2)), { trooper: 11, arc: 3, pilot: 3, sharp: 3, jet: 2 });
});

test("battle size scales each team's unit count, the human still first on his", () => {
  const sp = new ClassicSpawning({ calls: mission(), posts: theed(), random: new ClassicRandom(1), humans: [{ team: 2 }], unitScale: 0.5 });
  assert.equal(sp.team(1).length, 11);
  assert.equal(sp.team(2).length, 11);
  assert.equal(sp.team(2)[0].human, true);
  assert.equal(sp.characters.length, 22);
});

test("heroes: appended with target 1, the first class loses one, the last slot holds it for good", () => {
  const sp = new ClassicSpawning({ calls: mission(), posts: theed(), random: new ClassicRandom(2), humans: [{ team: 2 }], heroes: true });
  const cis = sp.teams[1];
  assert.deepEqual(cis.classes.map((c) => [c.name, c.target, c.hero]), [
    ["droid", 10, false], ["assault", 3, false], ["pilot", 3, false], ["sniper", 3, false], ["droideka", 2, false], ["dooku", 1, true],
  ]);
  assert.equal(cis.heroCount, 1);
  const last = sp.team(1).at(-1);
  assert.equal(last.className, "dooku");
  assert.equal(last.hero, true);
  // The Republic named no hero: its faction's default.
  assert.equal(sp.teams[2].classes.at(-1).name, "rep_inf_macewindu");
  // A hero keeps its class without drawing.
  const before = sp.random.state;
  assert.equal(sp.pickClass(last), cis.classes.length - 1);
  assert.equal(sp.random.state, before);
  // Off, unless the mission's bonus asks for it.
  const off = makeTeams();
  applyMission(off, mission([{ name: "ActivateBonus", args: [2, "HERO"] }]));
  appendHeroes(off, { heroes: false });
  assert.equal(off[1].heroCount, 0);
  assert.equal(off[2].heroCount, 1);
});

test("roster-deficit selection: one draw, RNG % total deficit, first running deficit above it", () => {
  const sp = new ClassicSpawning({ random: new ClassicRandom(0), start: false, objectives: null });
  const team = sp.teams[1];
  team.classes = [{ name: "a", target: 2 }, { name: "b", target: 3 }, { name: "c", target: 1 }];
  const requester = { team: 1, classIndex: 1 };
  for (let seed = 0; seed < 64; seed++) {
    sp.random = new ClassicRandom(seed);
    const twin = new ClassicRandom(seed);
    const r = twin.next() % 6;
    assert.equal(sp.pickClass(requester), r < 2 ? 0 : r < 5 ? 1 : 2);
    assert.equal(sp.random.state, twin.state);
  }
  // Full: keep the current class, no draw.
  team.characters = [0, 0, 1, 1, 1, 2].map((classIndex) => ({ classIndex }));
  const before = sp.random.state;
  assert.equal(sp.pickClass(requester), 1);
  assert.equal(sp.random.state, before);
});

test("roster-deficit selection matches the native selector's 250 cases", { skip: !supplemental8 && "native fixtures absent" }, () => {
  for (const row of supplemental8.classSelection) {
    const sp = new ClassicSpawning({ random: new ClassicRandom(0), start: false, objectives: null });
    const team = sp.teams[1];
    team.classes = row.targets.map((target, i) => ({ name: `c${i}`, target, hero: i >= row.targets.length - row.heroCount }));
    team.heroCount = row.heroCount;
    // Negative entries are Characters whose class pointer is null.
    team.characters = row.assigned.map((a) => ({ team: 1, classIndex: a >= 0 ? a : null }));
    const requester = { team: 1, classIndex: 1 }; // not in the roster; previous class 1
    const selected = {};
    for (let seed = 0; seed < row.seeds; seed++) {
      sp.random = new ClassicRandom(seed);
      const pick = sp.pickClass(requester);
      selected[pick] = (selected[pick] || 0) + 1;
    }
    assert.deepEqual(selected, row.selectedCounts, row.case);
  }
});

test("quota padding: max(RNG % 4n - 3n, 0), one target per missing Character", () => {
  const random = new ClassicRandom(31337);
  const twin = clone(random);
  const calls = [
    { name: "AddUnitClass", args: [1, "a", 1] },
    { name: "AddUnitClass", args: [1, "b", 1] },
    { name: "AddUnitClass", args: [1, "c"] }, // count defaults to 0
    { name: "SetUnitCount", args: [1, 10] },
  ];
  const sp = new ClassicSpawning({ calls, posts: [], random, start: false, objectives: null, names: null });
  // The bots' names come from the team's own generator: padding draws first.
  sp.begin();
  const expected = [1, 1, 0];
  for (let i = 0; i < 8; i++) expected[Math.max((twin.next() % 12) - 9, 0)]++;
  assert.deepEqual(sp.teams[1].classes.map((c) => c.target), expected);
  assert.ok(expected[0] >= 4, "the first class takes most of them");
  // Bias over many draws: P(first) = (3n + 1) / 4n.
  const r = new ClassicRandom(5);
  let first = 0;
  for (let i = 0; i < 40000; i++) first += Math.max((r.next() % 12) - 9, 0) === 0;
  assert.ok(Math.abs(first / 40000 - 10 / 12) < 0.01);
});

test("match start draws in the original game's order: padding, then per Character objective, class, post", () => {
  const random = new ClassicRandom(99);
  const twin = clone(random);
  const posts = [post("home", 1, { atk: 7 }), post("away", 2, { atk: 7 }), post("mid", 0, { atk: 7 })];
  const calls = [
    { name: "AddUnitClass", args: [1, "a", 1] },
    { name: "AddUnitClass", args: [1, "b", 1] },
    { name: "SetUnitCount", args: [1, 2] },
    { name: "AddUnitClass", args: [2, "c", 1] },
    { name: "SetUnitCount", args: [2, 1] },
  ];
  const sp = new ClassicSpawning({ calls, posts, random });
  // No draws for the names: each team names its bots from its own generator.
  // Team 1, Character 1: objective (one float draw over away/mid), class
  // (deficit b only: one draw), post (one eligible: no draw).
  const obj = (draw) => (draw <= 7 ? "away" : "mid");
  const [c1, c2] = sp.team(1);
  assert.equal(c1.objective.name, obj(f32(twin.uniform() * 14)));
  twin.next(); // class draw, RNG % 1
  assert.equal(c1.className, "b");
  assert.equal(c2.objective.name, obj(f32(twin.uniform() * 14)));
  assert.equal(c2.className, "a", "no deficit left: keeps class 0 without drawing");
  const [c3] = sp.team(2);
  assert.equal(c3.objective.name, f32(twin.uniform() * 14) <= 7 ? "home" : "mid");
  assert.equal(random.state, twin.state);
  assert.equal(c1.spawn.post.name, "home");
  assert.deepEqual([c1.spawn.ordinal, c2.spawn.ordinal, c3.spawn.ordinal], [0, 1, 0]);
  assert.deepEqual([c1.spawn.wave, c2.spawn.wave], [1, 1]);
});

test("bot names follow the faction's lists, each team drawing from its own generator seeded with its side type", () => {
  const random = new ClassicRandom(5);
  const twin = clone(random);
  const names = { 0: { first: [], last: [] }, 3: { first: ["ALPHA", "BETA"], last: [] }, 1: { first: ["HAN", "BEN"], last: ["SOLO", "KENOBI", "LARS"] }, local: "GEONOSIAN" };
  const calls = [
    { name: "SetTeamName", args: [1, "Republic"] },
    { name: "SetUnitCount", args: [1, 2] },
    { name: "SetTeamName", args: [2, "Alliance"] },
    { name: "SetUnitCount", args: [2, 1] },
    { name: "SetTeamName", args: [3, "locals"] },
    { name: "SetUnitCount", args: [3, 1] },
  ];
  const sp = new ClassicSpawning({ calls, posts: [], random, names, objectives: null, humans: [{ team: 1 }] });
  // The Republic's generator starts at 3, the Alliance's at 1; the human
  // takes no name from his team's.
  const rep = new ClassicRandom(3);
  const all = new ClassicRandom(1);
  assert.equal(sp.team(1)[0].name, "Player 1");
  assert.equal(sp.team(1)[1].name, names[3].first[rep.next() % 2] + " " + [0, 1, 2].map(() => rep.next() % 10).join(""));
  assert.equal(sp.team(2)[0].name, names[1].first[all.next() % 2] + " " + names[1].last[all.next() % 3]);
  assert.equal(sp.team(3)[0].name, "GEONOSIAN");
  // None of it moved the shared generator: the match start's own draws did.
  const quiet = new ClassicSpawning({ calls, posts: [], random: clone(twin), names: null, objectives: null, humans: [{ team: 1 }] });
  assert.equal(quiet.random.state, sp.random.state);
});

// ---------------------------------------------------------------- clocks

function clockOnly({ A = 10, B = 0.25, demo = false } = {}) {
  return new ClassicSpawning({ calls: [{ name: "SetSpawnDelay", args: [A, B] }], posts: [], random: new ClassicRandom(0), demo, objectives: null });
}

test("spawn clocks step once per update: a long frame does not catch up", () => {
  const sp = clockOnly();
  assert.equal(sp.Q, 40);
  sp.waveCountdown = 1;
  sp.stagger = f32(0.1);
  sp.wave = 0;
  sp.released = 8;
  sp.clock(35);
  assert.deepEqual([sp.waveCountdown, sp.wave, sp.stagger, sp.released], [-24, 1, -34.75, 1]);
  // Defaults without SetSpawnDelay: 15 / 0.5.
  const plain = new ClassicSpawning({ calls: [], posts: [], random: new ClassicRandom(0), objectives: null });
  assert.deepEqual([plain.A, plain.B, plain.Q], [15, 0.5, 30]);
  // The last setter wins.
  const twice = new ClassicSpawning({ calls: [{ name: "SetSpawnDelay", args: [10, 0.25] }, { name: "SetSpawnDelay", args: [1, 0.5] }], posts: [], random: new ClassicRandom(0), objectives: null });
  assert.deepEqual([twice.A, twice.B, twice.Q], [1, 0.5, 2]);
});

test("a wave transition releases the first slot in the same update; demo mode starts fast", () => {
  const sp = clockOnly();
  const dt = f32(1 / 30);
  let transition = null;
  for (let tick = 1; tick <= 400 && !transition; tick++) {
    const wave = sp.wave;
    sp.clock(dt);
    if (sp.wave !== wave) transition = { tick, released: sp.released };
  }
  assert.deepEqual(transition, { tick: 300, released: 1 });
  const demo = clockOnly({ demo: true });
  assert.equal(demo.waveCountdown, f32(0.1));
});

test("spawn clocks match the native recurrence at 30/60/120 Hz, ordinary and demo", { skip: !native8 && "native fixtures absent" }, () => {
  for (const block of native8.spawnWaveBlocks) {
    const sp = clockOnly({ demo: block.demo });
    const dt = f32(1 / block.hz);
    const events = [];
    for (let tick = 1; tick <= block.durationSeconds * block.hz; tick++) {
      const wave = sp.wave;
      sp.clock(dt);
      if (sp.wave !== wave) events.push({ tick, wave: sp.wave, release: sp.released, countdown: sp.waveCountdown });
    }
    assert.deepEqual(events, block.waveEvents, `${block.demo ? "demo" : "ordinary"} ${block.hz} Hz`);
    assert.deepEqual([sp.waveCountdown, sp.wave, sp.stagger, sp.released], block.end);
  }
});

test("the release inequality is strict: R must exceed J + (S - W) Q", () => {
  const sp = clockOnly();
  const c = { spawn: { wave: 1, ordinal: 3 } };
  sp.wave = 1;
  sp.released = 3;
  assert.equal(sp.waiting(c), true, "equality blocks");
  sp.released = 4;
  assert.equal(sp.waiting(c), false);
  sp.wave = 0;
  sp.released = 43;
  assert.equal(sp.waiting(c), true, "a wave early: 40 more releases needed");
  sp.released = 44;
  assert.equal(sp.waiting(c), false);
  c.spawn.wave = 0;
  sp.wave = 1;
  sp.released = 0;
  assert.equal(sp.waiting(c), false, "an earlier wave's schedule passes at R 0");
});

test("the release inequality matches the native gate cases", { skip: !spawn9 && "native fixtures absent" }, () => {
  const sp = clockOnly();
  for (const row of spawn9.waveRelease) {
    sp.wave = row.W;
    sp.released = row.R;
    sp.Q = row.Q;
    assert.equal(!sp.waiting({ spawn: { wave: row.S, ordinal: row.J } }), row.passes, JSON.stringify(row));
  }
});

// ---------------------------------------------------------------- queues

test("post queues: four counters, AI ordinals behind waiting humans, release restores", () => {
  const sp = clockOnly();
  const p = post("cp", 1);
  const set = () => sp.queues.set(p, [3, 7, 11, 13]);
  set();
  assert.equal(sp.reserve(p, false, false), 10, "AI at an ally's post: 3 AI + 7 humans");
  assert.deepEqual(sp.queueOf(p), [4, 7, 11, 13]);
  assert.equal(sp.release(p, false, false), 10);
  assert.deepEqual(sp.queueOf(p), [3, 7, 11, 13]);
  set();
  assert.equal(sp.reserve(p, true, false), 7);
  set();
  assert.equal(sp.reserve(p, false, true), 24);
  set();
  assert.equal(sp.reserve(p, true, true), 13);
  assert.deepEqual(sp.queueOf(p), [3, 7, 11, 14]);
});

test("post queues match the native reserve cases", { skip: !spawn9 && "native fixtures absent" }, () => {
  const sp = clockOnly();
  for (const row of spawn9.CPQueues.filter((r) => !r.secondary)) {
    const p = post("cp", 1);
    sp.queues.set(p, [3, 7, 11, 13]);
    assert.equal(sp.reserve(p, !!row.human, !!row.ownPost), row.ordinal);
    assert.deepEqual(sp.queueOf(p), row.afterReserve);
    assert.equal(sp.release(p, !!row.human, !!row.ownPost), row.releaseValue);
  }
});

test("changing post moves a held reservation; ordinals already handed out are never renumbered", () => {
  const sp = clockOnly();
  const a = post("a", 1);
  const b = post("b", 1);
  const [x, y, z] = bots(1, 3).map((c) => ({ ...c, spawn: { ordinal: -1, post: null } }));
  for (const c of [x, y, z]) {
    sp.changePost(c, a);
    c.spawn.ordinal = sp.reserve(a, false, true);
  }
  assert.deepEqual([x, y, z].map((c) => c.spawn.ordinal), [0, 1, 2]);
  sp.changePost(x, b);
  assert.equal(x.spawn.ordinal, 0, "joins b's queue");
  assert.deepEqual(sp.queueOf(a), [0, 0, 2, 0]);
  assert.deepEqual([y.spawn.ordinal, z.spawn.ordinal], [1, 2], "a's queue keeps its numbers");
  const late = { ...bots(1, 1)[0], spawn: { ordinal: -1, post: null } };
  sp.changePost(late, a);
  late.spawn.ordinal = sp.reserve(a, false, true);
  assert.equal(late.spawn.ordinal, 2, "a duplicate ordinal, as in the original");
  // No reservation held: changing post reserves nothing.
  const free = { ...bots(1, 1)[0], spawn: { ordinal: -1, post: a } };
  sp.changePost(free, b);
  assert.equal(free.spawn.ordinal, -1);
  assert.deepEqual(sp.queueOf(b), [0, 0, 1, 0]);
});

// ---------------------------------------------------------------- lifecycle

function world({ calls = mission(), posts = theed(), humans = [], seed = 1, failClass = null, battle = null, ...rest } = {}) {
  const spawned = [];
  const sp = new ClassicSpawning({
    calls,
    posts,
    random: new ClassicRandom(seed),
    humans,
    battle,
    spawnUnit: (character, className, p, at, yaw) => {
      if (failClass && failClass(className)) return null;
      const unit = { className, post: p, at, yaw, combat: { health: 100 } };
      spawned.push({ character, unit });
      return unit;
    },
    ...rest,
  });
  return { sp, spawned };
}

const step = (sp, seconds, hz = 30) => {
  const dt = f32(1 / hz);
  for (let i = 0; i < Math.round(seconds * hz); i++) sp.update(dt);
};

test("bots wait for the first wave; the ready human spawns at once", () => {
  const { sp, spawned } = world({ humans: [{ team: 1 }] });
  const human = sp.human;
  assert.equal(human.spawn.ready, false);
  step(sp, 1);
  assert.equal(spawned.length, 0, "nobody before the human is ready or the wave comes");
  sp.onHumanReady(human, { className: "sniper" });
  sp.update(f32(1 / 30));
  assert.equal(spawned.length, 1);
  assert.equal(spawned[0].character, human);
  assert.equal(spawned[0].unit.className, "sniper");
  step(sp, 8.5);
  assert.equal(spawned.length, 1, "bots still waiting for wave 1");
  step(sp, 1);
  assert.ok(spawned.length > 1);
  for (const { character } of spawned.slice(1)) assert.equal(character.human, false);
  // Releases are per post queue: each post lets one bot through per stagger.
  step(sp, 10);
  assert.equal(spawned.length, 44);
  assert.equal(sp.teams[1].living, 22);
  assert.equal(sp.teams[2].living, 22);
});

test("a dead human keeps his post and waits for the spawn screen after the 4 s timer", () => {
  const battle = { reinforcements: { 1: 200, 2: 200 }, spend(t) { this.reinforcements[t]--; } };
  const { sp, spawned } = world({ battle, humans: [{ team: 1 }] });
  const human = sp.human;
  assert.equal(sp.awaitingHuman(human), true, "prepared at match start");
  sp.onHumanReady(human, { className: "pilot" });
  sp.update(f32(1 / 30));
  const post = human.spawn.post;
  sp.onDeath(human);
  assert.equal(battle.reinforcements[1], 199);
  assert.equal(human.spawn.post, post, "humans keep their post");
  step(sp, 3.9);
  assert.equal(sp.awaitingHuman(human), false);
  step(sp, 0.2);
  assert.equal(sp.awaitingHuman(human), true);
  const count = spawned.length;
  step(sp, 2);
  assert.equal(spawned.length, count, "not without the spawn screen's choice");
  sp.onHumanReady(human, {});
  sp.update(f32(1 / 30));
  assert.equal(spawned.at(-1).character, human, "no wave wait for the human");
  assert.equal(spawned.at(-1).unit.className, "pilot");
});

test("spawn points: a random point of the post's path, one draw", () => {
  const posts = [post("home", 1), post("away", 2)];
  const { sp, spawned } = world({ posts, calls: [{ name: "AddUnitClass", args: [1, "a", 1] }, { name: "SetUnitCount", args: [1, 1] }, { name: "SetReinforcementCount", args: [1, 10] }, { name: "SetSpawnDelay", args: [1, 0.5] }] });
  const twin = clone(sp.random);
  step(sp, 2);
  assert.equal(spawned.length, 1);
  const points = posts[0].ai.spawnPath.points;
  const p = points[twin.next() % points.length];
  assert.deepEqual(spawned[0].unit.at, p.slice(0, 3));
  assert.equal(spawned[0].unit.yaw, p[3]);
  // Ally posts use the ally path; no path at all falls back to the post.
  assert.deepEqual(sp.spawnPoint(post("x", 2, { ally: true }), true), post("x", 2, { ally: true }).ai.allyPath.points[0]);
  assert.deepEqual(sp.spawnPoint(post("y", 1, { spawn: false, x: 4, z: 9 }), false), [4, 0, 9, 0]);
});

test("a failed spawn retries once as class 0; a second failure keeps the reservation", () => {
  const calls = [
    { name: "AddUnitClass", args: [1, "base", 0] },
    { name: "AddUnitClass", args: [1, "special", 1] },
    { name: "SetUnitCount", args: [1, 1] },
    { name: "SetReinforcementCount", args: [1, 10] },
    { name: "SetSpawnDelay", args: [1, 0.5] },
  ];
  const posts = [post("home", 1), post("away", 2)];
  const once = world({ calls, posts, failClass: (name) => name === "special" });
  step(once.sp, 2);
  assert.equal(once.spawned.length, 1);
  assert.equal(once.spawned[0].unit.className, "base");
  const never = world({ calls, posts: [post("home", 1), post("away", 2)], failClass: () => true });
  const bot = never.sp.characters[0];
  step(never.sp, 2);
  assert.equal(never.spawned.length, 0);
  assert.equal(bot.unit, null);
  assert.equal(bot.spawn.ordinal, 0, "the reservation is not consumed");
  assert.deepEqual(never.sp.queueOf(bot.spawn.post), [0, 0, 1, 0]);
});

test("bots need tickets above their team's living count; the human doesn't", () => {
  const battle = { reinforcements: { 1: 2, 2: 0 }, spend(t) { this.reinforcements[t]--; } };
  const { sp, spawned } = world({ battle, humans: [{ team: 2 }], calls: mission([{ name: "SetSpawnDelay", args: [1, 0.25] }]) });
  sp.onHumanReady(sp.human, {});
  step(sp, 4);
  const byTeam = (t) => spawned.filter(({ character }) => character.team === t).length;
  assert.equal(byTeam(1), 2, "tickets 2: spawns stop when living reaches 2");
  assert.equal(byTeam(2), 1, "no tickets: only the human");
});

test("death: a reinforcement spent, the next preparation 4 s later, scheduled for the next wave", () => {
  const battle = { reinforcements: { 1: 200, 2: 200 }, spend(t) { this.reinforcements[t]--; } };
  const { sp, spawned } = world({ battle });
  step(sp, 25);
  assert.equal(spawned.length, 44);
  const bot = sp.team(1)[3];
  const post = bot.spawn.post;
  sp.onDeath(bot);
  assert.equal(battle.reinforcements[1], 199);
  assert.equal(sp.teams[1].living, 21);
  assert.equal(bot.unit, null);
  assert.equal(bot.spawn.timer, RESPAWN_DELAY);
  assert.equal(bot.spawn.post, null, "a bot forgets its post");
  assert.ok(bot.objective, "and draws a fresh objective");
  // Prepared once the timer crosses zero: post, ordinal, wave W + 1.
  step(sp, 3.9);
  assert.equal(bot.spawn.ordinal, -1);
  step(sp, 0.2);
  assert.ok(bot.spawn.post);
  assert.ok(bot.spawn.ordinal >= 0);
  assert.equal(bot.spawn.wave, sp.wave + 1);
  void post;
  const count = spawned.length;
  step(sp, 10.1);
  assert.equal(spawned.length, count + 1, "back at the next wave");
});

test("a hero is killed when its team's tickets fall to its living count", () => {
  const killed = [];
  const battle = { reinforcements: { 1: 200, 2: 200 }, spend(t) { this.reinforcements[t]--; } };
  const { sp } = world({ battle, heroes: true, forceKill: (c) => killed.push(c) });
  step(sp, 25);
  const hero = sp.team(1).at(-1);
  assert.equal(hero.hero, true);
  assert.equal(sp.teams[1].living, 21, "heroes don't count as living");
  battle.reinforcements[1] = 22;
  sp.update(f32(1 / 30));
  assert.equal(killed.length, 0);
  battle.reinforcements[1] = 21;
  sp.update(f32(1 / 30));
  assert.deepEqual(killed, [hero]);
});

test("locals spawn at friendly posts' ally paths, capped by AllyCount and their unit count", () => {
  const posts = [post("cis", 2, { ally: true, allyCount: 2 }), post("rep", 1), post("cis2", 2, { ally: true, allyCount: 1 })];
  const calls = [
    { name: "AddUnitClass", args: [1, "trooper", 1] },
    { name: "SetUnitCount", args: [1, 1] },
    { name: "AddUnitClass", args: [2, "droid", 1] },
    { name: "SetUnitCount", args: [2, 1] },
    { name: "SetTeamAsFriend", args: [2, 3] },
    { name: "SetTeamAsFriend", args: [3, 2] },
    { name: "SetTeamName", args: [3, "locals"] },
    { name: "AddUnitClass", args: [3, "geonosian", 7] },
    { name: "SetUnitCount", args: [3, 7] },
    { name: "SetSpawnDelay", args: [1, 0.1] },
  ];
  const { sp, spawned } = world({ calls, posts });
  step(sp, 5);
  const locals = spawned.filter(({ character }) => character.team === 3);
  assert.equal(sp.localsCap, 3, "AllyCount 2 + 1 from posts held by team 3's friends");
  assert.equal(locals.length, 3);
  assert.equal(sp.tickets(3), 0x7fffffff, "locals default to unlimited reinforcements");
  for (const { unit } of locals) assert.equal(unit.post.ai.allyPath.points[0][2], unit.at[2]);
  // The Republic takes a post: the cap drops; living locals stay.
  posts[0].team = 1;
  sp.update(f32(1 / 30));
  assert.equal(sp.localsCap, 1);
});

test("map.json's mission block builds the same roster", () => {
  const calls = callsFromMapMission({
    teams: { 1: { name: "CIS", classes: [["droid", 3]], units: 3, reinforcements: 10 }, 2: { name: "Republic", classes: [["trooper", 2]], units: 2 } },
    spawnDelay: [10, 0.25],
  });
  const teams = makeTeams();
  assert.deepEqual(applyMission(teams, calls), [10, 0.25]);
  assert.deepEqual([teams[1].faction, teams[1].unitCount, teams[1].tickets, teams[2].faction], [4, 3, 10, 3]);
});

test("attachPostAI joins ai-mission records by name and falls back to map.json", () => {
  const posts = [
    { name: "cp1", team: 1, position: { x: 0, y: 0, z: 0 }, desc: { spawnPath: "CP1Spawn" } },
    { name: "cp9", team: 2, position: { x: 5, y: 0, z: 5 }, desc: { spawnPath: "cp9spawn", capture: { type: "box", size: [1, 1, 1], matrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 7, 8, 9, 1] } } },
  ];
  const record = {
    name: "CP1", order: 0, values: { atk: [100, 1, 2, 3, 4, 5], def: [100, 6, 7, 8, 9, 10] }, bans: 16, valueBleed: 10,
    spawnPath: { name: "CP1Spawn", present: true, points: [[1, 2, 3, 0]] }, allyPath: null, allyCount: 2, classSpawnPoints: 0,
    captureRegion: { name: "x", present: false }, radius: 0, yaw: 0,
  };
  attachPostAI(posts, { posts: [record] }, { CP9Spawn: [[4, 4, 4, 1]] });
  assert.deepEqual(posts[0].ai.atk, [100, 1, 2, 3, 4, 5]);
  assert.equal(posts[0].ai.bans, 16);
  assert.equal(posts[0].ai.capture, null);
  assert.deepEqual(posts[1].ai.atk, [100, 100, 100, 100, 100, 100], "class defaults");
  assert.equal(posts[1].ai.spawnPath.present, true);
  assert.deepEqual(posts[1].ai.capture.center, [7, 8, 9]);
  assert.equal(postWeight(posts[0], 1, 4), 4);
  assert.equal(SOLDIER, 1);
});

test("the Classic entry point's population services", () => {
  const posts = theed();
  const sp = new ClassicSpawning({ calls: mission(), posts, random: new ClassicRandom(3), humans: [{ team: 2 }] });
  const d = new ClassicDifficulty({ tier: HARD, playerTeam: 2 });
  const binding = classicBinding({ world: { spawning: sp, difficulty: d } });
  const bot = sp.team(1)[0];
  const previous = bot.objective;
  const twin = clone(sp.random);
  const expected = selectObjective({ posts, team: 1, faction: 4, random: twin, relation: sp.relation.bind(sp), excludePrevious: true, previous });
  const post = binding.requestObjective(bot, true, previous);
  assert.equal(post, expected, "the same draw as the selector");
  assert.equal(bot.objective, post, "and published");
  assert.notEqual(post, previous);
  assert.equal(binding.friendlySpawnEligible(posts[1], 1), true);
  assert.equal(binding.friendlySpawnEligible(posts[0], 1), false);
  assert.equal(curvesFor(d, 2), d.player);
  assert.equal(curvesFor(null, 2), undefined);
  assert.equal(isHard(d), true);
  assert.equal(isHard(null), undefined);
  // Objectives reach a live controller through publishObjective.
  const seen = [];
  bot.unit = {};
  bot.controller = { publishObjective: (p) => seen.push(p), onObjective: () => assert.fail("publishObjective first") };
  sp.objectives.republish(bot);
  assert.deepEqual(seen, [bot.objective]);
});
