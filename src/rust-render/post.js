// The post pass (post.rs): the frame's last passes over the near scene's
// image as graphics.js plans them (Graphics.postSteps): the soft focus's
// quarter-size copy, bloom's extract and two blurs, the composite, FXAA or
// FXAA (FAST), and the upscale (bilinear, spatial, or the temporal
// upscaler's motion resolve, resolve and sharpen). Each step is a
// full-screen pass (render-passes.js) into a target (three's, or the
// canvas), with the values it is drawn with (applyValues: set on its
// material as the step is written, as three's postStep sets them); the
// renderer uses source-owned programs for tagged post/HUD materials and the
// compiled-source bridge for unported materials (Kind::Plain). A refused step
// stops the battle. The temporal commit (`after`) runs once per completed chain.
import * as THREE from "three";
import { ownedProgram } from "./programs.js";
import { applyValues } from "../render-passes.js";

const PLAIN = 3;
// post.rs's step layout.
const STEP = { PROGRAM: 0, TARGET: 1, WIDTH: 2, HEIGHT: 3, UNIFORMS: 4, FIRST: 5 };
const UNIFORM = 18, MAX_UNIFORMS = 12, STRIDE = 5 + MAX_UNIFORMS * UNIFORM, MAX_STEPS = 8;
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
    const p = core.renderer.properties.get(value);
    // An image three would upload as it draws.
    if (!value.isRenderTargetTexture && value.version > 0 && p.__version !== value.version && value.image) core.renderer.initTexture(value);
    out[o + 2] = p.__webglTexture ? core.texture(value) : -1;
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
    this.core = core;
    this.name = "post";
    this.phase = "post";
    this.targets = new Map(); // WebGLTexture of a target -> post.rs target
    this.pending = null;
    this.complete = false;
    this.input = false;
  }

  // graphics.js: the frame's post steps ({ pass, target, camera, values }
  // each, or { run } for one only three can draw), drawn here when every one
  // is a pass the renderer has a program for; true when it drew them.
  cover(renderer, steps) {
    const core = this.core;
    if (!steps.length || steps.length > MAX_STEPS) return false;
    for (const step of steps) {
      if (!step.pass || !ownedProgram(core, step.pass.material.userData.bfeProgram, ATTRIBUTES)) return false;
      if (step.target && this.target(step.target) < 0) return false;
    }
    this.pending = steps;
    this.complete = false;
    core.phase("post", renderer, null, steps[0].camera, true);
    this.pending = null;
    return this.complete;
  }


  // A target of three's, by its colour texture (made again on a resize).
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

  // The phase: the steps written for post.rs.
  draw(ctx) {
    const steps = this.pending;
    if (!steps) return;
    const core = this.core;
    if (!this.input) { core.addView("postInput", Float64Array, () => core.rust.post_input_ptr(), () => core.rust.post_input_len()); this.input = true; }
    core.views();
    const out = core.postInput, size = this.size ||= new THREE.Vector4();
    let n = 0;
    for (const step of steps) {
      const program = ownedProgram(core, step.pass.material.userData.bfeProgram, ATTRIBUTES);
      const o = n * STRIDE;
      if (step.values) applyValues(step.pass.material, step.values);
      out[o + STEP.PROGRAM] = program.id;
      if (step.target) { out[o + STEP.TARGET] = this.target(step.target); out[o + STEP.WIDTH] = step.target.width; out[o + STEP.HEIGHT] = step.target.height; }
      else {
        // The canvas: three's viewport, the drawing buffer's pixels.
        core.renderer.getViewport(size).multiplyScalar(core.renderer.getPixelRatio()).floor();
        out[o + STEP.TARGET] = -1; out[o + STEP.WIDTH] = size.z; out[o + STEP.HEIGHT] = size.w;
      }
      if (program.uniforms.length > MAX_UNIFORMS) return;
      out[o + STEP.UNIFORMS] = program.uniforms.length;
      for (let k = 0; k < program.uniforms.length; k++) {
        if (!writeUniform(core, out, o + STEP.FIRST + k * UNIFORM, k, program.uniforms[k], step.pass.material, step.pass, step.camera)) return;
      }
      n++;
    }
    ctx.steps = n;
    this.complete = true;
  }
}

export function postPass(core) {
  return new PostPass(core);
}
