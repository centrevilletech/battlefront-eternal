// Controlled integration with real extracted heroes, controllers and damage
// callbacks, in the world the world worker runs (tools/headless-world.mjs:
// createGame headless under Node). Reads public/ or BFE_CONTENT_DIR; the
// report stays outside git (AI_TEST_ARTIFACTS, default /tmp/bf-ai-heroes).
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { headlessGame, idleCommand } from "../tools/headless-world.mjs";
import { consumeFire } from "../src/ai/classic/weapons-ai.js";

const out = process.env.AI_TEST_ARTIFACTS || "/tmp/bf-ai-heroes";
await fs.mkdir(out, { recursive: true });
// What the game reports as an error while it runs.
const errors = [];
const consoleError = console.error.bind(console);
console.error = (...args) => { errors.push(args.map(String).join(" ")); consoleError(...args); };

const load = async (heroes) => {
  const game = await headlessGame(`?map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&seed=1&heroes=${heroes}`);
  await Promise.race([game.ready, new Promise((_, reject) => setTimeout(() => reject(new Error("asset readiness timed out")), 60000))]);
  return game;
};
const step = (game) => game.step(idleCommand(game));

const offGame = await load(0);
const off = { heroes: offGame.ai.spawning.characters.filter((c) => c.hero).length, total: offGame.ai.spawning.characters.length };
assert.equal(off.heroes, 0, "Heroes Off reserves no hero slots");
console.log("Heroes Off ready", off);

const bf = await load(1);
const spawned = (() => {
  const ready = () => bf.ai.spawning.characters.filter((c) => c.hero && c.unit?.alive);
  for (let i = 0; i < 1200 && ready().length < 2; i++) step(bf);
  const chars = bf.ai.spawning.characters;
  const heroes = ready().map((c) => ({ team: c.team, class: c.unit.className, hero: c.unit.hero, health: c.unit.combat.health,
    capture: c.unit.classDef.CapturePosts, noVehicles: c.unit.classDef.NoEnterVehicles, weapons: c.unit.combat.arsenal.weapons.map((w) => w.def.base) }));
  return { total: chars.length, heroSlots: chars.filter((c) => c.hero).length, heroes };
})();
assert.equal(spawned.heroSlots, 2);
assert.equal(spawned.total, off.total, "heroes replace ordinary slots");
assert.equal(spawned.heroes.length, 2, "both heroes spawn through ordinary lifecycle");
for (const hero of spawned.heroes) {
  assert.equal(hero.hero, true); assert.equal(hero.capture, 0); assert.equal(hero.noVehicles, 1);
  assert.deepEqual(hero.weapons, ["melee"]);
}
console.log("Heroes On spawned", spawned);

const captureExcluded = (() => {
  const update = bf.posts.update;
  let observed = false;
  bf.posts.update = function (dt, soldiers, camera) {
    observed = true;
    if (soldiers.some((u) => u.hero)) throw new Error("Hero passed into capture contributors");
    return update.call(this, dt, soldiers, camera);
  };
  try { step(bf); }
  finally { bf.posts.update = update; }
  return observed;
})();
assert.equal(captureExcluded, true, "battle tick excludes heroes from capture contributors");

const combat = (() => {
  const heroes = bf.ai.spawning.characters.filter((c) => c.hero).map((c) => c.unit);
  const soldiers = bf.units.all().filter((u) => u.soldier && u.alive);
  // Freeze the world while running the real controller input/combat chain.
  bf.ai.update = () => {};
  const report = [];
  for (const hero of heroes) {
    const target = soldiers.find((u) => !u.hero && !u.human && u.team !== hero.team && u.alive);
    if (!target) throw new Error("No ordinary opponent for melee test");
    const alive = new Map(soldiers.map((u) => [u, u.alive]));
    for (const u of soldiers) if (u !== hero && u !== target) u.alive = false;
    const p = hero.position.clone();
    hero.soldier.aimYaw = hero.soldier.aimPitch = 0;
    hero.soldier.air = null; hero.soldier.lock = 0; hero.soldier.dive = null; hero.soldier.posture = "stand";
    target.soldier.position.copy(p).add(new bf.THREE.Vector3(0, 0, 1.4));
    target.tickCount++;
    target.combat.maxHealth = target.combat.health = 10000;
    const reader = bf.simEvents.reader();
    const hits = [];
    const controller = hero.controller;
    controller.target = null; controller.fireA = true; controller.fireB = false;
    const weapon = hero.combat.arsenal.current(0);
    const before = target.combat.health;
    for (let i = 0; i < 120; i++) {
      consumeFire(controller, hero, controller.world);
      hero.controls.latch();
      hero.combat.update(1 / 30, hero.controls, 0);
      reader.read((event) => { if (event.kind === "damage") hits.push(event.amount); });
    }
    const damage = before - target.combat.health;
    if (damage < 4000) throw new Error(`${hero.className}: only ${damage} melee damage in four seconds`);
    const killsBefore = hero.character.kills;
    target.combat.health = 500;
    for (let i = 0; i < 45 && target.alive; i++) {
      consumeFire(controller, hero, controller.world);
      hero.controls.latch();
      hero.combat.update(1 / 30, hero.controls, 0);
    }
    if (target.alive) throw new Error("Melee failed to kill a low-health opponent");
    if (hero.character.kills !== killsBefore + 1) throw new Error("Melee kill was not credited to the hero");
    hero.combat.damage(50000);
    const hurt = hero.combat.health;
    hero.intent.buttons = [-1, -1]; hero.controls.latch();
    hero.combat.update(1 / 30, hero.controls, 0);
    const healed = hero.combat.health;
    if (!(healed > hurt && healed <= hero.combat.maxHealth)) throw new Error("Hero regeneration failed");
    const candidate = bf.vehicles?.vehicles.find((v) => v.alive)?.aiTarget;
    if (candidate && bf.ai.classic.ai.vehicles.canEnter(controller, candidate)) throw new Error("Hero permitted to board");
    report.push({ class: hero.className, damage, creditedKills: hero.character.kills - killsBefore, hurt, healed,
      swings: weapon.melee != null, hitEvents: hits.length, deflection: weapon.def.CanDeflect });
    for (const [u, was] of alive) if (!u.combat.dead) u.alive = was;
  }
  return report;
})();
console.log("Hero combat", combat);

const forced = bf.units.all().filter((u) => u.hero && u.alive).map((u) => {
  u.combat.damage(Infinity);
  u.controls.latch(); u.combat.update(1 / 30, u.controls, 0);
  return { class: u.className, dead: u.combat.dead, health: u.combat.health };
});
for (const hero of forced) { assert.equal(hero.dead, true); assert.equal(hero.health, 0); }
assert.equal(forced.length, 2);
assert.deepEqual(errors, [], "game errors");
await fs.writeFile(`${out}/report.json`, JSON.stringify({ off, spawned, captureExcluded, combat, forced, errors }, null, 2));
console.log(JSON.stringify({ out, passed: true }));
