// The world pass (world.rs): the map's opaque meshes in the near scene, the
// static world's buckets. Each is registered once three has drawn it with
// its material as it is now (which gives the material its program), then
// hidden from three; the renderer culls and draws them in the near phase,
// before the soldiers. A material changed since (needsUpdate: another
// program) goes back to three until three has drawn it again.
//
// Buckets with shader hooks draw here too: scrolling ones (their scroll and
// the frame's time), the banners' wind (fabric.js: its time is the frame's,
// read from the frame block), and the leaves the tree canopies take over
// (tree-canopies.js: the canopies' state texture and count).
import * as THREE from "three";

// add_program's kind for the map's meshes (web/mod.rs).
export const WORLD = 0;
// Texture units (core.rs): the tree canopies' state, the terrain's layers.
export const UNIT_TREE_STATES = 3, UNIT_LAYER1 = 4, UNIT_DETAIL = 5, UNIT_LAYERS = 6;
// The material values a world program may read, which the renderer sets per
// draw (world.rs world_uniform).
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular", "scroll", "treeStateCount"]);
// Uniforms a world program may read besides the frame block: its textures,
// those material values, the point lights the renderer gives each draw, and
// lamp lighting's, read only while lamps are off (supported()).
const POLICY = {
  uniforms: new Set(["map", "farMap", "bumpMap", "treeStates", ...MATERIAL, "zeroLights", "zeroLightCount", "localLightPositions", "localLightColors", "localLightCount"]),
  material: MATERIAL,
};

// Whether three has drawn the material as it is now: its program is current.
export function drawnAsIs(renderer, material) {
  const props = renderer.properties.get(material);
  return Boolean(props.currentProgram) && props.__version === material.version;
}
const IDENTITY = new THREE.Matrix4();

class WorldPass {
  constructor(core) {
    this.core = core;
    this.name = "world";
    this.phase = "near";
    this.pending = null;
    this.count = 0;
    this.meshes = []; // { mesh, id, version (its material's, registered), stale }
    this.stateTextures = []; // { texture, version }: the tree canopies' state
    // three's own opaque order (by material) unless ?rustSort: sorting by
    // program saves a few switches but moves which of two surfaces at the
    // same depth wins at a few multisampled edges.
    this.byProgram = new URLSearchParams(location.search).has("rustSort");
  }

  draw() {
    // A material changed since it was registered goes back to three, to be
    // registered again once three has drawn it.
    for (const entry of this.meshes) {
      if (entry.stale || entry.mesh.material.version === entry.version) continue;
      entry.stale = true;
      this.core.rust.world_shown(entry.id, false);
      entry.mesh.material.visible = true;
      this.pending.push(entry.mesh);
    }
    // The canopies' state, when it changed (three uploads it as it binds it;
    // it no longer draws these materials).
    for (const t of this.stateTextures) if (t.texture.version !== t.version) { this.core.renderer.initTexture(t.texture); t.version = t.texture.version; }
    if (!this.pending || this.pending.length) this.registerWorld();
  }

  // The map's opaque meshes in the near scene that the renderer can draw.
  registerWorld() {
    const core = this.core, scene = core.graphics.scene, renderer = core.renderer, rust = core.rust;
    if (!this.pending) {
      this.pending = scene.children.filter((o) => o.isMesh && !o.matrixAutoUpdate && o.matrix.equals(IDENTITY) && !Array.isArray(o.material)
        && o.material.defines && "WORLD_SPACE" in o.material.defines && !("TERRAIN" in o.material.defines) && !o.material.transparent);
    }
    this.pending = this.pending.filter((mesh) => {
      const material = mesh.material, geometry = mesh.geometry;
      if (!drawnAsIs(renderer, material)) return true;
      const threeProgram = renderer.properties.get(material).currentProgram;
      // A bucket render-culling.js culls by cell (cellLights) has the draw
      // range of the last view three drew it in; the renderer draws its
      // cells from the whole index.
      const partial = geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0);
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || (partial && !mesh.userData.cellLights)) return false;
      const program = core.program(threeProgram, WORLD, material, POLICY);
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
      this.meshes.push({ mesh, id: w, version: material.version, stale: false, hooked });
      this.count++;
      return false;
    });
    rust.sort(this.byProgram);
  }

  setActive(on) {
    for (const entry of this.meshes) if (!entry.stale && entry.mesh.material) entry.mesh.material.visible = !on;
  }
}

export function worldPass(core) {
  return new WorldPass(core);
}
