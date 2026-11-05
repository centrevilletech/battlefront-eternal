import assert from 'node:assert/strict';
import * as THREE from 'three';
import { describe, zeroMaterial } from '../src/materials.js';
import { Foliage } from '../src/foliage.js';

for (const texture of ['yav_prop_treebillboard_03', 'yav_prop_bush1', 'nab2_prop_bark_branch']) {
  const record = { texture, flags: 6 };
  const look = describe(record);
  const material = zeroMaterial(look.kind, look);
  assert.equal(material.transparent, false, 'foliage must write depth before transparent holograms');
  assert.equal(material.depthWrite, true);
  assert.equal(material.alphaToCoverage, true);
  assert(!Object.hasOwn(material.defines, 'BLENDED'), 'use opaque near/far coverage transition');
}
{
  const record = { texture: 'nab2_prop_fountain_water', flags: 4 };
  assert.equal(describe(record).blending, 'normal', 'water remains transparent');
}
const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
const data = {
  classes: { tree: { base: 'leafpatch', Radius: 1.3, Height: 13, HeightScale: 1.6, ConeHeight: 8,
    MinSize: 1.2, MaxSize: 1.4, Seed: 651684651, NumParticles: 50, NumParts: 1, Alpha: 1 } },
  instances: [{ class: 'tree', matrix: new THREE.Matrix4().makeTranslation(0, 0, -20).toArray() }],
};
const scene = new THREE.Scene();
const foliage = new Foliage(scene, data, { texture: () => texture, nearEnd: () => 220 });
const kind = foliage.kinds.get('tree');
assert.equal(kind.cards.length, 50);
for (const leaf of kind.cards) {
  const height = leaf.p.y * 1.6;
  assert(height >= 0 && height <= 13, 'authored canopy height is retained');
  if (leaf.p.y > 5) assert(Math.hypot(leaf.p.x, leaf.p.z) <= 1.3 * (13 - leaf.p.y) / 8 + 1e-6, 'taper in foliage-local space');
}
assert.equal(kind.batch.geometry.attributes.position.count, 200, 'four static vertices per authored leaf cluster');
assert.equal(kind.batch.geometry.index.count, 300, 'two triangles per leaf cluster');
console.log('Foliage depth, canopy shape, and Rust input layout checks passed.');
