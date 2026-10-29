import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CHIP, DestructionFx, EMBER, FIRE, FLASH, GLOW, SCORCH, SMOKE, SPARK } from '../src/destruction-fx.js';

const at = new THREE.Vector3(1, 2, 3);
const still = new THREE.Vector3();

test('particles are written once, retired when their life is up, and the pools are one draw each', () => {
  const scene = new THREE.Scene();
  const fx = new DestructionFx(scene, { hot: 64, smoke: 32, arcs: 4 });
  assert.equal(fx.draws, 0);
  for (const style of [SPARK, FIRE, FLASH, GLOW, EMBER]) fx.hotParticle(style, at, still, 0.5, 0.1, 0.2, 1, 1, 1);
  for (const style of [SMOKE, CHIP, SCORCH]) fx.smokeParticle(style, at, still, 1.5, 0.1, 0.5, -0.1, 1, 0.8);
  fx.update(1 / 60);
  assert.deepEqual([fx.hot.count, fx.smoke.count, fx.draws], [5, 3, 2]);
  assert.equal(fx.hot.mesh.geometry.instanceCount, 5);
  for (let i = 0; i < 40; i++) fx.update(1 / 60);
  assert.deepEqual([fx.hot.count, fx.smoke.count], [0, 3], 'the hot ones are spent first');
  for (let i = 0; i < 60; i++) fx.update(1 / 60);
  assert.equal(fx.smoke.count, 0);
  assert.equal(fx.draws, 0, 'an empty pool draws nothing');
});

test('a full pool drops new particles instead of growing', () => {
  const fx = new DestructionFx(new THREE.Scene(), { hot: 8, smoke: 8, arcs: 2 });
  for (let i = 0; i < 20; i++) fx.hotParticle(SPARK, at, still, 1, 0.02, 0.01, 1, 1, 1);
  assert.equal(fx.hot.count, 8);
  assert.equal(fx.hot.dropped, 12);
  assert.equal(fx.hot.arrays[0].length, 8 * 4);
});

test('births in frames stepped without a draw still reach the GPU', () => {
  const fx = new DestructionFx(new THREE.Scene(), { hot: 64, smoke: 8, arcs: 2 });
  fx.hotParticle(SPARK, at, still, 5, 0.02, 0.01, 1, 1, 1);
  fx.update(1 / 60);
  // Uploaded, as the renderer does after drawing.
  for (const attribute of fx.hot.attributes) attribute.clearUpdateRanges();
  fx.hotParticle(FIRE, at, still, 5, 0.1, 0.3, -0.5, 3, 1);
  fx.update(1 / 60);
  fx.hotParticle(FIRE, at, still, 5, 0.1, 0.3, -0.5, 3, 1);
  fx.update(1 / 60); // stepped again before any draw
  for (const attribute of fx.hot.attributes) {
    assert.equal(attribute.updateRanges.length, 1);
    assert.deepEqual(attribute.updateRanges[0], { start: 4, count: 8 }, 'both undrawn births are in the range');
  }
});

test('a retired particle hands its slot to the last one, and the clock rebases', () => {
  const fx = new DestructionFx(new THREE.Scene(), { hot: 8, smoke: 8, arcs: 2 });
  fx.hotParticle(SPARK, at, still, 0.1, 0.02, 0.01, 1, 1, 1);
  fx.hotParticle(FIRE, new THREE.Vector3(9, 9, 9), still, 50, 0.1, 0.3, -0.5, 3, 1);
  for (let i = 0; i < 12; i++) fx.update(1 / 60);
  assert.equal(fx.hot.count, 1);
  assert.deepEqual(Array.from(fx.hot.arrays[0].slice(0, 3)), [9, 9, 9], 'the survivor moved into the empty slot');
  const birth = fx.hot.arrays[0][3];
  fx.time = 599.99;
  fx.update(0.02);
  assert.ok(fx.time < 1, 'rebased');
  assert.ok(Math.abs(fx.hot.arrays[0][3] - (birth - 600)) < 1e-3, 'births move with the clock');
});

test('arcs follow what they are attached to, crackle, and are bounded', () => {
  const scene = new THREE.Scene();
  const fx = new DestructionFx(scene, { hot: 8, smoke: 8, arcs: 3 });
  const holder = new THREE.Object3D();
  holder.position.set(0, 1, 0);
  holder.updateMatrixWorld();
  assert.ok(fx.arc(holder, new THREE.Vector3(), null, new THREE.Vector3(1, 1, 0), 0.5));
  assert.ok(fx.arc(null, at, null, new THREE.Vector3(1, 3, 3), 0.5));
  assert.ok(fx.arc(null, at, null, new THREE.Vector3(2, 2, 3), 0.5));
  assert.equal(fx.arc(null, at, null, new THREE.Vector3(3, 2, 3), 0.5), false, 'no more than the capacity');
  fx.update(1 / 60);
  assert.equal(fx.arcs.mesh.visible, true);
  const first = Array.from(fx.arcs.positions.slice(0, 3));
  assert.deepEqual(first.map((v) => Math.round(v * 1000) / 1000), [0, 1, 0], 'starts at the holder');
  holder.position.set(0, 2, 0);
  holder.updateMatrixWorld();
  fx.update(1 / 30);
  assert.ok(Math.abs(fx.arcs.positions[1] - 2) < 1e-6, 'and moves with it');
  const jag = fx.arcs.positions.slice(6, 9).join();
  fx.update(1 / 30);
  assert.notEqual(fx.arcs.positions.slice(6, 9).join(), jag, 'a new jag each flicker');
  for (let i = 0; i < 40; i++) fx.update(1 / 60);
  assert.equal(fx.arcs.live, 0);
  assert.equal(fx.arcs.mesh.visible, false);
});

test('lesser emitters back off as a pool fills, keeping a reserve for blasts and wrecks', () => {
  const fx = new DestructionFx(new THREE.Scene(), { hot: 100, smoke: 100, arcs: 2 });
  assert.equal(fx.room(fx.smoke), 1);
  const fill = (n) => { while (fx.smoke.count < n) fx.smokeParticle(SMOKE, at, still, 10, 0.1, 0.5, -0.1, 1, 0.8); };
  fill(40);
  assert.equal(fx.room(fx.smoke), 1, 'room to spare');
  fill(55);
  assert.ok(fx.room(fx.smoke) > 0 && fx.room(fx.smoke) < 1, 'giving way');
  fill(75);
  assert.equal(fx.room(fx.smoke), 0, 'the last quarter is kept for what matters');
});
