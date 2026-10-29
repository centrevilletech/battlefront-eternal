// A hover hull on the ground (src/vehicle-hover.js): its springs carry it
// over the terrain and buildings, pitching and rolling it with the ground,
// gravity and its grip limit what it climbs, and without springs it rides
// its lift spring. Synthetic hulls and ground; with an extract, the stock
// classes' own springs too.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { hoverClass, restHeight } from '../src/vehicle-hover.js';
import { packWith, vehiclePacks } from '../tools/vehicle-packs.mjs';

const DEG = Math.PI / 180;
const box = (x, y, z, w, h, d, rotateX = 0) => {
  const g = new THREE.BoxGeometry(w, h, d).rotateX(rotateX).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
// Terrain alone: a plane rising `slope` toward +z, as a height field.
function slopeWorld(slope) {
  const g = new THREE.PlaneGeometry(2000, 2000, 40, 40).rotateX(-Math.PI / 2);
  const p = g.attributes.position;
  for (let i = 0; i < p.count; i++) p.setY(i, Math.tan(slope) * p.getZ(i));
  return new CollisionWorld(new Float32Array(0), new Uint32Array(0), new Uint32Array(0), new Float32Array(p.array), new Uint32Array(g.index.array));
}
// A fighter tank's hover keys and springs, as its class sets them; its
// collision a box the size of its own.
const tank = {
  model: 'hull', label: 'Tank', base: 'hover',
  settings: { MaxHealth: 5000, ForwardSpeed: 12, ReverseSpeed: 10, StrafeSpeed: 4.5, Acceleration: 4.5, Deceleration: 7, SetAltitude: .5,
    GravityScale: 4, Traction: 20, VelocitySpring: 5, VelocityDamp: 2.5, OmegaXSpring: 3, OmegaXDamp: 3, OmegaZSpring: 5, OmegaZDamp: 1.5,
    LevelSpring: '2.0//low', LevelDamp: '1.0//low', TurnRate: 1.7 },
  springs: [
    { at: [-1.8, 1.4, 1.7], radius: 2.5, length: 1, factors: [1, 1, -1, 1, -1, 1] },
    { at: [1.8, 1.4, 1.7], radius: 2.5, length: 1, factors: [1, 1, -1, 1, 1, 1] },
    { at: [0, 2.4, -2], radius: 3.5, length: 1, factors: [1, 1, 2, 1, 0, 1] },
  ],
  seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null,
};
function hull(collision, def = tank, yaw = 0, y = 0) {
  const data = { models: { hull: { bones: [], parts: [] } }, collision: { hull: box(0, 1.15, -.9, 5, 2.3, 7) }, classes: { [def.label]: def },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().makeRotationY(yaw).setPosition(0, y, 0).toArray(), team: 1, post: '', classes: { 1: def.label } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision,
    texture: () => null, camera: new THREE.PerspectiveCamera(), ordnance: { explosion() {} }, effects: { play() { return null; } } });
  return { system, v: system.vehicles[0] };
}
// The pitch and roll of a plane rising `slope` toward +z, for a hull facing yaw.
const planeAngles = (slope, yaw) => {
  const n = new THREE.Vector3(0, Math.cos(slope), -Math.sin(slope));
  const s = Math.sin(yaw), c = Math.cos(yaw), x = c * n.x - s * n.z, z = s * n.x + c * n.z;
  return { tilt: -Math.atan2(z, n.y), roll: -Math.atan2(x, Math.hypot(n.y, z)) };
};
// How far the hull's box bottom corners dip under the plane (m, 0 when clear).
function dip(v, slope) {
  const p = new THREE.Vector3(), b = v.bounds;
  let worst = 0;
  for (const x of [b.min.x, b.max.x]) for (const z of [b.min.z, b.max.z]) {
    p.set(x, b.min.y, z).applyMatrix4(v.matrix);
    worst = Math.max(worst, Math.tan(slope) * p.z - p.y);
  }
  return worst;
}

test('the hover class reads its springs and keys over the original game\'s defaults', () => {
  const h = hoverClass(tank);
  assert.equal(h.springs.length, 3);
  assert.equal(h.levelSpring, 2);
  assert.deepEqual(h.springs[2].factors, [1, 1, 2, 1, 0, 1]);
  const bare = hoverClass({ settings: {}, seats: [] });
  assert.deepEqual([bare.altitude, bare.gravityScale, bare.liftSpring, bare.liftDamp, bare.traction, bare.levelSpring, bare.levelDamp], [1, 1, 5, 2, 10, 10, 3]);
  // On level ground its springs carry its 9.8 m/s^2: three of them, each
  // pressed 0.65 of its 1 m travel, hold its origin 0.45 m up.
  assert.ok(Math.abs(restHeight(tank) - (1.1 - 9.8 / 15)) < 1e-6);
});

test('a hover set down level on a slope comes to lie with it, facing up the slope and across it', () => {
  for (const degrees of [0, 10, 20]) for (const yaw of [0, Math.PI / 2, Math.PI / 4]) {
    const slope = degrees * DEG, collision = slopeWorld(slope);
    const { system, v } = hull(collision, tank, yaw);
    v.tilt = v.roll = 0; v.syncBody(); v.driver = {};
    for (let i = 0; i < 240; i++) system.update(1 / 60);
    const want = planeAngles(slope, yaw);
    assert.ok(Math.abs(v.tilt - want.tilt) < 3 * DEG, `${degrees}deg yaw ${yaw}: tilt ${v.tilt / DEG} vs ${want.tilt / DEG}`);
    assert.ok(Math.abs(v.roll - want.roll) < 3 * DEG, `${degrees}deg yaw ${yaw}: roll ${v.roll / DEG} vs ${want.roll / DEG}`);
    assert.ok(dip(v, slope) < .05, `${degrees}deg yaw ${yaw}: corners ${dip(v, slope)} m under the ground`);
    // Its collision body lies with it too.
    const up = new THREE.Vector3().setFromMatrixColumn(v.matrix, 1);
    assert.ok(up.angleTo(new THREE.Vector3(0, Math.cos(slope), -Math.sin(slope))) < 3 * DEG);
    system.dispose();
  }
});

test('the drive climbs what its grip beats gravity on and slides back down steeper ground', () => {
  const climb = (degrees) => {
    const slope = degrees * DEG, collision = slopeWorld(slope);
    const { system, v } = hull(collision, tank, 0);
    v.driver = {};
    for (let i = 0; i < 60; i++) system.update(1 / 60);
    const z = v.position.z;
    for (let i = 0; i < 300; i++) system.update(1 / 60, v, { forward: 1, right: 0, turn: 0 });
    const climbed = v.position.z - z;
    system.dispose();
    return climbed;
  };
  // Acceleration 4.5 times the up's share: up to about 24 degrees.
  assert.ok(climb(0) > 40, String(climb(0)));
  assert.ok(climb(12) > 15, String(climb(12)));
  assert.ok(climb(30) < 0, String(climb(30)));
});

test('a hover runs up a ramp on its springs, and a wall holds it off', () => {
  const ground = box(0, -1, 0, 400, 2, 400), ramp = box(0, 1.6, 30, 12, .4, 20, -12 * DEG), wall = box(0, 3, 52, 30, 6, .2);
  const positions = new Float32Array([...ground.positions, ...ramp.positions, ...wall.positions]);
  const count = ground.positions.length / 3, count2 = count + ramp.positions.length / 3;
  const indices = new Uint32Array([...ground.indices, ...[...ramp.indices].map((i) => i + count), ...[...wall.indices].map((i) => i + count2)]);
  const collision = new CollisionWorld(positions, indices, new Uint32Array(indices.length / 3).map((_, i) => i));
  const { system, v } = hull(collision);
  v.driver = {};
  let most = 0;
  for (let i = 0; i < 600; i++) {
    system.update(1 / 60, v, { forward: .6, right: 0, turn: 0 });
    if (v.position.z > 24 && v.position.z < 36) most = Math.max(most, v.tilt);
  }
  assert.ok(most > 7 * DEG && most < 17 * DEG, `on the 12 degree ramp: ${most / DEG}`);
  assert.ok(v.position.z > 40 && v.position.z < 51, `at the wall: ${v.position.z}`);
  system.dispose();
});

test('off a ledge it falls at GravityScale times 9.8 and lands on its springs', () => {
  const top = box(0, 39, 0, 40, 2, 40), low = box(0, -1, 0, 400, 2, 400);
  const positions = new Float32Array([...top.positions, ...low.positions]);
  const indices = new Uint32Array([...top.indices, ...[...low.indices].map((i) => i + top.positions.length / 3)]);
  const collision = new CollisionWorld(positions, indices, new Uint32Array(indices.length / 3).map((_, i) => i));
  const { system, v } = hull(collision, tank, 0, 40);
  v.driver = {};
  for (let i = 0; i < 60; i++) system.update(1 / 60);
  assert.ok(Math.abs(v.position.y - 40 - restHeight(tank)) < .05, String(v.position.y));
  v.position.z = 30; v.previous.copy(v.position); v.syncBody();
  // Its ground factor runs out in a second off the ground, and gravity
  // grows from 9.8 to GravityScale times that.
  for (let i = 0; i < 90 && v.support > 0; i++) system.update(1 / 60);
  assert.equal(v.support, 0);
  const before = v.velocity.y;
  system.update(1 / 60);
  assert.ok(Math.abs(before - v.velocity.y - 4 * 9.8 / 60) < .01, String((before - v.velocity.y) * 60));
  for (let i = 0; i < 600; i++) system.update(1 / 60);
  assert.ok(Math.abs(v.position.y - restHeight(tank)) < .1, String(v.position.y));
  assert.ok(v.support > .9);
  system.dispose();
});

test('without springs a hover rides its lift spring at SetAltitude', () => {
  const floor = box(0, -1, 0, 400, 2, 400);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const bare = { ...tank, label: 'Bare', springs: undefined, settings: { ...tank.settings, GravityScale: 1, LiftSpring: 10, LiftDamp: 3 } };
  const { system, v } = hull(collision, bare);
  v.driver = {};
  v.position.y += 1.5; v.previous.copy(v.position);
  for (let i = 0; i < 600; i++) system.update(1 / 60);
  // The 120 Hz integrator applies gravity before damping. At rest the
  // damping cancels part of that step, leaving this small height bias.
  const expected = .5 + (9.8 * 3 / 120) / (10 + 9.8 / (9 * .5));
  assert.ok(Math.abs(v.position.y - expected) < .001, String(v.position.y));
  assert.ok(Math.abs(v.tilt) < .01 && Math.abs(v.roll) < .01);
  system.dispose();
});

const maps = new URL('../public/maps/', import.meta.url);
const pack = packWith(maps, ['imp_hover_fightertank'], 'rhn1') || packWith(maps, ['rep_hover_fightertank']);
const springPack = pack && JSON.parse(fs.readFileSync(pack)).classes.imp_hover_fightertank?.springs;
test('every extracted hover class carries its springs, and the stock fighter tank lies with a slope', { skip: !springPack && 'no extract with hover springs' }, () => {
  for (const file of vehiclePacks(maps)) {
    for (const [name, def] of Object.entries(JSON.parse(fs.readFileSync(file)).classes)) {
      if (def.base !== 'hover') continue;
      assert.ok(def.springs?.length >= 3 && def.springs.length <= 6, `${name} in ${file}`);
      assert.ok(Number.isFinite(restHeight(def)) && restHeight(def) > -1, name);
    }
  }
  const def = JSON.parse(fs.readFileSync(pack)).classes.imp_hover_fightertank;
  const slope = 18 * DEG, collision = slopeWorld(slope);
  const { system, v } = hull(collision, { ...def, model: 'hull', label: 'Stock' }, 0);
  v.tilt = v.roll = 0; v.driver = {};
  for (let i = 0; i < 240; i++) system.update(1 / 60);
  assert.ok(Math.abs(v.tilt - slope) < 3 * DEG, String(v.tilt / DEG));
  system.dispose();
});
