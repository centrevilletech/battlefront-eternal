import { contentURL } from './helpers/content.js';
import { rustOrdnance } from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Weapon } from '../src/weapons.js';
import { hasMissileLock, vehicleMissileDefinition, targetingWeapon } from '../src/weapon-targeting.js';
import { vehiclePacks } from '../tools/vehicle-packs.mjs';

function checkGuidance(def) {
  assert.ok(hasMissileLock(def));
  const target = { id: 7, generation: 2, alive: true, team: 2, kind: 6,
    position: new THREE.Vector3(0, 0, 40), sphere: { radius: 1 },
    aimPoint(out) { return out.copy(this.position); } };
  const units = { raycast:()=>null, all: () => [target], byId: id => id === target.id ? target : null };
  const origin = new THREE.Vector3(), dir = new THREE.Vector3(0, 0, 1);
  const collision = { cast: () => null };
  // Acquisition and weapon clocks are native; this reader consumes the
  // resulting qualified shot identity and tests real missile guidance.
  const shot={targetId:target.id,targetGeneration:target.generation};
  const ordnance = rustOrdnance({ scene: new THREE.Scene(), collision,
    model: () => null, effects: { play() {} }, lights: { add() {} } });
  ordnance.units = units;
  const round = ordnance.fire(def.ordnance, origin, dir, null, null, null, null, shot);
  target.position.x = 10;
  ordnance.update(.1,false);
  assert.ok(round.desired.x > 0, `${def.class}: follows moving target`);
  target.generation++;
  ordnance.update(.1,false);
  assert.equal(round.targetId, -1);
}

test('the tank missile definitions acquire and follow vehicles, including with extracted AI masks', () => {
  for (const name of ['rep_weap_hover_fightertank_missile', 'imp_weap_hover_fightertank_missile', 'all_weap_hover_combatspeeder_missile']) {
    const original = { class: name, base: 'cannon', LockTime: 0, LockOnRange: 0,
      TargetVehicle: 0, ai: { categoryMask: 0, relationMask: 1 },
      RoundsPerClip: 4, ammo: 4, ShotDelay: 1, TriggerSingle: 1,
      ordnance: { base: 'bullet', Velocity: 50, LifeSpan: 5, Damage: 300 } };
    const def = vehicleMissileDefinition(original);
    checkGuidance(def);
    assert.equal(original.ordnance.base, 'bullet');
    assert.equal(def.ordnance.Damage, original.ordnance.Damage);
    assert.equal(def.ordnance.Velocity, original.ordnance.Velocity);
    assert.equal(def.ShotDelay, original.ShotDelay);
  }
});

test('mounted HUD selects a missile channel and falls back to primary for ordinary seats', () => {
  const primary = new Weapon({ base: 'cannon' });
  const secondary = new Weapon(vehicleMissileDefinition({ class: 'rep_weap_hover_fightertank_missile' }));
  assert.equal(targetingWeapon({ current: i => [primary, secondary][i] }), secondary);
  assert.equal(targetingWeapon({ current: i => [secondary, primary][i] }), secondary);
  assert.equal(targetingWeapon({ current: i => [primary][i] }), primary);
  assert.equal(targetingWeapon(null), undefined);
});

const maps = contentURL('maps/');
const files = vehiclePacks(maps);
test('all extracted ground vehicle missile launchers acquire and guide shots', { skip: !files.length }, () => {
  const checked = new Set();
  for (const file of files) for (const vehicle of Object.values(JSON.parse(fs.readFileSync(file)).classes)) {
    for (const seat of vehicle.seats) for (const w of seat.weapons) {
      const def = vehicleMissileDefinition(w.def);
      if (!hasMissileLock(def) || !/_(hover|tread|walk)_/.test(def.class) || checked.has(def.class)) continue;
      checkGuidance(def);
      checked.add(def.class);
    }
  }
  assert.ok(checked.has('cis_weap_tread_hailfire_missile'));
  assert.ok(checked.has('rep_weap_hover_fightertank_missile'));
  assert.ok(checked.has('all_weap_hover_combatspeeder_missile'));
});

// Native mounted lock and off-axis range behavior are recorded in the
// synthetic lock/range scheduler goldens in f2-firing.test.js.
