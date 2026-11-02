// The terrain pass: the near scene's ground (terrain-patches.js), its
// painted layers included, drawn by the world pass (world.rs) as one of the
// map's buckets: three's draw order puts it among them (by material), after
// every other, and its cells are the patches' own, each one range of the
// patches' index, culled by the cell's box and drawn in runs as three's
// patches draw them, in one multi-draw. Registered once three has drawn the
// ground as it is now (the layers arrive later than the map: world.js hides
// the ground until they do), then hidden from three, whose patches then
// skip their per-render cell walk (drawnElsewhere).
import { WORLD, UNIT_DETAIL, UNIT_LAYER1, UNIT_LAYERS, drawnAsIs } from "./world.js";

// The material values a terrain program may read, set per draw: the zero
// materials' own (materials.js) and the ground's.
const MATERIAL = new Set(["sunScale", "brightness", "ambientPull", "specular", "layerScales", "bakedLit", "layerScale0", "layerScale1", "farOrigin"]);
const LAYERS = Array.from({ length: 8 }, (_, i) => `layerMap${i}`);
const POLICY = {
  uniforms: new Set(["map", "farMap", "layer1Map", "detailMap", ...LAYERS, ...MATERIAL, "localLightPositions", "localLightColors", "localLightCount"]),
  material: MATERIAL,
};

class TerrainPass {
  constructor(core) {
    this.core = core;
    this.name = "terrain";
    this.phase = "near";
    this.pending = null;
    this.grounds = []; // { group, material, id, version, shown, stale }
    this.active = true;
  }

  draw(ctx) {
    const core = this.core, rust = core.rust;
    if (!this.pending) this.pending = core.graphics.scene.children.filter((o) => o.userData?.patches && o.material?.defines && "TERRAIN" in o.material.defines && "WORLD_SPACE" in o.material.defines);
    for (const e of this.grounds) {
      if (e.stale) continue;
      if (e.material.version !== e.version) {
        // Changed since (another program): three's again until it has drawn it.
        e.stale = true;
        rust.world_shown(e.id, false);
        e.material.visible = true;
        e.group.userData.drawnElsewhere = false;
        this.pending.push(e.group);
        continue;
      }
      let shown = true;
      for (let o = e.group; o && o !== ctx.scene; o = o.parent) if (!o.visible) { shown = false; break; }
      if (shown !== e.shown) { e.shown = shown; rust.world_shown(e.id, shown && this.active); }
    }
    if (this.active && this.pending.length) this.registerGrounds();
  }

  registerGrounds() {
    const core = this.core, renderer = core.renderer, rust = core.rust;
    let added = false;
    this.pending = this.pending.filter((group) => {
      const material = group.material, patches = group.userData.patches, geometry = group.children[0]?.geometry;
      // Hidden until its painted layers arrive (world.js), or not drawn yet.
      if (!group.visible || !geometry || !drawnAsIs(renderer, material)) return true;
      if (geometry.index !== patches.full.index) return false;
      const program = core.program(renderer.properties.get(material).currentProgram, WORLD, material, POLICY);
      if (!program) return false;
      const g = core.geometry(geometry, program);
      if (g === null) return false;
      const cells = patches.full.cells, ranges = new Uint32Array(cells.length * 2), boxes = new Float64Array(cells.length * 6);
      cells.forEach((cell, i) => {
        ranges[2 * i] = cell.start; ranges[2 * i + 1] = cell.end;
        boxes.set([cell.box.min.x, cell.box.min.y, cell.box.min.z, cell.box.max.x, cell.box.max.y, cell.box.max.z], 6 * i);
      });
      const s = patches.bounds, lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
      for (let i = 0; i < cells.length; i++) for (let k = 0; k < 3; k++) { lo[k] = Math.min(lo[k], boxes[6 * i + k]); hi[k] = Math.max(hi[k], boxes[6 * i + 3 + k]); }
      // 32: the terrain (its draws count for the terrain pass).
      const flags = material.side | (material.alphaToCoverage ? 4 : 0) | (material.depthWrite ? 8 : 0) | 32;
      const w = rust.add_world(g, program.id, material.id, flags, new Float64Array([s.center.x, s.center.y, s.center.z, s.radius, ...lo, ...hi]));
      rust.world_runs(w, ranges, boxes);
      const bind = (name, unit) => {
        if (!program.names.has(name)) return true;
        const t = core.texture(material.uniforms[name]?.value);
        if (t < 0) return false;
        rust.world_texture(w, unit, t);
        return true;
      };
      const bound = [bind("map", 0), bind("layer1Map", UNIT_LAYER1), bind("detailMap", UNIT_DETAIL), ...LAYERS.map((name, i) => bind(name, UNIT_LAYERS + i))];
      for (const name of program.names) {
        if (!MATERIAL.has(name)) continue;
        const v = material.uniforms[name]?.value;
        const values = typeof v === "number" ? [v] : v?.isVector4 ? [v.x, v.y, v.z, v.w] : v?.isVector3 ? [v.x, v.y, v.z] : v?.isVector2 ? [v.x, v.y] : null;
        if (values) rust.world_uniform(w, name, new Float32Array(values), false);
      }
      if (bound.some((ok) => !ok)) { rust.world_shown(w, false); core.errors.push(`terrain ${group.name}: textures`); return false; }
      material.visible = false;
      group.userData.drawnElsewhere = true;
      this.grounds.push({ group, material, id: w, version: material.version, shown: true, stale: false });
      added = true;
      return false;
    });
    if (added) rust.sort(core.byName.world.byProgram);
  }

  // Off: three draws the ground again (alone, or with the whole renderer).
  setActive(on) {
    this.active = on;
    for (const e of this.grounds) {
      if (e.stale) continue;
      e.material.visible = !on;
      e.group.userData.drawnElsewhere = on;
      this.core.rust.world_shown(e.id, on && e.shown);
    }
  }

  restore() {
    for (const e of this.grounds) e.group.userData.drawnElsewhere = false;
  }
}

export function terrainPass(core) {
  return new TerrainPass(core);
}
