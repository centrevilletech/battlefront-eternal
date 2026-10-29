import * as THREE from "three";

// The terrain is one 2 km mesh, so frustum culling could never reject any of
// it, and the near and far scenes each drew all of it. Its triangles are
// binned into a grid of cells instead, in one reordered index buffer (each
// cell a contiguous range, cells in row order). Every time a scene is drawn,
// for whichever camera draws it (the eye, a water reflection's mirror), the
// cells its frustum reaches are found and each run of consecutive ones is a
// single draw over the shared vertex buffer.
//
// The near scene's frustum ends at the fade end. The far scene also leaves
// out cells wholly nearer than the fade start: its shader discards every
// fragment there (FAR in materials.js), so the image is unchanged.
const CELL_SIZE = 128; // m: 16 x 16 cells on a 2 km map

export class TerrainPatches {
  constructor(geometry) {
    const position = geometry.getAttribute("position");
    geometry.computeBoundingBox();
    const { min, max } = geometry.boundingBox;
    this.position = position;
    this.min = min.clone();
    this.cellsX = Math.max(1, Math.min(32, Math.round((max.x - min.x) / CELL_SIZE)));
    this.cellsZ = Math.max(1, Math.min(32, Math.round((max.z - min.z) / CELL_SIZE)));
    this.sizeX = (max.x - min.x) / this.cellsX || 1;
    this.sizeZ = (max.z - min.z) / this.cellsZ || 1;
    this.full = this.bin(geometry.index.array);
    this.reduced = null;
    this.attributes = geometry.attributes;
    this.bounds = geometry.boundingBox.getBoundingSphere(new THREE.Sphere());
    this.geometries = [];
    this.frustum = new THREE.Frustum();
    this.viewProjection = new THREE.Matrix4();
    this.eye = new THREE.Vector3();
    this.run = new THREE.Box3();
  }

  // Triangles (an index array) sorted into cells: the index buffer and the
  // cells with triangles, each a range and its bounds. Empty cells have no
  // range, so the runs bridge them.
  bin(source) {
    const { position, min, cellsX, cellsZ, sizeX, sizeZ } = this;
    const triangles = source.length / 3;
    const cellOf = new Uint32Array(triangles);
    const counts = new Uint32Array(cellsX * cellsZ);
    for (let t = 0; t < triangles; t++) {
      let x = 0, z = 0;
      for (let k = 0; k < 3; k++) { const v = source[t * 3 + k]; x += position.getX(v); z += position.getZ(v); }
      const cx = Math.min(cellsX - 1, Math.max(0, Math.floor((x / 3 - min.x) / sizeX)));
      const cz = Math.min(cellsZ - 1, Math.max(0, Math.floor((z / 3 - min.z) / sizeZ)));
      counts[cellOf[t] = cz * cellsX + cx]++;
    }
    const starts = new Uint32Array(counts.length);
    for (let c = 1; c < counts.length; c++) starts[c] = starts[c - 1] + counts[c - 1] * 3;
    const fill = starts.slice();
    const sorted = new Uint32Array(source.length);
    for (let t = 0; t < triangles; t++) {
      const at = fill[cellOf[t]];
      sorted[at] = source[t * 3]; sorted[at + 1] = source[t * 3 + 1]; sorted[at + 2] = source[t * 3 + 2];
      fill[cellOf[t]] += 3;
    }
    const point = new THREE.Vector3();
    const cells = [];
    for (let c = 0; c < counts.length; c++) {
      if (!counts[c]) continue;
      const start = starts[c], end = start + counts[c] * 3;
      const box = new THREE.Box3();
      for (let i = start; i < end; i++) box.expandByPoint(point.fromBufferAttribute(position, sorted[i]));
      cells.push({ start, end, box });
    }
    return { index: new THREE.BufferAttribute(sorted, 1), cells };
  }

  // The far scene's "reduced" terrain: the height grid at twice its spacing,
  // by vertex clustering. Each 2 x 2 block of grid points collapses onto its
  // corner point (an existing vertex, so no new attributes); the triangles
  // that survive form the coarser grid, a quarter as many. The whole far
  // scene uses one level, so there are no cracks between its patches.
  level(reduced) {
    if (!reduced) return this.full;
    if (this.reduced) return this.reduced;
    const { position, min } = this;
    const source = this.full.index.array;
    // The grid spacing: the most common horizontal edge length.
    const lengths = new Map();
    for (let i = 0; i < Math.min(source.length, 30000); i += 3) {
      for (let k = 0; k < 3; k++) {
        const a = source[i + k], b = source[i + (k + 1) % 3];
        const d = Math.round(Math.max(Math.abs(position.getX(a) - position.getX(b)), Math.abs(position.getZ(a) - position.getZ(b))) * 100) / 100;
        if (d > 0) lengths.set(d, (lengths.get(d) || 0) + 1);
      }
    }
    let spacing = 0, most = 0;
    for (const [d, n] of lengths) if (n > most) { most = n; spacing = d; }
    if (!(spacing > 0)) return (this.reduced = this.full);
    const step = spacing * 2;
    const count = position.count;
    const keyOf = (v) => {
      const cx = Math.floor((position.getX(v) - min.x) / step + 0.25);
      const cz = Math.floor((position.getZ(v) - min.z) / step + 0.25);
      return cz * 65536 + cx;
    };
    const best = new Map();
    const keys = new Float64Array(count);
    for (let v = 0; v < count; v++) {
      const key = keys[v] = keyOf(v);
      const cx = key % 65536, cz = Math.floor(key / 65536);
      const dx = position.getX(v) - (min.x + cx * step), dz = position.getZ(v) - (min.z + cz * step);
      const d = dx * dx + dz * dz;
      const held = best.get(key);
      if (!held || d < held.d) best.set(key, { v, d });
    }
    const rep = new Uint32Array(count);
    for (let v = 0; v < count; v++) rep[v] = best.get(keys[v]).v;
    const area = (a, b, c) => (position.getX(b) - position.getX(a)) * (position.getZ(c) - position.getZ(a))
      - (position.getZ(b) - position.getZ(a)) * (position.getX(c) - position.getX(a));
    const kept = [], seen = new Set();
    for (let i = 0; i < source.length; i += 3) {
      const a = rep[source[i]], b = rep[source[i + 1]], c = rep[source[i + 2]];
      if (a === b || b === c || a === c) continue;
      // Keep the triangle's facing; drop any that folded over.
      const before = area(source[i], source[i + 1], source[i + 2]), after = area(a, b, c);
      if (before * after <= 0) continue;
      const [x, y, z] = [a, b, c].sort((p, q) => p - q);
      const key = (x * count + y) * count + z;
      if (seen.has(key)) continue;
      seen.add(key);
      kept.push(a, b, c);
    }
    this.reduced = this.bin(Uint32Array.from(kept));
    return this.reduced;
  }

  // A geometry over the shared buffers; its draw range is set per draw.
  geometry() {
    const geometry = new THREE.BufferGeometry();
    for (const [name, attribute] of Object.entries(this.attributes)) geometry.setAttribute(name, attribute);
    geometry.setIndex(this.full.index);
    geometry.boundingSphere = this.bounds.clone();
    this.geometries.push(geometry);
    return geometry;
  }

  // An attribute added later (the painted layer weights) reaches every draw.
  setAttribute(name, attribute) {
    this.attributes[name] = attribute;
    for (const geometry of this.geometries) geometry.setAttribute(name, attribute);
  }

  // The terrain as drawn in `scene` with `material`. `fade` (the shared fade
  // uniform's value, read per draw) marks the far scene; `reduced()`, asked
  // per draw, whether it uses the reduced level.
  view(scene, material, { fade = null, receiveShadow = false, reduced = () => false } = {}) {
    const group = new THREE.Group();
    group.matrixAutoUpdate = false;
    const add = () => {
      const mesh = new THREE.Mesh(this.geometry(), material);
      mesh.matrixAutoUpdate = false;
      mesh.frustumCulled = false;
      mesh.receiveShadow = receiveShadow;
      group.add(mesh);
      return mesh;
    };
    // Until the first draw (and for renderer.compile): all of it, once.
    add();
    const previous = scene.onBeforeRender;
    scene.onBeforeRender = (...args) => {
      previous?.apply(scene, args);
      if (group.visible && group.parent) this.update(group, args[2], fade, add, this.level(reduced()));
    };
    return group;
  }

  update(group, camera, fade, add, { index, cells }) {
    this.frustum.setFromProjectionMatrix(this.viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    const eye = this.eye.setFromMatrixPosition(camera.matrixWorld);
    const start = fade ? Math.max(0, fade.x - 1) : 0; // 1 m margin
    const limit = start * start;
    const run = this.run;
    let used = 0, from = -1, to = 0;
    const emit = () => {
      const mesh = group.children[used] || add();
      used++;
      if (mesh.geometry.index !== index) mesh.geometry.setIndex(index);
      mesh.geometry.setDrawRange(from, to - from);
      run.getBoundingSphere(mesh.geometry.boundingSphere);
      mesh.visible = true;
      from = -1;
    };
    for (const cell of cells) {
      const { min, max } = cell.box;
      let shown = this.frustum.intersectsBox(cell.box);
      if (shown && limit > 0) {
        const dx = Math.max(Math.abs(eye.x - min.x), Math.abs(eye.x - max.x));
        const dy = Math.max(Math.abs(eye.y - min.y), Math.abs(eye.y - max.y));
        const dz = Math.max(Math.abs(eye.z - min.z), Math.abs(eye.z - max.z));
        shown = dx * dx + dy * dy + dz * dz >= limit;
      }
      if (!shown) { if (from >= 0) emit(); continue; }
      if (from < 0) { from = cell.start; run.copy(cell.box); } else run.union(cell.box);
      to = cell.end;
    }
    if (from >= 0) emit();
    for (let i = used; i < group.children.length; i++) group.children[i].visible = false;
  }
}
