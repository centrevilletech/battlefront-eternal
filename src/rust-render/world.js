// The world pass (world.rs): the map's opaque meshes in the near scene, the
// static world's buckets. Each is registered the first time three has drawn
// it (which gives its material a program), then hidden from three; the
// renderer culls and draws them in the near phase, before the soldiers.
import * as THREE from "three";

// add_program's kind for the map's meshes (web/mod.rs).
const WORLD = 0;
// The material values a world program may read, which the renderer sets per
// draw (world.rs world_uniform).
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular"]);
// Uniforms a world program may read besides the frame block: its textures,
// those material values, the point lights the renderer gives each draw, and
// lamp lighting's, read only while lamps are off (supported()).
const POLICY = {
  uniforms: new Set(["map", "farMap", "bumpMap", ...MATERIAL, "zeroLights", "zeroLightCount", "localLightPositions", "localLightColors", "localLightCount"]),
  material: MATERIAL,
};
const IDENTITY = new THREE.Matrix4();

class WorldPass {
  constructor(core) {
    this.core = core;
    this.name = "world";
    this.phase = "near";
    this.pending = null;
    this.count = 0;
    this.meshes = [];
    // three's own opaque order (by material) unless ?rustSort: sorting by
    // program saves a few switches but moves which of two surfaces at the
    // same depth wins at a few multisampled edges.
    this.byProgram = new URLSearchParams(location.search).has("rustSort");
  }

  draw() {
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
      const threeProgram = renderer.properties.get(material).currentProgram;
      if (!threeProgram) return true;
      if (geometry.groups.length || Object.keys(geometry.morphAttributes).length || geometry.drawRange.start !== 0 || geometry.drawRange.count < (geometry.index?.count ?? 0)) return false;
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
      this.meshes.push(mesh);
      this.count++;
      return false;
    });
    rust.sort(this.byProgram);
  }

  setActive(on) {
    for (const mesh of this.meshes) if (mesh.material) mesh.material.visible = !on;
  }
}

export function worldPass(core) {
  return new WorldPass(core);
}
