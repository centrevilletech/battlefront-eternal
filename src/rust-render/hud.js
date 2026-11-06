// Rust draws the battle HUD and overlay batches. Changed vertex runs are
// uploaded to its persistent buffers; switching batches requires a full upload.
// Three retains texture and material ownership for the interface batch.
import * as THREE from "three";
import { writeUniform } from "./post.js";
import { HudRenderFrame as L, HudDraw as D } from '../hud-layout.js';
import { ownedProgram } from "./programs.js";

const ATTRIBUTES = "position:0,uv:1,maskUv:2,tint:3";
// hud.rs's layout.
const UNIFORM = 18;
const IN = { GROUPS: L.groups, PROGRAM: L.program, CAMERA: L.camera, SLOTS: L.slots, UPLOAD: L.upload, VIEWPORT: L.viewport, FIRST_GROUP: L.words };
const GROUP = D.words, MAX_GROUPS = 1024;
// Each array: the batch's attribute, the view of the renderer's mirror, its
// size a vertex, the mirror's binding.
const ARRAYS = [["position", "hudPosition", 3, "hud_position"], ["uv", "hudUv", 2, "hud_uv"], ["maskUv", "hudMaskUv", 2, "hud_mask_uv"], ["tint", "hudTint", 4, "hud_tint"]];

class HudPass {
  constructor(core) {
    this.core = core;
    this.name = "hud";
    this.phase = "hud";
    this.pending = null;
    this.complete = false;
    // Upload every vertex initially, after restoration, or when batches change.
    this.full = true;
    this.init = false;
    this.viewport = new THREE.Vector4();
  }

  // BattleHud.render, its batch built: true when the renderer drew it.
  cover(renderer, hud) {
    return this.drawBatch(renderer, hud);
  }

// BattleHud.render and its overlay batch share one retained Rust allocation.
// Switching batch identities requires a full upload even if neither batch
// reports dirty attributes: the previous batch owns the current GPU contents.
// The owned HUD shader key never depends on a source-bearing Three material.
  drawBatch(renderer, hud) {
    const core = this.core;
    const batch = hud.batch;
    if (batch.count === 0 && hud.nativeMode === undefined) return true;
    if (this.batch !== batch || this.nativeMode !== hud.nativeMode) { this.full = true; this.batch = batch; this.nativeMode = hud.nativeMode; }
    this.ensure(hud);
    // Resource registration can grow WASM memory. Finish it before draw()
    // takes the shared input/vertex views that it writes for this batch.
    if (hud.nativeMode === undefined) for(const group of batch.geometry.groups){
      const material=batch.mesh.material[group.materialIndex];
      if(material?.visible){this.texture(material.uniforms.map.value);this.texture(material.uniforms.mask.value);}
    }
    this.pending = hud;
    this.complete = false;
    core.phase("hud", renderer, hud.scene, hud.camera, true);
    this.pending = null;
    return this.complete;
  }

  // Both the battle HUD and shell map entry need these retained GPU mirrors.
  ensure(hud) {
    const core = this.core, batch = hud.batch;
    this.program ||= ownedProgram(core, "post-hud/hud", ATTRIBUTES);
    if (!this.init) {
      core.rust.hud_init(batch.geometry.index.array);
      for (const [, view, , binding] of ARRAYS) core.addView(view, Float32Array, () => core.rust[`${binding}_ptr`](), () => core.rust[`${binding}_len`]());
      core.addView("hudInput", Float64Array, () => core.rust.hud_input_ptr(), () => core.rust.hud_input_len());
      this.init = true;
    }
  }

// Pack only after sampler registration has completed. add_texture may grow
// WASM memory, detaching every previously acquired input and vertex view.
// Reacquiring here keeps camera/group records and copied vertices together.
// Otherwise registration can succeed while the HUD silently submits zero groups.
  draw() {
    const hud = this.pending;
    if (!hud) return;
    const core = this.core, renderer = core.renderer, batch = hud.batch, geometry = batch.geometry;
    core.views();
    const input = core.hudInput;
    input[IN.GROUPS] = 0; input[IN.UPLOAD] = 0; input[IN.UPLOAD + 1] = 0;
    // The camera's two matrices, and where map, mask and masked are.
    const uniforms = this.program.uniforms, slots = this.slots ||= new Int32Array(3);
    slots.fill(-1);
    let camera = 0;
    for (let k = 0; k < uniforms.length; k++) {
      const name = uniforms[k].name;
      if (name === "map") slots[0] = k;
      else if (name === "mask") slots[1] = k;
      else if (name === "masked") slots[2] = k;
      else if (camera < 2 && writeUniform(core, input, IN.CAMERA + camera * UNIFORM, k, uniforms[k], null, batch.mesh, hud.camera)) camera++;
      else { this.why = `uniform ${name}`; return; }
    }
    for (; camera < 2; camera++) input[IN.CAMERA + camera * UNIFORM] = -1;
    input[IN.PROGRAM] = this.program.id;
    input[IN.SLOTS] = slots[0]; input[IN.SLOTS + 1] = slots[1]; input[IN.SLOTS + 2] = slots[2];
    if (hud.nativeMode !== undefined) {
      renderer.getViewport(this.viewport).multiplyScalar(renderer.getPixelRatio()).floor();
      input[IN.VIEWPORT] = this.viewport.z; input[IN.VIEWPORT + 1] = this.viewport.w;
      core.rust.hud_model_build(hud.nativeMode, this.full);
      this.full = false; this.complete = true;
      return;
    }
    // The vertices that changed: the batch's update range (the same in
    // every array), or all of them.
// Batch.updateRanges describes one vertex span in position-float units.
// Convert that span to vertex indices before applying the different strides
// of position, UV, mask UV and tint. Restoration forces the full active span.
// Unchanged batches retain their existing GPU bytes rather than recopying all.
    let start = 0, end = batch.count * 4;
    if (!this.full) {
      const ranges = geometry.attributes.position.updateRanges;
      start = Infinity; end = 0;
      for (const r of ranges) { start = Math.min(start, r.start / 3); end = Math.max(end, (r.start + r.count) / 3); }
      if (!(end > start)) start = end = 0;
    }
    if (end > start) for (const [name, view, size] of ARRAYS) core[view].set(geometry.attributes[name].array.subarray(start * size, end * size), start * size);
    input[IN.UPLOAD] = start; input[IN.UPLOAD + 1] = end;
    renderer.getViewport(this.viewport).multiplyScalar(renderer.getPixelRatio()).floor();
    input[IN.VIEWPORT] = this.viewport.z; input[IN.VIEWPORT + 1] = this.viewport.w;
    // The runs: each group of the batch's mesh, within its draw range.
    const groups = geometry.groups, materials = batch.mesh.material, range = geometry.drawRange;
    let n = 0;
    for (const group of groups) {
      const first = Math.max(group.start, range.start), last = Math.min(group.start + group.count, range.start + range.count);
      if (last <= first) continue;
      if (n >= MAX_GROUPS) { this.why = "too many HUD groups"; return; }
      const material = materials[group.materialIndex];
      if (!material?.visible) continue;
      const u = material.uniforms, o = IN.FIRST_GROUP + n * GROUP;
      input[o] = first; input[o + 1] = last - first;
      input[o + 2] = this.texture(u.map.value); input[o + 3] = this.texture(u.mask.value); input[o + 4] = u.masked.value ? 1 : 0;
      n++;
    }
    input[IN.GROUPS] = n;
    this.full = false;
    this.complete = true;
  }


  // A texture's renderer id once three has its pixels (uploading them as
  // three's draw would), else -1.
  texture(texture) {
    if (!texture) return -1;
    const core = this.core, p = core.renderer.properties.get(texture);
    if (texture.version > 0 && p.__version !== texture.version && texture.image) core.renderer.initTexture(texture);
    return core.texture(texture);
  }

}

export function hudPass(core) {
  return new HudPass(core);
}
