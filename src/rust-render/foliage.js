// The foliage pass (foliage.rs): the near scene's placed foliage (foliage.js,
// foliage-gpu.js), each class's cards drawn instanced over the patches in
// view, in the near phase, with three's program for the class.
//
// The renderer chooses the patches too. The page's Foliage asks its drawer
// (this pass, once one is attached: Foliage.drawer) to select whenever it
// updates; a class is registered there the first time three has a program
// for it (three's draw or warmup made one), and from then on three neither
// updates nor draws it (its material is hidden). The selection is then the
// renderer's, made as FoliageGpuBatch.update makes it, from the camera the
// page's update was given; the near phase draws it.
const WORLD = 0;
// The material values the card shaders read, set per draw from the class.
const MATERIAL = ["foliageEye", "foliageRight", "foliageUp", "foliageBack", "foliageTime", "foliageEnd", "foliageWiggle", "foliageFogRange", "foliageFogColor", "diffuse", "opacity", "alphaTest"];
// Everything else a card program may read besides the frame block: its map
// and the map's uv transform, the model-view matrix (the cards are in world
// space: the camera's view), and the model matrix three's program reads
// under shadows (the identity).
const POLICY = { uniforms: new Set([...MATERIAL, "map", "mapTransform", "modelViewMatrix", "modelMatrix"]), material: new Set(MATERIAL) };
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
    // Foliage kind -> the renderer's class (null: three keeps it).
    this.classes = new Map();
    this.selected = 0;
    this.input = false;
  }

  // The near phase: the map's foliage, once it has one, takes this pass as
  // its drawer (the classes it registered are drawn in the renderer's call).
  draw() {
    const foliage = this.core.graphics.scene.userData.foliage;
    if (!foliage || foliage.drawer === this || this.on === false) return;
    if (foliage !== this.foliage) { this.foliage = foliage; this.classes = new Map(); }
    foliage.drawer = this;
  }

  // Whether the renderer draws a kind (Foliage.update leaves it alone).
  covers(kind) {
    return this.classes.get(kind) != null;
  }

  // Foliage.update's selection, for every class the renderer draws; a class
  // three has a program for is registered first.
  select(foliage, camera) {
    try { this.selectClasses(foliage, camera); } catch (error) { this.core.fail(error); }
  }

  selectClasses(foliage, camera) {
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

  registerClass(kind) {
    const core = this.core, batch = kind.batch, material = batch?.material;
    const geometry = batch?.geometry;
    if (!geometry?.isInstancedBufferGeometry || !geometry.index) { this.classes.set(kind, null); return; }
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (!threeProgram) return; // until three has made its program
    const program = core.program(threeProgram, WORLD, material, POLICY);
    const texture = core.texture(material.map);
    if (!program || texture < 0) { this.classes.set(kind, null); return; }
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
    // Each patch's row (what FoliageGpuBatch.update writes for it) and bounds.
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
    material.visible = !core.active;
    this.classes.set(kind, id);
  }

  setActive(on) {
    for (const [kind, id] of this.classes) if (id != null) kind.batch.material.visible = !on;
    if (this.foliage) this.foliage.drawer = on ? this : null;
    this.core.rust.foliage_on?.(on);
    this.on = on;
  }

  restore() {
    this.setActive(false);
  }
}

export function foliagePass(core) {
  return new FoliagePass(core);
}
