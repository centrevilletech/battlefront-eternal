// Classic AI combat: perception, aim, firing and weapon AI (docs/ai-combat.md).
//
//   node --test tests/ai-combat.test.js
//
// Synthetic units and weapons throughout; optional reference values measured
// in the original game (not part of the repository) are compared bit for bit
// when present and skipped otherwise.

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import { Units } from "../src/registry.js";
import { Arsenal, Weapon } from "../src/weapons.js";
import {
  ANGLE, Aimer, aimCurves, ballisticLead, filterDirection, isCannon, leadPoint, lookupCos, lookupSin, projectileOf, skillCurves,
} from "../src/ai/classic/aim.js";
import {
  ACTION, ammoCount, attachAIData, capacity, consumeFire, effectiveRange, expandedRange, firingCone, pressButton, prepareSpread,
  protectedVeto, rebuildProtectedHumans, registerAIData, selectWeapon, setTarget, sniperYaw, triggerFromCounts, validateFire, weaponAI, weaponUtility,
} from "../src/ai/classic/weapons-ai.js";
import {
  DeferredLos, Perception, PriorityHeap, TargetTracker, boundRadius, broadcastAcoustic, coneDistanceSq, dangerDelayRange,
  alertControllers, matchupPriority, notifyDamage, notifyDanger, notifyTargetedShot, rawCost, recordTarget, scan, sightRange, thin,
} from "../src/ai/classic/perception.js";
import { UnitCombatAgent, boardingClear, classicBinding, dispatch, inForwardCone, rangePolicy } from "../src/ai/classic/combat.js";

const f = Math.fround;
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const RESEARCH = path.join(ROOT, "extract", "ai-research");

function oracle(rel) {
  const file = path.join(RESEARCH, rel);
  if (!fs.existsSync(file)) return null;
  // The reference JSON writes bare NaN/Infinity.
  return JSON.parse(fs.readFileSync(file, "utf8").replace(/-?Infinity|NaN/g, "null"));
}

const hex3 = (v) => Buffer.from(new Float32Array([v.x, v.y, v.z]).buffer).toString("hex");

// ---------------------------------------------------------------------------
// Synthetic weapons (not retail values).

function bolt(extra = {}) {
  return { base: "bolt", Velocity: 150, LifeSpan: 1, Damage: 30, PersonScale: 1, DroidScale: 1, VehicleScale: 0.1, BuildingScale: 0.1, ...extra };
}

function rifle(extra = {}) {
  return {
    class: "test_rifle", base: "cannon", channel: 0, ammo: 4, RoundsPerClip: 50, ReloadTime: 1, ShotDelay: 0.2, TriggerSingle: 0,
    ShotsPerSalvo: 1, HeatPerShot: 0, YawSpread: 0.25, PitchSpread: 0.25, SpreadPerShot: 1, SpreadRecoverRate: 4, SpreadThreshold: 1,
    SpreadLimit: 10, ZoomMax: 2, AnimationBank: "rifle", ordnance: bolt(),
    ai: { minRange: 0, optimalRange: 16, maxRange: 48, implicitRange: false, extremeRange: 64, relationMask: 1, categoryMask: 0xffffff87 },
    ...extra,
  };
}

function sniper(extra = {}) {
  return rifle({
    class: "test_sniper", RoundsPerClip: 5, ShotDelay: 1, TriggerSingle: 1, ZoomMax: 6, YawSpread: 0, PitchSpread: 0,
    ai: { minRange: 32, optimalRange: 64, maxRange: 128, implicitRange: false, extremeRange: 0, relationMask: 1, categoryMask: 0xffffff87, triggerSingle: true, zoomMax: 6 },
    ...extra,
  });
}

function grenade(extra = {}) {
  return {
    class: "test_grenade", base: "grenade", channel: 1, ammo: 3, RoundsPerClip: 1, ShotDelay: 0.5, InitialSalvoDelay: 0.3, TriggerSingle: 1,
    AnimationBank: "grenade", ordnance: { base: "sticky", Velocity: 20, Gravity: 1, Damage: 0, ExplosionExpire: { Damage: 200, DamageRadiusInner: 2, DamageRadiusOuter: 6, PersonScale: 1 } },
    ai: { minRange: 8, optimalRange: 16, maxRange: 24, implicitRange: false, extremeRange: 30, relationMask: 1, categoryMask: 0xffffff87 },
    ...extra,
  };
}

// ---------------------------------------------------------------------------
// Synthetic units, world and controllers.

function makeUnit({ team = 1, kind = 0, at = [0, 0, 0], yaw = 0, pitch = 0, weapons = [rifle()], classDef = {}, hero = false, human = false, maxHealth = 300 } = {}) {
  const position = new THREE.Vector3(...at);
  const velocity = new THREE.Vector3();
  const soldier = { position, velocity, aimYaw: yaw, aimPitch: pitch, posture: "stand", air: null };
  const unit = {
    id: 0, generation: 1, team, kind, alive: true, dead: false, hero, human,
    classDef: { MaxHealth: maxHealth, MaxSpeed: 6, HealthType: "person", ...classDef },
    position, velocity, soldier,
    combat: { maxHealth, arsenal: new Arsenal(weapons, { player: false }) },
    intent: { buttons: [0, 0], aim: null },
    controller: null,
    character: { team, unit: null },
    _sphere: { center: new THREE.Vector3(), radius: 1.2 },
    get sphere() {
      this._sphere.center.set(position.x, position.y + 1, position.z);
      return this._sphere;
    },
    aimPoint(out) {
      return out.set(position.x, position.y + 1, position.z);
    },
    origin(out) {
      return out.set(position.x, position.y + 1.7, position.z);
    },
    forward(out) {
      return out.set(Math.sin(soldier.aimYaw) * Math.cos(soldier.aimPitch), Math.sin(soldier.aimPitch), Math.cos(soldier.aimYaw) * Math.cos(soldier.aimPitch));
    },
  };
  unit.character.unit = unit;
  return unit;
}

function makeWorld(units = [], { seed = 7, difficulty = { enemySkill: 14, hard: false } } = {}) {
  const registry = new Units();
  for (const u of units) registry.add(u);
  return {
    time: 10,
    random: new ClassicRandom(seed),
    randomB: new ClassicRandom(seed + 101),
    units: registry,
    los: () => true,
    difficulty,
    mission: {},
  };
}

function makeController(unit, world, extra = {}) {
  const c = {
    unit, world, generation: unit.generation, roll: 0, rangeScale: 1, state: 2,
    fireA: false, fireB: false, target: null, aimState: 2, aimPoint: new THREE.Vector3(), aimTarget: null,
    acoustic: [], shot: { pending: false, known: false, point: new THREE.Vector3(), source: null },
    danger: { pending: false, point: new THREE.Vector3() },
    addAcoustic(ref) {
      if (this.acoustic.length < 20) this.acoustic.push(ref);
    },
    notifyShot(source, known, point) {
      this.shot.pending = true;
      this.shot.source = source;
      if (known && point) {
        this.shot.known = true;
        this.shot.point.copy(point);
      }
    },
    notifyDanger(point) {
      this.danger.pending = true;
      this.danger.point.copy(point);
    },
    ...extra,
  };
  c.perception = new Perception(c);
  unit.controller = c;
  return c;
}

const ref = (u) => ({ unit: u, generation: u.generation });

// ---------------------------------------------------------------------------
// Difficulty curves.

test("skill curves match the native outputs at every skill", () => {
  const native = oracle("round8/combat/native-fixtures.json");
  // The curve table at the tiers' skills (rounded values).
  const table = { 4: [1.053846, 1.053846, 2.676923, 1.769231], 6: [1.223077, 1.223077, 2.461538, 1.615385], 14: [1.9, 1.9, 1.6, 1], 17: [10.95, 2.15, 1.3, 0.5] };
  for (const [skill, want] of Object.entries(table)) {
    const c = skillCurves(Number(skill));
    [c.A, c.D, c.B, c.S].forEach((v, i) => assert.ok(Math.abs(v - want[i]) < 1e-5, `skill ${skill} curve ${i}: ${v}`));
  }
  if (!native) return;
  for (const row of native.allSkillCurves) {
    const c = skillCurves(row.skill);
    assert.deepEqual([c.A, c.D, c.B, c.S], row.C1_C2_C3_C4.map(f), `skill ${row.skill}`);
  }
});

test("every tier's player and enemy curves, and the non-cannon halving", async () => {
  let difficulty = null;
  try {
    difficulty = await import("../src/ai/classic/difficulty.js");
  } catch {
    // Population's module not present: the local curves stand in.
  }
  const tiers = { 1: [6, 4], 2: [4, 14], 3: [4, 17] };
  for (const [tier, [player, enemy]] of Object.entries(tiers)) {
    const want = { 1: skillCurves(player), 2: skillCurves(enemy) };
    let got = want;
    if (difficulty?.ClassicDifficulty) {
      const d = new difficulty.ClassicDifficulty({ tier: Number(tier), playerTeam: 1 });
      got = { 1: d.curves(1), 2: d.curves(2) };
    }
    for (const team of [1, 2]) {
      assert.deepEqual({ ...got[team] }, { ...want[team] }, `tier ${tier} team ${team}`);
      const cannon = aimCurves(got[team], rifle());
      const thrown = aimCurves(got[team], grenade());
      assert.equal(cannon.A, got[team].A);
      assert.equal(cannon.D, got[team].D);
      assert.equal(thrown.A, f(got[team].A * 0.5));
      assert.equal(thrown.D, f(got[team].D * 0.5));
      assert.equal(thrown.B, got[team].B, "B and S are never halved");
      const none = aimCurves(got[team], null);
      assert.equal(none.A, got[team].A, "no weapon: no halving");
    }
  }
  assert.ok(isCannon({ base: "launcher" }) && isCannon({ base: "weapon" }) && !isCannon({ base: "melee" }));
});

// ---------------------------------------------------------------------------
// The direction filter.

test("direction filter matches the native SSE/SSE2 traces bit for bit", (t) => {
  const files = ["aim007f", "aim027f", "sse007f", "sse027f"].map((name) => `round9/precision/${name}.json`);
  let ran = 0;
  for (const file of files) {
    const data = oracle(file);
    if (!data) continue;
    for (const r of data.results) {
      const d = { x: r.desired[0], y: r.desired[1], z: r.desired[2] };
      const q = { x: r.current[0], y: r.current[1], z: r.current[2] };
      const v = { x: 0, y: 0, z: 0 };
      const out = { x: 0, y: 0, z: 0 };
      filterDirection(d, q, v, r.dt, f(1.25), f(1.125), out);
      assert.equal(hex3(out), r.result_hex, `${file} ${r.case} dt ${r.dt}`);
      r.v.forEach((want, i) => {
        const got = [v.x, v.y, v.z][i];
        if (want === null) assert.ok(Number.isNaN(got));
        else assert.equal(got, f(want));
      });
      ran += 1;
    }
  }
  if (!ran) t.skip("round9 precision traces not present");
});

test("the 30-degree correction turns the desired direction about world Y", () => {
  const out = { x: 0, y: 0, z: 0 };
  // d to the right of q: (d x q).y < 0 -> -30 degrees: q becomes (0.866, 0, 0.5).
  let v = { x: 0, y: 0, z: 0 };
  filterDirection({ x: 1, y: 0, z: 0 }, { x: 0, y: 0, z: 1 }, v, 1e-8, 1, 1, out);
  assert.equal(out.x, f(0.8660253882408142));
  assert.equal(out.z, 0.5);
  // d to the left: (d x q).y > 0 -> +30 degrees.
  v = { x: 0, y: 0, z: 0 };
  filterDirection({ x: -1, y: 0, z: 0 }, { x: 0, y: 0, z: 1 }, v, 1e-8, 1, 1, out);
  assert.equal(out.x, f(-0.8660253882408142));
  assert.equal(out.z, 0.5);
  // Antiparallel: cross Y is zero, the negative branch.
  v = { x: 0, y: 0, z: 0 };
  filterDirection({ x: 0, y: 0, z: -1 }, { x: 0, y: 0, z: 1 }, v, 1e-8, 1, 1, out);
  assert.equal(out.x, 0.5);
  assert.equal(out.z, f(-0.8660253882408142));
  // Vertical d is unchanged by the yaw: q jumps to it.
  v = { x: 0, y: 0, z: 0 };
  filterDirection({ x: 0, y: 1, z: 0 }, { x: 0, y: 0, z: 1 }, v, 1 / 60, 1, 1, out);
  assert.deepEqual([out.x, out.y, out.z], [0, 1, 0]);
  // Within 30 degrees there is no jump: the filter moves gradually.
  v = { x: 0, y: 0, z: 0 };
  const near = { x: Math.sin(0.3), y: 0, z: Math.cos(0.3) };
  filterDirection(near, { x: 0, y: 0, z: 1 }, v, 1 / 60, 1.9, 1.9, out);
  assert.ok(out.x > 0 && out.x < near.x * 0.1, `${out.x}`);
});

test("the Aimer halves for non-cannons, skips heroes and hands the weapon B and S", () => {
  const curves = skillCurves(14);
  const d = new THREE.Vector3(Math.sin(0.4), 0, Math.cos(0.4));
  const start = new THREE.Vector3(0, 0, 1);
  const cannon = new Weapon(rifle(), { player: false });
  const thrown = new Weapon(grenade(), { player: false });
  const a = new Aimer();
  a.reset(start);
  const b = new Aimer();
  b.reset(start);
  for (let i = 0; i < 10; i++) {
    a.update(d, 1 / 30, curves, cannon);
    b.update(d, 1 / 30, curves, thrown);
  }
  assert.ok(a.direction.angleTo(d) < b.direction.angleTo(d), "the halved filter lags");
  assert.equal(cannon.spreadScale, curves.B);
  assert.equal(cannon.sniperScale, curves.S);
  assert.equal(thrown.spreadScale, 1, "non-cannons keep no B");
  const hero = new Aimer();
  hero.reset(start);
  hero.update(d, 1 / 30, curves, cannon, true);
  assert.ok(hero.direction.equals(d), "a hero aims straight along its body");
  // dt 0 would poison the rate with NaN in the original game; the port skips it.
  const z = new Aimer();
  z.reset(start);
  z.update(d, 0, curves, cannon);
  assert.ok(z.direction.equals(start) && Number.isFinite(z.velocity.x));
});

// ---------------------------------------------------------------------------
// Lookup trig and leading.

test("lookup cosine matches the original game's table at its thresholds", () => {
  assert.equal(lookupCos(0), 1);
  assert.equal(lookupCos(f(Math.PI / 8)), 0.9238795042037964);
  assert.equal(lookupCos(f(Math.PI / 4)), 0.7071067690849304);
  assert.equal(lookupCos(1.3526301384), 0.21643958985805511);
  assert.equal(lookupCos(f(Math.PI / 2)), -4.371138828673793e-8);
  assert.equal(lookupCos(f(Math.PI)), -1);
  assert.ok(Math.abs(lookupSin(ANGLE.MELEE) - Math.SQRT1_2) < 1e-6);
});

test("ballistic lead matches the native solver, fallbacks included", (t) => {
  // Synthetic: a stationary target stays put; a crossing one is led.
  const out = { x: 0, y: 0, z: 0 };
  assert.ok(ballisticLead({ x: 100, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }, 50, 0, out));
  assert.deepEqual([out.x, out.y, out.z], [100, 0, 0]);
  ballisticLead({ x: 100, y: 0, z: 0 }, { x: 0, y: 0, z: 10 }, { x: 0, y: 0, z: 0 }, 50, 0, out);
  assert.equal(out.z, 20.41241455078125);
  const native = oracle("round4/navigation/lead-probe.json");
  if (!native) return t.skip("lead reference values not present");
  // (the oracle's NaN/Infinity were read as null: compare the hex)
  const raw = JSON.parse(fs.readFileSync(path.join(RESEARCH, "round4/navigation/lead-probe.json"), "utf8").replace(/-?Infinity|NaN/g, "0"));
  for (const r of raw) {
    const p = { x: r.point[0], y: r.point[1], z: r.point[2] };
    const v = { x: r.velocity[0], y: r.velocity[1], z: r.velocity[2] };
    const valid = ballisticLead(p, v, { x: 0, y: 0, z: 0 }, r.speed, r.gravity, out);
    assert.equal(hex3(out), r.result_hex, `${r.case} ${r.cw}`);
    assert.equal(Number(valid), r.valid, `${r.case} validity`);
  }
});

test("projectile speed and gravity by ordnance class", () => {
  assert.deepEqual({ ...projectileOf({ ordnance: { base: "bolt", Velocity: 150 } }) }, { speed: 150, gravity: 0 });
  assert.deepEqual({ ...projectileOf({ ordnance: { base: "missile", Velocity: 35 } }) }, { speed: 3e8, gravity: 0 });
  assert.deepEqual({ ...projectileOf({ ordnance: { base: "sticky", Velocity: 20, Gravity: 0.5 } }) }, { speed: 20, gravity: f(f(0.5) * f(-9.8)) });
  assert.deepEqual({ ...projectileOf({ ordnance: { base: "beam" } }) }, { speed: 0, gravity: 0 });
  assert.deepEqual({ ...projectileOf({}) }, { speed: 0, gravity: 0 });
  // Relative motion: h seconds of (target - shooter) velocity before the solver.
  const shooter = makeUnit({ at: [0, 0, 0] });
  const target = makeUnit({ at: [0, 0, 50], team: 2 });
  target.velocity.set(5, 0, 0);
  const out = new THREE.Vector3();
  leadPoint(0.5, shooter, target, null, out);
  assert.equal(out.x, 2.5);
});

// ---------------------------------------------------------------------------
// Weapon spread.

test("AI spread: SpreadPerShot x B once per salvo, kick never below zero", () => {
  const w = new Weapon(rifle({ ShotsPerSalvo: 3, RoundsPerClip: 60 }), { player: false });
  w.spreadScale = f(2.5);
  assert.equal(w.kick, 0, "no head start below zero for a bot");
  const shots = [];
  w.shoot(shots, { stick: 0, posture: "stand", zoomed: false });
  assert.equal(shots.length, 3);
  assert.equal(w.kick, f(2.5), "one buildup per salvo, not per round");
  const p = new Weapon(rifle(), { player: true });
  assert.equal(p.kick, -1, "a player's head start is unchanged");
  p.shoot([], { stick: 0, posture: "stand", zoomed: false });
  assert.equal(p.kick, 0);
});

test("AI spread draws come from the shared generator, pitch pair first", () => {
  const def = rifle({ YawSpread: 2, PitchSpread: 3, SpreadPerShot: 0 });
  const w = new Weapon(def, { player: false, random: () => draws.shift() });
  const draws = [0.9, 0.1, 0.25, 0.75];
  const shots = [];
  w.shoot(shots, { stick: 0, posture: "stand", zoomed: false });
  const DEG = Math.PI / 180;
  assert.ok(Math.abs(shots[0].pitch - 3 * (0.9 - 0.1) * DEG) < 1e-12, "pitch from the first pair");
  assert.ok(Math.abs(shots[0].yaw - 2 * (0.25 - 0.75) * DEG) < 1e-12, "yaw from the second pair");
  // A player's weapon keeps Math.random.
  const real = Math.random;
  let calls = 0;
  Math.random = () => (calls++, 0.5);
  try {
    new Weapon(def).shoot([], { stick: 0, posture: "stand", zoomed: false });
  } finally {
    Math.random = real;
  }
  assert.equal(calls, 4);
});

test("sniper extra yaw: every distance step and every gate", () => {
  const steps = [[0, 0.0997], [399.99, 0.0997], [400, 0.0499], [1599.9, 0.0499], [1600, 0.0333], [3600, 0.025], [6400, 0.02], [10000, 0.0167], [14399, 0.0167], [14400, 0.0143], [1e6, 0.0143]];
  for (const [d2, yaw] of steps) assert.equal(sniperYaw(f(d2)), f(yaw), `d^2 ${d2}`);
  const world = makeWorld();
  const run = (def, { human = false, target = true, distance = 50 } = {}) => {
    const unit = makeUnit({ weapons: [def], human });
    const foe = makeUnit({ team: 2, at: [0, 0, distance] });
    const c = makeController(unit, world);
    if (target) c.target = ref(foe);
    const weapon = unit.combat.arsenal.current(0);
    weapon.sniperScale = f(1.769231);
    prepareSpread(c, unit, weapon);
    return weapon.yawSpreadOverride;
  };
  const deg = (r) => f(f(1.769231) * f(r)) * (180 / Math.PI);
  assert.equal(run(sniper(), { distance: 50 }), deg(0.0333));
  assert.equal(run(sniper(), { distance: 19.99 }), deg(0.0997));
  assert.equal(run(sniper({ YawSpread: 0.1 })), null, "authored YawSpread");
  assert.equal(run(sniper(), { human: true }), null, "player owner");
  assert.equal(run(sniper({ TriggerSingle: 0, ai: { ...sniper().ai, triggerSingle: false } })), null, "not single-shot");
  assert.equal(run(sniper({ ZoomMax: 5.9, ai: { ...sniper().ai, zoomMax: 5.9 } })), null, "ZoomMax below 6");
  assert.equal(run(sniper(), { target: false }), null, "no target");
  // Skill 20: S = 0 removes the term.
  assert.equal(skillCurves(20).S, 0);
});

// ---------------------------------------------------------------------------
// Buttons and trigger shaping.

test("run-length button counts, codes and the trigger edges", () => {
  const b = { count: 0, code: 0 };
  assert.equal(pressButton(b, true).count, 2, "zero to +2");
  for (let i = 0; i < 200; i++) pressButton(b, true);
  assert.equal(b.count, 100, "saturates at 100");
  assert.equal(b.code, 3, "held 70 and more");
  pressButton(b, false);
  assert.equal(b.count, -1);
  assert.equal(b.code, 0, "released after a long hold");
  pressButton(b, false);
  assert.equal(b.count, -2);
  for (let i = 0; i < 200; i++) pressButton(b, false);
  assert.equal(b.count, -100);
  pressButton(b, true);
  assert.equal(b.count, 1, "release to +1");
  const c = { count: 0, code: 0 };
  pressButton(c, false);
  assert.equal(c.count, -2, "zero to -2");
  pressButton(c, true);
  pressButton(c, true);
  pressButton(c, false);
  assert.equal(c.code, 1, "short hold");
  pressButton(c, true);
  assert.equal(c.code, 2, "a press after a short hold");
  assert.deepEqual({ ...triggerFromCounts(-2, 1) }, { held: true, pressed: true });
  assert.deepEqual({ ...triggerFromCounts(1, 2) }, { held: true, pressed: false });
  assert.deepEqual({ ...triggerFromCounts(2, -1) }, { held: false, pressed: false });
});

test("a standing semiautomatic request presses two updates, releases two", () => {
  const world = makeWorld();
  const unit = makeUnit({ weapons: [rifle({ TriggerSingle: 1, ai: { ...rifle().ai, triggerSingle: true } })] });
  const c = makeController(unit, world);
  const counts = [];
  for (let i = 0; i < 9; i++) {
    c.fireA = true;
    consumeFire(c, unit, world);
    counts.push(unit.intent.buttons[0]);
    assert.ok(unit.intent.buttons[1] < 0, "the other channel stays released");
  }
  assert.deepEqual(counts, [2, -1, -2, 1, 2, -1, -2, 1, 2]);
});

test("the sniper gate: one request per uniform(2, 3) s from the shared generator, only when idle", () => {
  const world = makeWorld([], { seed: 99 });
  const unit = makeUnit({ weapons: [sniper()] });
  const c = makeController(unit, world);
  const expect = new ClassicRandom(99);
  const deadline = f(f(world.time) + expect.range(2, 3));
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true, "first request goes");
  assert.equal(c.perception.fire.sniperDeadline, deadline);
  for (const dt of [0.5, 1, deadline - world.time - 1.5]) {
    world.time = f(world.time + dt);
    c.fireA = true;
    assert.equal(consumeFire(c, unit, world), false, `blocked at ${world.time}`);
  }
  world.time = deadline;
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), false, "equality is still blocked");
  world.time = f(deadline + 0.01);
  c.fireA = true;
  unit.combat.arsenal.current(0).reloadLeft = 1;
  assert.equal(consumeFire(c, unit, world), false, "not while reloading");
  unit.combat.arsenal.current(0).reloadLeft = 0;
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true, "past the deadline with the weapon idle");
  // Exactly one draw per accepted request, none for the refused ones.
  assert.equal(c.perception.fire.sniperDeadline, f(world.time + expect.range(2, 3)));
  assert.equal(world.random.state, expect.state);
});

test("bursts: BarrageMin..Max rounds, the heat limit, and the unlimited-ammo quirk", () => {
  const world = makeWorld([], { seed: 5 });
  const def = rifle({ RoundsPerClip: 60, HeatPerShot: 0.1, ai: { ...rifle().ai, barrageMin: 3, barrageMax: 3, barrageDelay: 1 } });
  const unit = makeUnit({ weapons: [def] });
  const c = makeController(unit, world);
  const weapon = unit.combat.arsenal.current(0);
  let fired = 0;
  const pressed = [];
  for (let i = 0; i < 12; i++) {
    c.fireA = true;
    const down = consumeFire(c, unit, world);
    pressed.push(down);
    if (down) {
      weapon.shoot([], { stick: 0, posture: "stand", zoomed: false });
      fired++;
    }
    world.time = f(world.time + 0.05);
  }
  assert.equal(pressed.slice(0, 4).join(), "true,true,true,false", "three rounds, then the burst ends");
  assert.equal(fired, 3, "and waits BarrageDelay before the next");
  // Heat: a burst must fit under 1 - heat.
  weapon.heat = 0.75;
  c.perception.fire.burst.state = 0;
  world.time = f(world.time + 5);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), false, "0.3 of heat does not fit under 0.25");
  // No ammunition count and no heat: the burst never ends.
  const free = makeUnit({ weapons: [rifle({ RoundsPerClip: 0, ammo: 0, HeatPerShot: 0 })] });
  const fc = makeController(free, world);
  let held = 0;
  for (let i = 0; i < 50; i++) {
    fc.fireA = true;
    if (consumeFire(fc, free, world)) held++;
  }
  assert.equal(held, 50);
  assert.equal(ammoCount(free.combat.arsenal.current(0)), 2147483647);
});

test("a burst that just fills the heat gauge still starts (five rounds of 0.2)", () => {
  // Rounded to float32 the burst's heat came to exactly 1, not "greater
  // than 1", so it never fit under 1 - heat: its bot aimed and never fired
  // for as long as the target stayed in view.
  const world = makeWorld([], { seed: 5 });
  const def = rifle({ RoundsPerClip: 60, HeatPerShot: 0.2, ai: { ...rifle().ai, barrageMin: 5, barrageMax: 5, barrageDelay: 1 } });
  const unit = makeUnit({ weapons: [def] });
  const c = makeController(unit, world);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true);
});

test("charging: hold drawn from 0.25..1 of MaxPressedTime; falling rounds wait 4 s", () => {
  const world = makeWorld([], { seed: 11 });
  const lob = rifle({ MaxPressedTime: 1, TriggerSingle: 1, ordnance: { base: "shell", Velocity: 30, Gravity: 1 } });
  const unit = makeUnit({ weapons: [lob] });
  const c = makeController(unit, world);
  const expect = new ClassicRandom(11);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true);
  const fire = c.perception.fire;
  assert.equal(fire.hold, expect.range(0.25, 1));
  const weapon = unit.combat.arsenal.current(0);
  weapon.charging = true;
  weapon.held = fire.hold - 0.01;
  c.fireA = false;
  assert.equal(consumeFire(c, unit, world), true, "keeps charging without a request");
  weapon.held = fire.hold;
  assert.equal(consumeFire(c, unit, world), false, "lets go at the hold");
  assert.equal(fire.lastRelease, world.time);
  weapon.charging = false;
  world.time = f(world.time + 3.9);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), false, "a falling round waits 4 s after a release");
  world.time = f(fire.lastRelease + 4);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true, "4 s exactly is enough");
});

test("charging: after a shot the hold is drawn from the weapon timer's limit, its ShotDelay", () => {
  // The bowcaster in steady fire: ShotDelay 0.35 s of a 1.75 s charge, so a
  // bot's bolts are single ones (the original game reads the timer's limit).
  const world = makeWorld([], { seed: 5 });
  const bow = rifle({ MaxPressedTime: 1.75, ShotDelay: 0.35, TriggerSingle: 1 });
  const unit = makeUnit({ weapons: [bow] });
  const c = makeController(unit, world);
  const weapon = unit.combat.arsenal.current(0);
  weapon.limit = 0.35;
  const expect = new ClassicRandom(5);
  c.fireA = true;
  assert.equal(consumeFire(c, unit, world), true);
  assert.equal(c.perception.fire.hold, expect.range(f(0.35 * 0.25), f(0.35)));
  assert.ok(c.perception.fire.hold <= 0.35);
});

// ---------------------------------------------------------------------------
// Ranges, selection and utility.

test("effective range: implicit doubling, the scale and the extreme cap", () => {
  const soldier = makeUnit();
  const walker = makeUnit({ kind: 2 });
  const c = { rangeScale: 1 };
  const implicit = { ai: { ...ENGINE(), implicitRange: true, maxRange: 60, extremeRange: 0 } };
  assert.equal(effectiveRange(soldier, c, implicit), 60);
  assert.equal(effectiveRange(walker, c, implicit), 120, "doubled for a non-infantry owner");
  const capped = { ai: { ...ENGINE(), implicitRange: false, maxRange: 48, extremeRange: 64 } };
  assert.equal(effectiveRange(soldier, { rangeScale: 2 }, capped), 64, "scaled past the extreme: max(extreme, range)");
  assert.equal(effectiveRange(soldier, { rangeScale: 1 }, capped), 48);
  const low = { ai: { ...ENGINE(), implicitRange: false, maxRange: 48, extremeRange: 20 } };
  assert.equal(effectiveRange(soldier, { rangeScale: 2 }, low), 48);
  assert.equal(expandedRange(soldier, c, capped), f(48 + 30), "plus 5 s of the nominal speed");
});

function ENGINE() {
  return { minRange: 0, optimalRange: 30, maxRange: 60, implicitRange: true, extremeRange: 0, relationMask: 1, categoryMask: 0xffffff9f };
}

test("weapon selection: in band first, capacity over 20, then utility, ties earlier", () => {
  const world = makeWorld();
  const near = makeUnit({ team: 2, at: [0, 0, 12] });
  const low = rifle({ class: "a", ammo: 1, RoundsPerClip: 10, ordnance: bolt({ Damage: 500 }) });
  const big = rifle({ class: "b", ammo: 2, RoundsPerClip: 20, ordnance: bolt({ Damage: 100 }) });
  const unit = makeUnit({ weapons: [low, big] });
  const c = makeController(unit, world);
  const out = {};
  selectWeapon(c, unit, near, false, world, out);
  assert.equal(out.weapon.def.class, "b", "capacity above 20 wins over higher utility");
  assert.ok(out.inBand);
  // Out of band: the expanded range still allows a fallback.
  const far = makeUnit({ team: 2, at: [0, 0, 60] });
  selectWeapon(c, unit, far, false, world, out);
  assert.ok(out.ok && !out.inBand);
  // Strict bounds, eye to aim point (level here: 12 apart): exactly at
  // MinRange or at the effective range is out of band.
  const band = (minRange, maxRange) => {
    const u = makeUnit({ at: [0, -0.7, 0], weapons: [rifle({ ai: { ...rifle().ai, minRange, maxRange, extremeRange: 0 } })] });
    selectWeapon(makeController(u, world), u, near, false, world, out);
    return out.ok && out.inBand;
  };
  assert.equal(band(12, 48), false, "MinRange^2 < d^2 is strict");
  assert.equal(band(11.99, 48), true);
  assert.equal(band(0, 12), false, "d^2 < range^2 is strict");
  assert.equal(band(0, 12.01), true);
  // A tie in capacity class and utility keeps the earlier weapon.
  const twin = makeUnit({ weapons: [rifle({ class: "first" }), rifle({ class: "second" })] });
  selectWeapon(makeController(twin, world), twin, near, false, world, out);
  assert.equal(out.weapon.def.class, "first");
});

test("grenade priority on the roll's three-second slot or a recent cluster", () => {
  const world = makeWorld();
  const foe = makeUnit({ team: 2, at: [0, 0, 16] });
  const unit = makeUnit({ weapons: [rifle(), grenade()] });
  const c = makeController(unit, world);
  const out = {};
  c.roll = 3;
  world.time = 9.5; // trunc(9.5) / 3 = 3: slot 3
  selectWeapon(c, unit, foe, false, world, out);
  assert.equal(out.weapon.def.base, "grenade", "the roll's slot");
  assert.equal(out.action, ACTION.GRENADE);
  world.time = 12.5; // slot 4
  selectWeapon(c, unit, foe, false, world, out);
  assert.equal(out.weapon.def.base, "cannon");
});

test("utility: melee and hero cannons 1000, damage against half the maximum health, slow targets for grenades", () => {
  const owner = makeUnit();
  const target = makeUnit({ team: 2 });
  const out = {};
  const saber = new Weapon({ class: "s", base: "melee", channel: 0, ammo: 0, RoundsPerClip: 0 }, { player: false });
  assert.deepEqual({ ...weaponUtility(owner, saber, target, out) }, { ok: true, score: 1000 });
  const hero = makeUnit({ hero: true });
  const gun = new Weapon(rifle(), { player: false });
  assert.equal(weaponUtility(hero, gun, target, out).score, 1000);
  // min(C, 50) x D > 0.5 x MaxHealth: 50 x 3 = 150 is not above 150.
  const weak = new Weapon(rifle({ ordnance: bolt({ Damage: 3 }) }), { player: false });
  assert.equal(weaponUtility(owner, weak, target, out).ok, false);
  const ok = new Weapon(rifle({ ordnance: bolt({ Damage: 3.01 }) }), { player: false });
  assert.equal(weaponUtility(owner, ok, target, out).ok, true);
  // Hover targets: a grenade needs the target nearly still (speed^2 < 144).
  // (A crack grenade's mask: TargetVehicle set.)
  const hover = makeUnit({ team: 2, kind: 6, maxHealth: 5000, classDef: { HealthType: "vehicle" } });
  const crack = { ...grenade().ai, categoryMask: 0xffffff8f };
  const nade = new Weapon(grenade({ ai: crack, ordnance: { base: "sticky", Damage: 10, VehicleScale: 1 } }), { player: false });
  hover.velocity.set(11.9, 0, 0);
  assert.equal(weaponUtility(owner, nade, hover, out).ok, true);
  hover.velocity.set(12, 0, 0);
  assert.equal(weaponUtility(owner, nade, hover, out).ok, false);
  // The category mask decides for a soldier's soldier target, as in the
  // original game; other targets go by the damage rules alone: the mask
  // then only keeps the weapon's own target clear.
  const vehicleOnly = new Weapon(rifle({ ai: { ...rifle().ai, categoryMask: 8 } }), { player: false });
  assert.equal(weaponUtility(owner, vehicleOnly, target, out).ok, false);
  const personsOnly = new Weapon(grenade({ ordnance: { base: "sticky", Damage: 10, VehicleScale: 1 } }), { player: false });
  hover.velocity.set(0, 0, 0);
  assert.equal(weaponUtility(owner, personsOnly, hover, out).ok, true);
  // A turret control (classifier 7) with a cannon whose masks clear every
  // category (the Hoth turrets') still rates a soldier.
  const gunner = makeUnit({ kind: 7 });
  const noMask = new Weapon(rifle({ ai: { ...rifle().ai, categoryMask: 0xffffff80 } }), { player: false });
  assert.equal(weaponUtility(gunner, noMask, target, out).ok, true);
  assert.equal(capacity(gun), f(4 * 50));
});

test("SetTarget applies the selected weapon's relationship and category masks", () => {
  const world = makeWorld();
  const unit = makeUnit({ weapons: [rifle({ ai: { ...rifle().ai, categoryMask: 8 } })] });
  const foe = makeUnit({ team: 2 });
  world.units.add(unit);
  world.units.add(foe);
  const c = makeController(unit, world);
  setTarget(c, unit, foe, world);
  assert.equal(c.target.unit, foe, "the controller's target is set as given");
  assert.equal(c.perception.fire.weaponTarget, null, "the weapon's masks reject a person");
});

test("validated fire goes by the controllable's target, not the weapon's masked one", () => {
  // A cannon whose masks clear every category (the Hoth turrets'): its own
  // target stays clear, yet a target in its cone and range may be fired at.
  const unit = makeUnit({ kind: 7, weapons: [rifle({ ai: { ...rifle().ai, categoryMask: 0xffffff80 } })] });
  const foe = makeUnit({ team: 2, at: [0, 0, 20] });
  const world = makeWorld([unit, foe]);
  const c = makeController(unit, world);
  setTarget(c, unit, foe, world);
  assert.equal(c.perception.fire.weaponTarget, null);
  unit.forward(c.perception.aimer.direction);
  assert.equal(validateFire(c, unit, foe, world), true);
  c.target = null;
  assert.equal(validateFire(c, unit, foe, world), false, "not a target it does not hold");
});

test("a shot at, or damage to, a turret or hull alerts its AI crew", () => {
  // The registry entry of a manned turret (TurretTarget) has no controller
  // of its own; its gunner's seat control (SeatUnit) does.
  const gunner = makeUnit({ team: 1 });
  const world = makeWorld([gunner]);
  const seat = { controller: null };
  const seatController = makeController(gunner, world);
  seat.controller = seatController;
  const lease = { human: false, released: false, live: true, controllable: seat, unit: gunner };
  const turret = { lease };
  const entry = { alive: true, team: 1, generation: 1, turret, controller: null, position: new THREE.Vector3() };
  assert.deepEqual(alertControllers(entry), [seatController]);
  const shooter = makeUnit({ team: 2, at: [0, 0, 30] });
  world.units.add(shooter);
  assert.equal(notifyTargetedShot(world, shooter, entry), true);
  assert.equal(seatController.shot.pending, true);
  seatController.shot.pending = false;
  assert.equal(notifyDamage(world, entry, shooter, { amount: 5 }, true, new THREE.Vector3(0, 0, 30)), true);
  assert.equal(seatController.shot.pending, true);
  assert.equal(seatController.shot.known, true);
  // A hull: every AI seat aboard, a human's not.
  const pilot = makeController(makeUnit({ team: 1 }), world);
  const vehicle = { seats: [{ lease: { human: false, live: true, controllable: { controller: pilot } } }, { lease: { human: true, live: true } }] };
  assert.deepEqual(alertControllers({ alive: true, team: 1, vehicle, controller: null }), [pilot]);
  // A seated soldier is alerted through his seat's control.
  const sitter = makeUnit({ team: 1 });
  sitter.seat = { human: false, controllable: { controller: seatController } };
  assert.deepEqual(alertControllers(sitter), [seatController]);
});

// ---------------------------------------------------------------------------
// Fire validation and protection.

test("firing cone: 22.5 degrees (45 melee), aimer agreement, 20 units off the line beyond 50", () => {
  const world = makeWorld();
  const unit = makeUnit({ at: [0, -1.7, 0] });
  const c = makeController(unit, world);
  c.perception.aimer.reset(new THREE.Vector3(0, 0, 1));
  const at = (deg, d = 30) => new THREE.Vector3(Math.sin((deg * Math.PI) / 180) * d, 0, Math.cos((deg * Math.PI) / 180) * d);
  assert.equal(firingCone(c, unit, at(22)), true);
  assert.equal(firingCone(c, unit, at(23)), false);
  c.perception.aimer.reset(new THREE.Vector3(Math.sin(0.5), 0, Math.cos(0.5)));
  assert.equal(firingCone(c, unit, at(0)), false, "the aimer outside the cone");
  c.perception.aimer.reset(new THREE.Vector3(0, 0, 1));
  assert.equal(firingCone(c, unit, at(9, 120)), true, "18.8 off the line at 120");
  assert.equal(firingCone(c, unit, at(10, 120)), false, "20.8 off the line at 120, though within 22.5 deg");
  assert.equal(firingCone(c, unit, at(20, 49)), true, "inside 50 only the cone counts");
  assert.equal(firingCone(c, unit, new THREE.Vector3(19.9, 0, 100)), true);
  assert.equal(firingCone(c, unit, new THREE.Vector3(20, 0, 100)), false, "|r x d|^2 < 400 is strict");
  const sniperUnit = makeUnit({ at: [0, -1.7, 0], weapons: [sniper()] });
  const sc = makeController(sniperUnit, world);
  sc.perception.aimer.reset(new THREE.Vector3(0, 0, 1));
  assert.equal(firingCone(sc, sniperUnit, new THREE.Vector3(0.24, 0, 80)), true);
  assert.equal(firingCone(sc, sniperUnit, new THREE.Vector3(0.25, 0, 80)), false, "0.25 off the line for a zoomed single shot");
});

test("protected humans: allied humans within 50 veto a shot through them, hostile ones never", () => {
  const bot = makeUnit({ at: [0, 0, 0] });
  const friend = makeUnit({ at: [0, 0, 10], human: true, team: 1 });
  const enemy = makeUnit({ at: [3, 0, 10], human: true, team: 2 });
  const world = makeWorld([bot, friend, enemy]);
  const c = makeController(bot, world);
  rebuildProtectedHumans(world);
  assert.deepEqual(c.perception.fire.protected.map((r) => r.unit), [friend]);
  bot.soldier.aimPitch = Math.atan2(1 - 1.7, 10);
  assert.equal(protectedVeto(c, bot, bot.combat.arsenal.current(0)), true);
  bot.soldier.aimYaw = Math.PI;
  assert.equal(protectedVeto(c, bot, bot.combat.arsenal.current(0)), false, "behind the bot");
});

// ---------------------------------------------------------------------------
// Heaps.

test("target heap: 112 entries, strict replacement of the scanned minimum, native order", () => {
  const heap = new PriorityHeap(112, 20000);
  for (let id = 1; id <= 112; id++) assert.equal(heap.insert(100, id), null);
  assert.equal(heap.insert(100, 113), 113, "equal newcomer dropped");
  assert.equal(heap.insert(99, 114), 114, "lower newcomer dropped");
  assert.equal(heap.minIndex(), 55, "scan from floor(112/2), earlier index on ties (zero-based)");
  assert.equal(heap.insert(101, 115), 56, "replaces identity 56");
  assert.equal(heap.peek(), 115, "and bubbles to the root");
  assert.equal(heap.size, 112);
  const native = oracle("round8/combat/native-fixtures.json");
  if (native) assert.deepEqual(heap.slots(), native.targetHeap.heapAfterReplacement);
  const equal = new PriorityHeap(112, 20000);
  for (let id = 1; id <= 112; id++) equal.insert(100, id);
  const order = [];
  while (equal.size) order.push(equal.pop());
  assert.deepEqual(order, [1, ...Array.from({ length: 111 }, (_, i) => 112 - i)]);
  if (native) assert.deepEqual(order, native.equalHeapPopOrder);
  // Equal children: the left one rises.
  const kids = new PriorityHeap(4, 20000);
  [[100, "a"], [90, "b"], [90, "c"], [10, "d"]].forEach(([p, id]) => kids.insert(p, id));
  kids.pop();
  assert.equal(kids.peek(), "b");
});

test("deferred LOS queue: 200 entries, overflow count, drain and budget", () => {
  const los = new DeferredLos();
  const world = makeWorld();
  const units = [];
  const controllers = [];
  for (let i = 0; i < 3; i++) {
    const u = makeUnit({ at: [i * 3, 0, 0] });
    world.units.add(u);
    units.push(u);
    controllers.push(makeController(u, world));
  }
  const foe = makeUnit({ team: 2, at: [0, 0, 20] });
  world.units.add(foe);
  for (let i = 0; i < 200; i++) los.submit(controllers[i % 3], ref(foe), 50);
  assert.equal(los.active.size, 200);
  los.submit(controllers[0], ref(foe), 50);
  assert.equal(los.overflow, 1, "an equal request to a full queue is counted and dropped");
  los.submit(controllers[1], ref(foe), 51);
  assert.equal(los.overflow, 1, "a higher one displaces the minimum");
  assert.equal(los.active.size, 200);
  const own = (c) => los.active.slots().filter((r) => r.controller === c).length;
  const others = own(controllers[1]) + own(controllers[2]);
  los.drain(controllers[0]);
  assert.equal(own(controllers[0]), 0, "a new scan drops the controller's old requests");
  assert.equal(los.active.size, others);
  // A budget of five: five sight tests, each popping one request.
  let tested = 0;
  world.los = () => (tested++, false);
  assert.equal(los.service(world, 5), true);
  assert.equal(tested, 5);
  assert.equal(los.active.size, others - 5);
  // The 51 went first; with clear sight now, the next test makes a candidate,
  // and that controller's remaining requests are skipped without a test.
  world.los = () => (tested++, true);
  tested = 0;
  los.service(world, 1);
  assert.equal(tested, 1);
  const holder = controllers.find((c) => c.perception.candidate);
  assert.ok(holder);
  assert.equal(holder.perception.candidate.unit, foe);
  const other = controllers[holder === controllers[1] ? 2 : 1];
  const pending = own(holder);
  assert.ok(pending > 0);
  tested = 0;
  los.service(world, 1);
  assert.equal(tested, 1);
  assert.ok(other.perception.candidate);
  // Both hold candidates now: the rest go untested.
  tested = 0;
  assert.equal(los.service(world, 1000), false);
  assert.equal(tested, 0);
  assert.equal(los.active.size, 0);
  // So do requests for a dead target.
  for (const c of controllers) c.perception.candidate = null;
  los.submit(controllers[0], ref(foe), 9);
  foe.alive = false;
  assert.equal(los.service(world, 1000), false);
  assert.equal(tested, 0);
});

// ---------------------------------------------------------------------------
// Sight: priority, cost, cone, range, thinning.

test("matchup priority and cost, with the +10000 weapon-only penalty", () => {
  assert.equal(matchupPriority(0, 0), 10);
  assert.equal(matchupPriority(0, 2), 20);
  assert.equal(matchupPriority(0, 6), 15);
  assert.equal(matchupPriority(0, 7), 0);
  assert.equal(matchupPriority(6, 5), 10);
  assert.equal(matchupPriority(5, 4), 10);
  assert.equal(matchupPriority(5, 9), 10);
  assert.equal(matchupPriority(9, 0), 0);
  const unit = makeUnit({ at: [0, -0.7, 0] });
  const foe = makeUnit({ team: 2, at: [0, 0, 12.9] });
  assert.equal(rawCost(unit, foe), 12 * 11, "trunc(distance) x (21 - priority)");
  // A hero target is weapon-only (no pattern): queued at 20000 - (cost + 10000).
  const world = makeWorld();
  const hero = makeUnit({ team: 2, at: [0, 0, 12.9], hero: true });
  world.units.add(unit);
  world.units.add(hero);
  const c = makeController(unit, world);
  scan(c, world, (...a) => dispatch(...a));
  assert.equal(c.perception.heap.peekPriority(), 20000 - (12 * 11 + 10000));
});

test("sight cone at 77.5 degrees and the strict (30 + 20 r) range", () => {
  const unit = makeUnit({ at: [0, -0.7, 0] });
  const at = (deg, d) => makeUnit({ team: 2, at: [Math.sin((deg * Math.PI) / 180) * d, 0, Math.cos((deg * Math.PI) / 180) * d] });
  assert.ok(coneDistanceSq(unit, at(77, 20)) > 0);
  assert.equal(coneDistanceSq(unit, at(78, 20)), -2);
  assert.equal(coneDistanceSq(unit, at(180, 20)), -2);
  const r = boundRadius(unit);
  assert.equal(r, f(Math.sqrt(f(f(2 * f(f(0.7200000286) * f(0.7200000286))) + f(0.25 * f(f(2.0999999762) * f(2.0999999762)))))));
  assert.ok(Math.abs(r - 1.4626346) < 1e-6, "standing soldier radius");
  unit.soldier.posture = "crouch";
  assert.ok(Math.abs(boundRadius(unit) - 1.1022704) < 1e-6);
  unit.soldier.posture = "prone";
  assert.ok(Math.abs(boundRadius(unit) - 1.6220657) < 1e-6);
  const foe = makeUnit({ team: 2 });
  assert.equal(sightRange(unit, foe), f(f(f(boundRadius(foe) * 20)) + 30));
  // A standing soldier's sight range as measured in the original game:
  // 20 r = 29.2526912689, range 59.2526931763.
  assert.equal(f(boundRadius(foe) * 20), f(29.2526912689));
  assert.equal(sightRange(unit, foe), f(59.2526931763));
  const flyer = makeUnit({ kind: 5 });
  assert.equal(sightRange(flyer, foe), f(sightRange(unit, foe) * 2));
});

test("thinning: no draws up to four candidates; 1-in-2 skips beyond; the retained target ranks at least 110", () => {
  const world = makeWorld([], { seed: 3 });
  const unit = makeUnit();
  const c = makeController(unit, world);
  const foes = Array.from({ length: 6 }, (_, i) => makeUnit({ team: 2, at: [0, 0, 10 + i] }));
  const submitted = [];
  const queue = { submit: (controller, r, p) => submitted.push([r?.unit ?? null, p]) };
  const fill = (n) => {
    c.perception.heap.clear();
    foes.slice(0, n).forEach((u, i) => c.perception.heap.insert(1000 - i, ref(u)));
  };
  fill(4);
  const before = world.random.state;
  thin(c, world, null, queue);
  assert.equal(world.random.state, before, "four candidates: no draws");
  assert.deepEqual(submitted.map(([, p]) => p), [112, 111, 110, 109]);
  submitted.length = 0;
  fill(6);
  const expect = new ClassicRandom(world.random.state);
  thin(c, world, ref(foes[5]), queue);
  // Replay: the same draws decide the same skips.
  const order = [];
  const pending = foes.slice(0, 6);
  let rank = 112;
  while (pending.length) {
    let cur = pending.shift();
    let skip = true;
    while (pending.length && skip) {
      skip = expect.int(1, 2) !== 1;
      if (cur !== foes[5]) {
        if (skip) cur = pending.shift();
      } else skip = false;
    }
    order.push([cur, cur === foes[5] ? Math.max(rank, 110) : rank]);
    rank -= 1;
  }
  assert.deepEqual(submitted, order);
  assert.equal(world.random.state, expect.state);
});

test("recording a target flags a cluster of three soldiers with no ally of the attacker near", () => {
  const attacker = makeUnit({ team: 1 });
  const target = makeUnit({ team: 2, at: [0, 0, 30] });
  const mates = [makeUnit({ team: 2, at: [2, 0, 30] }), makeUnit({ team: 2, at: [-2, 0, 30] })];
  const world = makeWorld([attacker, target, ...mates]);
  const c = makeController(attacker, world);
  recordTarget(c, target, new THREE.Vector3(0, 1, 30), world);
  assert.equal(c.perception.candidate.unit, target);
  const flagged = world.time;
  // The grenade priority then prefers a grenade for 5 s.
  const out = {};
  const thrower = makeUnit({ weapons: [rifle(), grenade()], at: [0, 0, 14] });
  world.units.add(thrower);
  const tc = makeController(thrower, world);
  tc.roll = 1;
  world.time = flagged + 4.9;
  selectWeapon(tc, thrower, target, false, world, out);
  assert.equal(out.weapon.def.base, "grenade");
  // An ally of the attacker within 9 aborts the scan.
  const target2 = makeUnit({ team: 2, at: [50, 0, 50] });
  const mates2 = [makeUnit({ team: 2, at: [52, 0, 50] }), makeUnit({ team: 2, at: [48, 0, 50] }), makeUnit({ team: 1, at: [50, 0, 52] })];
  [target2, ...mates2].forEach((u) => world.units.add(u));
  world.time = 100;
  recordTarget(c, target2, new THREE.Vector3(50, 1, 50), world);
  thrower.position.set(50, 0, 34);
  world.time = 100.5; // slot 3 != roll 1
  selectWeapon(tc, thrower, target2, false, world, out);
  assert.equal(out.weapon.def.base, "cannon");
});

// ---------------------------------------------------------------------------
// The target tracker.

test("tracker: fresh match requests B; retention 3 s; cheaper candidates switch at once", () => {
  const unit = makeUnit({ at: [0, -0.7, 0] });
  const a = makeUnit({ team: 2, at: [0, 0, 30] });
  const b = makeUnit({ team: 2, at: [0, 0, 10] });
  const world = makeWorld([unit, a, b]);
  const c = makeController(unit, world);
  const tracker = new TargetTracker(c);
  c.perception.candidate = ref(a);
  c.perception.candidatePoint.set(0, 1, 30);
  tracker.update(world, null);
  assert.equal(tracker.target, a);
  assert.equal(tracker.fresh, true);
  assert.deepEqual([c.fireA, c.fireB], [false, true], "fresh: validated fire");
  // The candidate disappears: memory, countdown 3 s.
  c.perception.candidate = null;
  tracker.select(world);
  assert.equal(tracker.target, a);
  assert.equal(tracker.fresh, false);
  assert.equal(tracker.deadline, f(world.time + 3));
  world.time = tracker.deadline;
  tracker.select(world);
  assert.equal(tracker.target, a, "at the deadline it still holds");
  world.time = f(tracker.deadline + 0.01);
  tracker.select(world);
  assert.equal(tracker.target, null, "past it, the (missing) candidate replaces it");
  // A cheaper candidate replaces at once.
  c.perception.candidate = ref(a);
  tracker.select(world);
  c.perception.candidate = ref(b);
  tracker.select(world);
  assert.equal(tracker.target, b);
  // Frozen (cover): the deadline is FLT_MAX.
  tracker.freeze(true, world.time);
  c.perception.candidate = null;
  tracker.select(world);
  assert.equal(tracker.deadline, 3.4028234663852886e38);
});

test("remembered target: direct fire only within 3 units of the target, in cone and clear", () => {
  const unit = makeUnit({ at: [0, -0.7, 0] });
  const foe = makeUnit({ team: 2, at: [0, 0, 20] });
  const world = makeWorld([unit, foe]);
  const c = makeController(unit, world);
  c.perception.aimer.reset(new THREE.Vector3(0, 0, 1));
  const tracker = new TargetTracker(c);
  tracker.reset(foe);
  // The remembered point is the target's sight point (midway eye/aim point).
  tracker.point.set(0, 1.35, 20);
  tracker.fireControls(world);
  assert.deepEqual([c.fireA, c.fireB], [true, false]);
  foe.position.set(0, 0, 23.1);
  tracker.fireControls(world);
  assert.equal(c.fireA, false, "moved 3.1: the point is stale");
  foe.position.set(0, 0, 20);
  world.los = () => false;
  tracker.fireControls(world);
  assert.equal(c.fireA, false, "no clear ray");
});

// ---------------------------------------------------------------------------
// Hearing, danger and alerts.

test("acoustic broadcast: min(30, noise), enemies only, the vertical gate", () => {
  const source = makeUnit({ team: 2 });
  source.velocity.set(4, 0, 0); // noise 12
  const near = makeUnit({ team: 1, at: [11, 0, 0] });
  const far = makeUnit({ team: 1, at: [14.5, 0, 0] });
  const above = makeUnit({ team: 1, at: [0, 6, 0.5] });
  const close = makeUnit({ team: 1, at: [0, 3.4, 0] });
  const friend = makeUnit({ team: 2, at: [2, 0, 0] });
  const world = makeWorld([source, near, far, above, close, friend]);
  for (const u of [near, far, above, close, friend]) makeController(u, world);
  broadcastAcoustic(source, world);
  assert.equal(near.controller.acoustic.length, 1);
  assert.equal(far.controller.acoustic.length, 0, "beyond 12 (query uses the sphere)");
  assert.equal(above.controller.acoustic.length, 0, "steeply above, beyond 3.5 m");
  assert.equal(close.controller.acoustic.length, 1, "steeply above but within 3.5 m");
  assert.equal(friend.controller.acoustic.length, 0, "allies are not told");
  // A weapon in use: half its sound's MuteDistance (400 when unknown), but
  // never past 30.
  source.velocity.set(0, 0, 0);
  const idle = makeUnit({ team: 1, at: [5, 0, 0] });
  world.units.add(idle);
  makeController(idle, world);
  assert.equal(broadcastAcoustic(source, world), 0, "standing still with the weapon idle: silent");
  source.combat.arsenal.current(0).wait = 0.2;
  const edge = makeUnit({ team: 1, at: [29, -1, 0] });
  const beyond = makeUnit({ team: 1, at: [32, -1, 0] });
  [edge, beyond].forEach((u) => {
    world.units.add(u);
    makeController(u, world);
  });
  broadcastAcoustic(source, world);
  assert.equal(edge.controller.acoustic.length, 1, "firing: heard at 29");
  assert.equal(beyond.controller.acoustic.length, 0, "but not past 30 (plus the hearer's sphere)");
});

test("grenade danger delays per tier, one draw per call, strict renotification", () => {
  assert.deepEqual(dangerDelayRange(4), [f(1.1), f(2.1)]);
  assert.deepEqual(dangerDelayRange(13), [f(1.1), f(2.1)]);
  assert.deepEqual(dangerDelayRange(14), [f(0.6), f(1.3)]);
  assert.deepEqual(dangerDelayRange(16), [f(0.6), f(1.3)]);
  assert.deepEqual(dangerDelayRange(17), [0, f(0.8)]);
  const owner = makeUnit({ team: 2 });
  const victim = makeUnit({ team: 1, at: [3, 0, 0] });
  const ally = makeUnit({ team: 2, at: [0, 0, 3] });
  const world = makeWorld([owner, victim, ally], { seed: 21 });
  makeController(victim, world);
  makeController(ally, world);
  const state = { firstContact: Infinity, lastNotify: -1 };
  const before = world.random.state;
  assert.equal(notifyDanger(world, state, new THREE.Vector3(), owner, 4), 0, "before first contact");
  assert.notEqual(world.random.state, before, "the delay is drawn regardless");
  state.firstContact = world.time;
  const expect = new ClassicRandom(world.random.state);
  const delay = expect.range(f(1.1), f(2.1));
  world.time = f(state.firstContact + delay);
  assert.equal(notifyDanger(world, state, new THREE.Vector3(), owner, 4), 0, "equality waits");
  world.time = f(world.time + 2.2);
  let warned = null;
  assert.equal(notifyDanger(world, state, new THREE.Vector3(), owner, 4, (u) => (warned = u)), 2, "allies are warned too");
  assert.ok(victim.controller.danger.pending && ally.controller.danger.pending);
  assert.ok(warned);
  assert.equal(notifyDanger(world, state, new THREE.Vector3(), owner, 4), 0, "within 1 s of the last");
});

test("the binding's grenade step: state on the round, first contact kept, the FleeSound as a vo record", async () => {
  const { SimEvents } = await import("../src/sim-events.js");
  const owner = makeUnit({ team: 2 });
  const victim = makeUnit({ team: 1, at: [2, 0, 0] });
  const world = makeWorld([owner, victim], { seed: 8, difficulty: { enemySkill: 17 } });
  world.events = new SimEvents();
  makeController(victim, world);
  const binding = classicBinding({ world });
  const round = {};
  const vo = [];
  world.events.listen((e) => e.kind === "vo" && vo.push(e));
  world.time = 9;
  assert.equal(binding.grenadeUpdate(round, new THREE.Vector3(), owner), 0, "no contact yet");
  assert.deepEqual({ ...round.aiDanger }, { firstContact: Infinity, lastNotify: -1 });
  assert.equal(binding.grenadeUpdate(round, new THREE.Vector3(), owner, 9), 0, "the delay runs from the contact");
  assert.equal(binding.grenadeUpdate(round, new THREE.Vector3(), owner, 9.5), 0);
  assert.equal(round.aiDanger.firstContact, 9, "the earliest contact stands");
  world.time = 10.9; // past 9 + [0, 0.8) on Hard's row
  assert.equal(binding.grenadeUpdate(round, new THREE.Vector3(), owner, 9.5), 1);
  assert.ok(victim.controller.danger.pending);
  assert.equal(vo.length, 1);
  assert.equal(vo[0].property, "FleeSound");
  assert.equal(vo[0].source.unit, victim);
});

test("targeted shots alert only enemies, with the shooter's position", () => {
  const shooter = makeUnit({ team: 2, at: [5, 0, 5] });
  const target = makeUnit({ team: 1 });
  const friend = makeUnit({ team: 2 });
  const world = makeWorld([shooter, target, friend]);
  makeController(target, world);
  makeController(friend, world);
  assert.equal(notifyTargetedShot(world, shooter, target), true);
  assert.deepEqual(target.controller.shot.point.toArray(), [5, 0, 5]);
  assert.equal(notifyTargetedShot(world, shooter, friend), false);
});

test("the journal's shots and damage become alerts (a bot shooter's channel target when none is named)", async () => {
  const { SimEvents } = await import("../src/sim-events.js");
  const shooter = makeUnit({ team: 2, at: [5, 0, 5] });
  const target = makeUnit({ team: 1 });
  const bystander = makeUnit({ team: 1, at: [1, 0, 0] });
  const world = makeWorld([shooter, target, bystander]);
  world.events = new SimEvents();
  makeController(shooter, world);
  makeController(target, world);
  makeController(bystander, world);
  const binding = classicBinding({ world });
  // The controllable's target (SetTarget writes it whatever the weapon's
  // masks say), not the weapon's masked one.
  shooter.controller.target = ref(target);
  shooter.controller.perception.fire.weaponTarget = null;
  world.events.emit("shot", { source: ref(shooter), team: 2, position: new THREE.Vector3(5, 1.7, 5) });
  binding.ingest();
  assert.equal(target.controller.shot.pending, true, "the shooter's channel target is alerted");
  assert.equal(target.controller.shot.known, true);
  assert.deepEqual(target.controller.shot.point.toArray(), [5, 0, 5], "at the shooter's position");
  assert.equal(target.controller.shot.source, shooter.character);
  assert.equal(bystander.controller.shot.pending, false);
  world.events.emit("shot", { source: ref(shooter), target: ref(bystander), team: 2 });
  binding.ingest();
  assert.equal(bystander.controller.shot.pending, true, "a named target wins");
  // Damage: an attributed hostile source and more than 1 of damage.
  const hurt = makeUnit({ team: 1, at: [0, 0, 3] });
  world.units.add(hurt);
  makeController(hurt, world);
  world.events.emit("damage", { target: ref(hurt), amount: 1, attacker: ref(shooter) });
  binding.ingest();
  assert.equal(hurt.controller.shot.pending, false, "1 of damage is not enough");
  world.events.emit("damage", { target: ref(hurt), amount: 30, attacker: ref(shooter) });
  binding.ingest();
  assert.equal(hurt.controller.shot.pending, true);
  assert.equal(hurt.controller.shot.known, false, "position unknown unless the producer gives one");
});

// ---------------------------------------------------------------------------
// Dispatch and policies.

test("range policy table by enemy skill, Hard, and the roll", () => {
  const cases = [
    [10, false, [3, 3, 3, 3, 4, 5], [2, 2, 2, 3, 4, 5]],
    [14, false, [3, 3, 3, 3, 4, 5], [2, 2, 3, 3, 4, 5]],
    [17, false, [3, 3, 4, 4, 4, 5], [2, 3, 4, 4, 4, 5]],
    [17, true, [4, 4, 4, 4, 4, 5], [4, 4, 4, 4, 4, 5]],
  ];
  for (const [skill, hard, outBand, inBand] of cases) {
    for (let roll = 0; roll < 6; roll++) {
      const world = { difficulty: { enemySkill: skill, hard } };
      const out = rangePolicy({ roll }, world, { action: 1, inBand: false });
      assert.equal(out.pattern, outBand[roll], `skill ${skill} hard ${hard} roll ${roll}`);
      assert.equal(out.probability, 0.5);
      assert.equal(rangePolicy({ roll: roll + 6 }, world, { action: 1, inBand: true }).pattern, inBand[roll]);
    }
  }
  assert.equal(rangePolicy({ roll: 0 }, { difficulty: { enemySkill: 4 } }, { action: 2, inBand: true }).pattern, 2, "grenade in band");
  assert.equal(rangePolicy({ roll: 0 }, { difficulty: { enemySkill: 4 } }, { action: 2, inBand: false }).pattern, 0);
});

test("dispatch: scouts, assault melee, hero targets and vehicle targets", () => {
  const world = makeWorld();
  const foe = makeUnit({ team: 2, at: [0, 0, 12] });
  const scout = makeUnit({ classDef: { UnitType: "Scout" } });
  world.units.add(scout);
  assert.equal(dispatch(makeController(scout, world), scout, foe, false, world).pattern, 2, "scout with a rifle in band");
  const saber = { class: "s", base: "melee", channel: 0, ammo: 0, RoundsPerClip: 0, AnimationBank: "lightsabre",
    ai: { minRange: 0, optimalRange: 1.5, maxRange: 3, implicitRange: false, extremeRange: 1 } };
  const jedi = makeUnit({ classDef: { UnitType: "assault" }, weapons: [saber], hero: true, at: [0, 0, 10] });
  const e = dispatch(makeController(jedi, world), jedi, foe, false, world);
  assert.deepEqual([e.pattern, e.probability], [6, 0], "assault melee pursues");
  const villain = makeUnit({ team: 2, hero: true, at: [0, 0, 11] });
  assert.equal(dispatch(jedi.controller, jedi, villain, false, world).pattern, 0, "hero against hero: nothing");
  const trooper = makeUnit();
  const tc = makeController(trooper, world);
  const r = dispatch(tc, trooper, villain, false, world);
  assert.deepEqual([r.pattern, r.aux], [0, true], "a hero target is weapon-only");
  // A hover: a rifle (50 x 3 = 150 against half of 1000) is no use; a
  // missile launcher is. Even rolls take pattern 5, odd rolls 2 in band.
  const tank = makeUnit({ team: 2, kind: 6, at: [0, 0, 20], maxHealth: 1000, classDef: { HealthType: "vehicle" } });
  tc.roll = 2;
  const none = dispatch(tc, trooper, tank, false, world);
  assert.deepEqual([none.pattern, none.aux, none.probability], [0, false, f(0.75)], "the rifle cannot hurt it enough");
  const launcher = rifle({ class: "l", base: "launcher", AnimationBank: "bazooka", ammo: 4, RoundsPerClip: 1,
    ordnance: { base: "missile", Velocity: 35, Damage: 400, VehicleScale: 1 }, ai: { ...rifle().ai, categoryMask: 0xffffff8f } });
  const rocketeer = makeUnit({ weapons: [launcher] });
  const rc = makeController(rocketeer, world);
  const result = (roll, at) => {
    rc.roll = roll;
    tank.position.set(0, 0, at);
    return dispatch(rc, rocketeer, tank, false, world);
  };
  assert.deepEqual([result(2, 20).pattern, result(2, 20).action, result(2, 20).aux], [5, ACTION.LAUNCHER, true]);
  assert.equal(result(3, 20).pattern, 2);
  assert.equal(result(4, 60).pattern, 5, "even: pattern 5 out of band too");
  assert.equal(result(3, 60).pattern, 0, "odd and out of band: none");
  // An aircraft target: eligibility only.
  const flyer = makeUnit({ team: 2, kind: 5, at: [0, 10, 20], maxHealth: 1000, classDef: { HealthType: "vehicle" } });
  const air = dispatch(rc, rocketeer, flyer, false, world);
  assert.deepEqual([air.pattern, air.aux], [0, true]);
});

function makeAgent(unit, world, goal = { mode: 0 }, ai = {}) {
  const controller = makeController(unit, world, {
    ai: { createHelper: () => ({ tick() {}, stop() {} }), combat: null, ...ai },
    phase: 0,
    setStance() {},
  });
  return new UnitCombatAgent(controller, goal);
}

test("combat alert acceptance matches the native policy fixture", () => {
  const states = [2, 3, 4, 5];
  const points = { front: new THREE.Vector3(0, 0, 10), side: new THREE.Vector3(10, 0, 0), aboveFront: new THREE.Vector3(0, 100, 1) };
  const native = oracle("round9/combat/results-007f.json");
  // Without the oracle, the policy as the original game applies it.
  const rows = native?.combatAlertPolicy ?? states.flatMap((s) => [0, 1].flatMap((k) => Object.keys(points).map((p) => ({ state: s, known: k, point: p, accepted: !k || s === 3 || (s === 4 && p === "side") }))));
  assert.equal(rows.length, 24);
  const unit = makeUnit({ at: [0, 0, 0] });
  const agent = makeAgent(unit, makeWorld([unit]));
  for (const row of rows) {
    agent.state = row.state;
    assert.equal(agent.acceptsAlert(Boolean(row.known), points[row.point]), row.accepted, JSON.stringify(row));
  }
  // The flat cone: straight above-front is "front", 80 degrees off is not.
  assert.equal(inForwardCone(unit, new THREE.Vector3(Math.sin(1.396), 0, Math.cos(1.396))), false);
  assert.equal(inForwardCone(unit, new THREE.Vector3(Math.sin(1.222), 0, Math.cos(1.222))), true);
  // A zero-speed mount accepts what it cannot bring a weapon to bear on.
  const gun = makeUnit({ classDef: { ForwardSpeed: 0 } });
  const reach = (u, p) => p.x === 0 && p.y < 5;
  const mounted = makeAgent(gun, makeWorld([gun]), { mode: 0 }, { vehicles: { mountReach: reach } });
  mounted.state = 2;
  assert.equal(mounted.acceptsAlert(true, points.front), false, "reachable: the ordinary policy");
  assert.equal(mounted.acceptsAlert(true, points.side), true, "unreachable: accepted");
  // Walkers, hovers and flyers never take alerts.
  for (const kind of [2, 5, 6]) {
    const v = makeUnit({ kind });
    const a = makeAgent(v, makeWorld([v]));
    a.state = 3;
    assert.equal(a.acceptsAlert(false, points.front), false, `kind ${kind}`);
  }
});

test("the combat agent's other policies: no scanner-target goals, boarding only when uncontested", () => {
  const unit = makeUnit();
  const foe = makeUnit({ team: 2, at: [0, 0, 30] });
  const speeder = makeUnit({ team: 0, kind: 6, at: [5, 0, 0], classDef: { HealthType: "vehicle" } });
  const world = makeWorld([unit, foe, speeder]);
  const agent = makeAgent(unit, world);
  assert.equal(agent.acceptsTarget(foe), false, "the tracker follows the scanner; no new combat goal");
  assert.equal(agent.savesOnInterrupt(), false);
  assert.equal(agent.acceptsRecovery(), true);
  assert.equal(agent.acceptsBoard(speeder), true, "no hostile near: the base policy (a mobile vehicle)");
  foe.position.set(5, 0, 7);
  assert.equal(agent.acceptsBoard(speeder), false, "a hostile within 8 of it");
  foe.alive = false;
  assert.equal(agent.acceptsBoard(speeder), true, "the dead do not count");
  assert.equal(boardingClear(world, unit, speeder), true);
});

// ---------------------------------------------------------------------------
// End to end through the Classic AI's scheduler (the sibling modules bound,
// movement held still): two squads facing each other notice each other by
// sight, pick targets, filter their aim and press their triggers.

function post(team, x, z) {
  const position = new THREE.Vector3(x, 0, z);
  const matrix = new THREE.Matrix4().makeTranslation(x, 0, z).toArray();
  const inverse = new THREE.Matrix4().fromArray(matrix).invert();
  const size = [4, 2, 4];
  return {
    name: `cp${team}`, team, position, desc: { capture: { type: "box", size, matrix }, spawnPath: null },
    contains(point, scratch = new THREE.Vector3()) {
      scratch.copy(point).applyMatrix4(inverse);
      return Math.abs(scratch.x) < size[0] && Math.abs(scratch.y) < size[1] && Math.abs(scratch.z) < size[2];
    },
  };
}

test("two squads acquire each other, aim and open fire through the Classic AI step", async (t) => {
  let index, difficulty, events;
  try {
    index = await import("../src/ai/classic/index.js");
    difficulty = await import("../src/ai/classic/difficulty.js");
    events = await import("../src/sim-events.js");
  } catch (error) {
    return t.skip(`Classic AI modules not loadable: ${error.message}`);
  }
  const modules = await index.loadClassicModules();
  if (!modules.combat?.UnitCombatAgent || !modules.perception?.scan) return t.skip("combat modules not bound");
  const world = makeWorld([], { seed: 5 });
  world.time = 0;
  world.events = new events.SimEvents();
  world.battle = { characters: [] };
  world.nav = { hints: [] };
  world.difficulty = new difficulty.ClassicDifficulty({ tier: 2, playerTeam: 1 });
  const still = (controller) => ({
    controller, status: 0, arrived: true, dest: new THREE.Vector3(),
    goto(point) {
      this.dest.copy(point);
    },
    stop() {},
    tick() {},
    terminate() {},
    destinationPoint(out) {
      return out.copy(this.dest);
    },
  });
  const api = index.createClassicAI(world, { modules, quiet: true, createHelper: (agent, controller) => still(controller) });
  await api.ready;
  const squads = { 1: [], 2: [] };
  for (const team of [1, 2]) {
    for (let i = 0; i < 3; i++) {
      const unit = makeUnit({ team, at: [i * 3 - 3, 0, team === 1 ? 0 : 24], yaw: team === 1 ? 0 : Math.PI });
      unit.id = world.battle.characters.length + 1;
      Object.assign(unit.intent, { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false, use: false, reload: false });
      Object.assign(unit.soldier, { maxSpeed: 6, maxStrafe: 4.5, maxTurn: 4, mode: 0, rolled: false });
      // An objective across the field: the bots are busy (an agent running)
      // when they see each other.
      const character = { index: world.battle.characters.length, team, human: false, unit, controller: null, objective: post(team === 1 ? 2 : 1, 0, team === 1 ? 60 : -40) };
      unit.character = character;
      world.units.add(unit);
      world.battle.characters.push(character);
      api.attach(character);
      squads[team].push(unit);
    }
  }
  const all = [...squads[1], ...squads[2]];
  const previous = new Map(all.map((u) => [u, [0, 0]]));
  const agents = new Map(all.map((u) => [u, new Set()]));
  const shots = { 1: 0, 2: 0 };
  let aimed = 0;
  const h = 1 / 30;
  for (let step = 0; step < 30 * 6; step++) {
    world.time = f(world.time + h);
    api.update(h);
    for (const unit of all) {
      const s = unit.soldier;
      s.aimYaw -= unit.intent.turn * s.maxTurn * h;
      if (unit.intent.aim) aimed++;
      if (unit.controller?.agent instanceof modules.combat.UnitCombatAgent) agents.get(unit).add(unit.controller.agent);
      const input = modules.weaponsAi.arsenalInput(previous.get(unit), unit.intent);
      previous.set(unit, unit.intent.buttons.slice());
      const fired = unit.combat.arsenal.update(h, input, { blocked: false, stick: 0, posture: "stand", zoomed: false });
      shots[unit.team] += fired.length;
    }
  }
  for (const unit of all) {
    const target = unit.controller?.target?.unit;
    assert.ok(target && target.team !== unit.team, `unit ${unit.id} has an enemy target`);
    assert.equal(agents.get(unit).size, 1, "one combat agent keeps the fight");
  }
  assert.ok(shots[1] > 0 && shots[2] > 0, `both squads fired: ${JSON.stringify(shots)}`);
  assert.ok(aimed > 0, "the aimer's direction reaches intent.aim");
});

// ---------------------------------------------------------------------------
// The class/weapon extractor (synthetic archive).

function fnv1a(name) {
  let h = 2166136261;
  for (const ch of Buffer.from(name, "latin1")) {
    h ^= ch | 0x20;
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h >>> 0;
}

function chunk(tag, body) {
  const pad = (4 - (body.length % 4)) % 4;
  const head = Buffer.alloc(8);
  head.write(tag, 0, "latin1");
  head.writeUInt32LE(body.length, 4);
  return Buffer.concat([head, body, Buffer.alloc(pad)]);
}

function classChunk(tag, rec) {
  const parts = [chunk("TYPE", Buffer.from(rec.type + "\0", "latin1")), chunk("BASE", Buffer.from(rec.base + "\0", "latin1"))];
  for (const [key, value] of rec.props) {
    const h = Buffer.alloc(4);
    h.writeUInt32LE(key.startsWith("#") ? parseInt(key.slice(1), 16) : fnv1a(key));
    parts.push(chunk("PROP", Buffer.concat([h, Buffer.from(value + "\0", "latin1")])));
  }
  return chunk(tag, Buffer.concat(parts));
}

test("zero_ai_classes.py replays the parser on a synthetic archive", (t) => {
  const fixture = JSON.parse(fs.readFileSync(path.join(ROOT, "tests/fixtures/ai-combat/classes.json"), "utf8"));
  const body = Buffer.concat([...fixture.weapons.map((w) => classChunk("wpnc", w)), ...fixture.classes.map((c) => classChunk("entc", c))]);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ai-classes-"));
  const archive = path.join(dir, "test.lvl");
  const out = path.join(dir, "ai.json");
  fs.writeFileSync(archive, chunk("ucfb", body));
  const run = spawnSync("python3", ["tools/zero_ai_classes.py", "--archive", archive, "--out", out], { cwd: ROOT, encoding: "utf8" });
  if (run.error) return t.skip("python3 not available");
  assert.equal(run.status, 0, run.stderr);
  const data = JSON.parse(fs.readFileSync(out, "utf8"));
  assert.equal(data.schema, 1);
  const r = data.weapons.test_weap_rifle.ai;
  assert.deepEqual([r.minRange, r.optimalRange, r.maxRange, r.implicitRange], [2, 10, 20, false]);
  assert.equal(r.extremeRange, 30, "LockOnRange read first keeps the extreme range up");
  assert.equal(r.relationMask, 5);
  assert.equal(r.categoryMask, 9, "TargetAll 0 clears, then Person and Vehicle");
  assert.deepEqual([r.barrageMin, r.barrageMax, r.barrageDelay], [4, 7, 1.5]);
  assert.equal(r.zoomMax, 1, "ZoomMax 0 reads as 1");
  assert.equal(r.triggerSingle, true);
  assert.equal(r.action, 0, "pistol bank");
  assert.equal(r.engineClass, "WeaponCannon");
  assert.deepEqual(data.weapons.test_weap_rifle.raw.at(-1), ["0badf00d", null, "kept"], "unrecognized keys are kept in order");
  const lob = data.weapons.test_weap_lob;
  assert.equal(lob.ai.implicitRange, true);
  assert.equal(lob.ai.extremeRange, 40);
  assert.equal(lob.levels.length, 1);
  assert.deepEqual([lob.levels[0].at, lob.levels[0].ai.maxRange, lob.levels[0].ai.implicitRange, lob.levels[0].ai.categoryMask], [0.5, 15, false, 0xffffffff]);
  assert.equal(lob.ai.categoryMask, 0xffffff9f, "the level's TargetAll stays in the level");
  assert.equal(data.weapons.test_weap_default.ai.maxRange, 60);
  assert.ok(data.weapons.test_weap_default.defaulted.includes("MaxRange"));
  const soldier = data.classes.test_inf_soldier.ai;
  assert.deepEqual([soldier.role, soldier.sizeMask, soldier.maxHealth], [1, 8, 250]);
  assert.equal(soldier.fleeSound, "test_flee");
  assert.equal(soldier.tactical.ai.AISCFieldMoveOutSound, "test_moveout");
  assert.deepEqual(soldier.loadout.map((s) => [s.weapon, s.ammo, s.channel]), [["test_weap_rifle", 3, 0], ["test_weap_lob", 2, 1]]);
  assert.equal(data.classes.test_inf_blank.ai.role, 6, "unrecognized UnitType");
  // attachAIData hangs the weapon records on units.json-style classes.
  const units = { test_inf_soldier: { class: "test_inf_soldier", weapons: [{ class: "test_weap_rifle", charges: [] }, { class: "test_weap_lob", charges: [{ at: 0.5 }] }] } };
  assert.equal(attachAIData({ units }, data), 2, "units.json itself, or its units map");
  assert.equal(weaponAI(units.test_inf_soldier.weapons[0]).maxRange, 20);
  assert.equal(units.test_inf_soldier.weapons[1].charges[0].ai.maxRange, 15);
  assert.equal(units.test_inf_soldier.ai.role, 1);
  // Or by class name, for any weapon record of that class.
  const loose = { class: "test_weap_rifle", base: "cannon" };
  assert.equal(weaponAI(loose).maxRange, 60, "unregistered: the engine default");
  assert.equal(registerAIData(data), 3);
  assert.equal(weaponAI(loose).maxRange, 20);
  assert.equal(weaponAI({ class: "test_weap_rifle", ai: { maxRange: 7 } }).maxRange, 7, "a record's own ai wins");
  // Without any record, flattened keys on the class record count.
  const flat = weaponAI({ class: "test_unregistered", base: "cannon", MinRange: "2", MaxRange: 48, BarrageMax: 9 });
  assert.deepEqual([flat.minRange, flat.maxRange, flat.implicitRange, flat.barrageMax], [2, 48, false, 9]);
  fs.rmSync(dir, { recursive: true, force: true });
});
