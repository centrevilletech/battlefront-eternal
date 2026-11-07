// Uniform packing for auxiliary screen draws, plus the retained post host.
// Post policy, uniform values and temporal commit belong to post_chain.rs;
// source-bearing utility materials still serve the separate scene passes.
import * as THREE from "three";
import { ownedProgram, ownsEffectSource } from "./programs.js";
import { ownedEffectProgram } from "./effect-material.js";
import { FACTS, STATE, RESOURCE, MODE, AA, POST_KEYS } from "./post-layout.js";
const MODES = { off: MODE.OFF, bilinear: MODE.BILINEAR, spatial: MODE.SPATIAL, temporal: MODE.TEMPORAL };

const PLAIN = 3;
const SAMPLER = 32, SAMPLER_3D = 33;
// Where post.rs's triangle has its position and uv.
const ATTRIBUTES = "position:0,uv:1";

// A uniform's GL type as post.rs takes it: floats 1-4, int (bool too) 5,
// mat3 9, mat4 16, sampler 32, sampler3D 33; 0: one it can't set.
export function uniformType(gl, type) {
  switch (type) {
    case gl.FLOAT: return 1;
    case gl.FLOAT_VEC2: return 2;
    case gl.FLOAT_VEC3: return 3;
    case gl.FLOAT_VEC4: return 4;
    case gl.INT: case gl.BOOL: return 5;
    case gl.FLOAT_MAT3: return 9;
    case gl.FLOAT_MAT4: return 16;
    case gl.SAMPLER_2D: return SAMPLER;
    case gl.SAMPLER_3D: return SAMPLER_3D;
    default: return 0;
  }
}

// Rust's program for three's program of a material, as three compiled it,
// with its uniforms ({ name, type } in slot order); null when it has one the
// renderer can't set; undefined until three has made it.
const plainPrograms = new WeakMap();
// Whether plainProgram has made (or refused) the renderer's program for
// three's current one of a material, so that calling it makes no program.
export function plainProgramKnown(core, material) {
  if (material.userData?.bfeProgram) return core.ownedPrograms?.has(material.userData.bfeProgram) || false;
  const threeProgram = core.renderer.properties.get(material).currentProgram;
  return !threeProgram || plainPrograms.has(threeProgram);
}

// A uniform the program reads for nothing it draws, left at zero: an unlit
// zero material's packed point lights (materials.js), which its vertex
// shader adds up into a value its fragment shader never reads (three sets
// them to whatever the last lit draw chose). At zero, the loop over them
// does not run.
const unread = (material, name) => (name === "zeroLights" || name === "zeroLightCount") && Boolean(material?.defines) && "UNLIT" in material.defines;
// attributes: "name:location,..." where the pass's vertex arrays have them,
// or null for three's own attributes at locations 0, 1... (entry.attributes,
// for core.geometry).
export function plainProgram(core, material, attributes) {
  if (ownsEffectSource(material)) {
    const entry = ownedEffectProgram(core, material, PLAIN);
    if (attributes !== null && entry.attributes.map(([n,l]) => `${n}:${l}`).join(",") !== attributes) throw Error(`Attribute layout for ${entry.key}: ${attributes}`);
    return entry;
  }
  if (material.userData?.bfeProgram) return ownedProgram(core, material.userData.bfeProgram, attributes);
  const threeProgram = core.renderer.properties.get(material).currentProgram;
  if (!threeProgram) return undefined;
  let entry = plainPrograms.get(threeProgram);
  if (entry !== undefined) return entry;
  entry = null;
  const gl = core.gl, p = threeProgram.program;
  let locations = null;
  if (attributes === null) {
    locations = [];
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) locations.push([gl.getActiveAttrib(p, i).name, i]);
    attributes = locations.map(([name, location]) => `${name}:${location}`).join(",");
  }
  const uniforms = [];
  for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
    const u = gl.getActiveUniform(p, i), name = u.name.replace(/\[0\]$/, "");
    if (!unread(material, name)) uniforms.push({ name, type: uniformType(gl, u.type), size: u.size });
  }
  if (uniforms.every((u) => u.type && u.size === 1)) {
    const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
    try {
      entry = { id: core.rust.add_program(PLAIN, gl.getShaderSource(vs), gl.getShaderSource(fs), attributes, uniforms.map((u) => u.name).join(",")), uniforms, attributes: locations };
    } catch (error) {
      core.fail(error);
    }
  } else core.errors.push(`plain p${threeProgram.id}: ${uniforms.filter((u) => !u.type || u.size !== 1).map((u) => u.name).join(" ")}`);
  plainPrograms.set(threeProgram, entry);
  return entry;
}

// A uniform's value as three would set it for a full-screen pass (the
// object's and camera's matrices, else the material's), into f64 slots at
// `o` (slot, type, values); false when it has none the renderer can give.
const eye = new THREE.Vector3();
const IDENTITY3 = new THREE.Matrix3();
// A map's uv transform, as refreshTransformUniform sets it.
const transform = (map) => { if (!map) return IDENTITY3; if (map.matrixAutoUpdate) map.updateMatrix(); return map.matrix; };
// The uniforms of a built-in material three's refreshUniformsCommon sets
// that a basic material's program may read.
const BUILTIN = {
  diffuse: (m) => m.color,
  opacity: (m) => m.opacity,
  map: (m) => m.map,
  mapTransform: (m) => transform(m.map),
  alphaMap: (m) => m.alphaMap,
  alphaMapTransform: (m) => transform(m.alphaMap),
  alphaTest: (m) => m.alphaTest,
};
export function writeUniform(core, out, o, slot, uniform, material, object, camera) {
  const { name, type } = uniform;
  let value;
  if (name === "projectionMatrix") value = camera.projectionMatrix;
  else if (name === "modelViewMatrix") value = object.modelViewMatrix.multiplyMatrices(camera.matrixWorldInverse, object.matrixWorld);
  else if (name === "modelMatrix") value = object.matrixWorld;
  else if (name === "normalMatrix") value = object.normalMatrix.getNormalMatrix(object.modelViewMatrix.multiplyMatrices(camera.matrixWorldInverse, object.matrixWorld));
  else if (name === "viewMatrix") value = camera.matrixWorldInverse;
  else if (name === "cameraPosition") value = eye.setFromMatrixPosition(camera.matrixWorld);
  else if (name === "isOrthographic") value = camera.isOrthographicCamera === true;
  else if (material && !material.uniforms) {
    // A built-in material (MeshBasicMaterial): its values as three's
    // refreshUniforms sets them.
    const read = BUILTIN[name];
    if (!read) return false;
    value = read(material);
    if (value === undefined) return false;
  } else {
    const u = material?.uniforms?.[name];
    if (!u) return false;
    value = u.value;
  }
  out[o] = slot; out[o + 1] = type;
  if (type === SAMPLER || type === SAMPLER_3D) {
    // None, or a texture three has no pixels for (a target never drawn, an
    // image still loading): three's empty texture, which these passes only
    // read behind a switch that is off (the composite's glow without bloom).
    if (value === null || value === undefined) { out[o + 2] = -1; return true; }
    if (!value.isTexture) return false;
    // Registration precedes record writes. A repeated sampler read must not
    // allocate storage or detach the shared input view halfway through it.
    const sample=core.sampleTextures.get(value);
    if(!sample)throw new Error(`Sampler ${name} was not prepared before writing uniforms`);
    out[o + 2] = sample.id;
    return true;
  }
  if (typeof value === "number") { out[o + 2] = value; return true; }
  if (typeof value === "boolean") { out[o + 2] = value ? 1 : 0; return true; }
  if (value?.isMatrix4 || value?.isMatrix3) { for (let k = 0; k < value.elements.length; k++) out[o + 2 + k] = value.elements[k]; return true; }
  if (value?.isVector2) { out[o + 2] = value.x; out[o + 3] = value.y; return true; }
  if (value?.isVector3) { out[o + 2] = value.x; out[o + 3] = value.y; out[o + 4] = value.z; return true; }
  if (value?.isVector4) { out[o + 2] = value.x; out[o + 3] = value.y; out[o + 4] = value.z; out[o + 5] = value.w; return true; }
  if (value?.isColor) { out[o + 2] = value.r; out[o + 3] = value.g; out[o + 4] = value.b; return true; }
  return false;
}

class PostPass {
  constructor(core) {
    this.core = core; this.name = "post";
    this.targets = new Map(); this.programIds = new Uint32Array(POST_KEYS.length);
    this.ready = false; this.revision = -1; this.epoch = -1; this.depth = null;
    this.viewport = new THREE.Vector4();
    core.addView("postFacts", Float64Array, () => core.rust.post_chain_input_ptr(), () => core.rust.post_chain_input_len());
    core.addView("postState", Float64Array, () => core.rust.post_chain_state_ptr(), () => core.rust.post_chain_state_len());
    core.addView("postStats", Float64Array, () => core.rust.post_chain_state_ptr() + STATE.STATS * Float64Array.BYTES_PER_ELEMENT, () => STATE.LEN - STATE.STATS);
    core.addView("postInput", Float64Array, () => core.rust.post_input_ptr(), () => core.rust.post_input_len());
  }

  // Programs are finite cold resources. Paced loading yields between links;
  // synthetic callers of Graphics.warmup use the same synchronous registrar.
  programs() {
    if (this.ready) return;
    for (let i = 0; i < POST_KEYS.length; i++) this.programIds[i] = ownedProgram(this.core, POST_KEYS[i], ATTRIBUTES).id;
    this.bindPrograms();
  }
  async warm(settle) {
    if (this.ready) return;
    for (let i = 0; i < POST_KEYS.length; i++) {
      this.programIds[i] = ownedProgram(this.core, POST_KEYS[i], ATTRIBUTES).id;
      await settle();
    }
    this.bindPrograms();
  }
  bindPrograms() {
    const core = this.core;
    core.views(); core.postFacts.set(this.programIds, FACTS.PROGRAMS);
    this.ready = true; core.stale = true;
    core.graphics.upscaler.core = core;
  }

  // Resource ids change only at cold admission, resize or context restoration.
  // Finish every registration before acquiring the shared facts view.
  resource(target, row) {
    const core = this.core, id = this.target(target), texture = core.texture(target.texture);
    core.views(); const out = core.postFacts, at = FACTS.RESOURCES + row * RESOURCE.STRIDE;
    out[at + RESOURCE.TEXTURE] = texture; out[at + RESOURCE.TARGET] = id;
    out[at + RESOURCE.WIDTH] = target.width; out[at + RESOURCE.HEIGHT] = target.height;
  }
  resources() {
    const core = this.core, g = core.graphics, u = g.upscaler, epoch = core.resources.gpu.epoch();
    if (this.revision === g.postRevision && this.epoch === epoch) return;
    this.resource(g.sceneTarget, RESOURCE.SCENE); this.resource(g.quarterTarget, RESOURCE.QUARTER);
    this.resource(g.bloomTarget, RESOURCE.BLOOM); this.resource(g.bloomScratch, RESOURCE.SCRATCH);
    this.resource(g.postTarget, RESOURCE.POST); this.resource(g.aaTarget, RESOURCE.AA);
    this.resource(u.history[0], RESOURCE.HISTORY0); this.resource(u.history[1], RESOURCE.HISTORY1);
    this.resource(u.velocity, RESOURCE.VELOCITY); this.resource(u.motion, RESOURCE.MOTION);
    this.revision = g.postRevision; this.epoch = epoch; this.depth = null;
    core.stale = true;
  }

  // One native temporal preparation publishes jitter and current/previous
  // matrices. The scene/motion hosts only copy those published camera facts.
  begin(camera) {
    const core = this.core, g = core.graphics, u = g.upscaler;
    this.programs(); this.resources(); core.views();
    const f = core.postFacts, aa = g.settings.get("antialiasing");
    f[FACTS.MODE] = MODES[g.upscaling]; f[FACTS.AA] = aa === "off" ? AA.OFF : aa === "fxaa-fast" ? AA.FAST : AA.FXAA;
    f[FACTS.SOFT_FOCUS] = g.settings.get("softFocus") ? 1 : 0;
    f[FACTS.BLUR_ENABLED] = g.blur ? 1 : 0; f[FACTS.BLUR_BLEND] = g.blur ? g.blur.blend : 0;
    f[FACTS.BLOOM] = g.settings.get("bloom") ? 1 : 0; f[FACTS.SPEED_ENABLED] = g.settings.get("vehicleMotionBlur") ? 1 : 0;
    f[FACTS.SPEED_AMOUNT] = g.vehicleMotion; f[FACTS.SPEED_TURN] = g.vehicleTurn; f[FACTS.SHARPNESS] = u.sharpness;
    f[FACTS.RESET_SERIAL] = u.resetSerial; f[FACTS.RESET_FRAME] = u.resetFrame; f[FACTS.RESET_READ] = u.resetRead;
    f[FACTS.INPUT_WIDTH] = g.renderSize.x; f[FACTS.INPUT_HEIGHT] = g.renderSize.y;
    f[FACTS.OUTPUT_WIDTH] = g.outputSize.x; f[FACTS.OUTPUT_HEIGHT] = g.outputSize.y;
    core.renderer.getViewport(this.viewport).multiplyScalar(core.renderer.getPixelRatio()).floor();
    f[FACTS.CANVAS_WIDTH] = this.viewport.z; f[FACTS.CANVAS_HEIGHT] = this.viewport.w;
    f.set(camera.projectionMatrix.elements, FACTS.PROJECTION); f.set(camera.matrixWorldInverse.elements, FACTS.VIEW); f.set(camera.matrixWorld.elements, FACTS.WORLD);
    core.rust.post_chain_begin_frame();
    const s = core.postState, current = u.current.elements, previous = u.previous.elements;
    for (let i = 0; i < 16; i++) { current[i] = s[STATE.CURRENT+i]; previous[i] = s[STATE.PREVIOUS+i]; }
    u.jitter.set(s[STATE.JITTER], s[STATE.JITTER+1]);
  }

  // Draw and commit form one native phase. Statistics borrow retained memory
  // instead of allocating a context object, skeleton map or bindgen result.
  cover() {
    const core = this.core, depth = core.graphics.upscaler.depth;
    if (core.contextLost) return true;
    if (depth && depth !== this.depth) {
      const texture = core.texture(depth); core.views();
      core.postFacts[FACTS.RESOURCES + RESOURCE.DEPTH * RESOURCE.STRIDE + RESOURCE.TEXTURE] = texture;
      this.depth = depth;
    }
    const started = performance.now();
    if (!core.rust.post_chain_frame()) throw new Error(core.rust.post_chain_error_frame());
    core.views();
    core.phaseStats.post = core.postStats; core.phaseMs.post = performance.now() - started;
    if (core.postStats[4]) core.stale = true;
    return true;
  }

  // Auxiliary screen/replay draws share the existing Rust framebuffer wrapper.
  target(target) {
    const core = this.core;
    core.renderer.initRenderTarget(target);
    const handle = core.renderer.properties.get(target.texture).__webglTexture;
    if (!handle) throw Error("Post target allocation failed");
    let id = this.targets.get(handle);
    if (id === undefined) {
      const texture = core.texture(target.texture);
      id = texture < 0 ? -1 : core.rust.post_target(texture);
      if (id >= 0) this.targets.set(handle, id);
    }
    return id;
  }
}

export function postPass(core) { return new PostPass(core); }
