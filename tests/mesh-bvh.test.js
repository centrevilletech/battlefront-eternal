import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import * as THREE from 'three';
import { MeshBVH, BufferStack } from '../src/mesh-bvh.js';

// The package's own build, under Node its UMD bundle with a BufferStack of its
// own that src/mesh-bvh.js leaves alone: the reference for every cast.
const { MeshBVH: PackageBVH } = createRequire(import.meta.url)('three-mesh-bvh');

function random(seed) {
  return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
}
// A soup of boxes, as two trees over the same triangles: ours and the package's.
function trees(seed, count) {
  const rand = random(seed), parts = [];
  for (let i = 0; i < count; i++) {
    const box = new THREE.BoxGeometry(0.5 + rand() * 4, 0.5 + rand() * 4, 0.5 + rand() * 4)
      .rotateY(rand() * Math.PI).translate(rand() * 60 - 30, rand() * 10, rand() * 60 - 30);
    parts.push(box);
  }
  const positions = [], indices = [];
  for (const part of parts) {
    const base = positions.length / 3;
    positions.push(...part.attributes.position.array);
    indices.push(...[...part.index.array].map((v) => v + base));
  }
  const geometry = () => {
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(positions), 3));
    g.setIndex(new THREE.BufferAttribute(new Uint32Array(indices), 1));
    return g;
  };
  return { ours: new MeshBVH(geometry(), { indirect: true }), theirs: new PackageBVH(geometry(), { indirect: true }) };
}
const hit = (h) => h && [h.distance, h.point.x, h.point.y, h.point.z, h.faceIndex, h.face.normal.x, h.face.normal.y, h.face.normal.z];
// Every triangle a box query visits, in the tree's order, with what a ray
// cast from inside the visit (into the other tree) found: a nested tree
// pushes and pops the buffer stack mid-traversal.
function visits(bvh, other, box, ray) {
  const out = [], tri = new THREE.Box3();
  bvh.shapecast({
    intersectsBounds: (bounds) => bounds.intersectsBox(box),
    intersectsTriangle: (t, index) => {
      if (!tri.setFromPoints([t.a, t.b, t.c]).intersectsBox(box)) return false;
      out.push(index);
      if (out.length % 7 === 1) out.push(hit(other.raycastFirst(ray, THREE.DoubleSide, 0, 40)));
      return false;
    },
  });
  return out;
}

test('trees whose buffer views are made once cast exactly as the package does', () => {
  const a = trees(7, 120), b = trees(19, 60), rand = random(31);
  const ray = new THREE.Ray(), box = new THREE.Box3(), center = new THREE.Vector3();
  let hits = 0, nested = 0;
  for (let pass = 0; pass < 400; pass++) {
    ray.origin.set(rand() * 70 - 35, rand() * 12 - 1, rand() * 70 - 35);
    ray.direction.set(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1).normalize();
    const near = pass % 5 ? 0 : rand() * 3, far = 5 + rand() * 80;
    const ours = hit(a.ours.raycastFirst(ray, THREE.DoubleSide, near, far)), theirs = hit(a.theirs.raycastFirst(ray, THREE.DoubleSide, near, far));
    assert.deepEqual(ours, theirs, `ray ${pass}`);
    hits += Boolean(ours);
    const r = 0.5 + rand() * 6;
    center.set(rand() * 60 - 30, rand() * 10, rand() * 60 - 30);
    box.min.copy(center).addScalar(-r); box.max.copy(center).addScalar(r);
    const visited = visits(a.ours, b.ours, box, ray);
    assert.deepEqual(visited, visits(a.theirs, b.theirs, box, ray), `box ${pass}`);
    nested += visited.filter(Array.isArray).length;
  }
  assert.ok(hits > 50 && nested > 50, 'the rays met the boxes, and nested casts found them too');
  assert.equal(BufferStack.float32Array, null, 'the stack is empty between casts');
});

test('a tree buffer\'s typed-array views are made once', () => {
  const { ours } = trees(3, 40), seen = new Set();
  const query = { intersectsBounds: () => { seen.add(BufferStack.float32Array).add(BufferStack.uint16Array).add(BufferStack.uint32Array); return false; } };
  for (let i = 0; i < 50; i++) ours.shapecast(query);
  assert.equal(seen.size, 3, 'the same three views every cast');
  for (const view of seen) assert.equal(view.buffer, ours._roots[0]);
});
