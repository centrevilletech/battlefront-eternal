import { cullStaticColor, mortonOrder } from "./render-culling.js";
import * as THREE from "three";

// Keep one draw per material/cascade, but submit only the spatial cells near
// that cascade. Imported material buckets otherwise span the entire map. The
// shadow proxy's index buffer holds the cells one after another in Morton
// order, built once; each cascade draws the range from its first visible cell
// to its last, so nothing is copied or uploaded per frame. Shared vertex
// buffers and color/collision geometry are untouched.
export function partitionShadowCaster(mesh, register, layer, cellSize = 48) {
  const geometry = mesh.geometry, index = geometry.index;
  if (!index || index.count < 1536) { register(mesh); return [mesh]; }
  geometry.computeBoundingBox();
  const bounds = geometry.boundingBox;
  if (bounds.max.x - bounds.min.x < cellSize && bounds.max.z - bounds.min.z < cellSize) {
    register(mesh); return [mesh];
  }
  const position = geometry.attributes.position, cells = new Map();
  const triangleCells = new Uint32Array(index.count / 3);
  const point = new THREE.Vector3();
  for (let i = 0; i < index.count; i += 3) {
    const a = index.getX(i), b = index.getX(i + 1), c = index.getX(i + 2);
    const x = (position.getX(a) + position.getX(b) + position.getX(c)) / 3;
    const z = (position.getZ(a) + position.getZ(b) + position.getZ(c)) / 3;
    const cx = Math.floor(x / cellSize), cz = Math.floor(z / cellSize), key = `${cx},${cz}`;
    if (!cells.has(key)) cells.set(key, { id: cells.size, x: cx, z: cz, indices: [], bounds: new THREE.Box3() });
    const cell = cells.get(key);
    triangleCells[i / 3] = cell.id;
    for (const vertex of [a, b, c]) {
      cell.indices.push(vertex);
      cell.bounds.expandByPoint(point.fromBufferAttribute(position, vertex));
    }
  }
  if (cells.size === 1) { register(mesh); return [mesh]; }
  // Compact the import-time JS arrays too; large maps can have millions of
  // indices, and keeping boxed-number arrays would waste substantial RAM.
  const parts = [...cells.values()].map((part) => ({ ...part, indices: new index.array.constructor(part.indices) }));
  const shadowGeometry = new THREE.BufferGeometry();
  shadowGeometry.attributes = { ...geometry.attributes };
  shadowGeometry.boundingBox = bounds.clone();
  if (!geometry.boundingSphere) geometry.computeBoundingSphere();
  shadowGeometry.boundingSphere = geometry.boundingSphere.clone();
  const proxy = new THREE.Mesh(shadowGeometry, mesh.material);
  proxy.name = `${mesh.name}:shadow`;
  proxy.layers.set(layer);
  proxy.matrixAutoUpdate = false; // Identity child of the unchanged source mesh.
  const frustum = new THREE.Frustum(), matrix = new THREE.Matrix4();
  // The cells' triangles back to back, in Morton order; each cell's range.
  const order = mortonOrder(parts), sorted = new index.array.constructor(index.count);
  const starts = new Uint32Array(parts.length), ends = new Uint32Array(parts.length);
  let cursor = 0;
  for (const i of order) {
    starts[i] = cursor; sorted.set(parts[i].indices, cursor); cursor += parts[i].indices.length; ends[i] = cursor;
  }
  shadowGeometry.setIndex(new THREE.BufferAttribute(sorted, 1));
  const slots = Array.from({ length: 4 }, () => ({ camera: null, matrix: new THREE.Matrix4(), valid: false, start: 0, count: 0 }));
  let nextSlot = 0;
  proxy.onBeforeShadow = (renderer, object, camera, shadowCamera) => {
    matrix.multiplyMatrices(shadowCamera.projectionMatrix, shadowCamera.matrixWorldInverse).multiply(proxy.matrixWorld);
    let cached = null;
    for (const slot of slots) if (slot.camera === shadowCamera) cached = slot;
    if (!cached) {
      cached = slots[nextSlot++ % slots.length];
      cached.camera = shadowCamera; cached.valid = false;
    }
    if (!cached.valid || !cached.matrix.equals(matrix)) {
      frustum.setFromProjectionMatrix(matrix);
      let start = Infinity, end = 0;
      for (let i = 0; i < parts.length; i++) {
        if (!frustum.intersectsBox(parts[i].bounds)) continue;
        if (starts[i] < start) start = starts[i];
        if (ends[i] > end) end = ends[i];
      }
      cached.start = start === Infinity ? 0 : start;
      cached.count = start === Infinity ? 0 : end - start;
      cached.matrix.copy(matrix); cached.valid = true;
    }
    shadowGeometry.setDrawRange(cached.start, cached.count);
  };
  register(proxy); mesh.add(proxy); mesh.castShadow = false;
  cullStaticColor(mesh, parts, triangleCells);
  return [proxy];
}

// Every opaque static caster's triangles in one depth-only mesh per face
// side the shadow pass draws: hundreds of material buckets and small meshes
// were each a draw in each of four cascades, though a depth map needs no
// materials. Triangles go cell by cell (cellSize metres on x/z) in Morton
// order, and each cascade draws the one range spanning the cells it sees, as
// the partitioned proxies do. Alpha-cut and deforming casters keep their own.
export function mergeStaticShadows(meshes, layer, cellSize = 48) {
  const sides = new Map();
  for (const mesh of meshes) {
    const material = Array.isArray(mesh.material) ? mesh.material[0] : mesh.material;
    const side = material.shadowSide ?? { [THREE.FrontSide]: THREE.BackSide, [THREE.BackSide]: THREE.FrontSide, [THREE.DoubleSide]: THREE.DoubleSide }[material.side];
    if (!sides.has(side)) sides.set(side, []);
    sides.get(side).push(mesh);
  }
  const merged = [];
  for (const [side, list] of sides) {
    let vertices = 0, triangles = 0;
    for (const mesh of list) { vertices += mesh.geometry.attributes.position.count; triangles += mesh.geometry.index.count / 3; }
    const positions = new Float32Array(vertices * 3);
    const cellOf = new Uint32Array(triangles), triangleIndices = new Uint32Array(triangles * 3);
    const cells = new Map(), point = new THREE.Vector3();
    let base = 0, t = 0;
    for (const mesh of list) {
      mesh.updateMatrixWorld(true);
      const position = mesh.geometry.attributes.position, index = mesh.geometry.index;
      for (let v = 0; v < position.count; v++) {
        point.fromBufferAttribute(position, v).applyMatrix4(mesh.matrixWorld);
        positions[(base + v) * 3] = point.x; positions[(base + v) * 3 + 1] = point.y; positions[(base + v) * 3 + 2] = point.z;
      }
      for (let i = 0; i < index.count; i += 3, t++) {
        const a = base + index.getX(i), b = base + index.getX(i + 1), c = base + index.getX(i + 2);
        triangleIndices[t * 3] = a; triangleIndices[t * 3 + 1] = b; triangleIndices[t * 3 + 2] = c;
        const x = (positions[a * 3] + positions[b * 3] + positions[c * 3]) / 3, z = (positions[a * 3 + 2] + positions[b * 3 + 2] + positions[c * 3 + 2]) / 3;
        const cx = Math.floor(x / cellSize), cz = Math.floor(z / cellSize), key = (cx + 32768) * 65536 + (cz + 32768);
        let cell = cells.get(key);
        if (!cell) cells.set(key, (cell = { id: cells.size, x: cx, z: cz, count: 0, bounds: new THREE.Box3() }));
        cell.count++;
        cellOf[t] = cell.id;
        for (const vertex of [a, b, c]) cell.bounds.expandByPoint(point.fromArray(positions, vertex * 3));
      }
      base += position.count;
    }
    const parts = [...cells.values()].sort((p, q) => p.id - q.id);
    const order = mortonOrder(parts), starts = new Uint32Array(parts.length), ends = new Uint32Array(parts.length);
    let cursor = 0;
    for (const i of order) { starts[i] = cursor; cursor += parts[i].count * 3; ends[i] = cursor; }
    const sorted = new Uint32Array(triangles * 3), fill = starts.slice();
    for (let i = 0; i < triangles; i++) {
      const at = fill[cellOf[i]];
      sorted[at] = triangleIndices[i * 3]; sorted[at + 1] = triangleIndices[i * 3 + 1]; sorted[at + 2] = triangleIndices[i * 3 + 2];
      fill[cellOf[i]] = at + 3;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(sorted, 1));
    geometry.computeBoundingSphere();
    const material = new THREE.MeshBasicMaterial({ side: side === THREE.DoubleSide ? THREE.DoubleSide : THREE.FrontSide });
    material.shadowSide = side;
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = `static-shadows:${side}`;
    mesh.layers.set(layer);
    mesh.matrixAutoUpdate = false;
    mesh.receiveShadow = false;
    const frustum = new THREE.Frustum(), matrix = new THREE.Matrix4();
    const slots = Array.from({ length: 4 }, () => ({ camera: null, matrix: new THREE.Matrix4(), valid: false, start: 0, count: 0 }));
    let next = 0;
    mesh.onBeforeShadow = (renderer, object, camera, shadowCamera) => {
      matrix.multiplyMatrices(shadowCamera.projectionMatrix, shadowCamera.matrixWorldInverse);
      let cached = null;
      for (const slot of slots) if (slot.camera === shadowCamera) cached = slot;
      if (!cached) { cached = slots[next++ % slots.length]; cached.camera = shadowCamera; cached.valid = false; }
      if (!cached.valid || !cached.matrix.equals(matrix)) {
        frustum.setFromProjectionMatrix(matrix);
        let start = Infinity, end = 0;
        for (let i = 0; i < parts.length; i++) {
          if (!frustum.intersectsBox(parts[i].bounds)) continue;
          if (starts[i] < start) start = starts[i];
          if (ends[i] > end) end = ends[i];
        }
        cached.start = start === Infinity ? 0 : start; cached.count = start === Infinity ? 0 : end - start;
        cached.matrix.copy(matrix); cached.valid = true;
      }
      geometry.setDrawRange(cached.start, cached.count);
    };
    merged.push(mesh);
  }
  return merged;
}

// The per-cell color culling for a large static bucket, without a shadow
// proxy (its shadow is in mergeStaticShadows).
export function cullColorOnly(mesh, cellSize = 48) {
  const geometry = mesh.geometry, index = geometry.index;
  if (!index || index.count < 1536) return;
  geometry.computeBoundingBox();
  const bounds = geometry.boundingBox;
  if (bounds.max.x - bounds.min.x < cellSize && bounds.max.z - bounds.min.z < cellSize) return;
  const position = geometry.attributes.position, cells = new Map(), point = new THREE.Vector3();
  const triangleCells = new Uint32Array(index.count / 3);
  for (let i = 0; i < index.count; i += 3) {
    const a = index.getX(i), b = index.getX(i + 1), c = index.getX(i + 2);
    const x = (position.getX(a) + position.getX(b) + position.getX(c)) / 3, z = (position.getZ(a) + position.getZ(b) + position.getZ(c)) / 3;
    const key = `${Math.floor(x / cellSize)},${Math.floor(z / cellSize)}`;
    if (!cells.has(key)) cells.set(key, { id: cells.size, bounds: new THREE.Box3() });
    const cell = cells.get(key);
    triangleCells[i / 3] = cell.id;
    for (const vertex of [a, b, c]) cell.bounds.expandByPoint(point.fromBufferAttribute(position, vertex));
  }
  if (cells.size > 1) cullStaticColor(mesh, [...cells.values()], triangleCells);
}
