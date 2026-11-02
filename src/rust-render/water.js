// The water pass (water.rs): the near scene's terrain water (water.js) as
// items of the transparent list, in Classic settings: the surface drawn with
// the look's textures (Low) and the rings where things meet the water.
// Each is registered the first time three has a program for its material
// (made as three compiled it: Kind::Plain), then hidden from three; each
// frame, in the transparent phase, the pass writes whether three would have
// drawn it (its object's and parents' visibility, the camera's layers), its
// sort key as three's projectObject makes it, its draw range and every
// uniform its program reads. The rings' vertices that changed (their update
// ranges) are copied into the renderer's mirror of them. The Enhanced
// passes (refraction, reflection), which read the opaque scene's copy and
// arrays of waders, stay with three.
import * as THREE from "three";
import { plainProgram, writeUniform } from "./post.js";
import { blendOf, sidesOf, stateOf } from "./transparent.js";

// water.rs's layout.
const S = { SHOWN: 0, PROGRAM: 1, KEY: 2, CENTER: 5, WORLD: 8, RANGE: 24, UNIFORMS: 26, FIRST: 27 };
const UNIFORM = 18, MAX_UNIFORMS = 24, SURFACE = S.FIRST + MAX_UNIFORMS * UNIFORM, MAX_SURFACES = 8;
const RING_UPLOAD = MAX_SURFACES * SURFACE;
const RINGS = [["position", "waterRingsPosition", 3, "water_rings_position"], ["uv", "waterRingsUv", 2, "water_rings_uv"], ["alpha", "waterRingsAlpha", 1, "water_rings_alpha"]];
const frustum = new THREE.Frustum(), viewProjection = new THREE.Matrix4();

class WaterPass {
  constructor(core) {
    this.core = core;
    this.name = "water";
    this.phase = "transparent";
    this.water = null;
    // Registered meshes: { mesh, material, program, id }, in water.rs's order.
    this.surfaces = [];
    this.tried = new WeakSet();
    this.input = false;
    // The renderer's copy of the rings needs every vertex.
    this.full = true;
  }

  draw(ctx) {
    const core = this.core, water = core.graphics.scene.userData.water;
    if (!water) return;
    if (water !== this.water) { this.water = water; this.surfaces = []; this.tried = new WeakSet(); this.full = true; }
    for (const mesh of [water.lowPass, water.tintPass, water.mirrorPass, water.rings]) if (mesh && !this.tried.has(mesh)) this.registerMesh(mesh, mesh === water.rings);
    if (!this.surfaces.length) return;
    core.views();
    const input = core.waterInput, camera = ctx.camera;
    frustum.setFromProjectionMatrix(viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    input[RING_UPLOAD] = 0; input[RING_UPLOAD + 1] = 0;
    this.surfaces.forEach((surface, s) => this.write(surface, s, input, camera, ctx.scene));
  }

  // A water mesh the renderer can draw: one pass (single sided, or both
  // sides at once), a blending and state it knows, a program with no
  // uniform arrays; the rings with their own arrays.
  registerMesh(mesh, rings) {
    const core = this.core, material = mesh.material;
    const program = plainProgram(core, material, rings ? "position:0,uv:1,alpha:2" : "position:0,uv:1");
    if (program === undefined) return; // until three has made it
    this.tried.add(mesh);
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    if (!program || !blend || !state || sides.length !== 1 || !mesh.geometry.index || mesh.geometry.groups.length || program.uniforms.length > MAX_UNIFORMS || this.surfaces.length >= MAX_SURFACES) return;
    let id;
    if (rings) {
      if (!this.input) this.views();
      const index = mesh.geometry.index.array;
      id = core.rust.water_rings(index instanceof Uint32Array ? index : Uint32Array.from(index), new Uint32Array(blend), state.func, state.flags, sides[0]);
    } else {
      const g = core.geometry(mesh.geometry, { id: program.id, attributes: [["position", 0], ["uv", 1]] });
      if (g === null || g === undefined) return;
      id = core.rust.water_surface(g, new Uint32Array(blend), state.func, state.flags, sides[0]);
    }
    if (!this.input) this.views();
    if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
    this.surfaces.push({ mesh, material, program, id, rings });
    material.visible = !core.active;
  }

  views() {
    const core = this.core;
    core.addView("waterInput", Float64Array, () => core.rust.water_input_ptr(), () => core.rust.water_input_len());
    for (const [, view, , binding] of RINGS) core.addView(view, Float32Array, () => core.rust[`${binding}_ptr`](), () => core.rust[`${binding}_len`]());
    this.input = true;
  }

  // Its values this frame, as three would have listed and drawn it.
  write(surface, s, input, camera, scene) {
    const { mesh, material, program } = surface, o = s * SURFACE;
    input[o + S.SHOWN] = 0;
    // projectObject: hidden with any parent hidden; listed on the camera's
    // layers, in view when culled by its sphere; its material the same.
    for (let n = mesh; n && n !== scene; n = n.parent) if (!n.visible) return;
    if (!mesh.layers.test(camera.layers) || mesh.material !== material) return;
    const geometry = mesh.geometry;
    if (mesh.frustumCulled && !frustum.intersectsObject(mesh)) return;
    // The draw range, as renderBufferDirect clamps it to the index.
    const range = geometry.drawRange, count = geometry.index.count;
    const first = Math.max(0, range.start), last = Math.min(count, range.start + range.count);
    if (last <= first) return;
    input[o + S.RANGE] = first; input[o + S.RANGE + 1] = last - first;
    // Its sort key: the nearest group's render order, its own, its id; z
    // from its geometry's bounding sphere (as three last computed it).
    let groupOrder = 0;
    for (let n = mesh.parent; n; n = n.parent) if (n.isGroup) { groupOrder = n.renderOrder; break; }
    input[o + S.KEY] = groupOrder; input[o + S.KEY + 1] = mesh.renderOrder; input[o + S.KEY + 2] = mesh.id;
    const c = geometry.boundingSphere.center;
    input[o + S.CENTER] = c.x; input[o + S.CENTER + 1] = c.y; input[o + S.CENTER + 2] = c.z;
    input.set(mesh.matrixWorld.elements, o + S.WORLD);
    input[o + S.PROGRAM] = program.id;
    input[o + S.UNIFORMS] = program.uniforms.length;
    for (let k = 0; k < program.uniforms.length; k++) {
      if (!writeUniform(this.core, input, o + S.FIRST + k * UNIFORM, k, program.uniforms[k], material, mesh, camera)) return;
    }
    if (surface.rings) this.rings(geometry, input);
    input[o + S.SHOWN] = 1;
  }

  // The rings' vertices that changed (all of them the first time) into the
  // renderer's mirror.
  rings(geometry, input) {
    const core = this.core, position = geometry.attributes.position;
    let start = 0, end = position.count;
    if (!this.full) {
      start = Infinity; end = 0;
      for (const r of position.updateRanges) { start = Math.min(start, r.start / 3); end = Math.max(end, (r.start + r.count) / 3); }
      if (!(end > start)) return;
    }
    for (const [name, view, size] of RINGS) core[view].set(geometry.attributes[name].array.subarray(start * size, end * size), start * size);
    input[RING_UPLOAD] = start; input[RING_UPLOAD + 1] = end;
    this.full = false;
  }

  setActive(on) {
    for (const surface of this.surfaces) surface.material.visible = !on;
    // Three's copy of the rings is whole: the renderer's needs every vertex
    // when it draws them again.
    if (on) this.full = true;
  }

  restore() {
    this.setActive(false);
  }
}

export function waterPass(core) {
  return new WaterPass(core);
}
