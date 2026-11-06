import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { radarRelation, findReticleTarget, reticleRange } from "../src/targeting-facts.js";

const vector = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z);
const unit = (x = 0, z = 0, team = 2) => ({ position: vector(x, 0, z), sphere: { center: vector(x, 0, z), radius: .5 }, kind: 0, classDef: {}, team, alive: true, health: 100, generation: 1 });
const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-10, `${a} != ${b}`);

const pick = (units, options = {}) => findReticleTarget({ origin: vector(), dir: vector(0, 0, 1), viewer: unit(0, 0, 1), units,
  weapon: { LockOnAngle: 1, LockOnRange: 100, TargetAll: 1, TargetEnemy: 0, ordnance: { Velocity: 100, LifeSpan: 2 } }, ...options });

test("radar palette: a locals team is yellow, friend or foe, as the original's team colour table has it", () => {
  assert.equal(radarRelation(1, 0), "neutral");
  assert.equal(radarRelation(1, 1), "friendly");
  assert.equal(radarRelation(1, 2), "enemy");
  assert.equal(radarRelation(1, 3), "locals");
  // Kashyyyk's Wookiees are the Rebels' friends and still yellow to them.
  assert.equal(radarRelation(2, 3, () => 1), "locals");
  // By the mission's factions: Theed's Naboo Guard (team 2, faction Locals)
  // is yellow to the CIS and friendly to itself; Hoth's unnamed team 4 is red.
  const factions = [0, 4, 5, 5, 0];
  const locals = (team) => factions[team] === 5;
  assert.equal(radarRelation(1, 2, undefined, locals), "locals");
  assert.equal(radarRelation(2, 2, undefined, locals), "friendly");
  assert.equal(radarRelation(1, 4, undefined, locals), "enemy");
  assert.equal(radarRelation(1, 4, () => 0, locals), "neutral");
});
test("reticle cone finds an off-ray target and orders by 16d² + t", () => {
  const near = unit(.65, 10), centred = unit(0, 15), farOutside = unit(10, 10);
  assert.equal(pick([near]).unit, near);
  assert.equal(pick([near, centred, farOutside]).unit, centred);
  assert.equal(pick([unit(0, -3), farOutside]), null);
});
test("lockable targets win; blocked locks do not hide a visible fallback", () => {
  const friend = unit(0, 3, 1), enemy = unit(0, 20, 2);
  const weapon = { LockOnAngle: 1, LockOnRange: 100, TargetEnemy: 1, TargetFriendly: 0, TargetPerson: 1 };
  assert.equal(pick([friend, enemy], { weapon }).unit, enemy);
  assert.equal(pick([friend, enemy], { weapon, los: (a, b, viewer, target) => target !== enemy }).unit, friend);
  enemy.seat = {};
  assert.equal(pick([enemy], { weapon }), null);
});
test("reticle range supports exact extracted override and projectile fallback", () => {
  const def = { LockOnRange: 64, ordnance: { Velocity: 150, LifeSpan: .8 } };
  assert.equal(reticleRange(def), 120);
  assert.equal(reticleRange({ ...def, ReticleRange: 64 }), 64);
  assert.equal(pick([unit(0, 121)], { weapon: def }), null);
});
test("reticle rejects dead targets and honours category masks", () => {
  const corpse = unit(0, 1); corpse.health = 0;
  const tank = unit(0, 5); tank.kind = 6;
  const infantry = unit(0, 10);
  const weapon = { LockOnAngle: 1, LockOnRange: 50, TargetAll: 0, TargetPerson: 1 };
  assert.equal(pick([corpse, tank, infantry], { weapon }).unit, infantry);
});