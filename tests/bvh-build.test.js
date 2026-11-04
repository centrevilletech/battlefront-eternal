import test from 'node:test';
import assert from 'node:assert/strict';
import { Box3, BufferAttribute, BufferGeometry, InterleavedBuffer, InterleavedBufferAttribute, Matrix4, Ray, Vector3, DoubleSide } from 'three';
import { loadRayKernel } from '../src/ray-kernel.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { MeshBVH } from '../src/mesh-bvh.js';
import { builderCases, geometry, referenceTree } from './fixtures/bvh-build-cases.js';

function equalTree(actual, expected, label) {
  assert.deepEqual(actual._roots.map(b => new Uint32Array(b)), expected._roots.map(b => new Uint32Array(b)), `${label}: roots`);
  assert.deepEqual(actual._indirectBuffer, expected._indirectBuffer, `${label}: indirect`);
  assert.deepEqual(actual.geometry.boundingBox, expected.geometry.boundingBox, `${label}: geometry bounds`);
}
test('native BVH builder matches exact node bytes, partition order and indirect width', async () => {
  const rays = await loadRayKernel(); let count = 0;
  for (const { label, positions, indices } of builderCases()) {
    const g = geometry(positions, indices), original = indices.slice(), expected = referenceTree(positions, indices);
    if (count % 19 === 0) rays.memory.grow(1);
    const actual = rays.buildBVH(g);
    assert.ok(rays.treeIds.has(actual), `${label}: native path ran`);
    equalTree(actual, expected, label);
    assert.deepEqual(indices, original, `${label}: original polygon indexing retained`);
    assert.equal(rays.tree(actual), rays.treeIds.get(actual), 'native storage reused');
    count++;
  }
  assert.equal(count, 107);
});

test('live collision construction uses native map, terrain, prop and moving shape trees', async () => {
  const rays = await loadRayKernel(), fixture = [...builderCases({ large: false })].find(c => c.label === 'soup-800-903-64');
  const { positions: p, indices: i } = fixture, polygons = Uint32Array.from({ length: i.length / 3 }, (_, n) => n);
  const world = new CollisionWorld(p, i, polygons, p, i, rays);
  const body = world.addBody(p, i, polygons), shape = world.createBodyShape(p, i, polygons);
  const moving = world.addMovingBody(shape, new Matrix4().makeTranslation(1, 2, 3));
  for (const tree of [world.bvh, world.terrain, body.bvh, shape.bvh]) assert.ok(rays.treeIds.has(tree), 'live tree built natively');
  const expected = referenceTree(p, i); equalTree(world.bvh, expected, 'live map');
  const box = new Box3(new Vector3(-50, -20, -50), new Vector3(50, 20, 50)), visits = tree => {
    const order = []; tree.shapecast({ intersectsBounds: bounds => bounds.intersectsBox(box), intersectsTriangle: (_, index) => { order.push(index); return false; } }); return order;
  };
  assert.deepEqual(visits(world.bvh), visits(expected));
  for (let j = 0; j < 80; j++) {
    const ray = new Ray(new Vector3(j - 40, 15, j - 40), new Vector3(0, -1, 0));
    assert.deepEqual(world.bvh.raycastFirst(ray, DoubleSide), expected.raycastFirst(ray, DoubleSide));
  }
  assert.equal(moving.bvh, shape.bvh);
});

test('geometry outside the collision builder format retains the general MeshBVH path', async () => {
  const rays = await loadRayKernel(), p = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), i = new Uint32Array([0, 1, 2]);
  const cases = [
    () => { const g = geometry(p, i); g.addGroup(0, 3); return g; },
    () => { const g = geometry(p, i); g.setDrawRange(0, 3); return g; },
    () => { const g = geometry(p, i); g.setIndex(null); return g; },
    () => { const g = geometry(p, i); g.setAttribute('position', new BufferAttribute(new Int16Array([0, 0, 0, 32767, 0, 0, 0, 32767, 0]), 3, true)); return g; },
    () => { const g = new BufferGeometry(); g.setAttribute('position', new InterleavedBufferAttribute(new InterleavedBuffer(p, 3), 3, 0)); g.setIndex(new BufferAttribute(i, 1)); return g; },
  ];
  for (const make of cases) {
    const actual = rays.buildBVH(make()), expected = new MeshBVH(make(), { indirect: true });
    assert.equal(rays.treeIds.has(actual), false);
    equalTree(actual, expected, 'general geometry');
  }
});
