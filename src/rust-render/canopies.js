// The Enhanced leaf cards use a fixed Rust-owned World program. The host
// supplies selection and changed instance slots; no browser material is compiled.
import * as THREE from "three";

import { ownedGeometryProgram } from "./programs.js";
// canopies.rs's input.
const C = { SHOWN: 0, PROGRAM: 1, TEXTURE: 2, INSTANCES: 3, COPIED: 4 };
const INSTANCED = ["leafCenter", "leafSize", "leafShade"];

class CanopiesPass {
  constructor(core) {
    this.core = core;
    this.name = "canopies";
    this.phase = "near";
    this.canopies = null;
    // { program, attributes: [{ attribute, size, at }], max }, null when the
    // renderer can't draw the mesh, undefined until three has drawn it.
    this.entry = undefined;
  }

  draw(ctx) {
    const core = this.core, canopies = core.graphics.treeCanopies;
    if (!canopies) return;
    if (canopies !== this.canopies) { this.canopies = canopies; this.entry = undefined; }
    // Taken now: three has listed the mesh in this render and draws it; the
    // renderer draws it from the next one.
    if (this.entry === undefined) this.take(canopies);
    const entry = this.entry;
    if (!entry) return;
    const mesh = canopies.mesh, material = mesh.material;
    // Its texture, registered before the input's views are made (registering
    // can move the module's memory).
    const texture = core.texture(material.uniforms.map?.value);
    core.views();
    const input = core.canopiesInput;
    input[C.SHOWN] = 0;

    // The leaves that changed into the renderer's copy, each attribute's at
    // its instances' places.
    const dirty = canopies.takeDirty();
    if (dirty) {
      const [first, end] = dirty, leaves = core.canopiesLeaves;
      for (const { attribute, size, at } of entry.attributes) leaves.set(attribute.array.subarray(first * size, end * size), at + first * size);
      input[C.COPIED] = first; input[C.COPIED + 1] = end;
    }
    // Whether three would draw it in this render: leaves to draw, the mesh
    // and its parents shown, on the camera's layers (never culled).
    let shown = canopies.drawn && mesh.layers.test(ctx.camera.layers);
    for (let n = mesh.parent; shown && n && n !== ctx.scene; n = n.parent) if (!n.visible) shown = false;
    input[C.PROGRAM] = entry.program.id;
    input[C.TEXTURE] = texture;
    input[C.INSTANCES] = Math.min(mesh.geometry.instanceCount ?? 0, entry.max);
    input[C.SHOWN] = shown ? 1 : 0;
  }

  // Register the fixed program and card geometry before the first leaf draw.
  take(canopies) {
    const core = this.core, mesh = canopies.mesh, material = mesh.material, geometry = mesh.geometry;
    this.entry = null;
    if (material.transparent || !material.alphaToCoverage || material.side !== THREE.DoubleSide) throw Error("Invalid canopy material state");
    const program = ownedGeometryProgram(core, "world/canopies");
    const instanced = program.attributes.filter(([name]) => INSTANCED.includes(name));
    const attributes = instanced.map(([name, location]) => ({ name, location, attribute: geometry.attributes[name] }));
    if (attributes.length !== INSTANCED.length || attributes.some((a) => !(a.attribute?.array instanceof Float32Array) || !a.attribute.isInstancedBufferAttribute)) return;
    const g = core.geometry(geometry, { id: program.id, attributes: program.attributes.filter(([name]) => !INSTANCED.includes(name)) });
    if (g === null || g === undefined) return;
    const max = Math.min(...attributes.map((a) => a.attribute.count));
    core.rust.canopies_mesh(g, new Uint32Array(attributes.map((a) => a.location)), new Uint32Array(attributes.map((a) => a.attribute.itemSize)), max);
    let at = 0;
    for (const a of attributes) { a.size = a.attribute.itemSize; a.at = at; at += max * a.size; }
    core.addView("canopiesInput", Float64Array, () => core.rust.canopies_input_ptr(), () => core.rust.canopies_input_len());
    core.addView("canopiesLeaves", Float32Array, () => core.rust.canopies_leaves_ptr(), () => core.rust.canopies_leaves_len());
    this.entry = { program, attributes, max };
    canopies.dirtyFirst = 0;
    canopies.dirtyEnd = max;
  }


}

export function canopiesPass(core) {
  return new CanopiesPass(core);
}
