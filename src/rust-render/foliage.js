import { ownedGeometryProgram } from "./programs.js";
// The foliage pass (foliage.rs) selects placed grass and leaf cards and draws
// them with declared Rust-owned variants. The host supplies immutable geometry,
// placement rows and the camera inputs used by Foliage.update.
// A card vertex's attributes, then a patch row's (foliage.rs VERTEX, ROW).
const VERTEX = [["position", 3], ["color", 4], ["uv", 2], ["foliageCorner", 2], ["foliageShape", 4], ["foliageRate", 1]];
const ROW = ["foliagePatch", "foliageTint", "foliageAxisX", "foliageAxisY", "foliageAxisZ", "foliageDistance"];
const VERTEX_FLOATS = 16, ROW_FLOATS = 16, BOUNDS = 9;
// The selection's inputs (foliage.rs IN_*).
const IN = { PROJECTION: 0, VIEW: 16, WORLD: 32, EYE: 48, END: 51, TIME: 52 };

class FoliagePass {
  constructor(core) {
    this.core = core;
    this.name = "foliage";
    this.phase = "near";
    this.foliage = null;
    // Foliage kind -> its required Rust class id.
    this.classes = new Map();
    this.selected = 0;
    this.input = false;
  }

  // The near phase: the map's foliage, once it has one, takes this pass as
  // its drawer (the classes it registered are drawn in the renderer's call).
  // The page publishes placement data to the required Rust selector/drawer.
  // Only changed class/resource state is registered; view selection belongs to
  // the Rust frame and uses the same data for ordinary and reflected cameras.
  // An unsupported class is a registration failure, not a CPU draw request.
  draw(ctx) {
    const foliage = this.core.graphics.scene.userData.foliage;
    if (!foliage) return;
    if (foliage.drawer === this && this.foliage === foliage && this.classes.size === foliage.kinds.size) return;
    if (foliage !== this.foliage) { this.foliage = foliage; this.classes = new Map(); }
    foliage.drawer = this;
    this.selectClasses(foliage, ctx.camera);
  }

  // Foliage.update's selection registers each class before selecting patches.
  select(foliage, camera) {
    try { this.selectClasses(foliage, camera); } catch (error) { this.core.fail(error); }
  }

  selectClasses(foliage, camera) {
    if (this.core.contextLost) return;
    if (foliage !== this.foliage) { this.foliage = foliage; this.classes.clear(); }
    this.camera = camera;
    for (const kind of foliage.kinds.values()) if (!this.classes.has(kind)) this.registerClass(kind);
    if (!this.classes.size) return;
    const core = this.core;
    if (!this.input) { core.addView("foliageInput", Float64Array, () => core.rust.foliage_input_ptr(), () => core.rust.foliage_input_len()); this.input = true; }
    core.views();
    const input = core.foliageInput;
    input.set(camera.projectionMatrix.elements, IN.PROJECTION);
    input.set(camera.matrixWorldInverse.elements, IN.VIEW);
    input.set(camera.matrixWorld.elements, IN.WORLD);
    input[IN.EYE] = camera.position.x; input[IN.EYE + 1] = camera.position.y; input[IN.EYE + 2] = camera.position.z;
    input[IN.END] = foliage.nearEnd();
    input[IN.TIME] = foliage.time;
    this.selected = core.rust.foliage_frame();
  }

  // Keep seeded base cards separate from placement rows. The latter are
  // selected per view; reconstructing the former every frame changes both
  // allocation cost and the authored random sequence.
  registerClass(kind) {
    const core = this.core, batch = kind.batch, material = batch?.material;
    const geometry = batch?.geometry;
    if (!geometry?.isInstancedBufferGeometry || !geometry.index) throw Error("Invalid foliage geometry");
    const key = "world/foliage-" + (kind.grass ? "grass" : "leaves") + (core.graphics.csm ? "-shadows" : "");
    const program = ownedGeometryProgram(core, key);
    const texture = core.texture(material.map);
    if (!program || texture < 0) core.fail(new Error("Invalid foliage program or texture"));
    const at = new Map(program.attributes);
    const locations = Int32Array.from([...VERTEX.map(([name]) => at.get(name) ?? -1), ...ROW.map((name) => at.get(name) ?? -1)]);
    // The cards' vertices, interleaved.
    const count = geometry.attributes.position.count, vertices = new Float32Array(count * VERTEX_FLOATS);
    let offset = 0;
    for (const [name, size] of VERTEX) {
      const array = geometry.attributes[name].array;
      for (let v = 0; v < count; v++) for (let k = 0; k < size; k++) vertices[v * VERTEX_FLOATS + offset + k] = array[v * size + k];
      offset += size;
    }
    const index = geometry.index.array instanceof Uint32Array ? geometry.index.array : Uint32Array.from(geometry.index.array);
    // Immutable placement rows and bounds, selected and uploaded by Rust.
    const n = kind.instances.length, rows = new Float32Array(n * ROW_FLOATS), bounds = new Float64Array(n * BOUNDS);
    kind.instances.forEach((inst, i) => {
      const r = i * ROW_FLOATS, b = i * BOUNDS, m = inst.matrix.elements;
      rows[r] = inst.position.x; rows[r + 1] = inst.position.y; rows[r + 2] = inst.position.z;
      rows.set(inst.tint, r + 3);
      for (let axis = 0; axis < 3; axis++) for (let k = 0; k < 3; k++) rows[r + 6 + axis * 3 + k] = m[axis * 4 + k];
      rows[r + 15] = inst.maxDistance || 0;
      bounds[b] = inst.position.x; bounds[b + 1] = inst.position.y; bounds[b + 2] = inst.position.z;
      if (inst.sphere) { bounds[b + 3] = inst.sphere.center.x; bounds[b + 4] = inst.sphere.center.y; bounds[b + 5] = inst.sphere.center.z; bounds[b + 6] = inst.sphere.radius; }
      else bounds[b + 6] = -1;
      bounds[b + 7] = inst.cullDistance;
      bounds[b + 8] = inst.maxDistance;
    });
    // The map's uv transform, colour, opacity and alpha test, as three sets them.
    const map = material.map;
    if (map.matrixAutoUpdate) map.updateMatrix();
    const id = core.rust.foliage_class(program.id, texture, Boolean(kind.grass), batch.uniforms.foliageWiggle.value, vertices, index, locations, rows, bounds,
      Float32Array.from(map.matrix.elements), Float32Array.of(material.color.r, material.color.g, material.color.b), material.opacity, material.alphaTest);

    this.classes.set(kind, id);
  }


}

export function foliagePass(core) {
  return new FoliagePass(core);
}
