import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';

function ramp(slope = 0) {
  // A broad continuous walkable face: any repeating step-up here is false.
  const p = [-100, -100 * slope, -100, 100, -100 * slope, -100,
    100, 100 * slope, 100, -100, 100 * slope, 100];
  return new CollisionWorld(new Float32Array(p), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
}
function track(soldier, ticks, input) {
  const samples = [];
  for (let i = 0; i < ticks; i++) {
    const p = soldier.position.clone();
    soldier.tick(typeof input === 'function' ? input(i) : input, 1 / 60);
    samples.push({ dy: soldier.position.y - p.y,
      distance: Math.hypot(soldier.position.x - p.x, soldier.position.z - p.z),
      grounded: soldier.grounded, air: soldier.air, dive: soldier.dive });
    assert(soldier.position.toArray().every(Number.isFinite), 'finite movement');
  }
  return samples;
}
function metrics(samples) {
  // Discard spawn/acceleration so the measurements describe travel.
  const rows = samples.slice(30);
  const meanSpeed = rows.reduce((sum, row) => sum + row.distance * 60, 0) / rows.length;
  return {
    maxVerticalTick: Math.max(...rows.map(row => Math.abs(row.dy))),
    maxVerticalChange: Math.max(...rows.slice(1).map((row, i) => Math.abs(row.dy - rows[i].dy))),
    verticalReversals: rows.filter((row, i) => i > 0 && row.dy * rows[i - 1].dy < -0.000005).length,
    unsupportedTicks: samples.filter(row => !row.grounded).length,
    meanSpeed,
    speedDeviation: Math.sqrt(rows.reduce((sum, row) => sum + (row.distance * 60 - meanSpeed) ** 2, 0) / rows.length),
  };
}

test('continuous slope never triggers riser stepping or launches the walking soldier', () => {
  for (const yaw of [0, Math.PI / 4, Math.PI / 2, Math.PI]) {
    const soldier = new Soldier(ramp(0.4), {});
    soldier.spawn(0, 0, 0, yaw);
    const samples = track(soldier, 180, { forward: 1, right: 0 });
    const m = metrics(samples);
    assert.equal(m.unsupportedTicks, 0, `ground support yaw=${yaw}`);
    assert(m.maxVerticalTick < 0.08, `no false step lift yaw=${yaw}: ${m.maxVerticalTick}`);
    assert.equal(m.verticalReversals, 0, `no alternating height correction yaw=${yaw}`);
    assert(m.speedDeviation < 0.015, `steady slope drive yaw=${yaw}: ${m.speedDeviation}`);
  }
});

test('turning while traversing a slope keeps continuous support and bounded travel', () => {
  const soldier = new Soldier(ramp(0.4), {});
  soldier.spawn(0, 0, 0, 0);
  const samples = track(soldier, 300, { forward: 1, right: 0, turn: 0.3 });
  assert.equal(samples.filter(row => !row.grounded).length, 0);
  assert(Math.max(...samples.slice(5).map(row => Math.abs(row.dy))) < 0.09);
  assert(Math.max(...samples.map(row => row.distance)) < 0.2, 'turning cannot inject horizontal speed');
});

test('jump remains ballistic and never reports grounded presentation corrections in flight', () => {
  const soldier = new Soldier(ramp(), {});
  soldier.spawn(0, 0, 0, 0);
  track(soldier, 60, { forward: 0, right: 0 });
  const initialAdjustment = soldier.groundAdjustment;
  let apex = 0, airborneTicks = 0;
  for (let i = 0; i < 180; i++) {
    soldier.tick({ forward: 0, right: 0, jump: i === 0 }, 1 / 60);
    apex = Math.max(apex, soldier.position.y);
    if (soldier.air) {
      airborneTicks++;
      assert.equal(soldier.groundAdjustment, initialAdjustment, 'jump is not a stair correction');
    }
  }
  assert(apex > 1.4 && apex < 2.1, `normal jump height: ${apex}`);
  assert(airborneTicks > 30 && airborneTicks < 90);
  assert(soldier.grounded && !soldier.air);
  soldier.spawn(0, 0, 0, 0);
  assert.equal(soldier.groundAdjustment, 0, 'teleport resets the accumulated correction');
});

function retailWorld(name) {
  const dir = new URL(`../public/maps/${name}/`, import.meta.url);
  if (!fs.existsSync(new URL('collision.bin', dir))) return null;
  const load = (file, Type) => {
    const bytes = fs.readFileSync(new URL(file, dir));
    return new Type(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  };
  return new CollisionWorld(load('collision.bin', Float32Array), load('collision_index.bin', Uint32Array),
    load('collision_poly.bin', Uint32Array), load('terrain_collision.bin', Float32Array), load('terrain_collision_index.bin', Uint32Array));
}
const retail = Object.fromEntries(['nab2', 'bes2'].map(name => [name, retailWorld(name)]));
for (const [name, map, start, yaw, ticks] of [
  ['Theed uphill', 'nab2', [-168, -49, -25], Math.PI, 180],
  ['Theed downhill', 'nab2', [-168, -44, -48], 0, 150],
  ['Bespin uphill', 'bes2', [-163.068, -41, 81.088], Math.PI / 2, 180],
  ['Bespin downhill', 'bes2', [-143.068, -33, 81.088], -Math.PI / 2, 150],
]) {
  test(`${name}: actual staircase retains support without repeated height oscillation`, { skip: !retail[map] }, () => {
    const soldier = new Soldier(retail[map], {});
    soldier.spawn(...start, yaw);
    const m = metrics(track(soldier, ticks, { forward: 1, right: 0 }));
    assert.equal(m.unsupportedTicks, 0, 'no false airborne contacts on stairs');
    assert(m.maxVerticalTick < 0.08, 'no 20–30cm false step jumps');
    assert(m.maxVerticalChange < 0.075, 'no up/down pogo corrections');
    assert(m.verticalReversals <= 1, 'only the real staircase crest can reverse the vertical direction');
    assert(m.meanSpeed > 7, 'continuous uphill and downhill progress');
    console.log(`${name} (60 Hz): ${JSON.stringify(m)}`);
  });
}
for (const [map, start, yaw, right] of [
  ['nab2', [-168, -49, -29], Math.PI / 2, -1],
  ['nab2', [-168, -44, -46], Math.PI / 2, 1],
  ['bes2', [-160, -40, 81.088], 0, -1],
  ['bes2', [-145, -34, 81.088], 0, 1],
]) {
  test(`${map}: roll ${right < 0 ? 'uphill' : 'downhill'} remains attached to the staircase`, { skip: !retail[map] }, () => {
    const soldier = new Soldier(retail[map], {});
    soldier.spawn(...start, yaw);
    soldier.diveTime = 0.75;
    soldier.stateTime = 1;
    const rows = track(soldier, 70, i => ({ forward: 0, right, jump: i === 0 }));
    assert(rows.every(row => row.grounded && !row.air), 'roll never becomes an unintended jump/fall');
    assert.equal(rows.filter(row => row.dive).length, 45, 'complete the whole roll');
    assert.equal(metrics(rows).verticalReversals, 0, 'roll does not pogo on stairs');
    assert(Math.max(...rows.slice(3).map(row => Math.abs(row.dy))) < 0.085);
    assert.equal(soldier.posture, 'stand');
  });
}
