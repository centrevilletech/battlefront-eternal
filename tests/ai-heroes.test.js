import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import * as THREE from "three";
import { Arsenal } from "../src/weapons.js";
import { PlayerCombat } from "../src/combat.js";
import { Units } from "../src/registry.js";
import { blankIntent, IntentControls } from "../src/units.js";
import { ClassicRandom } from "../src/ai/random.js";
import { Perception } from "../src/ai/classic/perception.js";
import { consumeFire, weaponAI } from "../src/ai/classic/weapons-ai.js";
import { attachBlade, meleeStrike, prepareMelee, regenerate } from "../src/heroes.js";

const saber = (extra = {}) => ({ class: "synthetic_saber", base: "melee", channel: 0, AnimationBank: "lightsabre", RoundsPerClip: 75, ReloadTime: 5, CanDeflect: 0, swings: [{ duration: .7, hit: .5 }], ...extra });

function combatFixture({ hero = true, at = [0, 0, 0], weapons = [saber()] } = {}) {
  const soldier = { position: new THREE.Vector3(...at), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, posture: "stand", air: null, lock: 0, jet: {}, pitchRange: () => [-1, 1] };
  let deaths = 0;
  const actor = { hand: new THREE.Group(), materials: [] };
  const actions = [];
  const animator = { setBank() {}, weaponAction: (action) => actions.push(action), fire() {}, endWeaponAction() {}, die() { deaths++; } };
  const units = new Units();
  const ordnance = { units, onHitUnit: (u, packet, p, owner) => u.combat.damage(packet.Damage, packet.Damage, p, owner) };
  const combat = new PlayerCombat({ soldier, def: { MaxHealth: hero ? 1e6 : 300, AddHealth: hero ? 1e6 : 0, weapons }, actor, animator, collision: { cast: () => null }, camera: null, ordnance, lights: { flash() {} }, weaponModels: new Map(), firePoints: {}, texture: () => null, player: false });
  const unit = { kind: 0, team: 1, hero, generation: 1, soldier, combat, classDef: combat.def, position: soldier.position, intent: blankIntent(), get alive() { return !combat.dead; }, get dead() { return combat.dead; }, sphere: { center: soldier.position, radius: 1 }, forward: (out) => out.set(0, 0, 1) };
  units.add(unit);
  combat.owner = unit;
  const controls = new IntentControls(unit);
  return { combat, unit, units, controls, actions, deaths: () => deaths };
}

test("authored finite regeneration follows damage, clamps, and never revives lethal damage", () => {
  const { combat, deaths } = combatFixture();
  combat.damage(40000);
  assert.equal(combat.health, 960000);
  regenerate(combat, 1 / 30);
  assert.ok(Math.abs(combat.health - 993333.3333333334) < 1e-6);
  regenerate(combat, 1 / 30);
  assert.equal(combat.health, 1e6);
  combat.damage(1e6);
  regenerate(combat, 1);
  assert.equal(combat.health, 0);
  assert.equal(combat.dead, true);
  assert.equal(deaths(), 1);
  combat.reset();
  combat.damage(Infinity);
  regenerate(combat, 1);
  assert.equal(combat.dead, true, "forced environmental/ticket death bypasses regeneration");
  assert.equal(combat.health, 0);
});

test("a third-person network shot converges on the crosshair ray, not parallel to it", () => {
  const gun = { class: "rifle", base: "cannon", channel: 0, AnimationBank: "rifle", RoundsPerClip: 10, ReloadTime: 1, ordnance: { base: "bolt" } };
  const { combat } = combatFixture({ hero: false, weapons: [gun] });
  combat.camera = { position: new THREE.Vector3(0.55, 2.1, -4) };
  combat.firstPerson = false;
  // Body aim straight down +z; the gun sits low and left of the camera
  // axis, so the converged direction must angle back toward the
  // crosshair line: its ray from the gun origin meets the camera ray.
  const angles = combat.convergedAim(0, 0);
  const dir = new THREE.Vector3(Math.sin(angles.aimYaw) * Math.cos(angles.aimPitch), Math.sin(angles.aimPitch), Math.cos(angles.aimYaw) * Math.cos(angles.aimPitch));
  combat.aimBasis(0, 0);
  const at60 = combat.origin.clone().addScaledVector(dir, 60 / Math.max(dir.z, 1e-9));
  assert.ok(Math.abs(at60.x - 0.55) < 0.09, `converged ray passes through the crosshair line at reach (x=${at60.x.toFixed(2)})`);
  assert.notEqual(angles.aimYaw, 0, "convergence actually turns the shot");
  combat.firstPerson = true;
  assert.deepEqual(combat.convergedAim(0.3, -0.1), { aimYaw: 0.3, aimPitch: -0.1 }, "first person shoots along body aim");
  combat.camera = null;
  assert.deepEqual(combat.convergedAim(0.3, -0.1), { aimYaw: 0.3, aimPitch: -0.1 }, "no camera (server/bot) keeps body aim");
});

test("the arc caster chains between soldiers, dropping damage per link, never backward", () => {
  const ord = { class: "arc_ord", base: "emitterordnance", ConeLength: 30, ConeAngle: 50, FirstRadius: 2, MaxTargets: 4, MaxJumpDistance: 20, Damage: 100, MaxDamage: 250, DamageDropPerTarget: 75, DamageThreshold: 75 };
  const gun = { class: "arccaster", base: "cannon", channel: 0, AnimationBank: "rifle", RoundsPerClip: 0, ReloadTime: 3, ordnance: ord };
  const { combat } = combatFixture({ hero: false, weapons: [gun] });
  const hurt = [];
  const at = (z) => ({ origin: new THREE.Vector3(0, 0, z), hurt: (amount) => hurt.push(amount) });
  combat.arcTargets = () => [at(5), at(10), at(15), at(-5)];
  combat.origin.set(0, 1.5, 0);
  combat.shotDir.set(0, 0, 1);
  combat.random = () => 0.5;
  combat.arc({ charge: 1 }, ord);
  assert.deepEqual(hurt, [250, 175, 100], "full charge chains forward with the per-target drop; the soldier behind is untouched");
  hurt.length = 0;
  combat.arc({ charge: 0 }, ord);
  assert.deepEqual(hurt, [100], "an uncharged shot starts at base damage and falls under the chain threshold after one link");
});

test("a new life starts on the primary weapon, not the last life's selection", () => {
  const gun = (name, channel) => ({ class: name, base: "cannon", channel, AnimationBank: "rifle", RoundsPerClip: 10, ReloadTime: 1, ordnance: { base: "bolt" } });
  const { combat } = combatFixture({ hero: false, weapons: [gun("rifle", 0), gun("pistol", 0), gun("grenade", 1), gun("detpack", 1)] });
  combat.arsenal.selected = [1, 1]; // died holding pistol and detpack
  combat.reset();
  assert.deepEqual(combat.arsenal.selected, [0, 0], "selection resets with the life");
  assert.equal(combat.arsenal.current(0).name, "rifle");
  combat.updateHeld();
  assert.equal(combat.shown.name, "rifle", "the primary is in hand at spawn");
});

test("melee swing entry, strict hit interval, completion and press-only timing", () => {
  const w = prepareMelee(new Arsenal([saber()], { player: false })).current(0);
  w.random = () => 0;
  const tick = (dt, pressed = false, held = true) => w.update(dt, true, { pressed, held }, false, {});
  assert.ok(tick(.1, true).some((s) => s.meleeStart));
  assert.equal(w.melee.time, 0, "entry update consumes no duration");
  assert.equal(tick(.5, true).length, 0, "press during the swing is dropped; equality at hit does not strike");
  assert.equal(tick(.001).filter((s) => s.meleeHit).length, 1);
  assert.equal(tick(.4).filter((s) => s.meleeHit).length, 0, "one hit callback per swing");
  assert.ok(tick(0).some((s) => s.meleeEnd));
  assert.equal(tick(1, false, true).length, 0, "holding does not begin a new swing");
  assert.ok(tick(0, true).some((s) => s.meleeStart));
  assert.equal(w.loaded, Infinity);
  assert.equal(w.reloading, false);
});

function victim(units, x, z, radius = .35) {
  const p = new THREE.Vector3(x, 1, z);
  const damage = [];
  return units.add({ kind: 0, team: 2, generation: 1, alive: true, position: p, soldier: {}, sphere: { center: p, radius }, hitShapes: () => [{ type: "sphere", center: p, radius }], combat: { damage: (amount, shield, point, owner) => damage.push({ amount, owner }) }, damage });
}

test("melee multi-object hit has no team filter, strict 2m narrow phase and a ten-object cap", () => {
  const env = combatFixture();
  env.combat.origin.set(0, 1, 0);
  env.combat.aimDir.set(0, 0, 1);
  const friend = victim(env.units, -.3, 1.3); friend.team = 1;
  const enemy = victim(env.units, .3, 1.3);
  const far = victim(env.units, 0, 2.5, .5);
  const excluded = victim(env.units, 0, 1); excluded.seat = {};
  assert.equal(meleeStrike(env.combat), 2);
  assert.equal(friend.damage[0].amount, 1000);
  assert.ok(enemy.damage[0].owner === env.unit);
  assert.equal(far.damage.length, 0);
  assert.equal(excluded.damage.length, 0);
  env.units.remove(far); env.units.remove(excluded);
  for (let i = 0; i < 12; i++) victim(env.units, .1, 1);
  assert.equal(meleeStrike(env.combat), 10);
});

test("an actual Classic fire consumer repeatedly swings through IntentControls and PlayerCombat", () => {
  const env = combatFixture();
  const target = victim(env.units, 0, 1.4);
  const world = { time: 0, units: env.units, random: new ClassicRandom(37) };
  const controller = { unit: env.unit, world, roll: 0, state: 2, fireA: true, fireB: false, target: null };
  controller.perception = new Perception(controller);
  env.unit.controller = controller;
  // The registered AI record says false: melee's pressed-input adapter
  // still emits new presses and keeps the remainder of the AI record.
  const weapon = env.combat.arsenal.current(0);
  weapon.def.ai = { triggerSingle: false, minRange: 0, optimalRange: 1.5, maxRange: 3, action: 4 };
  weapon.random = () => world.random.uniform();
  assert.equal(weaponAI(weapon.def).maxRange, 3);
  for (let i = 0; i < 30 * 4; i++) {
    world.time += 1 / 30;
    consumeFire(controller, env.unit, world);
    env.controls.latch();
    env.combat.update(1 / 30, env.controls, 0);
  }
  assert.ok(env.actions.length >= 4 && env.actions.length <= 6, `swing count ${env.actions.length}`);
  assert.ok(target.damage.length >= 4, `hits ${target.damage.length}`);
  assert.ok(target.damage.every((hit) => hit.amount === 1000));
  assert.equal(weapon.loaded, Infinity);
  assert.equal(weapon.reloading, false);
});

test("stock compatibility does not block; explicit melee block ignores facing and swing state", () => {
  const env = combatFixture();
  assert.equal(env.combat.tryMeleeBlock(new THREE.Vector3(0, 0, -1), new THREE.Vector3()), false);
  const w = env.combat.arsenal.current(0);
  w.def.CanDeflect = 1;
  for (const z of [-1, 1]) {
    assert.equal(env.combat.tryMeleeBlock(new THREE.Vector3(0, 0, z), new THREE.Vector3()), true);
    w.update(0, true, { pressed: true });
    assert.equal(env.combat.tryMeleeBlock(new THREE.Vector3(0, 0, z), new THREE.Vector3()), true);
  }
});

test("blade attaches at the hilt's authored point with native length and core width", () => {
  const group = new THREE.Group();
  const texture = new THREE.Texture();
  const blade = attachBlade(group, [.1, .2, .3], { Texture: "test" }, () => texture);
  assert.deepEqual(blade.position.toArray(), [.1, .2, .3]);
  assert.equal(blade.children.length, 2);
  assert.equal(blade.children[0].geometry.parameters.width, .08);
  assert.ok(Math.abs(blade.children[0].geometry.parameters.height - Math.hypot(.8, .5)) < 1e-12);
  const camera = new THREE.PerspectiveCamera(); camera.position.set(4, 2, 1);
  group.updateMatrixWorld(true);
  blade.children[0].onBeforeRender(null, null, camera);
  assert.ok(blade.children[0].quaternion.toArray().every(Number.isFinite));
});

const stock = { rep: ["rep_inf_macewindu"], cis: ["cis_inf_countdooku"], all: ["all_inf_lukeskywalker", "all_inf_lukeskywalkersnow"], imp: ["imp_inf_darthvader"] };
for (const [side, names] of Object.entries(stock)) test(`extracted ${side} heroes carry stock health, permissions and effective defense profile`, { skip: !existsSync(`public/sides/${side}/units.json`) }, () => {
  const data = JSON.parse(readFileSync(`public/sides/${side}/units.json`));
  for (const name of names) {
    const def = data.units[name];
    assert.ok(def, `${name}: refresh this side's extraction`);
    assert.equal(def.MaxHealth, 1e6);
    assert.equal(def.AddHealth, 1e6);
    assert.equal(def.CapturePosts, 0);
    assert.equal(def.NoEnterVehicles, 1);
    assert.equal(def.weapons[0].CanDeflect, 0);
    assert.equal(def.weapons[0].canDeflectAuthored, null);
    assert.equal(def.weapons[0].deflectionProfile, "bf1-cloned-melee-cleared-v1");
    assert.ok(data.textures[def.weapons[0].Texture]);
    assert.ok(existsSync(`public/sides/${side}/actors/${def.model}.json`));
  }
});
