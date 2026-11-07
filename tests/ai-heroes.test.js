import { contentPath } from './helpers/content.js';
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import * as THREE from "three";

import { PlayerCombat } from "./helpers/combat.js";
import { Units } from "../src/registry.js";
import { blankIntent, IntentControls } from "../src/units.js";

import { attachBlade } from "../src/heroes.js";

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

test("a third-person network shot converges on the crosshair ray, not parallel to it", () => {
  const gun = { class: "rifle", base: "cannon", channel: 0, AnimationBank: "rifle", RoundsPerClip: 10, ReloadTime: 1, ordnance: { base: "bolt" } };
  const { combat } = combatFixture({ hero: false, weapons: [gun] });
  combat.camera = new THREE.PerspectiveCamera();combat.camera.position.set(0.55,2.1,-4);combat.camera.updateMatrixWorld();
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

test("blade attaches at the hilt's authored point with native length and core width", () => {
  const group = new THREE.Group();
  const texture = new THREE.Texture();
  const blade = attachBlade(group, [.1, .2, .3], { Texture: "test" }, () => texture);
  assert.deepEqual(blade.position.toArray(), [.1, .2, .3]);
  assert.equal(blade.children.length, 2);
  assert.equal(blade.children[0].material.userData.bfeProgram, "effects/glow-basic");
  assert.equal(blade.children[0].material.vertexShader, undefined);
  assert.equal(blade.children[0].geometry.parameters.width, .08);
  assert.ok(Math.abs(blade.children[0].geometry.parameters.height - Math.hypot(.8, .5)) < 1e-12);
  const camera = new THREE.PerspectiveCamera(); camera.position.set(4, 2, 1);
  group.updateMatrixWorld(true);
  blade.children[0].onBeforeRender(null, null, camera);
  assert.ok(blade.children[0].quaternion.toArray().every(Number.isFinite));
});

const stock = { rep: ["rep_inf_macewindu"], cis: ["cis_inf_countdooku"], all: ["all_inf_lukeskywalker", "all_inf_lukeskywalkersnow"], imp: ["imp_inf_darthvader"] };
for (const [side, names] of Object.entries(stock)) test(`extracted ${side} heroes carry stock health, permissions and effective defense profile`, { skip: !existsSync(contentPath(`sides/${side}/units.json`)) }, () => {
  const data = JSON.parse(readFileSync(contentPath(`sides/${side}/units.json`)));
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
    assert.ok(existsSync(contentPath(`sides/${side}/actors/${def.model}.json`)));
  }
});
