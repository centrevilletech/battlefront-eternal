// Enhanced AI: equal difficulty, objective draws and defense, spawn posts,
// objective focus, and aim. Synthetic fixtures throughout.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import { ClassicDifficulty, EASY, HARD, MEDIUM, isHard, skillCurves } from "../src/ai/classic/difficulty.js";
import { ClassicObjectives } from "../src/ai/classic/goals.js";
import {
  AIM_PROFILES,
  ENHANCED_FOCUS,
  ENHANCED_OBJECTIVES,
  ENHANCED_SCHEDULE,
  ENHANCED_SKILL,
  EnhancedAimer,
  EnhancedDifficulty,
  EnhancedObjectives,
  EnhancedSpawning,
  enhancedCombat,
} from "../src/ai/enhanced/index.js";
import { EnhancedRepairAgent, EnhancedTrooperAgent } from "../src/ai/enhanced/agents.js";
import { playerSpread } from "../src/ai/enhanced/aim.js";

// ---------------------------------------------------------------- difficulty

test("both teams get the tier's skill, and mission setters do not split them", () => {
  for (const tier of [EASY, MEDIUM, HARD]) {
    const d = new EnhancedDifficulty({ tier, playerTeam: 1 });
    d.apply([{ name: "SetPlayerTeamDifficulty", args: [2] }, { name: "SetEnemyTeamDifficulty", args: [20] }]);
    d.advancedTraining();
    assert.equal(d.playerSkill, ENHANCED_SKILL[tier]);
    assert.equal(d.enemySkill, ENHANCED_SKILL[tier]);
    assert.deepEqual(d.curves(1), d.curves(2), "the player's team aims like the enemy");
    assert.deepEqual(d.curves(1), skillCurves(ENHANCED_SKILL[tier]));
    assert.equal(d.cycle, ENHANCED_SCHEDULE[tier].cycle);
    assert.equal(d.losBudget, ENHANCED_SCHEDULE[tier].losBudget);
    assert.equal(isHard(d), tier === HARD, "Classic's tier test still reads it");
  }
  const classic = new ClassicDifficulty({ tier: MEDIUM, playerTeam: 1 });
  assert.notEqual(classic.playerSkill, classic.enemySkill, "Classic stays asymmetric");
});

// ---------------------------------------------------------------- objectives

function post(name, team, x, { weight = 10, spawn = true, z = 0 } = {}) {
  const values = [100, weight, weight, weight, weight, weight];
  return {
    name,
    team,
    position: { x, y: 0, z },
    neutralize: 0,
    ai: {
      atk: values,
      def: values,
      bans: 0,
      spawnPath: spawn ? { present: true, points: [[x, 0, z, 0]] } : null,
      allyPath: null,
      classSpawnPoints: 0,
      classSpawnWorld: [],
      capture: null,
    },
  };
}

const relation = (a, b) => (a === 0 || b === 0 ? 0 : a === b ? 1 : -1);

function objectives(posts, { characters = [], now = 0, units = null, random = new ClassicRandom(7) } = {}) {
  const clock = { now };
  const o = new EnhancedObjectives({
    posts,
    random,
    relation,
    faction: () => 1,
    units,
    characters: () => characters,
    now: () => clock.now,
  });
  return { o, clock };
}

function shares(o, character, draws = 6000) {
  const counts = {};
  for (let i = 0; i < draws; i++) {
    const pick = o.select(character);
    counts[pick?.name ?? "none"] = (counts[pick?.name ?? "none"] ?? 0) + 1;
  }
  for (const k of Object.keys(counts)) counts[k] /= draws;
  return counts;
}

test("nearer enemy posts are drawn more, by (nearest / distance)^2 from the team's front", () => {
  const posts = [post("home", 1, 0), post("a", 2, 100), post("b", 2, 200), post("c", 2, 300)];
  const { o } = objectives(posts);
  const s = shares(o, { team: 1 });
  const w = [1, 1 / 4, 1 / 9];
  const total = w.reduce((x, y) => x + y);
  assert.ok(Math.abs(s.a - w[0] / total) < 0.03, `a ${s.a}`);
  assert.ok(Math.abs(s.b - w[1] / total) < 0.03, `b ${s.b}`);
  assert.ok(Math.abs(s.c - w[2] / total) < 0.03, `c ${s.c}`);
  assert.equal(s.home, undefined, "never its own spawn post");
});

test("a living bot measures from where it stands", () => {
  const posts = [post("home", 1, 0), post("a", 2, 100), post("b", 2, 200)];
  const { o } = objectives(posts);
  const bot = { team: 1, unit: { alive: true, position: { x: 200, y: 0, z: 0 } } };
  const s = shares(o, bot);
  assert.ok(s.b > 0.95, `standing at b, it takes b (${s.b})`);
});

test("authored zero weights stay excluded; neutral posts weigh 1.5x", () => {
  const posts = [post("home", 1, 0), post("zero", 2, 50, { weight: 0 }), post("enemy", 2, 200, { z: 10 }), post("neutral", 0, 200, { z: -10 })];
  const { o } = objectives(posts);
  const s = shares(o, { team: 1 });
  assert.equal(s.zero, undefined);
  assert.ok(Math.abs(s.neutral / s.enemy - ENHANCED_OBJECTIVES.neutralBias) < 0.15, `neutral/enemy ${s.neutral / s.enemy}`);
});

test("with nothing worth drawing it falls back to Classic's uniform pick", () => {
  const posts = [post("home", 1, 0, { weight: 0 }), post("a", 2, 100, { weight: 0 }), post("b", 2, 200, { weight: 0 })];
  const { o } = objectives(posts);
  const classic = new ClassicObjectives({ posts, random: new ClassicRandom(7), relation, faction: () => 1 });
  for (let i = 0; i < 20; i++) assert.equal(o.select({ team: 1 }), classic.select({ team: 1 }));
});

test("crowding: past six teammates on a post its weight falls", () => {
  const posts = [post("home", 1, 0), post("a", 2, 100), post("b", 2, 100, { z: 1 })];
  const crowd = Array.from({ length: 12 }, () => ({ team: 1, objective: posts[1], unit: { alive: true } }));
  const { o } = objectives(posts, { characters: crowd });
  const s = shares(o, { team: 1 });
  // a weighs 1 / (1 + 6/6) = 0.5 of b.
  assert.ok(Math.abs(s.a / s.b - 0.5) < 0.08, `a/b ${s.a / s.b}`);
});

test("a friendly post being taken is defended, kept through revalidation, then let go", () => {
  const posts = [post("home", 1, 0), post("front", 1, 100), post("enemy", 2, 400)];
  const { o, clock } = objectives(posts);
  assert.equal(shares(o, { team: 1 }, 500).front, undefined, "not while it is quiet");
  posts[1].neutralize = 0.3;
  o.tick();
  const s = shares(o, { team: 1 }, 2000);
  assert.ok(s.front > 0.9, `defend the front (${s.front})`);
  const defender = { team: 1, objective: posts[1] };
  assert.equal(o.select(defender, true, posts[1]), posts[1], "revalidation keeps a defender on a post under attack");
  posts[1].neutralize = 0;
  clock.now = ENHANCED_OBJECTIVES.defenseGrace - 1;
  assert.ok(o.threatened(posts[1], 1), "for a while after the meter stops");
  clock.now = ENHANCED_OBJECTIVES.defenseGrace + 1;
  assert.equal(o.threatened(posts[1], 1), false);
  assert.equal(o.select(defender, true, posts[1]), posts[2], "then on to the enemy");
});

test("a post under attack calls in its nearest free bots, at most four", () => {
  const posts = [post("home", 1, 0), post("front", 1, 100), post("enemy", 2, 400)];
  const at = (x, extra = {}) => {
    const character = { team: 1, objective: posts[2], unit: null };
    const unit = { team: 1, kind: 0, human: false, seat: null, position: new THREE.Vector3(x, 0, 0), character, controller: { state: 3 }, ...extra };
    character.unit = unit;
    character.controller = unit.controller;
    return unit;
  };
  const list = [at(110), at(150), at(90), at(130), at(120), at(105, { seat: {} }), at(101, { controller: { state: 2 } }), at(102, { human: true })];
  const units = { query: (_c, _r, out, filter) => list.filter(filter) };
  const published = [];
  const { o } = objectives(posts, { units });
  o.publish = (character, p) => published.push([character, p]);
  posts[1].neutralize = 0.1;
  o.tick();
  assert.equal(published.length, ENHANCED_OBJECTIVES.defenseLimit);
  assert.deepEqual(published.map(([c]) => c.unit.position.x), [110, 90, 120, 130], "nearest first, skipping seated, fighting and human");
  assert.ok(published.every(([, p]) => p === posts[1]));
  published.length = 0;
  o.tick();
  assert.equal(published.length, 0, "not again at once");
});

test("a bot defending one post under attack is not pulled to another", () => {
  const posts = [post("home", 1, 0), post("a", 1, 100), post("b", 1, 140), post("enemy", 2, 400)];
  const character = { team: 1, objective: posts[1] };
  const unit = { team: 1, kind: 0, human: false, seat: null, position: new THREE.Vector3(120, 0, 0), character, controller: { state: 3 } };
  character.unit = unit;
  const units = { query: (_c, _r, out, filter) => [unit].filter(filter) };
  const { o } = objectives(posts, { units });
  const published = [];
  o.publish = (c, p) => published.push(p.name);
  posts[1].neutralize = 0.2;
  posts[2].neutralize = 0.2;
  o.tick();
  assert.deepEqual(published, [], "it stays on a");
});

test("a bot spawns at the friendly post nearest its objective; the human as in Classic", () => {
  const posts = [post("back", 1, 0), post("front", 1, 300), post("enemy", 2, 400)];
  const spawning = new EnhancedSpawning({ posts, random: new ClassicRandom(3), start: false });
  assert.ok(spawning.objectives instanceof EnhancedObjectives);
  const bot = { human: false, team: 1, objective: posts[2], spawn: { post: posts[0] } };
  assert.equal(spawning.choosePost(bot), posts[1]);
  const human = { human: true, team: 1, objective: posts[2], spawn: { post: posts[0] } };
  assert.equal(spawning.choosePost(human), posts[0], "the human keeps his post");
});

// ---------------------------------------------------------------- focus

// An objective agent without its machinery: the policies only.
function agent(Agent, { state = 3, patrolState = 0, inside = false } = {}) {
  const a = Object.create(Agent.prototype);
  a.state = state;
  a.unit = { position: new THREE.Vector3(0, 0, 0) };
  a.world = { time: 100 };
  a.ai = { combat: {}, warnOnce() {} };
  a.interrupts = { charging: () => false };
  a.patrol = { state: patrolState, post: { contains: () => inside }, acceptsTarget: () => true, acceptsAlert: () => true, acceptsAcoustic: () => true };
  a.snipe = {};
  a.mine = {};
  a.controller = {
    looked: null,
    setStance() {},
    aimAt() {},
    lookAt(p) {
      this.looked = p.clone();
    },
    lookAlongPath() {
      this.looked = "path";
    },
  };
  return a;
}

const at = (x, z = 0) => ({ position: new THREE.Vector3(x, 0, z), id: x, generation: 1 });

test("walking to its post, a bot fights only close enemies and keeps going past far ones", () => {
  const a = agent(EnhancedTrooperAgent);
  assert.equal(a.acceptsTarget(at(ENHANCED_FOCUS.engageRadius + 5)), false);
  assert.equal(a.acceptsTarget(at(ENHANCED_FOCUS.engageRadius - 5)), true);
  assert.equal(a.acceptsAcoustic(at(ENHANCED_FOCUS.hearRadius + 5)), false);
  assert.equal(a.acceptsAcoustic(at(ENHANCED_FOCUS.hearRadius - 5)), true);
  assert.equal(a.acceptsAlert(true, new THREE.Vector3(ENHANCED_FOCUS.alertRadius - 5, 0, 0)), true);
  assert.equal(a.acceptsAlert(true, new THREE.Vector3(60, 0, 0)), false);
  a.aim(0);
  assert.equal(a.controller.looked.x, 60, "it turns toward the shooter it did not stop for");
  a.world.time += ENHANCED_FOCUS.lookTime + 0.1;
  a.aim(0);
  assert.equal(a.controller.looked, "path", "then back along its path");
});

test("taking a post, a bot inside the capture region holds it", () => {
  const a = agent(EnhancedTrooperAgent, { state: 2, patrolState: 2, inside: true });
  assert.equal(a.acceptsTarget(at(5)), false);
  assert.equal(a.acceptsAcoustic(at(5)), false);
  assert.equal(a.acceptsAlert(true, new THREE.Vector3(5, 0, 0)), false);
  const outside = agent(EnhancedTrooperAgent, { state: 2, patrolState: 2, inside: false });
  assert.equal(outside.acceptsTarget(at(5)), true, "on its way in, a close enemy is fought");
});

test("off the run Classic's policies apply, but a heard enemy is checked once per 10 s", () => {
  const a = agent(EnhancedTrooperAgent, { state: 2, patrolState: 3 });
  assert.equal(a.acceptsTarget(at(50)), true, "defending: as Classic");
  assert.equal(a.acceptsAlert(true, new THREE.Vector3(60, 0, 0)), true);
  const source = at(50);
  assert.equal(a.acceptsAcoustic(source), true);
  a.world.time += 5;
  assert.equal(a.acceptsAcoustic(source), false, "the same source again soon");
  a.world.time += ENHANCED_FOCUS.recheck;
  assert.equal(a.acceptsAcoustic(source), true);
});

test("a repair bot's run uses its own approach state", () => {
  const a = agent(EnhancedRepairAgent, { state: 2 });
  assert.equal(a.acceptsTarget(at(ENHANCED_FOCUS.engageRadius + 2)), false);
  assert.equal(a.acceptsTarget(at(10)), true);
});

// ---------------------------------------------------------------- aim

const FORWARD = new THREE.Vector3(0, 0, 1);

test("a new target waits out a reaction time; one regained at once does not", () => {
  const profile = AIM_PROFILES[MEDIUM];
  const aimer = new EnhancedAimer(new ClassicRandom(11), profile);
  const target = { id: 1, generation: 1 };
  aimer.observe(target, 10);
  assert.ok(aimer.reacting(10));
  assert.ok(aimer.readyAt - 10 >= profile.reactionFloor - 1e-9);
  assert.equal(aimer.reacting(aimer.readyAt + 0.01), false);
  aimer.observe(null, 12);
  aimer.observe(target, 12.5);
  assert.equal(aimer.reacting(12.5), false, "lost for half a second: no new reaction");
  aimer.observe({ id: 2, generation: 1 }, 13);
  assert.ok(aimer.reacting(13), "another target: a new reaction");
});

test("reaction times centre on the tier's median and never go under its floor", () => {
  for (const tier of [EASY, MEDIUM, HARD]) {
    const profile = AIM_PROFILES[tier];
    const aimer = new EnhancedAimer(new ClassicRandom(5 + tier), profile);
    const samples = [];
    for (let i = 0; i < 2000; i++) {
      aimer.observe({ id: i, generation: 1 }, 0);
      samples.push(aimer.readyAt);
    }
    samples.sort((a, b) => a - b);
    assert.ok(samples[0] >= profile.reactionFloor - 1e-9);
    const median = samples[1000];
    assert.ok(Math.abs(median - profile.reaction) < 0.03, `${profile.name} median ${median}`);
  }
});

function rmsError(profile, distance, steps = 20000) {
  const aimer = new EnhancedAimer(new ClassicRandom(23), profile);
  aimer.observe({ id: 1, generation: 1 }, -100);
  let sum = 0;
  for (let i = 0; i < steps; i++) {
    const d = aimer.update(FORWARD, 1 / 60, { distance, now: i / 60 });
    const angle = d.angleTo(FORWARD);
    sum += angle * angle;
  }
  return Math.sqrt(sum / steps) / Math.SQRT2; // per axis
}

test("aim error settles at the tier's sigma at 25 m and grows with range", () => {
  for (const tier of [EASY, MEDIUM, HARD]) {
    const profile = AIM_PROFILES[tier];
    const near = (rmsError(profile, 25) * 180) / Math.PI;
    assert.ok(Math.abs(near / profile.sigma25 - 1) < 0.15, `${profile.name}: ${near.toFixed(3)} deg vs ${profile.sigma25}`);
  }
  const far = rmsError(AIM_PROFILES[MEDIUM], 100) / rmsError(AIM_PROFILES[MEDIUM], 25);
  assert.ok(Math.abs(far - 1.9) < 0.3, `100 m against 25 m: ${far}`);
});

test("the acquisition bias fades over the convergence time", () => {
  const profile = AIM_PROFILES[MEDIUM];
  const early = [];
  const late = [];
  for (let seed = 1; seed < 400; seed++) {
    const aimer = new EnhancedAimer(new ClassicRandom(seed), profile);
    aimer.observe({ id: 1, generation: 1 }, 0);
    // A few steps for the slew-limited gun to reach the biased aim.
    for (let t = 1; t <= 6; t++) aimer.update(FORWARD, 1 / 60, { now: t / 60 });
    early.push(aimer.direction.angleTo(FORWARD));
    for (let t = 7; t < 120; t++) aimer.update(FORWARD, 1 / 60, { now: t / 60 });
    late.push(aimer.direction.angleTo(FORWARD));
  }
  const mean = (a) => a.reduce((x, y) => x + y) / a.length;
  assert.ok(mean(early) > 1.4 * mean(late), `early ${mean(early)} late ${mean(late)}`);
});

test("the gun turns no faster than the tier's slew rate; heroes aim straight", () => {
  const profile = AIM_PROFILES[EASY];
  const aimer = new EnhancedAimer(new ClassicRandom(3), profile);
  aimer.reset(FORWARD);
  const side = new THREE.Vector3(1, 0, 0);
  const before = aimer.direction.clone();
  const after = aimer.update(side, 0.1, { now: 0 });
  const turned = (before.angleTo(after) * 180) / Math.PI;
  assert.ok(turned <= profile.slew * 0.1 + 1e-6, `turned ${turned}`);
  assert.equal(aimer.update(side, 0.1, { hero: true }).angleTo(side), 0);
});

test("bot weapons take the player's spread rules", () => {
  const weapon = { def: { SpreadThreshold: 2 }, floor: 0, kick: 0, spreadScale: 2.68, sniperScale: 1.77 };
  playerSpread(weapon);
  assert.equal(weapon.spreadScale, 1);
  assert.equal(weapon.sniperScale, 0);
  assert.equal(weapon.floor, -2);
  assert.equal(weapon.kick, -2);
  weapon.kick = 1;
  playerSpread(weapon);
  assert.equal(weapon.kick, 1, "set once, not reset every update");
});

test("fire is held while a bot reacts to a new target", () => {
  const world = { time: 0 };
  const combat = enhancedCombat(world, { tier: MEDIUM, seed: 1 });
  const target = { id: 9, generation: 1, alive: true, position: new THREE.Vector3(0, 0, 20) };
  const unit = { alive: true, intent: {} };
  const controller = { perception: {}, character: {}, target: { unit: target, generation: 1 }, fireA: true, fireB: true };
  combat.consumeFire(controller, unit, 1 / 60, null);
  assert.equal(controller.fireA, false);
  assert.equal(controller.fireB, false);
  world.time = 5;
  controller.fireA = controller.fireB = true;
  combat.consumeFire(controller, unit, 1 / 60, null);
  assert.equal(controller.fireB, true, "free to fire once the reaction has passed");
});
