// The arc caster's shot as the original game fires it and as a page draws it,
// and the repair tool's ticks as a page draws them.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { PlayerCombat } from "../src/combat.js";

import { presentEntityEvent } from "../src/runtime/entity-presentation-events.js";
import { Arsenal } from "../src/weapons.js";

const BOLT = { Texture: "arccaster_lightning", Width: 0.5, FadeTime: 0.6, SpreadFactor: 0.8, InterpolationSpeed: 5, BranchFactor: 0.15, BranchSpreadFactor: 0.8, BranchLength: 4, MaxBranches: 3, BreakDistance: 1, TextureSize: 3.5, ChildBreakDistance: 1.5, ChildTextureSize: 2, ChildWidth: 0.4, ChildSpreadFactor: 1, NumChildren: 3, Color: [130, 170, 255, 250], ChildColor: [140, 180, 255, 100] };
// rep_weap_inf_arccaster_ord as the retail class sets it.
const ORD = { class: "rep_weap_inf_arccaster_ord", base: "emitterordnance", ConeLength: 30, ConeAngle: 50, FirstRadius: 2, MaxTargets: 4, MaxJumpDistance: 20, Damage: 100, MaxDamage: 250, DamageDropPerTarget: 75, DamageThreshold: 75, LightningEffect: "arccaster_lightning" };
const ARC = { class: "rep_weap_inf_arccaster", base: "cannon", channel: 0, AnimationBank: "rifle", RoundsPerClip: 0, ReloadTime: 3, ShotDelay: 0.8, TriggerSingle: 1, MaxPressedTime: 2.25, HeatPerShot: 0.42, GeometryName: "rep_weap_inf_rifle", ordnance: ORD };
const CUTTER = { class: "rep_weap_inf_fusioncutter", base: "repair", channel: 1, AnimationBank: "tool", RoundsPerClip: -1, ShotDelay: 0.5, MuzzleFlashEffect: "repairkit", GeometryName: "rep_weap_inf_fusioncutter" };

function fixture({ hits = () => null } = {}) {
  const soldier = { position: new THREE.Vector3(0, 0, 0), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, posture: "stand", air: null, pitchRange: () => [-1, 1] };
  const banks = [];
  const animator = { setBank: (bank) => banks.push(bank), weaponAction() {}, fire() {}, endWeaponAction() {}, holdAlert() {}, die() {} };
  const collision = { cast: (from, dir, far, out) => hits(from, dir, far, out) };
  const combat = new PlayerCombat({ soldier, def: { MaxHealth: 300, weapons: [ARC, CUTTER] }, actor: { hand: new THREE.Group(), materials: [] }, animator, collision, camera: null, ordnance: {}, lights: { flash() {} }, weaponModels: new Map(), firePoints: {}, texture: () => null, player: false });
  const events = [];
  combat.onPresentation = (event) => events.push(event);
  combat.effects = { lightning: { arccaster_lightning: BOLT }, played: [], play(name, position, normal, velocity, quaternion) { this.played.push({ name, position: position.clone(), quaternion }); return null; } };
  combat.random = () => 0.5;
  combat.origin.set(0, 1.6, 0);
  combat.shotDir.set(0, 0, 1);
  return { combat, soldier, events, banks };
}

const lightning = (events) => events.filter((e) => e.kind === "lightning");

test("a page draws a held bolt from its shooter's gun as it draws him, and any bolt at once", () => {
  const spawned = [];
  const shooter = { generation: 3, combat: { drawnGunOrigin: (out) => out.set(5, 6, 7) } };
  const game = { effects: { lightning: { arccaster_lightning: BOLT } }, lightning: { spawn: (...args) => spawned.push(args) }, units: { byId: (id) => (id === 9 ? shooter : null) } };
  const base = { presentationKind: "lightning", name: "arccaster_lightning", from: [0, 1, 0], to: [0, 1, 10], charge: 1, ownerGeneration: 3 };
  assert.equal(presentEntityEvent(game, { ...base, ownerId: 9, channel: 0 }), true);
  assert.equal(typeof spawned[0][4], "function");
  const out = new THREE.Vector3();
  spawned[0][4](out);
  assert.deepEqual(out.toArray(), [5, 6, 7]);
  presentEntityEvent(game, { ...base, ownerId: 9 });
  assert.equal(spawned[1][4], null, "a link between targets holds to nothing");
  assert.equal(presentEntityEvent(game, { ...base, ownerId: 4, channel: 0 }), true, "an unknown shooter's bolt is drawn where the world put it");
  assert.equal(spawned[2][4], null);
  assert.deepEqual(spawned[2][1].toArray(), [0, 1, 0]);
});

test("a repair tick's sparks fly from the tool's fire point, and the tool stays in the hand between ticks", () => {
  const { combat, banks } = fixture();
  const cutter = combat.arsenal.current(1);
  const at = { position: new THREE.Vector3(1, 2, 3), quaternion: new THREE.Quaternion() };
  combat.firePoint = (weapon) => (weapon === cutter ? at : null);
  combat.updateHeld();
  assert.equal(combat.shown.name, ARC.class);
  combat.presentRepairTick(cutter);
  assert.equal(combat.effects.played.length, 1);
  assert.equal(combat.effects.played[0].name, "repairkit");
  assert.deepEqual(combat.effects.played[0].position.toArray(), [1, 2, 3]);
  assert.equal(combat.shown, cutter, "the tool comes to hand with its first tick");
  assert.equal(banks.at(-1), "tool");
  combat.presentCharge(0.5);
  assert.equal(combat.shown, cutter, "still in hand within the tick's interval");
  combat.presentCharge(0.3);
  assert.equal(combat.shown.name, ARC.class, "and put away once no tick has come for the interval and a little more");
});

test("a repair tick plays the tool's shoot in first person and holds the body alert; a page's lock keeps it alert", () => {
  const { combat } = fixture();
  const cutter = combat.arsenal.current(1);
  combat.firePoint = () => null;
  let alerts = 0;
  combat.animator.fire = () => { alerts += 1; };
  const plays = [];
  combat.view = { play: (event, weapon) => plays.push([event, weapon.name]) };
  combat.firstPerson = true;
  combat.presentRepairTick(cutter);
  assert.deepEqual(plays, [["shoot", CUTTER.class]], "the tick is the tool firing: _1st_tool_shoot");
  assert.equal(alerts, 1);
  combat.firstPerson = false;
  combat.presentRepairTick(cutter);
  assert.equal(plays.length, 1, "no arms to play in third person");
  assert.equal(alerts, 2);
  // Between ticks a page knows the lock only from the soldier's record.
  alerts = 0;
  combat.repairLock = "l3";
  combat.presentCharge(1 / 60);
  assert.equal(alerts, 1, "a lock keeps the soldier alert, as the world's update does");
  combat.repairLock = "";
  combat.presentCharge(1 / 60);
  assert.equal(alerts, 1);
});
