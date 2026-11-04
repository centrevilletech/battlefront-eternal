// The HUD pass (hud.rs): the battle HUD's quads (hud.js, drawn with the
// interface's batch, src/interface/batch.js) over the finished frame on the
// canvas, in place of three's render of the HUD's scene: BattleHud.render
// asks the bridge to cover each frame's batch once built (core.coverHud),
// and three renders the scene only when it did not.
//
// The batch's vertex arrays are mirrored in the renderer: each frame the
// vertices the batch changed (its attributes' update range: at most one run)
// are copied in, and every vertex after a frame three drew instead. Three's
// own copy goes stale while the renderer draws, so a frame given back to
// three uploads the whole arrays. Each run of quads with one material (the
// batch's groups) is a draw with its texture and mask; a texture still
// loading is skipped, as three's empty texture draws nothing there.
import * as THREE from "three";
import { plainProgram, writeUniform } from "./post.js";

const ATTRIBUTES = "position:0,uv:1,maskUv:2,tint:3";
// hud.rs's layout.
const UNIFORM = 18;
const IN = { GROUPS: 0, PROGRAM: 1, CAMERA: 2, SLOTS: 2 + 2 * UNIFORM, UPLOAD: 5 + 2 * UNIFORM, VIEWPORT: 7 + 2 * UNIFORM, FIRST_GROUP: 9 + 2 * UNIFORM };
const GROUP = 5, MAX_GROUPS = 1024;
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
    // Whether the renderer drew the last frame's batch, and whether its copy
    // needs every vertex (the first frame, or after three drew one).
    this.drew = false;
    this.full = true;
    this.init = false;
    this.viewport = new THREE.Vector4();
  }

  // BattleHud.render, its batch built: true when the renderer drew it.
  cover(renderer, hud) {
    const drew = this.drawBatch(renderer, hud);
    if (!drew) {
      this.full = true;
      // Three draws this frame from its own copy, stale while the renderer
      // drew: all of it goes up.
      if (this.drew) for (const [name] of ARRAYS) { const a = hud.batch.geometry.attributes[name]; a.clearUpdateRanges(); a.needsUpdate = true; }
    }
    this.drew = drew;
    return drew;
  }

  drawBatch(renderer, hud) {
    const core = this.core;
    if (!core.enabled || !core.active || this.on === false || !core.rust.hud_init) return false;
    const batch = hud.batch;
    // One program for every material of the batch (batch.js makes them all
    // from the same shaders): the first one three has made.
    if (this.ownedSources !== (core.ownedShaderSources !== false)) { this.program = null; this.ownedSources = core.ownedShaderSources !== false; }
    if (!this.program) {
      for (const material of batch.mesh.material) { const p = plainProgram(core, material, ATTRIBUTES); if (p !== undefined) { this.program = p; break; } }
      if (!this.program) return false;
    }
    if (!this.init) {
      core.rust.hud_init(batch.geometry.index.array);
      for (const [, view, , binding] of ARRAYS) core.addView(view, Float32Array, () => core.rust[`${binding}_ptr`](), () => core.rust[`${binding}_len`]());
      core.addView("hudInput", Float64Array, () => core.rust.hud_input_ptr(), () => core.rust.hud_input_len());
      this.init = true;
    }
    this.pending = hud;
    this.complete = false;
    core.phase("hud", renderer, hud.scene, hud.camera, true);
    this.pending = null;
    return this.complete;
  }

  draw() {
    const hud = this.pending;
    if (!hud) return;
    const core = this.core, renderer = core.renderer, batch = hud.batch, geometry = batch.geometry;
    core.views();
    const input = core.hudInput;
    input[IN.GROUPS] = 0; input[IN.UPLOAD] = 0; input[IN.UPLOAD + 1] = 0;
    // The camera's two matrices, and where map, mask and masked are.
    const uniforms = this.program.uniforms, slots = [-1, -1, -1];
    let camera = 0;
    for (let k = 0; k < uniforms.length; k++) {
      const name = uniforms[k].name;
      if (name === "map") slots[0] = k;
      else if (name === "mask") slots[1] = k;
      else if (name === "masked") slots[2] = k;
      else if (camera < 2 && writeUniform(core, input, IN.CAMERA + camera * UNIFORM, k, uniforms[k], null, batch.mesh, hud.camera)) camera++;
      else return;
    }
    for (; camera < 2; camera++) input[IN.CAMERA + camera * UNIFORM] = -1;
    input[IN.PROGRAM] = this.program.id;
    input[IN.SLOTS] = slots[0]; input[IN.SLOTS + 1] = slots[1]; input[IN.SLOTS + 2] = slots[2];
    // The vertices that changed: the batch's update range (the same in
    // every array), or all of them.
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
      if (n >= MAX_GROUPS) return;
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

  // Off (this pass alone): three draws the HUD.
  setActive(on) {
    this.on = on;
  }

  // A texture's renderer id once three has its pixels (uploading them as
  // three's draw would), else -1.
  texture(texture) {
    if (!texture) return -1;
    const core = this.core, p = core.renderer.properties.get(texture);
    if (texture.version > 0 && p.__version !== texture.version && texture.image) core.renderer.initTexture(texture);
    if (!p.__webglTexture) return -1;
    return core.texture(texture);
  }

}

export function hudPass(core) {
  return new HudPass(core);
}
