// The collision world's queries in WebAssembly (rust/rays), for
// CollisionWorld's cast, castMoving, ground, gather, overlapsSphere and
// terrainNear when a battle asks for them (?rays=wasm). The JavaScript in
// src/collision.js stays the default; the module answers every query as it
// does, to the last bit (rust/rays says how).
//
// The module holds copies of what the queries read: each tree's packed nodes
// from three-mesh-bvh, its triangles' corners, index and indirect buffers
// and polygon numbers, the terrain's grid, the fixed sets gathers file
// (each triangle's bounds), and each body's bounds and matrices. Trees never
// change once built, so each is copied once; a body's place is copied as it
// moves; and before each query, whether the bodies it can meet are enabled.
import { RAY_KERNEL_WASM } from "./ray-kernel-wasm.js";

let compiling = null;

// A kernel of its own for a battle: the module is compiled once per thread,
// and each battle gets its own instance, whose copies go with it when the
// battle ends (a dedicated server loads one battle after another).
export async function loadRayKernel() {
  compiling ||= (async () => {
    const text = atob(RAY_KERNEL_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return WebAssembly.compile(bytes);
  })();
  return new RayKernel((await WebAssembly.instantiate(await compiling, {})).exports);
}

export class RayKernel {
  constructor(exports) {
    this.x = exports;
    this.memory = exports.memory;
    // Copies made, by the array, tree or set they copy.
    this.positionIds = new WeakMap();
    this.treeIds = new WeakMap();
    this.setIds = new WeakMap();
    // Trees whose polygon numbers are copied.
    this.treePolygons = new Set();
    // Every body by its slot, and every fixed set by its id.
    this.bodies = [];
    this.sets = [];
    // Views of a gather's output and terrainNear's (outF64, outU32).
    this.f64Out = null;
    this.u32Out = null;
    this.refresh();
  }

  // Views of the module's memory; growing it (as copies are made) detaches
  // the old ones.
  refresh() {
    this.io = new Float64Array(this.memory.buffer, this.x.io_ptr(), 64);
    this.enabled = new Uint8Array(this.memory.buffer, this.x.enabled_ptr(), 1 << 16);
    this.candidateList = new Uint32Array(this.memory.buffer, this.x.candidates_ptr(), 1 << 16);
  }

  // A view of memory at `ptr`, made after the call that returned `ptr` (which
  // may have grown it).
  u32(ptr, length) { return new Uint32Array(this.memory.buffer, ptr, length); }
  f64(ptr, length) { return new Float64Array(this.memory.buffer, ptr, length); }

  // The exchange slots (rust/rays IO).
  view() {
    if (this.io.byteLength === 0) this.refresh();
    return this.io;
  }

  // Corners from a Float32Array stay f32 there (each read widened, as
  // JavaScript widens it); any other array's are f64.
  positions(array) {
    let id = this.positionIds.get(array);
    if (id === undefined) {
      const wide = array instanceof Float32Array ? 0 : 1;
      id = this.x.positions_new(array.length, wide);
      const at = this.x.positions_at(id);
      (wide ? this.f64(at, array.length) : new Float32Array(this.memory.buffer, at, array.length)).set(array);
      this.positionIds.set(array, id);
    }
    return id;
  }

  // A MeshBVH (indirect, as collision.js builds them) as the module's tree.
  tree(bvh) {
    let id = this.treeIds.get(bvh);
    if (id !== undefined) return id;
    const geometry = bvh.geometry, position = geometry.attributes.position, index = geometry.index?.array || null;
    const indirect = bvh._indirectBuffer;
    if (!indirect || position.itemSize !== 3 || position.normalized || position.isInterleavedBufferAttribute) throw Error("Unsupported tree for the ray kernel");
    const positions = this.positions(position.array);
    id = this.x.tree_new(positions, index ? index.length : 0, indirect.length);
    if (index) this.u32(this.x.tree_index(id), index.length).set(index);
    this.u32(this.x.tree_indirect(id), indirect.length).set(indirect);
    for (const root of bvh._roots) {
      const words = new Uint32Array(root);
      this.u32(this.x.tree_root(id, words.length), words.length).set(words);
    }
    this.treeIds.set(bvh, id);
    return id;
  }

  // collision.js's TriangleGrid.
  grid(grid) {
    const positions = this.positions(grid.positions), io = this.view();
    io[0] = grid.minX; io[1] = grid.minZ; io[2] = grid.maxX; io[3] = grid.maxZ; io[4] = grid.size; io[5] = grid.nx; io[6] = grid.nz;
    const id = this.x.grid_new(positions, grid.indices.length, grid.cells.length, grid.start.length, grid.indices.length / 3);
    this.u32(this.x.grid_index(id), grid.indices.length).set(grid.indices);
    this.u32(this.x.grid_cells(id), grid.cells.length).set(grid.cells);
    this.u32(this.x.grid_start(id), grid.start.length).set(grid.start);
    this.refresh();
    return id;
  }

  world(tree, terrain, grid) {
    return this.x.world_new(tree, terrain, grid);
  }

  // A body's slot: its tree (with its polygon numbers), whether it moves,
  // and the offset of its polygon numbers.
  body(body, moving) {
    const tree = this.tree(body.bvh);
    if (!this.treePolygons.has(tree)) {
      // Every triangle's polygon number is a number in JavaScript too.
      if (!(body.polygonOf.length >= body.bvh.geometry.index.count / 3)) throw Error("A body's triangle has no polygon number");
      this.u32(this.x.tree_polygons(tree, body.polygonOf.length), body.polygonOf.length).set(body.polygonOf);
      this.treePolygons.add(tree);
    }
    const slot = this.x.body_new(tree, moving ? 1 : 0);
    if (slot < 0) throw Error("Too many bodies for the ray kernel");
    this.x.body_offset(slot, body.offset);
    this.bodies[slot] = body;
    this.refresh();
    return slot;
  }

  // A fixed set of collision.js (the buildings, the vehicle-only barriers):
  // its tree, each triangle's bounds and polygon number, and their offset.
  fixedSet(set, bvh) {
    let id = this.setIds.get(set);
    if (id !== undefined) return id;
    const count = set.triBounds.length / 6;
    if (!(set.polygonOf.length >= count)) throw Error("A fixed triangle has no polygon number");
    id = this.x.set_new(this.tree(bvh), count, set.offset);
    new Float32Array(this.memory.buffer, this.x.set_bounds(id), count * 6).set(set.triBounds);
    this.u32(this.x.set_polygons(id), count).set(set.polygonOf.subarray(0, count));
    this.setIds.set(set, id);
    this.sets[id] = set;
    this.refresh();
    return id;
  }

  // A gather cache's id in the module (made the first time).
  cache(cache) {
    if (cache.ray < 0) {
      cache.ray = this.x.cache_new(cache.margin);
      this.refresh();
    }
    return cache.ray;
  }

  // The module's output at `ptr` (a gather's, terrainNear's), as f64s or
  // u32s: one view to the end of memory, re-made when the output moves or
  // memory grows.
  outF64(ptr) {
    const view = this.f64Out;
    if (view && view.byteOffset === ptr && view.byteLength) return view;
    return this.f64Out = new Float64Array(this.memory.buffer, ptr, (this.memory.buffer.byteLength - ptr) >> 3);
  }

  outU32(ptr) {
    const view = this.u32Out;
    if (view && view.byteOffset === ptr && view.byteLength) return view;
    return this.u32Out = new Uint32Array(this.memory.buffer, ptr, (this.memory.buffer.byteLength - ptr) >> 2);
  }

  // A body's bounds (Box3) and, if it moves, its matrix and inverse.
  place(slot, bounds, matrix, inverse) {
    const io = this.view();
    io[20] = bounds.min.x; io[21] = bounds.min.y; io[22] = bounds.min.z;
    io[23] = bounds.max.x; io[24] = bounds.max.y; io[25] = bounds.max.z;
    if (matrix) { io.set(matrix.elements, 26); io.set(inverse.elements, 42); }
    this.x.body_set(slot);
  }

  // The candidates of the cast about to run (rust/rays CANDIDATES: places
  // in the world's order), and whether each is enabled (ENABLED).
  candidates() {
    if (this.candidateList.byteLength === 0) this.refresh();
    return this.candidateList;
  }

  flags() {
    if (this.enabled.byteLength === 0) this.refresh();
    return this.enabled;
  }

  // A world's bodies, in its list's order.
  order(world, bodies) {
    const out = this.u32(this.x.world_order(world, bodies.length), bodies.length);
    for (let i = 0; i < bodies.length; i++) out[i] = bodies[i].raySlot;
    this.refresh();
  }
}
