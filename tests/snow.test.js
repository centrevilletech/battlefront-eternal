import { SurfaceOwner, stepSurface } from './helpers/surfaces.js';
import { contentURL } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Snow } from '../src/snow.js';
import { EFFECT } from '../src/rust-render/effect-layout.js';
import { loadEnvironment } from '../src/environment/module.js';
import { runSnow } from './support/snow-scenario.js';

await loadEnvironment();

// Hoth's snow and wind as the importer writes them (tools/zero_weather.py).
const HOTH = {
  precipitation: { Type: ['Quads'], Range: [15], Color: [255, 255, 255], Velocity: [2], VelocityRange: [0.8], ParticleDensity: [100],
    ParticleDensityRange: [0], ParticleSize: [0.015], AlphaMinMax: [0.3, 0.45], Texture: ['fx_snow'], RotationRange: [2] },
  wind: [4, 0.3], gusts: { range: 1, rate: 0.2 },
};
const camera = (fov = 60) => { const c = new THREE.PerspectiveCamera(fov, 16 / 9, 0.5, 300); c.position.set(2, 1.8, 3); c.lookAt(40, 1.8, 3); c.updateMatrixWorld(); return c; };

// The snow over a scripted walk: Hoth's precipitation and
// gusting wind over a roofed and open ground, a camera that moves, turns and
// changes its lens, frames of every length and a teleport that spends the
// cover cache's budget, digested per frame (the draw's counts and visibility,
// the camera axes and a hash of every record the shader reads), held to a recorded run.
test('flakes move, the wind gusts and the boxes in view are chosen exactly as the recorded snow has them', () => {
  const golden = JSON.parse(fs.readFileSync(new URL('./fixtures/snow-golden.json', import.meta.url), 'utf8'));
  const rows = runSnow(Snow);
  assert.equal(rows.length, golden.length);
  rows.forEach((row, i) => assert.deepEqual(row, golden[i], `frame ${i}`));
});

const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
const ground = { cast(from, down, range, hit) { hit.point.set(from.x, 0, from.z); return hit; } };

test('snow builds one bounded instanced draw from the imported weather', () => {
  const scene = new THREE.Scene();
  const snow = new Snow({surfaceOwner:new SurfaceOwner(), data: HOTH, scene, collision: ground, texture: () => texture });
  assert.deepEqual(scene.children.map(m => m.name), ['map-snow']);
  const c = camera();
  stepSurface(snow,1 / 60, c);
  assert.ok(snow.mesh.visible);
  assert.equal(snow.count, snow.stats.boxes * 100);
  assert.ok(snow.stats.flakes <= 32768);
  assert.equal(snow.values[EFFECT.SNOW.FLAKE_COUNT], 100);
  assert.equal(snow.values[EFFECT.SNOW.SIZE], 0.015);
  assert.ok(Math.abs(snow.values[EFFECT.SNOW.CAMERA_RIGHT+2] - 1) < 1e-6 || Math.abs(snow.values[EFFECT.SNOW.CAMERA_RIGHT+2] + 1) < 1e-6);
  snow.dispose();
  assert.equal(scene.children.length, 0);
});

const hoth = contentURL('maps/hot1/map.json');
test('hot1: its imported precipitation is snow and builds', { skip: !fs.existsSync(hoth) }, () => {
  const data = JSON.parse(fs.readFileSync(hoth)).weather;
  assert.equal(data.precipitation.Type[0], 'Quads');
  const scene = new THREE.Scene(), snow = new Snow({surfaceOwner:new SurfaceOwner(), data, scene, collision: ground, texture: () => texture });
  stepSurface(snow,1 / 60, camera());
  assert.ok(snow.stats.boxes > 0);
  snow.dispose();
});
