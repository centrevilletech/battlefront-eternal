// The arc caster's shot as the original game fires it and as a page draws it,
// and the repair tool's ticks as a page draws them.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { PlayerCombat } from "../src/combat.js";
import { Lightning } from "../src/lightning.js";
import { presentEntityEvent } from "../src/runtime/entity-presentation-events.js";
import { BattleHud } from "../src/hud.js";
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
const target = (x, y, z, hurt) => ({ origin: new THREE.Vector3(x, y, z), hurt: (amount) => hurt.push([z, amount]) });
const lightning = (events) => events.filter((e) => e.kind === "lightning");

test("the arc caster's first bolt leaves the gun origin and is held to the shooter; later ones run target to target", () => {
  const { combat, events } = fixture();
  const hurt = [];
  combat.arcTargets = () => [target(0, 0, 8, hurt), target(0.5, 0, 14, hurt), target(-0.5, 0, 20, hurt)];
  combat.arc({ charge: 1, weapon: combat.arsenal.current(0) }, ORD);
  assert.deepEqual(hurt, [[8, 250], [14, 175], [20, 100]], "nearest first, 75 lost a link, stopping below 75");
  const bolts = lightning(events);
  assert.equal(bolts.length, 3);
  assert.deepEqual(bolts[0].from, [0, 1.6, 0], "from the gun origin, not a held model's muzzle");
  assert.deepEqual(bolts[0].to, [0, 1, 8], "to the target's origin + 1 m");
  assert.equal(bolts[0].channel, 0, "the first bolt names its weapon's channel: held to the shooter");
  assert.deepEqual(bolts[1].from, [0, 1, 8]);
  assert.equal(bolts[1].channel, undefined, "a link between targets is held to nothing");
  assert.equal(bolts[0].charge, 1);
});

test("each link's damage comes from where its arc starts: the gun origin, then the target before", () => {
  const { combat } = fixture();
  const from = [];
  const at = (x, y, z) => ({ origin: new THREE.Vector3(x, y, z), hurt: (amount, source) => from.push([z, source.toArray()]) });
  combat.arcTargets = () => [at(0, 0, 8), at(0.5, 0, 14), at(-0.5, 0, 20)];
  combat.arc({ charge: 1, weapon: combat.arsenal.current(0) }, ORD);
  assert.deepEqual(from, [[8, [0, 1.6, 0]], [14, [0, 1, 8]], [20, [0.5, 1, 14]]], "the gun origin, then each target's origin + 1 m");
});

test("the gathering box and the double cone: a target on the line behind the shooter comes first", () => {
  const { combat } = fixture();
  const hurt = [];
  // The box reaches 0.1 x ConeLength (3 m) behind the gun origin along +z.
  combat.arcTargets = () => [target(0, 1.6, -2, hurt), target(0, 1.6, -5, hurt), target(0, 0, 10, hurt)];
  combat.arc({ charge: 1, weapon: combat.arsenal.current(0) }, ORD);
  assert.deepEqual(hurt, [[-2, 250], [10, 175]], "the one 2 m behind on the line is linked first; 5 m behind is outside the box");
});

test("the first link's FirstRadius is measured from its origin + 1 m, and a blocked link is skipped", () => {
  const blocked = new THREE.Vector3(0, 1, 12);
  const { combat } = fixture({ hits: (from, dir, far, out) => {
    // Something stands between the gun and the second target only.
    const end = from.clone().addScaledVector(dir, far);
    return end.distanceTo(blocked) < 1e-6 ? Object.assign(out, { distance: far / 2 }) : null;
  } });
  const hurt = [];
  // Its feet 2.6 m under the line (outside FirstRadius), its point 1.6 m.
  combat.arcTargets = () => [target(0, -1, 10, hurt), target(0, 0, 12, hurt), target(0, 0, 16, hurt)];
  combat.arc({ charge: 1, weapon: combat.arsenal.current(0) }, ORD);
  assert.deepEqual(hurt, [[10, 250], [16, 175]]);
});

test("with nothing to link one bolt runs along the shot to what it meets, held to the shooter", () => {
  const { combat, events } = fixture({ hits: (from, dir, far, out) => Object.assign(out, { distance: 12 }) });
  combat.arcTargets = () => [];
  combat.arc({ charge: 0.5, weapon: combat.arsenal.current(0) }, ORD);
  const [bolt] = lightning(events);
  assert.deepEqual(bolt.from, [0, 1.6, 0]);
  assert.deepEqual(bolt.to, [0, 1.6, 12]);
  assert.equal(bolt.channel, 0);
});

test("a world that draws holds its first bolt's start to the shooter's gun as drawn", () => {
  const { combat, soldier } = fixture();
  combat.lightning = new Lightning(new THREE.Scene(), () => null);
  combat.owner = { presentation: { state: soldier } };
  combat.arcTargets = () => [target(0, 0, 8, [])];
  combat.arc({ charge: 1, weapon: combat.arsenal.current(0) }, ORD);
  const [bolt] = combat.lightning.bolts;
  soldier.position.set(2, 0, 0);
  combat.lightning.update(1 / 60, new THREE.PerspectiveCamera());
  const expected = combat.viewBasis(soldier).origin;
  assert.ok(bolt.start.distanceTo(expected) < 1e-6, "the start moved with him");
  assert.ok(Math.abs(bolt.start.x - (2 - 0.06)) < 1e-6);
});

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

test("a held charge fills the weapon panel's bar in green, as the original game's state 4 does", () => {
  const arsenal = new Arsenal([ARC, CUTTER], { player: true });
  const arc = arsenal.current(0);
  const bars = [];
  const hud = Object.create(BattleHud.prototype);
  Object.defineProperty(hud, "height", { value: 768 });
  Object.assign(hud, {
    arsenal, vehicle: null, strings: {}, supply: { ammo: false }, numbers: ["", ""], named: [null, null], nameLeft: [0, 0],
    weaponFader: { alpha: 1, wake() {} },
    texture: (name) => (name ? { name } : null),
    anchor: () => new THREE.Matrix4(),
    batch: { quad: (m, x0, y0, x1, y1, texture, uv, color) => { if (texture?.name === "hud1" && y0 === 2) bars.push({ width: x1 - x0, color }); } },
    fonts: { draw() {} },
  });
  const draw = () => { bars.length = 0; hud.drawWeapons(); return bars[0]; };
  // Half charged: half the bar, green.
  arc.charging = true;
  arc.held = ARC.MaxPressedTime / 2;
  let bar = draw();
  assert.ok(Math.abs(bar.width - 84 / 2) < 1e-9);
  assert.deepEqual(bar.color.slice(0, 3).map((c) => Math.round(c * 255)), [32, 223, 32]);
  // Let go and overheated: what heat leaves, in red.
  arc.charging = false;
  arc.held = 0;
  arc.heat = 0.75;
  arc.overheated = true;
  bar = draw();
  assert.ok(Math.abs(bar.width - 84 / 4) < 1e-9);
  assert.deepEqual(bar.color.slice(0, 3).map((c) => Math.round(c * 255)), [223, 32, 32]);
});
