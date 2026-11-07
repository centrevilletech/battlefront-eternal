// The Enhanced leaf cards use a fixed Rust-owned World program. The host
// supplies selection and changed instance slots; no browser material is compiled.

import { STATIC } from "./static-layout.js";
// canopies.rs's input.
const C = STATIC.CANOPIES;

class CanopiesPass {
  constructor(core) {
    this.core = core;
    this.name = "canopies";
    this.phase = "near";
    this.canopies = null;
    // Geometry and instance layout, registered before the first native draw.
    this.entry = undefined;
  }

  draw(ctx) {
    const core = this.core, canopies = core.graphics.treeCanopies;
    if (!canopies) return;
    if (canopies !== this.canopies) { this.canopies = canopies; this.entry = undefined; }
    if (this.entry === undefined) this.take(canopies);
    const entry = this.entry;
    if (!entry) return;
    const mesh = canopies.node;
    // Its texture, registered before the input's views are made (registering
    // can move the module's memory).
    const texture = entry.texture;
    core.views();
    const input = core.canopiesInput;
    input[C.SHOWN] = 0;

    // The leaves that changed into the renderer's copy, each attribute's at
    // its instances' places.
    const dirty = canopies.takeDirty();
    if (dirty) {
      const [first, end] = dirty, leaves = core.canopiesLeaves;
      for (const { array, size, at } of entry.attributes) {
        for (let i = first * size; i < end * size; i++) leaves[at + i] = array[i];
      }
      input[C.COPIED] = first; input[C.COPIED + 1] = end;
    }
    // Whether three would draw it in this render: leaves to draw, the mesh
    // and its parents shown, on the camera's layers (never culled).
    input[C.SHADOWS] = core.graphics.csm ? 1 : 0;
    input[C.TEXTURE] = texture;
    input[C.INSTANCES] = Math.min(canopies.instanceCount, entry.max);
    input[C.SHOWN] = canopies.drawn ? 1 : 0;
  }

  // Register the fixed program and card geometry before the first leaf draw.
  take(canopies) {
    const core = this.core;
    const max = canopies.maxLeaves;
    core.rust.canopies_mesh(max);
    const attributes = [{ array: canopies.centers, size: 3, at: 0 },
      { array: canopies.sizes, size: 2, at: max * 3 }, { array: canopies.shades, size: 3, at: max * 5 }];
    const texture = core.texture(core.resources.staticSample(canopies.map));
    core.addView("canopiesInput", Float64Array, () => core.rust.canopies_input_ptr(), () => core.rust.canopies_input_len());
    core.addView("canopiesLeaves", Float32Array, () => core.rust.canopies_leaves_ptr(), () => core.rust.canopies_leaves_len());
    this.entry = { texture, attributes, max };
    canopies.dirtyFirst = 0;
    canopies.dirtyEnd = max;
  }


}

export function canopiesPass(core) {
  return new CanopiesPass(core);
}
