// How fast a Classic bot fires each kind of weapon, end to end: the fire
// request through the consumer (src/ai/classic/weapons-ai.js), the trigger
// edges a unit takes from its button counts, and the weapon itself
// (src/weapons.js), at the simulation's 60 updates a second (docs/ai-combat.md,
// "Fire pace").
//
//   node --test tests/ai-fire-pace.test.js
//
// Synthetic weapons (not retail values) that exercise the retail rules.

import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import { Arsenal } from "../src/weapons.js";
import { arsenalInput, consumeFire } from "../src/ai/classic/weapons-ai.js";
import { Perception } from "../src/ai/classic/perception.js";

const HZ = 60;
const AIM = { stick: 0, posture: "stand", zoomed: false, blocked: false };

function bolt() {
  return { base: "bolt", Velocity: 150, LifeSpan: 1, Damage: 30, PersonScale: 1 };
}

const RANGES = { minRange: 0, optimalRange: 16, maxRange: 32, implicitRange: false, extremeRange: 0, relationMask: 1, categoryMask: 0xffffff87 };

// A semiautomatic pistol: one round per press, ShotDelay apart, heat-bound.
function pistol(extra = {}) {
  return {
    class: "test_pistol", base: "cannon", channel: 0, ammo: 0, RoundsPerClip: 0, ReloadTime: 2, ShotDelay: 0.2, TriggerSingle: 1,
    SalvoCount: 1, ShotsPerSalvo: 1, HeatPerShot: 0.2, HeatRecoverRate: 0.3, HeatThreshold: 0.25, YawSpread: 0, PitchSpread: 0,
    SpreadPerShot: 0, ZoomMax: 2, AnimationBank: "pistol", ordnance: bolt(),
    ai: { ...RANGES, barrageMin: 3, barrageMax: 6, barrageDelay: 2, triggerSingle: true, zoomMax: 2, action: 0 },
    ...extra,
  };
}

// An automatic rifle: rounds while the trigger is held, from a clip.
function rifle(extra = {}) {
  return {
    class: "test_rifle", base: "cannon", channel: 0, ammo: 4, RoundsPerClip: 60, ReloadTime: 1.5, ShotDelay: 0.2, TriggerSingle: 0,
    SalvoCount: 1, ShotsPerSalvo: 1, HeatPerShot: 0, HeatRecoverRate: 0, HeatThreshold: 1, YawSpread: 0, PitchSpread: 0,
    SpreadPerShot: 0, ZoomMax: 2, AnimationBank: "rifle", ordnance: bolt(),
    ai: { ...RANGES, maxRange: 48, barrageMin: 3, barrageMax: 6, barrageDelay: 2, triggerSingle: false, zoomMax: 2, action: 1 },
    ...extra,
  };
}

function makeBot(def) {
  const position = new THREE.Vector3();
  const unit = {
    id: 1, generation: 1, team: 1, kind: 0, alive: true, human: false, position,
    velocity: new THREE.Vector3(),
    combat: { arsenal: new Arsenal([def], { player: false }) },
    intent: { buttons: [0, 0] },
    origin: (out) => out.set(position.x, position.y + 1.7, position.z),
    forward: (out) => out.set(0, 0, 1),
  };
  const controller = { unit, generation: 1, fireA: false, fireB: false, target: null, rangeScale: 1 };
  controller.perception = new Perception(controller);
  unit.controller = controller;
  return { unit, controller };
}

// Seconds of a standing direct fire request. The consumer runs on the
// updates `execute` allows (every one for a bot near or in view; every other
// one for a bot the distance/visibility gate skips), the weapon every
// update. The button starts released, as after any update without a
// request. Returns the updates a fire cycle left the weapon on.
function standingFire(def, { seconds = 10, execute = () => true, seed = 3 } = {}) {
  const world = { time: 10, random: new ClassicRandom(seed) };
  const { unit, controller } = makeBot(def);
  const weapon = unit.combat.arsenal.current(0);
  consumeFire(controller, unit, world); // no request: the button goes to -2
  let previous = [...unit.intent.buttons];
  const input = arsenalInput(previous, unit.intent);
  const cycles = [];
  const heat = [];
  for (let tick = 0; tick < seconds * HZ; tick++) {
    world.time = 10 + (tick + 1) / HZ;
    if (execute(tick)) {
      controller.fireA = true;
      consumeFire(controller, unit, world);
    }
    arsenalInput(previous, unit.intent, input);
    previous = [...unit.intent.buttons];
    const shots = unit.combat.arsenal.update(1 / HZ, input, AIM);
    if (shots.length) cycles.push(tick);
    heat.push(weapon.heat);
  }
  return { cycles, heat, weapon };
}

const gapsOf = (ticks) => ticks.slice(1).map((t, i) => t - ticks[i]);

// The consumer's two-held, two-released presses themselves are pinned in
// tests/ai-combat.test.js; these follow them into the weapon.

test("a pistol near the player fires every ShotDelay (12 updates), never faster, until it overheats", () => {
  const { cycles, heat } = standingFire(pistol(), { seconds: 6 });
  assert.equal(cycles[0], 0, "a press from release fires at once");
  const gaps = gapsOf(cycles);
  const string = gaps.findIndex((g) => g !== 12);
  assert.ok(string >= 4, `a string of shots 0.2 s apart (${gaps.join(",")})`);
  for (const g of gaps) assert.ok(g >= 12, "a press inside ShotDelay is dropped");
  // 0.2 heat a round, 0.005 back an update: the seventh round takes it to 1.
  assert.equal(string, 6, "seven rounds, then overheated");
  const last = cycles[string];
  assert.ok(heat[last] >= 1, "the round that overheats it");
  // Silent until the heat is back down to HeatThreshold, then the next press.
  const resumed = cycles[string + 1];
  const cooled = heat.findIndex((h, t) => t > last && h <= 0.25);
  assert.ok(cooled > last && resumed >= cooled && resumed - cooled < 4, `resumes on the first press once cooled (${cooled} ${resumed})`);
});

test("a bot the visibility gate runs every other update fires that pistol 16 updates apart", () => {
  const { cycles } = standingFire(pistol({ HeatPerShot: 0 }), { seconds: 3, execute: (t) => t % 2 === 0 });
  assert.deepEqual([...new Set(gapsOf(cycles))], [16], "a press every 8 updates: 0.267 s");
});

test("a ShotDelay shorter than the press cycle does not fire faster than every fourth update", () => {
  const { cycles } = standingFire(pistol({ ShotDelay: 0.05, HeatPerShot: 0 }), { seconds: 2 });
  assert.deepEqual([...new Set(gapsOf(cycles))], [4], "15 rounds a second at 60 updates, not 20");
});

test("an automatic rifle fires bursts of BarrageMin..Max rounds with BarrageDelay between", () => {
  const { cycles } = standingFire(rifle(), { seconds: 30, seed: 11 });
  const bursts = [];
  let run = [cycles[0]];
  for (let i = 1; i < cycles.length; i++) {
    if (cycles[i] - cycles[i - 1] === 12) run.push(cycles[i]);
    else {
      bursts.push(run);
      run = [cycles[i]];
    }
  }
  bursts.push(run);
  assert.ok(bursts.length >= 8, `${bursts.length} bursts in 30 s`);
  for (const b of bursts.slice(0, -1)) assert.ok(b.length >= 3 && b.length <= 6, `burst of ${b.length}`);
  for (let i = 1; i < bursts.length; i++) {
    const pause = (bursts[i][0] - bursts[i - 1].at(-1)) / HZ;
    assert.ok(pause > 2 && pause < 2.1, `BarrageDelay after the burst's last round (${pause.toFixed(3)} s)`);
  }
  const perSecond = cycles.length / 30;
  assert.ok(perSecond > 1.4 && perSecond < 1.9, `about 1.6 rounds a second (${perSecond.toFixed(2)})`);
});

test("a bottomless heat weapon's burst runs until its heat would not fit another round", () => {
  // The round count never moves (ammo reads INT_MAX), so heat ends a burst:
  // one round short of the heat limit, never overheated.
  const repeater = rifle({ ammo: 0, RoundsPerClip: 0, ShotDelay: 0.1, HeatPerShot: 0.1, HeatRecoverRate: 0.5, HeatThreshold: 0.2 });
  const { cycles, heat } = standingFire(repeater, { seconds: 6 });
  const first = [cycles[0]];
  for (let i = 1; i < cycles.length && cycles[i] - cycles[i - 1] === 6; i++) first.push(cycles[i]);
  assert.ok(first.length > 6, `longer than BarrageMax: ${first.length} rounds`);
  assert.ok(Math.max(...heat) < 1, "never overheats");
  assert.ok(heat[first.at(-1)] >= 0.8, "ends at the heat limit");
  const next = cycles[first.length];
  assert.ok((next - first.at(-1)) / HZ > 2, "then BarrageDelay");
});

test("a burst that exactly fills the heat gauge still starts (the Hoth turret cannon's 4 x 0.25)", () => {
  // The original game's single-precision test (needed < 1 - heat, needed above
  // 1 counted as 0.99) never starts a 4-round burst of 0.25 a round, even
  // stone cold, and a target held in view never lets the count be redrawn:
  // the gunner aimed and never fired again. Port: a full gauge counts as
  // 0.99, so such a burst starts once the heat is all but gone.
  const cannon = rifle({ ammo: 0, RoundsPerClip: 0, ShotDelay: 0.4, SalvoCount: 2, HeatPerShot: 0.25, HeatRecoverRate: 0.4, HeatThreshold: 0.25,
    ai: { ...rifle().ai, barrageMin: 4, barrageMax: 4 } });
  const { cycles } = standingFire(cannon, { seconds: 12 });
  assert.ok(cycles.length > 0 && cycles[0] < 5, `fires at once (${cycles[0]})`);
  const late = cycles.filter((t) => t > 6 * HZ).length;
  assert.ok(late > 0, `and keeps firing (${cycles.length} rounds, ${late} after 6 s)`);
});
