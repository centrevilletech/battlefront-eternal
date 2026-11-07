import { STATIC } from "./static-layout.js";
// The foliage pass (foliage.rs) selects placed grass and leaf cards and draws
// them with declared Rust-owned variants. The host supplies immutable geometry,
// placement rows and the camera inputs used by Foliage.update.
// A card vertex's attributes, then a patch row's (foliage.rs VERTEX, ROW).
const ROW_FLOATS = 16, BOUNDS = 9;
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
    const core = this.core;
    const texture = core.texture(core.resources.staticSample(kind.map));
    if (texture < 0) core.fail(new Error("Invalid foliage program or texture"));
    // Authored cards remain asset data. Atlas, packing and material policy are
    // compiled natively; no geometry or material shell is made for this family.
    const cards = new Float64Array(kind.cards.length * STATIC.CARD.STRIDE);
    kind.cards.forEach((c, i) => cards.set(kind.grass
      ? [c.x,c.y,c.z,c.grey,c.width,c.height,c.phase,c.rate,c.part]
      : [c.p.x,c.p.y,c.p.z,c.grey,c.size,0,c.phase,0,c.part], i * STATIC.CARD.STRIDE));
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
    // Texture transforms are a cold loader input, shared with the existing
    // source/sampler owner. No material version or UV scan runs in a frame.
    const map = kind.map;
    if (map.matrixAutoUpdate) map.updateMatrix();
    const id = core.rust.foliage_class(texture, Boolean(kind.grass), Boolean(core.graphics.csm), JSON.stringify(kind.def), cards, rows, bounds,
      Float32Array.from(map.matrix.elements));

    this.classes.set(kind, id);
  }


}

export function foliagePass(core) {
  return new FoliagePass(core);
}
