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
// An attribute's buffer source (an interleaved one's shared buffer).
const sourceOf = (a) => (a?.isInterleavedBufferAttribute ? a.data : a);

// A source's new contents (s: its buffer's slot, -1 for the index, the
// version and size it was sent with, and how much of it from the start is
// valid) as three's WebGLAttributes sends them: its update ranges, sorted and
// merged as three merges them (adjacent ones too), else all of it, into the
// buffer as made (bufferSubData); all of it into a buffer made again when its
// array is no longer that size. The ranges stay (three clears them when it
// sends them itself), so they say what changed only when the version moved
// by one since the pass sent it: else (three sent one between, clearing its
// ranges, or the page marked it twice) it is all of it. An index without
// ranges is sent only as far as the pass's draws read it (need), the rest
// when a draw reads further (geometry): the crowd's mesh draws a tenth of its
// index. Returns how much of it from the start is valid now.
function send(rust, id, s, need = Infinity) {
  const array = s.source.array, per = array.BYTES_PER_ELEMENT;
  const view = (start, count) => new Uint8Array(array.buffer, array.byteOffset + start * per, count * per);
  const part = (start, count) => {
    if (s.slot < 0) rust.geometry_index_sub_data(id, start * per, view(start, count));
    else rust.geometry_buffer_sub_data(id, s.slot, start * per, view(start, count));
  };
  if (array.byteLength !== s.bytes) {
    if (s.slot < 0) rust.geometry_index_data(id, view(0, array.length));
    else rust.geometry_buffer_data(id, s.slot, view(0, array.length));
    return array.length;
  }
  const ranges = [], marked = s.source.version === s.version + 1 ? s.source.updateRanges || [] : [];
  for (const r of [...marked].sort((a, b) => a.start - b.start)) {
    const last = ranges[ranges.length - 1];
    if (last && r.start <= last.start + last.count + 1) last.count = Math.max(last.count, r.start + r.count - last.start);
    else ranges.push({ start: r.start, count: r.count });
  }
  if (!ranges.length) {
    const n = s.slot < 0 ? Math.min(need, array.length) : array.length;
    if (n > 0) part(0, n);
    return n;
  }
  let valid = s.valid;
  for (const { start, count } of ranges) {
    const n = Math.min(count, array.length - start);
    if (start < 0 || n <= 0) continue;
    part(start, n);
    if (start <= valid) valid = Math.max(valid, start + n);
  }
  return valid;
}

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
    this.geometries = new Map(); // "geometry uuid:program" -> { id, sources, index } (geometry())
    this.boneFrames = new WeakMap(); // skeleton -> the render count its bones were worked out at
    this.found = new Map(); // a pass camera's layer mask -> this frame's casters it sees (candidates)
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

  // three's renderObject, for one cascade: each caster the pass camera's
  // layers see (candidates) in the cascade's frustum, in scene order.
  walk(scene, camera, shadowCamera, frustum, renderer) {
    for (const object of this.candidates(scene, camera, renderer)) {
      if (this.why) return;
      if (object.frustumCulled && !frustum.intersectsObject(object)) continue;
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

  // The casters a pass camera's layers see this frame: shown (with every
  // parent, a hidden object hiding its subtree), on those layers, casting.
  // One walk of the scene a frame and set of layers (three walks it for each
  // cascade): the frame's other cascades test the same list against their
  // own frustum. Nothing between a frame's cascade passes shows, hides or
  // moves a caster.
  candidates(scene, camera, renderer) {
    const frame = renderer.info.render.frame, mask = camera.layers.mask;
    let entry = this.found.get(mask);
    if (!entry || entry.frame !== frame || entry.scene !== scene) {
      entry = { frame, scene, list: entry?.list ?? [] };
      entry.list.length = 0;
      const collect = (object) => {
        if (object.visible === false) return;
        if (object.castShadow && object.layers.test(camera.layers) && (object.isMesh || object.isLine || object.isPoints)) entry.list.push(object);
        const children = object.children;
        for (let i = 0; i < children.length; i++) collect(children[i]);
      };
      collect(scene);
      this.found.set(mask, entry);
    }
    return entry.list;
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
    // renderBufferDirect's range: the draw range within the group, the index.
    const index = geometry.index;
    if (!index) { this.why = `${object.name || object.type}: no index`; return; }
    let start = geometry.drawRange.start, end = geometry.drawRange.start + geometry.drawRange.count;
    if (group) { start = Math.max(start, group.start); end = Math.min(end, group.start + group.count); }
    start = Math.max(start, 0); end = Math.min(end, index.count);
    const g = this.geometry(geometry, program, end > start ? end : 0);
    if (g === null) { this.why = `${object.name || object.type}: geometry`; return; }
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
  // as they join, its drawn index as they come and go), as three would send
  // it (send); its index valid at least as far as a draw reads it (need). A
  // geometry given another attribute or index since is left to three (null),
  // which makes buffers for it.
  geometry(geometry, program, need = Infinity) {
    const core = this.core, key = `${geometry.uuid}:${program.id}`;
    let entry = this.geometries.get(key);
    if (!entry) {
      const id = core.geometry(geometry, { id: program.id, attributes: program.attributes });
      if (id === null || id === undefined) return null;
      // Its buffers' slots as core.geometry made them: each source once, in
      // the program's attribute order, the index last; each with the version
      // and size it was sent with.
      const sources = [];
      for (const [name] of program.attributes) {
        const source = sourceOf(geometry.attributes[name]);
        if (source && !sources.some((s) => s.source === source)) sources.push({ name, source, slot: sources.length, version: source.version, bytes: source.array.byteLength });
      }
      const index = geometry.index;
      entry = { id, sources, index: { source: index, slot: -1, version: index.version, bytes: index.array.byteLength, valid: index.array.length } };
      this.geometries.set(key, entry);
      return id;
    }
    if (geometry.index !== entry.index.source) return null;
    for (const s of entry.sources) if (sourceOf(geometry.attributes[s.name]) !== s.source) return null;
    for (const s of entry.sources) {
      if (s.source.version === s.version) continue;
      send(core.rust, entry.id, s);
      s.version = s.source.version;
      s.bytes = s.source.array.byteLength;
    }
    const x = entry.index, array = x.source.array;
    if (x.source.version !== x.version) {
      x.valid = send(core.rust, entry.id, x, need);
      x.version = x.source.version;
      x.bytes = array.byteLength;
    }
    if (need > x.valid && x.valid < array.length) {
      // A draw reading further than what was sent of this version.
      const end = Math.min(need, array.length), per = array.BYTES_PER_ELEMENT;
      core.rust.geometry_index_sub_data(entry.id, x.valid * per, new Uint8Array(array.buffer, array.byteOffset + x.valid * per, (end - x.valid) * per));
      x.valid = end;
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
