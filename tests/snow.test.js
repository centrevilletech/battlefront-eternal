import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { BOX, Snow, SnowFlakes, Wind, snowBoxes } from '../src/snow.js';

// Hoth's snow and wind as the importer writes them (tools/zero_weather.py).
const HOTH = {
  precipitation: { Type: ['Quads'], Range: [15], Color: [255, 255, 255], Velocity: [2], VelocityRange: [0.8], ParticleDensity: [100],
    ParticleDensityRange: [0], ParticleSize: [0.015], AlphaMinMax: [0.3, 0.45], Texture: ['fx_snow'], RotationRange: [2] },
  wind: [4, 0.3], gusts: { range: 1, rate: 0.2 },
};
const camera = (fov = 60) => { const c = new THREE.PerspectiveCamera(fov, 16 / 9, 0.5, 300); c.position.set(2, 1.8, 3); c.lookAt(40, 1.8, 3); c.updateMatrixWorld(); return c; };
const frustum = (c) => new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(c.projectionMatrix, c.matrixWorldInverse));

test('a box of flakes falls with the wind, wraps in its box and eases toward new wander, alpha and spin', () => {
  const flakes = new SnowFlakes(HOTH.precipitation);
  assert.equal(flakes.count, 100);
  const start = Float32Array.from(flakes.position);
  assert.ok(flakes.alphas.every(a => a >= 0.3 - 1e-6 && a <= 0.45 + 1e-6));
  assert.ok(flakes.spins.every(s => Math.abs(s) <= 1 + 1e-6), 'Rotation 0 ± RotationRange / 2 turns a second');
  const still = new THREE.Vector3();
  const mean = () => { let y = 0; for (let i = 1; i < flakes.position.length; i += 3) y += flakes.position[i]; return y / flakes.count; };
  // With no wander, a flake drops Velocity x dt / 5 of its box.
  const calm = new SnowFlakes({ ...HOTH.precipitation, VelocityRange: [0] });
  const y0 = calm.position[1];
  calm.update(0.1, still);
  assert.ok(Math.abs(((y0 - calm.position[1] + 1) % 1) - 0.1 * 2 / BOX) < 1e-5);
  for (let i = 0; i < 600; i++) flakes.update(1 / 60, new THREE.Vector3(4, 0, 0.3));
  assert.ok(flakes.position.every(v => v >= 0 && v < 1), 'positions wrap in the unit box');
  assert.notDeepEqual(Array.from(flakes.position), Array.from(start));
  assert.ok(Number.isFinite(mean()));
  // Every half second each wander component eases a fifth of the way to
  // ±VelocityRange: it stays within that, and settles to a spread of about
  // 0.16 / sqrt(1 - 0.8^2) = 0.27 m/s.
  let sum = 0;
  for (const v of flakes.velocity) { assert.ok(Math.abs(v) <= 0.8 + 1e-6); sum += v * v; }
  const spread = Math.sqrt(sum / flakes.velocity.length);
  assert.ok(spread > 0.15 && spread < 0.45, `spread ${spread}`);
  assert.ok(flakes.alphas.every(a => a >= 0.3 - 1e-6 && a <= 0.45 + 1e-6));
});

test('the wind eases between gusts around its authored velocity', () => {
  const steady = new Wind({ velocity: [4, 0.3] });
  assert.deepEqual(steady.update(5).toArray(), [4, 0, 0.3]);
  const wind = new Wind({ velocity: [4, 0.3], range: 1, rate: 0.2 });
  let previous = wind.update(0).clone(), jumps = 0;
  for (let i = 0; i < 60 * 30; i++) {
    const v = wind.update(1 / 60);
    assert.ok(Math.abs(v.x - 4) <= 1 + 1e-6 && Math.abs(v.z - 0.3) <= 1 + 1e-6 && v.y === 0);
    if (v.distanceTo(previous) > 0.05) jumps++;
    previous = v.clone();
  }
  assert.equal(jumps, 0, 'it eases; it never jumps');
});

test('boxes tile the view on the 5 m grid out to the range, farthest first, fading over its last third', () => {
  const c = camera(), boxes = [];
  const count = snowBoxes(c, 15, frustum(c), (x, y, z, alpha) => boxes.push({ x, y, z, alpha }));
  assert.equal(count, boxes.length);
  assert.ok(count > 10 && count < 200, `${count} boxes`);
  const depth = b => b.x - 2;
  for (const b of boxes) {
    assert.ok([b.x, b.y, b.z].every(v => Math.abs(((v / BOX) % 1 + 1) % 1 - 0.5) < 1e-9), 'centred on the grid');
    assert.ok(depth(b) < 15);
    const fade = depth(b) > 9.9 ? (15 - depth(b)) / (15 - 9.9) : 1;
    assert.ok(Math.abs(b.alpha - fade) < 1e-9);
  }
  assert.ok(boxes.every((b, i) => i === 0 || depth(b) <= depth(boxes[i - 1]) + 1e-9), 'farthest first');
  assert.ok(boxes.some(b => b.x === 2.5), 'the box around the camera');
  // A limit keeps the nearest.
  const near = [];
  snowBoxes(c, 15, frustum(c), (x, y, z) => near.push(x), 5);
  assert.equal(near.length, 5);
  assert.ok(near.every(x => x - 2 <= Math.min(...boxes.slice(-5).map(b => b.x - 2)) + BOX * 3));
});

const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
const ground = { cast(from, down, range, hit) { hit.point.set(from.x, 0, from.z); return hit; } };

test('snow builds one bounded instanced draw from the imported weather', () => {
  const scene = new THREE.Scene();
  const snow = new Snow({ data: HOTH, scene, collision: ground, texture: () => texture });
  assert.deepEqual(scene.children.map(m => m.name), ['map-snow']);
  const c = camera();
  snow.update(1 / 60, c);
  assert.ok(snow.mesh.visible);
  assert.equal(snow.geometry.instanceCount, snow.stats.boxes * 100);
  assert.ok(snow.stats.flakes <= 32768);
  assert.equal(snow.material.uniforms.flakeCount.value, 100);
  assert.equal(snow.material.uniforms.size.value, 0.015);
  assert.ok(Math.abs(snow.material.uniforms.cameraRight.value.z - 1) < 1e-6 || Math.abs(snow.material.uniforms.cameraRight.value.z + 1) < 1e-6);
  snow.dispose();
  assert.equal(scene.children.length, 0);
});

const hoth = new URL('../public/maps/hot1/map.json', import.meta.url);
test('hot1: its imported precipitation is snow and builds', { skip: !fs.existsSync(hoth) }, () => {
  const data = JSON.parse(fs.readFileSync(hoth)).weather;
  assert.equal(data.precipitation.Type[0], 'Quads');
  const scene = new THREE.Scene(), snow = new Snow({ data, scene, collision: ground, texture: () => texture });
  snow.update(1 / 60, camera());
  assert.ok(snow.stats.boxes > 0);
  snow.dispose();
});
