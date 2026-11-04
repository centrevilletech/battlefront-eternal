// The screen pass (post.rs draw_screen): the full-screen passes outside the
// post chain that the renderer draws in three's place. Each is one draw of a
// full-screen pass's material with three's program for it (Kind::Plain, made
// as three compiled it), written as a post step with its material's own
// state (post.rs `states`: its depth test and function, depth write and
// blending) and, where a varying must be interpolated over three's own
// triangles, three's geometry for it.
//
// The far scene's image (graphics.background) is the first: three draws it
// last of the near scene's opaque objects, at the far plane and depth tested
// (less or equal), so it fills only the samples nothing opaque covered. The
// renderer draws it at that very point: first in the transparent phase (or a
// mirror's), which starts right after three's last opaque item. Three leaves
// it out of a render of the near scene whose transparent phase draws it:
// place() decides as three starts the render (the scene's onBeforeRender,
// before three lists anything), with the near hook's own test (the frame's
// camera into the scene's target, or a mirror's camera) and the renderer's
// state; any other render of the near scene (the fountain's reflection)
// keeps three's. A near scene drawn whole in three's place (near.js) has its
// image already (its begin phase).
import * as THREE from "three";
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

// Rust's program for a full-screen material (post.js plainProgram), when a
// step can draw it: no more uniforms than a step holds, and a state a step
// has (no stencil, polygon offset or alpha to coverage, its colour written,
// a blending the renderer sets). undefined until three has made its
// program; null when never. Making it can make a program of the renderer's:
// only inside a phase, whose end resets three's state.
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
export function writeScreenStep(core, k, program, material, object, camera, { target = TARGET_BOUND, size = null, geometry = null } = {}) {
  // The textures it reads registered first: registering can grow the
  // renderer's memory, which detaches the views written below.
  const renderer = core.renderer;
  for (const u of program.uniforms) {
    if (u.type < 32) continue;
    const value = material.uniforms?.[u.name]?.value;
    if (!value?.isTexture) continue;
    const p = renderer.properties.get(value);
    if (!value.isRenderTargetTexture && value.version > 0 && p.__version !== value.version && value.image) renderer.initTexture(value);
    if (p.__webglTexture) core.texture(value);
  }
  views(core);
  const out = core.postInput, o = k * STRIDE;
  out[o + STEP.PROGRAM] = program.id;
  out[o + STEP.TARGET] = target;
  out[o + STEP.WIDTH] = size?.[0] ?? 0;
  out[o + STEP.HEIGHT] = size?.[1] ?? 0;
  out[o + STEP.UNIFORMS] = program.uniforms.length;
  for (let i = 0; i < program.uniforms.length; i++) {
    if (!writeUniform(core, out, o + STEP.FIRST + i * UNIFORM, i, program.uniforms[i], material, object, camera)) return false;
  }
  const state = stateOf(material), blend = blendOf(material), s = core.postStates, at = k * STATE.SIZE;
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

// The far scene's image, drawn first in the transparent phase of a render
// of the near scene that leaves it out (place()).
class ImagePass {
  constructor(core) {
    this.core = core;
    this.name = "image";
    this.phase = "transparent";
    this.on = true;
    // Three's programs for the image's material the renderer has made its
    // own of (the image is left out of three's renders from the next one).
    this.made = new WeakSet();
    // The render whose transparent phase draws the image: its camera and
    // target, as three started it.
    this.pending = null;
    // For the measurement tools: images drawn, and transparent phases of a
    // render the image was left out of that could not draw it (should stay 0).
    this.drawn = 0;
    this.missed = 0;
  }

  // As three starts each render of the near scene, before it lists
  // anything (chained to the scene's onBeforeRender, as particle-depth.js
  // and point-lights.js are).
  register() {
    const scene = this.core.graphics?.scene, pass = this;
    if (!scene) return;
    const previous = scene.onBeforeRender;
    scene.onBeforeRender = function (...args) {
      previous.apply(this, args);
      pass.place(...args);
    };
  }

  // Whether this render's transparent phase draws the image: a render the
  // near hook draws in (index.js pass(): the frame's camera into the scene's
  // target, or a mirror's camera), the renderer on and covering the battle's
  // configuration, and a program of its own made for three's (draw()). Then
  // three leaves the image out of its list; else three draws it.
  place(renderer, scene, camera, target) {
    const core = this.core, graphics = core.graphics, background = graphics.background, material = background?.material;
    this.pending = null;
    if (!material || scene !== graphics.scene) return;
    const ours = (camera === graphics.camera && target === graphics.sceneTarget) || camera.userData?.mirror === true;
    const take = ours && this.on && core.enabled && core.active && !core.lost && background.parent === scene && background.visible
      && core.supported() && this.made.has(renderer.properties.get(material).currentProgram);
    material.visible = !take;
    if (take) this.pending = { camera, target };
  }

  // The transparent phase (a mirror's too): the image's step, first, when
  // place() left it out of this render. Three's program for it is made the
  // renderer's here, inside a phase (whose end resets three's state).
  draw(ctx) {
    const core = this.core, background = core.graphics.background, material = background?.material;
    const pending = this.pending;
    this.pending = null;
    if (!material) return;
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (threeProgram && !this.made.has(threeProgram)) {
      const program = screenProgram(core, material);
      if (program) { this.made.add(threeProgram); ctx.reset = true; }
    }
    if (!pending || ctx.walked || ctx.camera !== pending.camera || ctx.renderer.getRenderTarget() !== pending.target) {
      if (pending && !ctx.walked) this.missed++;
      return;
    }
    const program = screenProgram(core, material);
    if (!program || !writeScreenStep(core, 0, program, material, background, ctx.camera)) {
      // Left out and not drawn: three draws it from the next render on.
      this.missed++;
      this.made.delete(threeProgram);
      return;
    }
    // Opaque-scene copies made by the effects and water passes below must
    // already contain the background, as they do after three draws it.
    core.phase("image", ctx.renderer, ctx.scene, ctx.camera, ctx.defer, null, { steps: 1 });
    this.drawn++;
  }

  // Off (this pass alone): three draws the image.
  setActive(on) {
    this.on = on;
    if (!on) this.show();
  }

  restore() {
    this.on = false;
    this.show();
  }

  show() {
    this.pending = null;
    const material = this.core.graphics?.background?.material;
    if (material) material.visible = true;
  }
}

export function imagePass(core) {
  return new ImagePass(core);
}

// A full-screen pass three renders on its own (renderer.render of the mesh,
// or of a scene holding it), drawn by the renderer in a phase of its own
// (graphics' callers ask the core to cover it): the storm's cloud panorama
// into its target (storm-sky.js, phase skyClouds), the Low water's tint over
// the finished frame with the eye under the surface (water.js, phase
// underwater). Complete when three would draw one mesh, whose material a
// step can draw (its program made, three having drawn it once), into the
// target bound now (a target of three's, or the canvas), without a clear of
// three's.
class ScreenPass {
  constructor(core, name, phase) {
    this.core = core;
    this.name = name;
    this.phase = phase;
    this.on = true;
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
    if (!this.on) return;
    const core = this.core, renderer = ctx.renderer;
    if (renderer.autoClear) return;
    const mesh = this.mesh(ctx.scene, ctx.camera);
    if (!mesh || Array.isArray(mesh.material) || mesh.geometry.groups.length || mesh.isInstancedMesh || mesh.isSkinnedMesh) return;
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

  // Off (this pass alone): three draws it.
  setActive(on) {
    this.on = on;
  }
}

export function cloudsPass(core) {
  return new ScreenPass(core, "clouds", "skyClouds");
}

export function underwaterPass(core) {
  return new ScreenPass(core, "underwater", "underwater");
}

// The Enhanced water's underwater composite (water.js underwaterFog, shown
// with the eye under the surface at High and Ultra): three draws it last of
// the near scene's transparent list (its render order the list's highest),
// right after its hook copies the scene drawn so far (water.captureSurface).
// The renderer draws it at that very point: the transparent pass runs the
// pass's atEnd after the list's last draw (its own last segment, or three's
// last item), where the copy is made as three makes it, then the composite
// drawn (the underwater phase) with three's program for its material and
// three's own geometry for it (its uv interpolated as three's are). Three
// leaves it out of a render whose transparent phase draws it (place(), as
// three starts the render); until three has drawn it once, and in any other
// render, three draws it.
class FogPass {
  constructor(core) {
    this.core = core;
    this.name = "fog";
    this.phase = "transparent";
    this.on = true;
    // Three's programs for the composite's material the renderer has made
    // its own of, with its geometry: three program -> { id, count }.
    this.made = new WeakMap();
    this.pending = null;
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

  // As three starts a render of the near scene: the frame's own, the
  // composite shown, the renderer on in the battle's configuration and its
  // program the renderer's: three leaves it out.
  place(renderer, scene, camera, target) {
    const core = this.core, graphics = core.graphics, fog = this.composite();
    this.pending = null;
    if (!fog || scene !== graphics.scene) return;
    const take = this.on && core.enabled && core.active && !core.lost && fog.visible && fog.parent === scene && camera === graphics.camera
      && target === graphics.sceneTarget && core.supported() && this.made.has(renderer.properties.get(fog.material).currentProgram);
    fog.material.visible = !take;
    if (take) this.pending = { camera, target };
  }

  // The transparent phase: three's program for the composite made the
  // renderer's (inside a phase), and, where place() left it out, the list's
  // end set to draw it.
  draw(ctx) {
    const core = this.core, fog = this.composite(), pending = this.pending;
    this.pending = null;
    if (!fog) return;
    const threeProgram = core.renderer.properties.get(fog.material).currentProgram;
    if (threeProgram && !this.made.has(threeProgram)) {
      const program = screenProgram(core, fog.material);
      const geometry = program && fog.geometry.index && !fog.geometry.groups.length ? core.geometry(fog.geometry, { id: program.id, attributes: [["position", 0], ["uv", 1]] }) : null;
      if (geometry !== null && geometry !== undefined) { this.made.set(threeProgram, { id: geometry, count: fog.geometry.index.count }); ctx.reset = true; }
    }
    if (!pending || ctx.camera !== pending.camera || ctx.renderer.getRenderTarget() !== pending.target) return;
    core.byName.transparent.atEnd = () => this.drawComposite(ctx.renderer, ctx.scene, ctx.camera);
  }

  // The list's end: the copy, as the composite's hook makes it, then the
  // composite.
  drawComposite(renderer, scene, camera) {
    const core = this.core, fog = this.composite(), water = core.graphics.scene.userData.water;
    if (!fog || !water || !core.enabled || !core.active) { this.missed++; return; }
    water.captureSurface(renderer, camera);
    const program = screenProgram(core, fog.material), geometry = this.made.get(renderer.properties.get(fog.material).currentProgram);
    if (!program || !geometry || !writeScreenStep(core, 0, program, fog.material, fog, camera, { geometry })) { this.missed++; return; }
    core.phase("underwater", renderer, scene, camera, false, null, { steps: 1 });
    this.drawn++;
  }

  // Off (this pass alone): three draws it.
  setActive(on) {
    this.on = on;
    if (!on) this.show();
  }

  restore() {
    this.on = false;
    this.show();
  }

  show() {
    this.pending = null;
    const fog = this.composite();
    if (fog) fog.material.visible = true;
  }
}

export function fogPass(core) {
  return new FogPass(core);
}
