import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Weapon } from '../src/weapons.js';
import { Ordnance } from '../src/ordnance.js';
import { hasMissileLock, vehicleMissileDefinition, targetingWeapon, updateWeaponLock } from '../src/weapon-targeting.js';
import { vehiclePacks } from '../tools/vehicle-packs.mjs';

function checkGuidance(def) {
  const weapon = new Weapon(def);
  const target = { id: 7, generation: 2, alive: true, team: 2, kind: 6,
    position: new THREE.Vector3(0, 0, 40), sphere: { radius: 1 },
    aimPoint(out) { return out.copy(this.position); } };
  const units = { all: () => [target], byId: id => id === target.id ? target : null };
  const origin = new THREE.Vector3(), dir = new THREE.Vector3(0, 0, 1);
  const collision = { cast: () => null };
  updateWeaponLock(weapon, (def.LockTime || def.MaxPressedTime) + .01,
    { units, collision, origin, dir, viewer: { team: 1 } });
  assert.equal(weapon.lockComplete, true, def.class);
  const shots = weapon.update(0, true, { pressed: true, held: true }, false, { posture: 'stand', stick: 0 });
  assert.ok(shots.length, def.class);
  assert.equal(shots[0].targetId, target.id, def.class);
  const ordnance = new Ordnance({ scene: new THREE.Scene(), collision,
    model: () => null, effects: { play() {} }, lights: { add() {} } });
  ordnance.units = units;
  const round = ordnance.fire(def.ordnance, origin, dir, null, null, null, null, shots[0]);
  target.position.x = 10;
  ordnance.steer(round, .1);
  assert.ok(round.desired.x > 0, `${def.class}: follows moving target`);
  target.generation++;
  ordnance.steer(round, .1);
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

const maps = new URL('../public/maps/', import.meta.url);
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

test('vehicle construction, player firing and AI firing preserve secondary missile guidance', async () => {
  const { Vehicles } = await import('../src/vehicles.js');
  const { CollisionWorld } = await import('../src/collision.js');
  const geometry = (w, h, d, y) => {
    const g = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
    return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
  };
  const floor = geometry(200, 2, 200, -1);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const scene = new THREE.Scene(), effects = { play() {} }, lights = { add() {}, flash() {} };
  const ordnance = new Ordnance({ scene, collision, effects, lights, model: () => null });
  const missile = { class: 'rep_weap_hover_fightertank_missile', base: 'cannon',
    RoundsPerClip: 4, ammo: 4, ShotDelay: .25, TriggerSingle: 1,
    ordnance: { base: 'bullet', Velocity: 50, LifeSpan: 5 } };
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: geometry(2, 2, 4, 1) },
    classes: { tank: { model: 'tank', settings: { MaxHealth: 1000 }, seats: [{ name: 'BODY', settings: {}, weapons: [
      { aimers: [], def: { class: 'laser', base: 'cannon' } }, { aimers: [], def: missile },
    ] }] } }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, classes: { 1: 'tank' } }] };
  const system = new Vehicles({ scene, collision, data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0),
    texture: () => null, camera: new THREE.PerspectiveCamera(), ordnance, effects });
  const v = system.vehicles[0];
  const target = { id: 7, generation: 2, alive: true, team: 2, kind: 6,
    position: v.seatEye(0, new THREE.Vector3()).addScaledVector(v.seatForward(0, new THREE.Vector3()), 40),
    sphere: { radius: 1 }, aimPoint(out) { return out.copy(this.position); } };
  system.units = ordnance.units = { all: () => [v.aiTarget, target], byId: id => id === 7 ? target : null };
  const idle = { take: () => false, held: () => false };
  for (let i = 0; i < 61; i++) v.fire(1 / 60, idle);
  assert.equal(v.current(1).lockComplete, true);
  assert.equal(targetingWeapon(v), v.current(1));
  v.fire(0, { take: action => action === 'fireSecondary', held: () => false });
  assert.equal(ordnance.rounds.at(-1).targetId, target.id);
  assert.equal(ordnance.rounds.at(-1).kind, 'missile');
  v.current(1).wait = 0;
  v.seat.lease = { team: 1, unit: { controller: { perception: { fire: { weaponTarget: { unit: target, generation: target.generation } } } } },
    input: { takeReload: () => false, take: channel => channel === 1, held: [false, true] } };
  v.fireSeat(0, 1.1);
  assert.equal(ordnance.rounds.length, 2);
  assert.equal(ordnance.rounds.at(-1).targetId, target.id);
  assert.equal(ordnance.rounds.at(-1).targetGeneration, target.generation);
  v.seat.lease = null;
  system.units = null;
  system.dispose();
});

test('a seat locks along its reticle line (through the point under the crosshair), not its eye\'s forward', async () => {
  const { Vehicles } = await import('../src/vehicles.js');
  const { CollisionWorld } = await import('../src/collision.js');
  const geometry = (w, h, d, y) => {
    const g = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
    return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
  };
  const floor = geometry(400, 2, 400, -1);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const scene = new THREE.Scene(), effects = { play() {} }, lights = { add() {}, flash() {} };
  const ordnance = new Ordnance({ scene, collision, effects, lights, model: () => null });
  const missile = { class: 'imp_weap_hover_fightertank_missile', base: 'cannon', RoundsPerClip: 2, ammo: 50, ShotDelay: 1,
    ordnance: { base: 'bullet', Velocity: 40, LifeSpan: 7 } };
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: geometry(2, 2, 4, 1) },
    classes: { tank: { model: 'tank', settings: { MaxHealth: 1000 }, seats: [{ name: 'BODY', settings: {}, weapons: [
      { aimers: [], def: { class: 'laser', base: 'cannon' } }, { aimers: [], def: missile },
    ] }] } }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, classes: { 1: 'tank' } }] };
  const system = new Vehicles({ scene, collision, data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0),
    texture: () => null, camera: new THREE.PerspectiveCamera(), ordnance, effects });
  const v = system.vehicles[0];
  assert.equal(hasMissileLock(v.current(1).def), true, 'the IFT-T missile takes the same lock as the IFT-X');
  const eye = v.seatEye(0, new THREE.Vector3()), ahead = v.seatForward(0, new THREE.Vector3());
  // 20 degrees off the eye's forward: where a chase camera's reticle can sit.
  const side = ahead.clone().applyAxisAngle(new THREE.Vector3(0, 1, 0), 20 * Math.PI / 180);
  const target = { id: 7, generation: 2, alive: true, team: 2, kind: 6, position: eye.clone().addScaledVector(side, 40),
    sphere: { radius: 1 }, aimPoint(out) { return out.copy(this.position); } };
  system.units = ordnance.units = { all: () => [v.aiTarget, target], byId: id => id === 7 ? target : null };
  const idle = { take: () => false, held: () => false };
  v.seats[0].target = null;
  for (let i = 0; i < 61; i++) v.fire(1 / 60, idle);
  assert.equal(v.current(1).lockTargetId, -1, 'the eye\'s forward misses it');
  v.seats[0].target = target.position.clone();
  for (let i = 0; i < 61; i++) v.fire(1 / 60, idle);
  assert.equal(v.current(1).lockComplete, true, 'the reticle is on it');
  system.units = null;
  system.dispose();
});
