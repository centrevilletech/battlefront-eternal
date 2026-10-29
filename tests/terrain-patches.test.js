import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { TerrainPatches } from '../src/terrain-patches.js';

// A 1 km heightfield on an 8 m grid, like a map's terrain.
function grid(size = 1024, spacing = 8) {
  const n = size / spacing + 1, positions = [], indices = [];
  for (let z = 0; z < n; z++) for (let x = 0; x < n; x++) positions.push(x * spacing - size / 2, Math.sin(x * .3) * 4 + Math.cos(z * .2) * 3, z * spacing - size / 2);
  for (let z = 0; z < n - 1; z++) for (let x = 0; x < n - 1; x++) {
    const a = z * n + x, b = a + 1, c = a + n, d = c + 1;
    indices.push(a, c, b, b, c, d);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(indices);
  return geometry;
}

const key = (a, b, c) => [a, b, c].sort((p, q) => p - q).join();
function triangles(level) {
  const set = new Set(), array = level.index.array;
  for (const cell of level.cells) for (let i = cell.start; i < cell.end; i += 3) set.add(key(array[i], array[i + 1], array[i + 2]));
  return set;
}

test('patches hold every terrain triangle exactly once', () => {
  const geometry = grid();
  const source = geometry.index.array.slice();
  const patches = new TerrainPatches(geometry);
  const held = triangles(patches.full);
  assert.equal(patches.full.cells.reduce((n, c) => n + (c.end - c.start), 0), source.length);
  for (let i = 0; i < source.length; i += 3) assert.ok(held.has(key(source[i], source[i + 1], source[i + 2])));
  assert.equal(patches.full.cells.length, 64);
});

test('the reduced level is the grid at twice the spacing, facing the same way', () => {
  const patches = new TerrainPatches(grid());
  const reduced = patches.level(true);
  const count = reduced.index.count / 3;
  assert.equal(count, 64 * 64 * 2);
  const p = patches.position, a = reduced.index.array;
  for (let i = 0; i < a.length; i += 3) {
    for (let k = 0; k < 3; k++) assert.equal(Math.abs((p.getX(a[i + k]) + 512) % 16), 0);
    const area = (p.getX(a[i + 1]) - p.getX(a[i])) * (p.getZ(a[i + 2]) - p.getZ(a[i])) - (p.getZ(a[i + 1]) - p.getZ(a[i])) * (p.getX(a[i + 2]) - p.getX(a[i]));
    assert.ok(area < 0, 'same winding as the source grid (clockwise in x/z)');
  }
});

test('a view draws only the runs of cells in its frustum, and the far scene skips cells nearer than the fade', () => {
  const patches = new TerrainPatches(grid());
  const scene = new THREE.Scene(), fade = new THREE.Vector2(150, 200);
  const near = patches.view(scene, new THREE.MeshBasicMaterial());
  scene.add(near);
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 200);
  camera.position.set(64, 20, 64);
  camera.lookAt(64, 15, -100);
  camera.updateMatrixWorld();
  scene.onBeforeRender(null, scene, camera, null);
  const drawn = near.children.filter((m) => m.visible).reduce((n, m) => n + m.geometry.drawRange.count, 0);
  assert.ok(drawn > 0 && drawn < patches.full.index.count / 4, `near view draws ${drawn} indices`);

  const farScene = new THREE.Scene();
  const far = patches.view(farScene, new THREE.MeshBasicMaterial(), { fade });
  farScene.add(far);
  camera.far = 3000; camera.updateProjectionMatrix();
  farScene.onBeforeRender(null, farScene, camera, null);
  const ranges = far.children.filter((m) => m.visible).map((m) => m.geometry.drawRange);
  const box = new THREE.Box3(), at = camera.position;
  let skipped = 0;
  for (const cell of patches.full.cells) {
    const inside = ranges.some((r) => cell.start >= r.start && cell.end <= r.start + r.count);
    box.copy(cell.box);
    const farthest = Math.hypot(Math.max(Math.abs(at.x - box.min.x), Math.abs(at.x - box.max.x)), Math.max(Math.abs(at.y - box.min.y), Math.abs(at.y - box.max.y)), Math.max(Math.abs(at.z - box.min.z), Math.abs(at.z - box.max.z)));
    if (farthest < fade.x - 1) { assert.ok(!inside, 'a cell wholly inside the fade start is not drawn'); skipped++; }
  }
  assert.ok(ranges.length > 0 && skipped > 0);
});
