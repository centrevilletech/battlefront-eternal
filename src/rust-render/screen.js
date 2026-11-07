// The screen pass (post.rs draw_screen): the full-screen passes outside the
// post chain that the renderer draws in three's place. Each is one draw of a
// full-screen pass's material with a catalog-owned Plain program,
// written as a post step with its material's own
// state (post.rs `states`: its depth test and function, depth write and
// blending) and, where a varying must be interpolated over three's own
// triangles, three's geometry for it.
//
import * as THREE from "three";
import { ownedEffectProgram } from "./effect-material.js";
import { ownsEffectSource } from "./programs.js";
import { plainProgram, writeUniform } from "./post.js";
import { blendOf, stateOf } from "./transparent.js";

// post.rs's step and state layouts.
const STEP = { PROGRAM: 0, TARGET: 1, WIDTH: 2, HEIGHT: 3, UNIFORMS: 4, FIRST: 5 };
const UNIFORM = 18, MAX_UNIFORMS = 12, STRIDE = STEP.FIRST + MAX_UNIFORMS * UNIFORM;
export const TARGET_CANVAS = -1, TARGET_BOUND = -2;
const STATE = { FLAGS: 0, DEPTH_FUNC: 1, EQUATIONS: 2, FACTORS: 4, GEOMETRY: 8, SIZE: 10 };
const DEPTH_TEST = 1, DEPTH_WRITE = 2, BLEND = 4, OWN_GEOMETRY = 8;
// A material's state flags (transparent.js stateOf): colour written.
const COLOR_WRITE = 4;
// Where post.rs's triangle has its position and uv.
export const ATTRIBUTES = "position:0,uv:1";

// The post pass's steps and their states, as views of the renderer's memory.
const viewed = new WeakSet();
function views(core) {
  if (!viewed.has(core)) {
    core.addView("postInput", Float64Array, () => core.rust.post_input_ptr(), () => core.rust.post_input_len());
    core.addView("postStates", Float64Array, () => core.rust.post_states_ptr(), () => core.rust.post_states_len());
    viewed.add(core);
  }
  core.views();
}

// screenProgram validates an owned layout against the screen-step limits.
// No stencil, polygon offset or alpha coverage can enter this draw contract;
// registration reports unsupported state before a native phase submits it.
export function screenProgram(core, material, attributes = ATTRIBUTES) {
  const program = plainProgram(core, material, attributes);
  if (!program) return program;
  const state = stateOf(material);
  if (program.uniforms.length > MAX_UNIFORMS || !state || !(state.flags & COLOR_WRITE) || !blendOf(material)) return null;
  return program;
}

// Step k: the material drawn with `program` into `target` (TARGET_BOUND: the
// framebuffer bound now, its viewport as three set it; TARGET_CANVAS; or a
// post target, `size` [width, height]), every uniform it reads as three
// would set it drawing `object` from `camera`, and its state; geometry:
// { id, count }, a registered geometry and its index count, drawn in place
// of post.rs's triangle. False when a uniform has no value the renderer can
// give (nothing of the step is to be drawn then).
export function writeScreenStep(core, k, program, material, object, camera, { target = TARGET_BOUND, size = null, geometry = null, buffer = "postInput", offset = 0, stateBuffer = "postStates", stateOffset = 0 } = {}) {
  // The textures it reads registered first: registering can grow the
  // renderer's memory, which detaches the views written below.
  const renderer = core.renderer;
  for (const u of program.uniforms) {
    if (u.type < 32) continue;
    const value = material.uniforms?.[u.name]?.value;
    if (!value?.isTexture) continue;
    const p = renderer.properties.get(value);
    if (!value.isRenderTargetTexture && value.version > 0 && p.__version !== value.version && value.image) renderer.initTexture(value);
    core.texture(value);
  }
  views(core);
  const out = core[buffer], o = offset + k * STRIDE;
  out[o + STEP.PROGRAM] = program.id;
  out[o + STEP.TARGET] = target;
  out[o + STEP.WIDTH] = size?.[0] ?? 0;
  out[o + STEP.HEIGHT] = size?.[1] ?? 0;
  out[o + STEP.UNIFORMS] = program.uniforms.length;
  for (let i = 0; i < program.uniforms.length; i++) {
    if (!writeUniform(core, out, o + STEP.FIRST + i * UNIFORM, i, program.uniforms[i], material, object, camera)) return false;
  }
  // Resource commands share this heap. Refresh state views after binding
  // uniforms as well, before addressing the independent state record.
  core.views();
  const state = stateOf(material), blend = blendOf(material), s = core[stateBuffer], at = stateOffset + k * STATE.SIZE;
  s.fill(0, at, at + STATE.SIZE);
  s[at + STATE.FLAGS] = (state.flags & 1 ? DEPTH_TEST : 0) | (state.flags & 2 ? DEPTH_WRITE : 0) | (blend[0] ? BLEND : 0) | (geometry ? OWN_GEOMETRY : 0);
  if (state.flags & 1) s[at + STATE.DEPTH_FUNC] = state.func;
  if (blend[0]) {
    s[at + STATE.EQUATIONS] = blend[1]; s[at + STATE.EQUATIONS + 1] = blend[2];
    for (let f = 0; f < 4; f++) s[at + STATE.FACTORS + f] = blend[3 + f];
  }
  if (geometry) { s[at + STATE.GEOMETRY] = geometry.id; s[at + STATE.GEOMETRY + 1] = geometry.count; }
  return true;
}

// One-mesh screen passes outside the post chain: storm clouds and the Low
// underwater tint. Compile the material before registration, then draw through
// Rust into the currently selected target without clearing it.
class ScreenPass {
  constructor(core, name, phase) {
    this.core = core;
    this.name = name;
    this.phase = phase;
    this.complete = false;
    this.drawn = 0;
    this.size = null;
  }

  // The one mesh three's render would draw: the mesh rendered itself, or a
  // scene's one shown mesh on the camera's layers (never culled).
  mesh(scene, camera) {
    if (scene.isMesh) return scene.visible && scene.material?.visible ? scene : null;
    let found = null;
    for (const child of scene.children) {
      if (!child.visible) continue;
      if (!child.isMesh || child.children.length || found || !child.layers.test(camera.layers) || child.frustumCulled) return null;
      if (child.material?.visible) found = child;
    }
    return found;
  }

  draw(ctx) {
    this.complete = false;

    const core = this.core, renderer = ctx.renderer;
    const mesh = this.mesh(ctx.scene, ctx.camera);
    if (!mesh || Array.isArray(mesh.material) || mesh.geometry.groups.length || mesh.isInstancedMesh || mesh.isSkinnedMesh) return;
    // Screen materials are catalog programs from the moment they are made.
    if (!ownsEffectSource(mesh.material)) { core.fail(Error(`Screen material ${mesh.name} has no catalog program`)); return; }
    const program = screenProgram(core, mesh.material);
    if (!program) return;
    const target = renderer.getRenderTarget();
    let id = TARGET_CANVAS, size;
    if (target) {
      id = core.byName.post?.target(target) ?? -1;
      if (id < 0) return;
      size = [target.width, target.height];
    } else {
      // The canvas: three's viewport, the drawing buffer's pixels.
      const v = renderer.getViewport(this.size ??= new THREE.Vector4()).multiplyScalar(renderer.getPixelRatio()).floor();
      size = [v.z, v.w];
    }
    if (!writeScreenStep(core, 0, program, mesh.material, mesh, ctx.camera, { target: id, size })) return;
    ctx.steps = 1;
    this.complete = true;
    this.drawn++;
  }

}

export function cloudsPass(core) {
  return new ScreenPass(core, "clouds", "skyClouds");
}

export function underwaterPass(core) {
  return new ScreenPass(core, "underwater", "underwater");
}

// Water.captureSurface's fog composite follows the native transparent list.
// Its preparation records samplers and geometry; the frame owner copies the
// completed scene immediately before drawing it. No render hook resumes JS.
class FogPass {
  constructor(core) {
    this.core = core;
    this.name = "fog";
    this.phase = "transparent";
    // Three's programs for the composite's material the renderer has made
    // its own of, with its geometry: three program -> { id, count }.
    this.made = new WeakMap();
    this.pending = false;
    this.drawn = 0;
    this.missed = 0;
  }

  register() {
    const scene = this.core.graphics?.scene, pass = this;
    if (!scene) return;
    const previous = scene.onBeforeRender;
    scene.onBeforeRender = function (...args) {
      previous.apply(this, args);
      pass.place(...args);
    };
  }

  composite() {
    return this.core.graphics?.scene?.userData.water?.underwaterFog ?? null;
  }

  beforeSort() {
    const core = this.core, fog = this.composite();
    if (!fog) return;
    if (!ownsEffectSource(fog.material)) { core.fail(Error("Fog material has no catalog program")); return; }
    core.registry.claim(fog);
    const current = fog.material;
    if (this.made.has(current)) return;
    const program = screenProgram(core, fog.material);
    if (!program) throw new Error("Missing Rust underwater fog program");
    const geometry = core.geometry(fog.geometry, { id: program.id, attributes: [["position", 0], ["uv", 1]] });
    if (geometry === null || geometry === undefined) throw new Error("Missing Rust underwater fog geometry");
    this.made.set(current, { id: geometry, count: fog.geometry.index.count });
    core.stale = true;
  }

  // As three starts a render of the near scene: the frame's own, the
  // composite shown, the renderer on in the battle's configuration and its
  // program the renderer's: three leaves it out.
  place(renderer, scene, camera, target) {
    const core = this.core, graphics = core.graphics, fog = this.composite();
    this.pending = false;
    if (!fog || scene !== graphics.scene) return;
    if (ownsEffectSource(fog.material) && !this.made.has(fog.material)) {
      const program = screenProgram(core, fog.material);
      const geometry = core.geometry(fog.geometry, program);
      if (geometry === null || geometry === undefined) throw Error("Owned underwater geometry failed to register");
      this.made.set(fog.material, { id: geometry, count: fog.geometry.index.count });
      renderer.resetState(); renderer.setRenderTarget(target);
    }
    const take = !core.contextLost && fog.visible && fog.parent === scene && camera === graphics.camera
      && target === graphics.sceneTarget && this.made.has(fog.material);
    core.registry.claim(fog);
    if (take) this.pending = true;
  }

  // Fog uniforms are prepared with the scene. The native schedule copies
  // the completed transparent surface immediately before this packet draws.
  draw(ctx) {
    const core=this.core,fog=this.composite();
    if(!this.pending || !fog || ctx.camera!==core.graphics.camera)return;
    this.pending=false;
    const water=core.graphics.scene.userData.water;
    // The fog reads the completed transparent scene. Ordinary water readers
    // request their earlier opaque snapshot through the same descriptor hook.
    core.frame.preparingFog=true;
    try{water.captureSurface(ctx.renderer,ctx.camera);}finally{core.frame.preparingFog=false;}
    const program=screenProgram(core,fog.material),geometry=this.made.get(fog.material);
    if(program && geometry && writeScreenStep(core,0,program,fog.material,fog,ctx.camera,{geometry}))core.frame.screenFog();
  }


}

export function fogPass(core) {
  return new FogPass(core);
}
