import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { buildWorld, SKY_ORDER } from '../src/world.js';

// Three sky records as tools/extract.py writes them: the dome, then a
// blended model on its horizon placed by its MovementScale and Offset.
function skyMap() {
  const quad = [[-1, 0, -1], [1, 0, -1], [1, 0, 1], [-1, 0, 1]];
  const verts = [], indices = [];
  const meshes = ['SKY_dome', 'mountains', 'moon'].map((texture, i) => {
    const record = { texture, flags: i ? 4 : 0, attributes: ['position', 'normal', 'uv'], stride: 8,
      vertexOffset: verts.length * 4, vertexCount: 4, indexOffset: indices.length * 4, indexCount: 6, sky: true };
    for (const [x, y, z] of quad) verts.push(x, y + i, z, 0, 1, 0, 0, 0);
    indices.push(0, 1, 2, 0, 2, 3);
    if (i) record.skyLayer = i;
    if (i === 1) record.skyFollow = [0.995, 80];
    return record;
  });
  return { map: { name: 'test', meshes }, geom: new Float32Array(verts).buffer, index: new Uint32Array(indices).buffer };
}

test('every sky model draws around the eye in the config order, horizon models blended before the far scene', () => {
  const { map, geom, index } = skyMap();
  const graphics = { scene: new THREE.Scene(), farScene: new THREE.Scene(), skyLayers: [], localLights: { register() {} } };
  const texture = new THREE.Texture();
  buildWorld(graphics, map, geom, index, () => texture);
  assert.equal(graphics.skyLayers.length, 3);
  assert.equal(graphics.sky, graphics.skyLayers[0].mesh, 'the dome is still the sky the water and storm read');
  assert.equal(SKY_ORDER, -1e9);
  assert.deepEqual(graphics.skyLayers.map(l => l.mesh.renderOrder), [SKY_ORDER, SKY_ORDER + 1, SKY_ORDER + 2]);
  assert.deepEqual(graphics.skyLayers.map(l => l.follow), [null, [0.995, 80], null]);
  // Which model of the sky each is: 0 the dome, which a replacement sky
  // (the Enhanced storm) stands in for; 1 on the models on its horizon.
  assert.deepEqual(graphics.skyLayers.map(l => l.layer), [0, 1, 2]);
  assert.ok(graphics.skyLayers.every(l => graphics.farScene.children.includes(l.mesh)));
  const [dome, mountains] = graphics.skyLayers.map(l => l.mesh.material);
  assert.equal(dome.blending, THREE.NormalBlending);
  assert.equal(dome.transparent, false);
  // Blended, yet sorted with the opaque sky so it draws before the far
  // scene's terrain and buildings.
  assert.equal(mountains.transparent, false);
  assert.equal(mountains.blending, THREE.CustomBlending);
  assert.equal(mountains.blendDst, THREE.OneMinusSrcAlphaFactor);
  assert.ok('BLENDED' in mountains.defines);
});

const extracted = new URL('../public/maps/hot1/map.json', import.meta.url);
test('hot1: the imported sky keeps its mountains and ion cannon', { skip: !fs.existsSync(extracted) }, () => {
  const sky = JSON.parse(fs.readFileSync(extracted)).meshes.filter(m => m.sky);
  if (!sky.some(m => m.skyLayer)) return; // an import from before the sky's models
  assert.deepEqual([...new Set(sky.map(m => m.texture))], ['SKY_Hoth', 'hoth_mountain', 'hoth_ion']);
  assert.ok(sky.filter(m => m.skyLayer).every(m => m.skyFollow?.length === 2));
});
