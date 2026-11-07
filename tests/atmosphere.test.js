import { SurfaceOwner, stepSurface } from './helpers/surfaces.js';
import { contentURL } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Atmosphere } from '../src/atmosphere.js';
import { loadEnvironment } from '../src/environment/module.js';
import { runAtmosphere } from './support/atmosphere-scenario.js';

await loadEnvironment();

// Geonosis's haze as the importer writes it (tools/zero_atmosphere.py).
const FOG = { texture: 'brownmist', range: [90, 120], color: [168, 172, 180, 128], velocity: [5, 0], rotation: 0.1, height: 16, size: 28, density: 60, cell: 256 };
const DUST = {
  texture: 'browndust', count: 25, minPos: [-50, 4, -50], maxPos: [-10, 12, -10], minVel: [2, 0, 2], maxVel: [10, 1, 10],
  minSize: 15, maxSize: 20, minLifetime: 3, maxLifetime: 5, alpha: 0.5, minDistance: 0, maxDistance: 150, radiusFade: [50, 60], heightScale: 0.2, radius: 70,
};
// The haze over a scripted flight: a FogCloud and two
// dusteffect classes over a terrain with a hole in it, a camera that moves,
// turns and changes its lens, and frames of every length, digested per frame
// (each batch's count and a hash of its records, its camera axes and the
// statistics), held to a recorded run.
test('the squares in view are gathered and ordered exactly as the recorded haze has them', () => {
  const golden = JSON.parse(fs.readFileSync(new URL('./fixtures/atmosphere-golden.json', import.meta.url), 'utf8'));
  const rows = runAtmosphere(Atmosphere);
  assert.equal(rows.length, golden.length);
  rows.forEach((row, i) => assert.deepEqual(row, golden[i], `frame ${i}`));
});

test('the terrain is asked once per grid corner, however many frames use it', () => {
  let asked = 0;
  const collision = { ground(x, z, top, out) { asked++; out.height = x * 0.01; return out; } };
  const scene = new THREE.Scene();
  const atmosphere = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: { fogCloud: FOG }, scene, collision, texture: () => texture });
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 2000);
  camera.position.set(0, 2, 0);
  camera.lookAt(100, 2, 0);
  stepSurface(atmosphere,1 / 60, camera);
  const first = asked;
  assert.ok(first > 0);
  stepSurface(atmosphere,0, camera);
  assert.equal(asked, first, 'a frame in the same place asks nothing new');
  atmosphere.dispose();
});

const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
const geonosis = {
  fogCloud: FOG,
  dust: { classes: { geo_prop_dust: DUST }, instances: [{ class: 'geo_prop_dust', position: [30, 0, 0] }, { class: 'geo_prop_dust', position: [5000, 0, 0] }] },
};
const flatCollision = { ground(x, z, top, out) { out.height = 0; return out; } };

test('the map haze builds one bounded draw per texture and fills it from the camera', () => {
  const scene = new THREE.Scene();
  const atmosphere = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: geonosis, scene, collision: flatCollision, texture: () => texture });
  assert.deepEqual(scene.children.map(m => m.name), ['map-fog-cloud', 'map-dust-geo_prop_dust']);
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 2000);
  camera.position.set(0, 2, 0); camera.lookAt(100, 2, 0);
  for (let i = 0; i < 30; i++) stepSurface(atmosphere,1 / 30, camera);
  const [fog, dust] = atmosphere.batches;
  assert.ok(fog.geometry.instanceCount > 0 && fog.mesh.visible);
  assert.equal(atmosphere.stats.objects, 1, 'the far copy is past its MaxDistance');
  assert.equal(dust.geometry.instanceCount, atmosphere.stats.dust);
  assert.ok(dust.geometry.instanceCount <= DUST.count);
  // Back to front: depths along the view never grow down the batch.
  const centers = dust.center.array, depths = [];
  for (let i = 0; i < dust.geometry.instanceCount; i++) depths.push(centers[i * 4]);
  assert.ok(depths.every((d, i) => i === 0 || d <= depths[i - 1] + 1e-4));
  // Flattened by the height scale: centres sit within 0.2 x 12 m of the copy.
  for (let i = 0; i < dust.geometry.instanceCount; i++) assert.ok(centers[i * 4 + 1] >= 0.8 - 1e-4 && centers[i * 4 + 1] <= 2.4 + 0.2 * 1 * 5 + 1e-3);
  assert.ok(Math.abs(dust.material.uniforms.axisU.value.y + 0.2) < 1e-6, 'the square is squashed vertically');
  // The band goes with the camera; the copies stay put.
  camera.position.set(0, 2, 3000); camera.lookAt(100, 2, 3000);
  stepSurface(atmosphere,1 / 30, camera);
  assert.ok(fog.geometry.instanceCount > 0);
  assert.equal(atmosphere.stats.objects, 0);
  assert.equal(dust.geometry.instanceCount, 0);
  assert.equal(dust.mesh.visible, false);
  atmosphere.dispose();
  assert.equal(scene.children.length, 0);
});

test('a map without haze, or with a texture missing, draws nothing and costs nothing', () => {
  const scene = new THREE.Scene();
  const none = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: null, scene, collision: flatCollision, texture: () => texture });
  stepSurface(none,1 / 60, new THREE.PerspectiveCamera());
  assert.equal(scene.children.length, 0);
  const missing = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: geonosis, scene, collision: flatCollision, texture: () => null });
  assert.equal(scene.children.length, 0);
  stepSurface(missing,1 / 60, new THREE.PerspectiveCamera());
  missing.dispose(); none.dispose();
});

// Imported maps, where an extraction is present (tools/extract.py).
const HAZE = {
  geo1: { fog: 'brownmist', dust: { geo_prop_dust: 23 } },
  end1: { fog: 'fluffy', dust: {} },
  rhn2: { fog: 'fluffy', dust: { Rhn2_prop_cloud: 13 } },
  yav1: { fog: 'fluffy', dust: { yav_prop_mist: 23 } },
  yav2: { fog: 'fluffy', dust: { yav_prop_mist: 9 } },
  hot1: { fog: null, dust: { hoth_prop_dust: 52, hoth_prop_mist: 15 } },
  kam1: { fog: null, dust: { kam1_prop_mist: 22 } },
  tat1: { fog: null, dust: { tat_prop_dust: 66 } },
  bes2: null, nab2: null, tat2: null,
};
for (const [map, expected] of Object.entries(HAZE)) {
  const path = contentURL(`maps/${map}/map.json`);
  const data = fs.existsSync(path) ? JSON.parse(fs.readFileSync(path)) : null;
  test(`${map}: its imported haze builds`, { skip: !data || !('atmosphere' in data) }, () => {
    if (!expected) return assert.equal(data.atmosphere, null);
    assert.equal(data.atmosphere.fogCloud?.texture ?? null, expected.fog);
    const counts = {};
    for (const i of data.atmosphere.dust?.instances || []) counts[i.class] = (counts[i.class] || 0) + 1;
    assert.deepEqual(counts, expected.dust);
    for (const name of [expected.fog, ...Object.values(data.atmosphere.dust?.classes || {}).map(c => c.texture)].filter(Boolean))
      assert.ok(data.textures[name], `${name} exported`);
    const scene = new THREE.Scene();
    const atmosphere = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: data.atmosphere, scene, collision: flatCollision, texture: () => texture });
    assert.equal(scene.children.length, (expected.fog ? 1 : 0) + Object.keys(expected.dust).length);
    atmosphere.dispose();
  });
}
