import { ownedWorldProgram } from "./world-programs.js";
// The world pass (world.rs): the map's opaque meshes in the near scene, the
// static world's buckets. Each registers its declared Rust source variant
// before drawing, then Rust culls and draws it before the soldiers. Changed
// material versions register another declared variant without a browser draw.
//
// Buckets with shader hooks draw here too: scrolling ones (their scroll and
// the frame's time), the banners' wind (fabric.js: its time is the frame's,
// read from the frame block), and the leaves the tree canopies take over
// (tree-canopies.js: the canopies' state texture and count).
import * as THREE from "three";

// Texture units (core.rs): the tree canopies' state, the terrain's layers.
export const UNIT_TREE_STATES = 3, UNIT_LAYER1 = 4, UNIT_DETAIL = 5, UNIT_LAYERS = 6;
// The material values a world program may read, which the renderer sets per
// draw (world.rs world_uniform); receiveShadow is the mesh's own (a shadow
// receiver's, with shadows on).
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular", "scroll", "treeStateCount", "receiveShadow"]);
const IDENTITY = new THREE.Matrix4();

class WorldPass {
  constructor(core) {
    this.core = core;
    this.name = "world";
    this.phase = "near";
    this.pending = null;
    this.count = 0;
    this.meshes = []; // { mesh, id, version (its material's, registered), stale }
    this.lamped = []; // the entries whose program reads lamps from the lamp table
    this.lampValues = new Float32Array(5);
    this.stateTextures = []; // { texture, version }: the tree canopies' state

  }

  draw() {
    for (const entry of this.meshes) {
      if (entry.stale || entry.mesh.material.version === entry.version) continue;
      entry.stale = true;
      this.core.rust.world_shown(entry.id, false);
      entry.mesh.material.visible = false;
      this.pending.push(entry.mesh);
    }
    // The canopies' state, when it changed (three uploads it as it binds it;
    // it no longer draws these materials).
    for (const t of this.stateTextures) if (t.texture.version !== t.version) { this.core.renderer.initTexture(t.texture); t.version = t.texture.version; }
    // A DYNAMIC bucket's lamps, as its material's onBeforeRender selects them
    // (its object is the bucket, at the identity).
    for (const entry of this.lamped) {
      if (entry.stale) continue;
      const lamps = this.core.lamps(entry.mesh, false);
      this.lampValues.set(lamps.indices); this.lampValues[4] = lamps.count;
      this.core.rust.world_lamps(entry.id, this.lampValues);
    }
    if (!this.pending || this.pending.length) this.registerWorld();
  }

  // The map's opaque meshes in the near scene that the renderer can draw.
  registerWorld() {
    const core = this.core, scene = core.graphics.scene, rust = core.rust;
    if (!this.pending) {
      this.pending = scene.children.filter((o) => o.isMesh && !o.matrixAutoUpdate && o.matrix.equals(IDENTITY) && !Array.isArray(o.material)
        && o.material.defines && "WORLD_SPACE" in o.material.defines && !("TERRAIN" in o.material.defines) && !o.material.transparent);
    }
    this.pending = this.pending.filter((mesh) => {
      const material = mesh.material, geometry = mesh.geometry;
      // A bucket render-culling.js culls by cell (cellLights) has the draw
      // range of the last view three drew it in; the renderer draws its
      // cells from the whole index.
      const partial = geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0);
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || (partial && !mesh.userData.cellLights)) return false;
      const program = ownedWorldProgram(core, material);
      if (!program) return false;
      const g = core.geometry(geometry, program);
      if (g === null) return false;
      if (!geometry.boundingSphere) geometry.computeBoundingSphere();
      if (!geometry.boundingBox) geometry.computeBoundingBox();
      const s = geometry.boundingSphere, b = geometry.boundingBox;
      const pointLit = Boolean(material.userData.pointLit) && program.names.has("zeroLightCount");
      const flags = material.side | (material.alphaToCoverage ? 4 : 0) | (material.depthWrite ? 8 : 0) | (pointLit ? 16 : 0);
      const w = rust.add_world(g, program.id, material.id, flags, new Float64Array([s.center.x, s.center.y, s.center.z, s.radius, b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]));
      const map = core.texture(material.uniforms.map?.value);
      if (map >= 0) rust.world_texture(w, 0, map);
      if (program.names.has("bumpMap")) { const bump = core.texture(material.uniforms.bumpMap?.value); if (bump >= 0) rust.world_texture(w, 2, bump); }
      if (program.names.has("treeStates")) {
        const states = material.uniforms.treeStates.value, t = core.texture(states);
        if (t < 0) { rust.world_shown(w, false); return false; }
        rust.world_texture(w, UNIT_TREE_STATES, t);
        if (!this.stateTextures.some((e) => e.texture === states)) this.stateTextures.push({ texture: states, version: states.version });
      }
      for (const name of program.names) {
        if (!MATERIAL.has(name)) continue;
        if (name === "receiveShadow") { rust.world_uniform(w, name, new Float32Array([mesh.receiveShadow ? 1 : 0]), true); continue; }
        const v = material.uniforms[name]?.value;
        const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
        if (values) rust.world_uniform(w, name, new Float32Array(values), false);
      }
      if (mesh.userData.cellLights) {
        // The positions alone (the map's vertices are interleaved).
        const position = geometry.attributes.position, positions = new Float32Array(position.count * 3);
        for (let i = 0; i < position.count; i++) { positions[3 * i] = position.getX(i); positions[3 * i + 1] = position.getY(i); positions[3 * i + 2] = position.getZ(i); }
        const index = geometry.index.array;
        rust.world_cells(w, positions, index instanceof Uint32Array ? index : Uint32Array.from(index), 48);
      }
      material.visible = false;
      // hooked: drawn here only with a shader hook's inputs (scroll, the canopies' state, a frame-value alias).
      const hooked = ["scroll", "treeStates"].some((n) => program.names.has(n)) || program.aliased;
      const entry = { mesh, id: w, version: material.version, stale: false, hooked };
      this.meshes.push(entry);
      if (program.lamps) this.lamped.push(entry);
      this.count++;
      return false;
    });
    rust.sort(false);
  }


  // Shown to three while one of three's own renders needs them (index.js
  // reveal): three's shadow pass draws the casters among the buckets taken
  // (alpha-cut buckets too small to partition, the banners, and the
  // partitioned ones' proxies, which share their material).
  reveal(on) {
    for (const entry of this.meshes) if (!entry.stale && entry.mesh.material) entry.mesh.material.visible = on;
  }
}

export function worldPass(core) {
  return new WorldPass(core);
}
