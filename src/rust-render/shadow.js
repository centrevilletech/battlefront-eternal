// The shadow pass (shadow.rs): the sun's shadow cascades drawn by the renderer
// in place of three's shadow pass. graphics.js renderCascades offers it each
// cascade's pass (cover): the static cache the frame its window lands (its
// casters on the static layer), and every frame (the last cascade every other
// one) the live map: the cache's depth copied in, then the moving casters.
// Three draws a pass only when the renderer can't draw all of it.
//
// The scene is walked as three's shadow pass walks it (WebGLShadowMap
// renderObject): what is shown, on the camera's layers (the static casters'
// camera or the moving casters'), casting, in the cascade's frustum; each of a
// mesh's materials (its groups') that is shown (three's shadow pass runs
// inside reveal(), so what the other passes took from three is shown), drawn
// with the mesh's depth material (customDepthMaterial: graphics.js
// depthMaterial's, fabric's) as three compiled it, on the side its material
// casts (shadowSide, else the side opposite its own), over the range three
// would draw (onBeforeShadow gives the merged static meshes and the alpha-cut
// proxies their cells' range for the cascade). A skinned caster's bones are
// worked out once a frame as three's shadow pass does (objects.update) and its
// bone texture sent as three would. A caster the renderer can't draw (no depth
// material of its own, one three hasn't compiled, not a mesh) leaves the pass
// to three. Everything is registered inside the phase (its end resets three's
// state), so a pass left to three finds three's state as three left it.
import * as THREE from "three";
import { lighting } from "../materials.js";
import { plainProgram, writeUniform } from "./post.js";

const SHADOW_SIDE = { [THREE.FrontSide]: THREE.BackSide, [THREE.BackSide]: THREE.FrontSide, [THREE.DoubleSide]: THREE.DoubleSide };
// shadow.rs's layout.
const S = { TARGET: 0, FROM: 1, SIZE: 2, DRAWS: 3, FIRST: 4 };
const D = { PROGRAM: 0, GEOMETRY: 1, START: 2, COUNT: 3, SIDE: 4, MIRRORED: 5, UNIFORMS: 6, FIRST: 7 };
const UNIFORM = 18, MAX_UNIFORMS = 8, DRAW = D.FIRST + MAX_UNIFORMS * UNIFORM, MAX_DRAWS = 256;
const SAMPLER = 32;

class ShadowPass {
  constructor(core) {
    this.core = core;
    this.name = "shadow";
    this.phase = "shadow";
    this.request = null;
    this.draws = [];
    this.done = false;
    this.input = false;
    this.geometries = new Map(); // "geometry uuid:program" -> { id, sources: [[source, slot, version]], index version }
    this.boneFrames = new WeakMap(); // skeleton -> the render count its bones were worked out at
    this.drawn = 0;
    this.left = 0;
    this.why = null;
  }

  // graphics.js renderCascades: one cascade's pass (light: the cascade's; its
  // shadow's map is drawn into; from: the target whose depth is copied in
  // first, the cache, or null to clear), drawn here when every caster three
  // would draw is one the renderer can draw; true when it was.
  cover(renderer, light, scene, camera, from) {
    const core = this.core;
    if (!core.enabled || !core.active || this.on === false || !core.supported()) return false;
    const shadow = light.shadow;
    if (!shadow?.map?.depthTexture || shadow.mapSize.x !== shadow.mapSize.y || (from && !from.depthTexture)) return false;
    this.request = { renderer, light, scene, camera, from };
    this.done = false;
    try {
      core.phase("shadow", renderer, scene, camera);
    } finally {
      this.request = null;
    }
    if (this.done) { shadow.needsUpdate = false; this.drawn++; } else this.left++;
    return this.done;
  }

  // Off (this pass alone): three draws the cascades.
  setActive(on) {
    this.on = on;
  }

  // The phase: the walk, registration, and the pass's input.
  draw() {
    const req = this.request;
    if (!req) return;
    const core = this.core, { renderer, light, scene, camera, from } = req, shadow = light.shadow;
    // As three's pass does for the light: its camera placed, its frustum.
    shadow.updateMatrices(light);
    const shadowCamera = shadow.camera, frustum = shadow.getFrustum();
    this.draws.length = 0;
    this.why = null;
    this.walk(scene, camera, shadowCamera, frustum, renderer);
    const target = this.why ? -1 : core.texture(shadow.map.depthTexture);
    const source = from ? core.texture(from.depthTexture) : -1;
    // The pass is written into a buffer of the page's first: a value's texture
    // registered on the way (core.texture) can grow the renderer's memory,
    // which detaches every view of it; the view is taken once all is written.
    const out = this.scratch ||= new Float64Array(S.FIRST + MAX_DRAWS * DRAW);
    let n = 0;
    const ok = () => {
      if (this.why || target < 0 || (from && source < 0) || this.draws.length > MAX_DRAWS) return false;
      out[S.TARGET] = target; out[S.FROM] = source; out[S.SIZE] = shadow.mapSize.x;
      for (const d of this.draws) {
        const o = S.FIRST + n * DRAW, { program } = d;
        out[o + D.PROGRAM] = program.id; out[o + D.GEOMETRY] = d.geometry; out[o + D.START] = d.start; out[o + D.COUNT] = d.count;
        out[o + D.SIDE] = d.side; out[o + D.MIRRORED] = d.mirrored ? 1 : 0;
        if (program.uniforms.length > MAX_UNIFORMS) { this.why = `${program.uniforms.length} uniforms`; return false; }
        out[o + D.UNIFORMS] = program.uniforms.length;
        for (let k = 0; k < program.uniforms.length; k++) {
          if (!this.uniform(out, o + D.FIRST + k * UNIFORM, k, program.uniforms[k], d, shadowCamera)) { this.why = `no value for ${program.uniforms[k].name}`; return false; }
        }
        n++;
      }
      out[S.DRAWS] = n;
      return true;
    };
    const written = ok();
    if (!this.input) { core.addView("casterInput", Float64Array, () => core.rust.caster_input_ptr(), () => core.rust.caster_input_len()); this.input = true; }
    core.views();
    const input = core.casterInput;
    if (!written) {
      this.why ||= "textures";
      input[S.TARGET] = -1; input[S.DRAWS] = 0;
      return;
    }
    input.set(out.subarray(0, S.FIRST + n * DRAW));
    this.done = true;
  }

  // three's renderObject: a hidden object hides its subtree; one on other
  // layers is not drawn, but its children may be.
  walk(object, camera, shadowCamera, frustum, renderer) {
    if (object.visible === false || this.why) return;
    if (object.layers.test(camera.layers) && (object.isMesh || object.isLine || object.isPoints)) {
      if (object.castShadow && (!object.frustumCulled || frustum.intersectsObject(object))) {
        if (!object.isMesh) { this.why = "a line or points caster"; return; }
        object.modelViewMatrix.multiplyMatrices(shadowCamera.matrixWorldInverse, object.matrixWorld);
        if (object.isSkinnedMesh) this.bones(object, renderer);
        const material = object.material, geometry = object.geometry;
        if (Array.isArray(material)) {
          for (const group of geometry.groups) {
            const m = material[group.materialIndex];
            if (m && m.visible) this.add(object, geometry, m, group, camera, shadowCamera, renderer);
          }
        } else if (material.visible) this.add(object, geometry, material, null, camera, shadowCamera, renderer);
      }
    }
    const children = object.children;
    for (let i = 0; i < children.length; i++) this.walk(children[i], camera, shadowCamera, frustum, renderer);
  }

  // A caster's draw: its depth material as three's getDepthMaterial dresses it
  // for this material, its program, geometry and range.
  add(object, geometry, material, group, camera, shadowCamera, renderer) {
    if (this.why) return;
    const depth = object.customDepthMaterial;
    if (!depth) { this.why = `${object.name || object.type}: three's own depth material`; return; }
    depth.visible = material.visible;
    depth.wireframe = material.wireframe;
    depth.side = material.shadowSide ?? SHADOW_SIDE[material.side];
    depth.alphaMap = material.alphaMap;
    depth.alphaTest = material.alphaTest;
    depth.map = material.map;
    if (object.isInstancedMesh || object.isBatchedMesh || object.morphTargetInfluences?.length || depth.wireframe) { this.why = `${object.name || object.type}: kind`; return; }
    object.onBeforeShadow(renderer, object, camera, shadowCamera, geometry, depth, group);
    const program = plainProgram(this.core, depth, null);
    if (!program) { this.why = `${depth.name || depth.type}: ${program === undefined ? "not compiled yet" : "program"}`; return; }
    const g = this.geometry(geometry, program);
    if (g === null) { this.why = `${object.name || object.type}: geometry`; return; }
    // renderBufferDirect's range: the draw range within the group, the index.
    const index = geometry.index;
    if (!index) { this.why = `${object.name || object.type}: no index`; return; }
    let start = geometry.drawRange.start, end = geometry.drawRange.start + geometry.drawRange.count;
    if (group) { start = Math.max(start, group.start); end = Math.min(end, group.start + group.count); }
    start = Math.max(start, 0); end = Math.min(end, index.count);
    if (end - start > 0 && end - start !== Infinity) {
      this.draws.push({ program, geometry: g, start, count: end - start, side: depth.side, mirrored: object.matrixWorld.determinant() < 0, object, depth });
    }
    object.onAfterShadow(renderer, object, camera, shadowCamera, geometry, depth, group);
  }

  // A skinned caster's bones, worked out once a frame as three's shadow pass
  // does (objects.update), and its bone texture sent as three would bind it.
  bones(object, renderer) {
    const skeleton = object.skeleton, frame = renderer.info.render.frame;
    if (this.boneFrames.get(skeleton) !== frame) { skeleton.update(); this.boneFrames.set(skeleton, frame); }
    if (skeleton.boneTexture === null) skeleton.computeBoneTexture();
    const props = renderer.properties.get(skeleton.boneTexture);
    if (props.__version !== skeleton.boneTexture.version) renderer.initTexture(skeleton.boneTexture);
  }

  // Rust's copy of a geometry for a program (core.geometry), sent again where
  // the page rewrote it since (the crowd's shadow mesh: its soldiers' vertices
  // as they join, its drawn index as they come and go).
  geometry(geometry, program) {
    const core = this.core, key = `${geometry.uuid}:${program.id}`;
    let entry = this.geometries.get(key);
    if (!entry) {
      const id = core.geometry(geometry, { id: program.id, attributes: program.attributes });
      if (id === null || id === undefined) return null;
      // Its buffers' slots as core.geometry made them: each source once, in
      // the program's attribute order, the index last.
      const sources = [];
      for (const [name] of program.attributes) {
        const a = geometry.attributes[name], source = a?.isInterleavedBufferAttribute ? a.data : a;
        if (source && !sources.some((s) => s[0] === source)) sources.push([source, sources.length, source.version]);
      }
      entry = { id, sources, index: geometry.index?.version ?? 0 };
      this.geometries.set(key, entry);
      return id;
    }
    for (const s of entry.sources) {
      if (s[0].version === s[2]) continue;
      const array = s[0].array;
      core.rust.geometry_buffer_data(entry.id, s[1], new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
      s[2] = s[0].version;
    }
    const index = geometry.index;
    if (index && index.version !== entry.index) {
      const array = index.array;
      core.rust.geometry_index_data(entry.id, new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
      entry.index = index.version;
    }
    return entry.id;
  }

  // A draw's uniform as three's renderBufferDirect would set it for the
  // shadow camera: the object's skinning (bind matrices, bone texture), its
  // normal matrix as it stands (three doesn't work it out for a shadow), the
  // banners' clock (the frame's time), else as a full-screen pass's are.
  uniform(out, o, slot, uniform, d, camera) {
    const { name, type } = uniform, object = d.object;
    let value;
    if (name === "bindMatrix") value = object.bindMatrix;
    else if (name === "bindMatrixInverse") value = object.bindMatrixInverse;
    else if (name === "normalMatrix") value = object.normalMatrix;
    else if (name === "fabricTime") value = lighting.time.value;
    else if (name === "boneTexture") {
      const id = this.core.texture(object.skeleton?.boneTexture);
      if (id < 0) return false;
      out[o] = slot; out[o + 1] = SAMPLER; out[o + 2] = id;
      return true;
    } else return writeUniform(this.core, out, o, slot, uniform, d.depth, object, camera);
    out[o] = slot; out[o + 1] = type;
    if (typeof value === "number") { out[o + 2] = value; return true; }
    if (value?.isMatrix4 || value?.isMatrix3) { for (let k = 0; k < value.elements.length; k++) out[o + 2 + k] = value.elements[k]; return true; }
    return false;
  }
}

export function shadowPass(core) {
  return new ShadowPass(core);
}
