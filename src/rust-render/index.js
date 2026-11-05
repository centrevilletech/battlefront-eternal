// Rust draws every battle phase. Three supplies scene updates, compiled GLSL,
// textures and targets. This core registers those resources, publishes frame
// inputs, and treats incomplete coverage or a failed program as a fatal error.
import { rendererFailure } from "./failure.js";
import * as THREE from "three";
import { lighting } from "../materials.js";
import { packed } from "../point-lights.js";
import { worldPass } from "./world.js";
import { instancesPass, compileFor } from "./instances.js";
import { farPass, seaPass } from "./far.js";
import { terrainPass } from "./terrain.js";
import { transparentPass } from "./transparent.js";
import { effectsPass } from "./effects.js";
import { foliagePass } from "./foliage.js";
import { viewPass } from "./view.js";
import { postPass } from "./post.js";
import { hudPass } from "./hud.js";
import { waterPass } from "./water.js";
import { nearPass } from "./near.js";
import { canopiesPass } from "./canopies.js";
import { motionPass } from "./motion.js";
import { shadowPass } from "./shadow.js";
import { cloudsPass, underwaterPass, fogPass } from "./screen.js";
import { mirrorPass } from "./mirror.js";

// The views a frame draws, each with a frame block of its own (core.rs
// VIEW_*). This file imports the passes, so a pass reads these inside its
// functions, not as it loads.
export const VIEW_NEAR = 0, VIEW_FAR = 1, VIEW_FIRST_PERSON = 2, VIEW_SPARE = 3, VIEW_FAR_MIRROR = 4;
// The frame block's members (shader.rs FRAME_MEMBERS), in its float layout.
const FRAME = [
  ["projectionMatrix", 0, 16], ["viewMatrix", 16, 16], ["cameraPosition", 32, 3], ["time", 35, 1],
  ["sunDirection", 36, 3], ["bump", 39, 1], ["sunColor", 40, 3], ["shadowStrength", 43, 1],
  ["ambient", 44, 3], ["aaSamples", 47, 1], ["ambientTop", 48, 3], ["terrainShade", 51, 1],
  ["ambientBottom", 52, 3], ["farAaSamples", 55, 1], ["characterAmbient", 56, 3], ["cascadeNear", 59, 1],
  ["fogColor", 60, 3], ["cascadeFar", 63, 1], ["fade", 64, 2], ["resolution", 66, 2], ["fogRange", 68, 2],
];
const FRAME_NAMES = new Set(FRAME.map(([name]) => name));
// A material's uniform that is one of the frame's values under another name
// (the same object: fabric.js's fabricTime is lighting.time) reads the frame
// block's member instead: its declaration becomes a #define of the member.
const FRAME_BY_VALUE = new Map(FRAME.filter(([name]) => lighting[name]).map(([name, , size]) => [lighting[name], { name, size }]));
const UNIFORM_SIZES = { 0x1406: 1, 0x8b50: 2, 0x8b51: 3, 0x8b5c: 16 };
// add_program's kinds whose programs shader.rs makes shadow receivers of
// (world, skinned, rigid), and the instanced ones among them.
const RECEIVER_KINDS = new Set([0, 1, 2]), INSTANCED_KINDS = new Set([1, 2]);
// A shadow receiver's uniforms the BfeShadow block gives it (shader.rs), and
// the lamps' (materials.js DYNAMIC).
const SHADOW_UNIFORM = /^(cascadeMaps|directionalShadowMatrix|cascadePlanes|cascades)$|^directionalLightShadows\[\d+\]\./;
const LAMP_UNIFORMS = new Set(["localLightPositions", "localLightColors", "localLightCount"]);
// The shadow block's float layout (shader.rs SHADOW_*): per cascade its
// shadow values (8: intensity, bias, normal bias, radius, map size, pad),
// matrix (16), plane matrix (12: three columns of four) and slice (4).
const SHADOW = { VALUES: 0, MATRICES: 32, PLANES: 96, SLICES: 144, FLOATS: 160, CASCADES: 4 };
const LIGHT_COUNT = 70, LIGHTS = 72, MAX_LIGHTS = 16;
const GL_TYPES = { Float32Array: 0x1406, Uint16Array: 0x1403, Uint8Array: 0x1401, Int16Array: 0x1402, Int8Array: 0x1400, Uint32Array: 0x1405 };
// The phases of a frame, in the order three draws them: the view whose frame
// block a phase's passes read (written before they draw; null: none) and
// the renderer's call that draws them (renderer.rs).
const PHASES = {
  far: { view: VIEW_FAR, draw: (rust) => rust.far_frame() },
  near: { view: VIEW_NEAR, draw: (rust, ctx) => rust.frame(ctx.records) },
  // ctx.image: the far scene's image drawn first (screen.js).
  transparent: { view: VIEW_NEAR, draw: (rust, ctx) => rust.transparent_frame(ctx.commands ?? 0, ctx.image ?? 0) },
  view: { view: VIEW_FIRST_PERSON, draw: (rust, ctx) => rust.view_frame(ctx.records) },
  post: { view: null, draw: (rust, ctx) => rust.post_frame(ctx.steps ?? 0) },
  hud: { view: null, draw: (rust) => rust.hud_frame() },
  // The near phase's passes from a mirror camera (water.js's reflection).
  mirror: { view: VIEW_SPARE, draw: (rust, ctx) => rust.mirror_frame(ctx.records) },
  // The transparent phase's passes from that mirror camera.
  mirrorTransparent: { view: VIEW_SPARE, draw: (rust, ctx) => rust.mirror_transparent_frame(ctx.commands ?? 0, ctx.image ?? 0) },
  // The near scene's start when it is drawn in place of three's render
  // (near.js): its target cleared, the far scene's image drawn into it.
  begin: { view: null, draw: (rust, ctx) => rust.near_begin(ctx.clear, ctx.steps ?? 0) },
  // The far phase's passes from a water's mirror camera (water.js reflect),
  // into the mirror's target, before the frame's own far scene.
  farMirror: { view: VIEW_FAR_MIRROR, draw: (rust) => rust.far_mirror_frame() },
  // The far phase's passes for a sky probe's cube face (sky-probe.js), into
  // the face three bound; its frame blocks are the far pass's own, one a
  // face (ctx.face, which the far pass reads off the face's camera).
  probe: { view: VIEW_FAR, draw: (rust, ctx) => rust.far_probe_frame(ctx.face ?? 0) },
  // The temporal upscaler's motion of the bodies that move on their own
  // (motion.js), once the scenes are drawn.
  motion: { view: null, draw: (rust, ctx) => rust.motion_frame(ctx.records) },
  // A shadow cascade's pass in three's shadow pass's place (shadow.js).
  shadow: { view: null, draw: (rust) => rust.shadow_frame() },
  // Full-screen passes three renders on their own (screen.js): the storm's
  // cloud panorama into its target (storm-sky.js), and the Low water's tint
  // over the finished frame with the eye under the surface (water.js).
  image: { view: null, draw: (rust, ctx) => rust.screen_frame(ctx.steps ?? 0) },
  skyClouds: { view: null, draw: (rust, ctx) => rust.screen_frame(ctx.steps ?? 0) },
  underwater: { view: null, draw: (rust, ctx) => rust.screen_frame(ctx.steps ?? 0) },
};

// three's renderer's entry points that read or change its state cache, each
// settling a deferred phase's GL state first (watchThree).
const SETTLES = ["render", "setRenderTarget", "clear", "clearColor", "clearDepth", "clearStencil", "setClearColor", "setClearAlpha", "readRenderTargetPixels",
  "readRenderTargetPixelsAsync", "copyFramebufferToTexture", "copyTextureToTexture", "copyTextureToTexture3D", "initTexture", "initRenderTarget", "compile",
  "compileAsync", "setViewport", "setScissor", "setScissorTest", "resetState", "dispose"];

// The renderer with each registration call it is given kept (typed arrays
// copied), so another build of it can be given the same scene. The phases'
// calls and the memory layout's are not registration.
function recorder(rust, recording) {
  return new Proxy(rust, {
    get(target, name) {
      const value = target[name];
      if (typeof value !== "function") return value;
      if (/frame$|_segment$|_(ptr|len)$|^(records_at|record_stride|rebuild)$/.test(String(name))) return value.bind(target);
      return (...args) => {
        const result = value.apply(target, args);
        recording.calls.push([name, args.map((a) => (ArrayBuffer.isView(a) ? a.slice() : a?.constructor?.name === "WebGLTexture" ? null : a)), result]);
        return result;
      };
    },
  });
}

// A geometry's groups with consecutive ranges of one material merged: a
// soldier's two ranges with the same material draw as one.
const mergedGroups = new WeakMap();
function groupsOf(geometry) {
  let groups = mergedGroups.get(geometry);
  if (groups) return groups;
  groups = [];
  for (const g of geometry.groups) {
    const last = groups[groups.length - 1];
    if (last && last.materialIndex === g.materialIndex && last.start + last.count === g.start) last.count += g.count;
    else groups.push({ start: g.start, count: g.count, materialIndex: g.materialIndex });
  }
  mergedGroups.set(geometry, groups);
  return groups;
}

export async function createRustRender(graphics) {
  const { default: init, BfeRenderer } = await import("../rust-pkg/bfe_render.js");
  const wasm = await init();
  return new RustRender(graphics, wasm, BfeRenderer);
}

export class RustRender {
  constructor(graphics, wasm, BfeRenderer) {
    this.graphics = graphics;
    this.renderer = graphics.renderer;
    this.gl = this.renderer.getContext();
    this.wasm = wasm;
    this.rust = new BfeRenderer(this.gl);
    // ?rustRecord: every call made to the renderer, and each frame's inputs,
    // kept for a replay outside the browser (rust/bfe-native).
    this.recording = new URLSearchParams(location.search).has("rustRecord") ? { calls: [], textures: [], frames: [], keepFrames: 0 } : null;
    if (this.recording) this.rust = recorder(this.rust, this.recording);
    this.buffer = null;
    this.viewSpecs = [];
    this.programs = new Map(); // three's program -> Rust's (or null: unsupported)
    this.geometries = new Map(); // "geometry uuid:program" -> Rust geometry
    this.textureSources = [];
    this.textureTargets = new Map();
    this.textures = new Map(); // WebGLTexture -> Rust texture
    this.farTexture = null;
    this.phaseStats = {};
    this.phaseMs = {};
    this.ownedShaderSources = !new URLSearchParams(location.search).has("rustShaderBridge");
    this.errors = [];
    // The cascades' maps last given to the renderer.
    this.shadowMaps = "";
    this.shadowView = false;
    // Programs whose lamps come from the lamp table (a pass that gives each
    // draw its lamps, lamp lighting on), kept apart from the same three
    // programs' others; the lamp table's sources and their indices there.
    this.lampPrograms = new Map();
    this.lampSources = 0;
    this.lampIndex = new Map();
    this.lampOut = { indices: [-1, -1, -1, -1], count: 0 };
    // The renderer's inputs: the frame block's values, then the passes'
    // records (f64), where they start and a record's size (instances.rs);
    // and the bones of the rigs drawn (f32).
    this.recordsAt = this.rust.records_at();
    this.stride = this.rust.record_stride();
    this.addView("input", Float64Array, () => this.rust.input_ptr(), () => this.rust.input_len());
    this.addView("bones", Float32Array, () => this.rust.bones_ptr(), () => this.rust.bones_len());
    // Phases drawn between three's renders reset three's state once, when
    // three is next used.
    this.stale = false;
    this.settleThree = null;
    this.watchThree();
    const initTarget = this.renderer.initRenderTarget?.bind(this.renderer);
    if (initTarget) this.renderer.initRenderTarget = (target) => {
      this.textureTargets.set(target.texture, target);
      if (target.depthTexture) this.textureTargets.set(target.depthTexture, target);
      return initTarget(target);
    };
    this.contextLost = false;
    this.renderer.domElement?.addEventListener?.("webglcontextlost", (event) => {
      event.preventDefault();
      this.contextLost = true;
      this.stale = false;
    });
    this.renderer.domElement?.addEventListener?.("webglcontextrestored", () => {
      try { this.rebuild(); } catch (error) { this.fail(error); }
    });
    // The passes' work before three lists a render of the near scene
    // (beforeSort): hooked first, so that what hooks in after it
    // (particle-depth.js's copier) sees what they registered.
    this.passes = [];
    if (graphics.scene) this.watchScene(graphics.scene);
    // The passes, in the renderer's order.
    this.phases = Object.fromEntries(Object.keys(PHASES).map((name) => [name, []]));
    // A mirror draws the near phase's passes, then the transparent phase's;
    // its far scene, the far phase's.
    this.phases.mirror = this.phases.near;
    this.phases.mirrorTransparent = this.phases.transparent;
    this.phases.farMirror = this.phases.far;
    this.phases.probe = this.phases.far;
    this.byName = {};
    for (const make of [worldPass, instancesPass, farPass, terrainPass, transparentPass, effectsPass, foliagePass, viewPass, postPass, hudPass, waterPass, nearPass, canopiesPass, seaPass, motionPass, shadowPass, cloudsPass, underwaterPass, mirrorPass, fogPass]) this.addPass(make(this));
  }

  // The scene and texture owners keep their CPU resources. Rust replays its
  // retained resource descriptions into the restored context, preserving ids.
  rebuild() {
    this.stale = false;
    const renderer = this.renderer, handles = [];
    this.graphics.warmup();
    // Native program objects change on restore; Rust replay keeps its ids.
    for (const entry of this.byName.transparent.taken.values()) entry.program = renderer.properties.get(entry.material).currentProgram;
    const targets = new Set();
    for (const texture of this.textureSources) {
      const target = this.textureTargets.get(texture);
      if (target) targets.add(target);
    }
    for (const target of targets) renderer.initRenderTarget(target);
    this.textures.clear();
    for (let id = 0; id < this.textureSources.length; id++) {
      const texture = this.textureSources[id];
      if (!texture.isRenderTargetTexture && !texture.isDepthTexture) renderer.initTexture(texture);
      const handle = renderer.properties.get(texture).__webglTexture;
      if (!handle) throw new Error(`Missing restored texture ${texture.name || id}`);
      handles[id] = handle;
      this.textures.set(handle, id);
    }
    this.rust.rebuild(this.gl, handles);
    this.buffer = null;
    this.views();
    this.shadowMaps = "";
    this.byName.hud.full = true;
    this.byName.water.full = true;
    for (const window of this.graphics.cascadeWindows || []) window.invalidate();
    const water = this.graphics.scene.userData.water;
    if (water) {
      water.reflectFrames = 0;
      if (water.ocean?.probe) water.ocean.probe.last = -Infinity;
      if (water.ocean?.storm) water.ocean.storm.lastCloud = -Infinity;
    }
    this.graphics.upscaler?.invalidate?.();
    renderer.resetState();
    this.contextLost = false;
  }

  addPass(pass) {
    if (!this.phases[pass.phase]) throw new Error(`rust-render: pass ${pass.name} has no phase ${pass.phase}`);
    this.passes.push(pass);
    this.phases[pass.phase].push(pass);
    this.byName[pass.name] = pass;
    pass.register?.();
  }

  // Whether the frame has shadow receivers' inputs to give (C2: the cascades'
  // block and maps).
  shadowsOn() {
    return this.renderer.shadowMap.enabled && Boolean(this.graphics.csm) && lighting.cascadeMaps.value.length > 0;
  }

  // Shadow and motion walks need the materials and layers hidden by the
  // colour passes. Reveal them only for those Rust walks, then hide them.
  reveal(fn) {

    for (const pass of this.passes) pass.reveal?.(true);
    try {
      return fn();
    } finally {
      for (const pass of this.passes) pass.reveal?.(false);
    }
  }

  // Transparent candidates retained as roots enter the battle scene.
  nearList(renderer, scene) {
    return this.sceneItems ?? null;
  }

  // A typed view of the renderer's memory, kept as this[name]: a Type over
  // len() elements from the byte offset ptr(). views() makes them again
  // whenever the memory has grown, which detaches the old ones: registering
  // anything can.
  addView(name, Type, ptr, len) {
    this.viewSpecs.push({ name, Type, ptr, len });
    this.buffer = null;
  }

  views() {
    if (this.buffer === this.wasm.memory.buffer) return;
    this.buffer = this.wasm.memory.buffer;
    for (const { name, Type, ptr, len } of this.viewSpecs) this[name] = new Type(this.buffer, ptr(), len());
  }

  texture(texture) {
    if (!texture) return -1;
    if (!this.renderer.properties.get(texture).__webglTexture) this.renderer.initTexture(texture);
    const handle = this.renderer.properties.get(texture).__webglTexture;
    if (!handle) return -1;
    let id = this.textures.get(handle);
    if (id === undefined) {
      id = this.rust.add_texture(handle);
      this.textureSources[id] = texture;
      this.textures.set(handle, id);
      if (this.recording) this.recording.textures[id] = { handle, width: texture.image?.width ?? texture.mipmaps?.[0]?.width ?? texture.width ?? 0, height: texture.image?.height ?? texture.mipmaps?.[0]?.height ?? texture.height ?? 0 };
    }
    return id;
  }

  // Rust's program for three's (made once), or null when the renderer can't
  // give it what it reads. kind: the pass's (add_program's); material: one
  // that draws with it, whose shared frame values must be the frame's;
  // policy: { uniforms, material }, what the pass gives a program of the
  // kind besides the frame block (per draw or per instance), and which of
  // those come from a draw's material (the renderer is told their names).
  // cache: where it is kept (made once each): a pass that keeps what it last
  // set in a program's uniforms passes a cache of its own, so that no other
  // pass sets them behind its back.
  // The Enhanced inputs a program may read beyond its pass's own: a shadow
  // receiver's (the BfeShadow block; receiveShadow too for an instance, from
  // its row) when its pass feeds shadows (policy.shadows: it gives a draw's
  // receiveShadow and reveals the casters it hides), and the lamps' while
  // lamps are on only when its pass feeds them (policy.lamps). Anything else
  // makes the program three's.
  program(threeProgram, kind, material, policy, cache = this.programs) {
    const lampsOn = Boolean(this.graphics.settings.get("lampLighting"));
    // A pass that gives its draws their lamps: its programs read them from the
    // lamp table (add_program_lamps), kept apart from three's same programs.
    const fed = lampsOn && Boolean(policy.lamps) && RECEIVER_KINDS.has(kind);
    if (fed && cache === this.programs) cache = this.lampPrograms;
    let entry = cache.get(threeProgram);
    if (entry !== undefined) return entry;
    entry = null;
    const gl = this.gl, p = threeProgram.program;
    const names = [], aliases = [];
    const shadows = Boolean(policy.shadows) && RECEIVER_KINDS.has(kind) && this.shadowsOn();
    const lamps = !lampsOn || fed;
    let ok = true, why = "";
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
      const u = gl.getActiveUniform(p, i), name = u.name.replace(/\[0\]$/, "");
      names.push(name);
      if (FRAME_NAMES.has(name)) {
        // A frame value this material keeps of its own can't come from the block.
        if (!["projectionMatrix", "viewMatrix", "cameraPosition"].includes(name) && material.uniforms?.[name] !== lighting[name]) ok = false;
      } else if (SHADOW_UNIFORM.test(name) || (name === "receiveShadow" && INSTANCED_KINDS.has(kind))) {
        if (!shadows) { ok = false; why = " (shadows)"; }
      } else if (LAMP_UNIFORMS.has(name) && !lamps) {
        ok = false; why = " (lamps)";
      } else if (!policy.uniforms.has(name)) {
        const member = FRAME_BY_VALUE.get(material.uniforms?.[name]);
        if (member && u.size === 1 && UNIFORM_SIZES[u.type] === member.size) aliases.push([name, member.name]);
        else ok = false;
      }
      if (!ok) this.fail(new Error(`Program ${threeProgram.id} (${material.name || material.type}) cannot bind ${name}${why}`));
    }
    if (ok) {
      const attributes = [];
      for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) {
        const a = gl.getActiveAttrib(p, i);
        attributes.push(`${a.name}:${gl.getAttribLocation(p, a.name)}`);
      }
      const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
      const source = (shader) => aliases.reduce((text, [name, member]) => text.replace(new RegExp(`^\\s*uniform\\s+(?:(?:highp|mediump|lowp)\\s+)?\\w+\\s+${name}\\s*;\\s*$`, "m"), `#define ${name} ${member}`), gl.getShaderSource(shader));
      try {
        const lamped = fed && names.some((name) => LAMP_UNIFORMS.has(name));
        const add = lamped ? this.rust.add_program_lamps : this.rust.add_program;
        entry = { id: add.call(this.rust, kind, source(vs), source(fs), attributes.join(","), names.filter((name) => policy.material.has(name)).join(",")),
          attributes: attributes.map((a) => { const [name, location] = a.split(":"); return [name, Number(location)]; }), names: new Set(names), aliased: aliases.length > 0, lamps: lamped };
      } catch (error) {
        this.fail(error);
      }
    }
    cache.set(threeProgram, entry);
    return entry;
  }

  // A geometry's groups, consecutive ranges of one material merged.
  groups(geometry) {
    return groupsOf(geometry);
  }

  // Rust's copy of a geometry's attributes (those the program reads, at its
  // locations) and index.
  geometry(geometry, program) {
    const key = `${geometry.uuid}:${program.id}`;
    let id = this.geometries.get(key);
    if (id !== undefined) return id;
    id = null;
    const attributes = [];
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      const array = a?.isInterleavedBufferAttribute ? a.data.array : a?.array;
      if (!array || !(array.constructor.name in GL_TYPES)) { this.errors.push(`attribute ${name}`); this.geometries.set(key, null); return null; }
      attributes.push([a, location, array]);
    }
    if (!geometry.index) { this.geometries.set(key, null); return null; }
    id = this.rust.add_geometry();
    // Each buffer once (interleaved attributes share theirs).
    const buffers = new Map();
    for (const [a, location, array] of attributes) {
      const source = a.isInterleavedBufferAttribute ? a.data : a;
      let slot = buffers.get(source);
      if (slot === undefined) { slot = this.rust.geometry_buffer(id, new Uint8Array(array.buffer, array.byteOffset, array.byteLength)); buffers.set(source, slot); }
      const bytes = array.BYTES_PER_ELEMENT;
      const stride = a.isInterleavedBufferAttribute ? a.data.stride * bytes : 0, offset = a.isInterleavedBufferAttribute ? a.offset * bytes : 0;
      this.rust.geometry_pointer(id, slot, location, a.itemSize, GL_TYPES[array.constructor.name], a.normalized, stride, offset);
    }
    const index = geometry.index.array;
    this.rust.geometry_index(id, new Uint8Array(index.buffer, index.byteOffset, index.byteLength), index.BYTES_PER_ELEMENT === 2 ? 0x1403 : 0x1405);
    for (const group of groupsOf(geometry)) this.rust.geometry_group(id, group.start, group.count);
    this.geometries.set(key, id);
    return id;
  }

  // The frame's shared values, as three would upload them, for a view's
  // frame block (renderer.rs upload_frame). The views drawn over the far
  // scene's image read it too (farMap); it is taken first, so that its
  // texture is registered before anything a pass registers.
  writeFrame(scene, camera, view) {
    // The far scene's views (the eye's, a water mirror's) read neither the far
    // image nor shadows nor lamps.
    const far = view === VIEW_FAR || view === VIEW_FAR_MIRROR;
    if (!far) {
      // The far image the materials read (lighting.farMap: the far target,
      // or a mirror's own far image while it draws).
      const image = lighting.farMap.value ?? this.graphics.farTarget.texture;
      const far = this.renderer.properties.get(image).__webglTexture;
      if (far !== this.farTexture) { this.farTexture = far; this.rust.set_far_map(this.texture(image)); }
    }
    // The Enhanced inputs first: they may register the cascades' maps and send
    // the lamp table, which can grow the renderer's memory (every view is made
    // again then): the frame block's view is taken after them.
    if (!far && view !== VIEW_FIRST_PERSON) this.writeShadow();
    if (!far) this.writeLamps();
    this.views();
    const input = this.input;
    input.set(camera.projectionMatrix.elements, 0);
    input.set(camera.matrixWorldInverse.elements, 16);
    const m = camera.matrixWorld.elements;
    input[32] = m[12]; input[33] = m[13]; input[34] = m[14];
    for (const [name, at, size] of FRAME) {
      if (at < 35) continue;
      const v = lighting[name].value;
      if (size === 1) input[at] = v;
      else { input[at] = v.x; input[at + 1] = v.y; if (size === 3) input[at + 2] = v.z; }
    }
    const pack = packed(scene, camera);
    const count = Math.min(pack.count, MAX_LIGHTS);
    input[LIGHT_COUNT] = count;
    for (let i = 0; i < 2 * count; i++) {
      const v = pack.lights[i], o = LIGHTS + 4 * i;
      input[o] = v.x; input[o + 1] = v.y; input[o + 2] = v.z; input[o + 3] = v.w;
    }
  }

  // The shadow receivers' block (shader.rs BfeShadow) as three uploads its
  // uniforms for the near scene's receivers: each cascade light's shadow
  // values and matrix (three's lights state, the cascade order: CSM's lights
  // are the scene's only shadow-casting ones), the plane matrices and slices
  // the materials share (lighting.cascadePlanes, lighting.cascades), read
  // where three's draws would read them (after this frame's shadow pass; a
  // mirror's, drawn before it, last frame's); and the cascades' maps. With
  // shadows off, no maps: the renderer sends and binds nothing.
  writeShadow() {
    const on = this.shadowsOn();
    const maps = on ? lighting.cascadeMaps.value : [];
    const ids = maps.map((t) => this.texture(t));
    const key = ids.join(",");
    if (key !== this.shadowMaps) { this.shadowMaps = key; this.rust.set_shadow_maps(new Uint32Array(ids.every((id) => id >= 0) ? ids : [])); }
    if (!on) return;
    if (!this.shadowView) { this.addView("shadowInput", Float64Array, () => this.rust.shadow_input_ptr(), () => this.rust.shadow_input_len()); this.shadowView = true; }
    this.views();
    const out = this.shadowInput, lights = this.graphics.csm.lights, planes = lighting.cascadePlanes.value, slices = lighting.cascades.value;
    out.fill(0);
    for (let i = 0; i < Math.min(lights.length, SHADOW.CASCADES); i++) {
      const shadow = lights[i].shadow, v = SHADOW.VALUES + 8 * i;
      out[v] = shadow.intensity ?? 1; out[v + 1] = shadow.bias; out[v + 2] = shadow.normalBias; out[v + 3] = shadow.radius;
      out[v + 4] = shadow.mapSize.x; out[v + 5] = shadow.mapSize.y;
      out.set(shadow.matrix.elements, SHADOW.MATRICES + 16 * i);
      const e = planes[i]?.elements;
      if (e) for (let c = 0; c < 3; c++) for (let r = 0; r < 3; r++) out[SHADOW.PLANES + 12 * i + 4 * c + r] = e[3 * c + r];
    }
    for (let i = 0; i < Math.min(slices.length, SHADOW.CASCADES); i++) { out[SHADOW.SLICES + 4 * i] = slices[i].x; out[SHADOW.SLICES + 4 * i + 1] = slices[i].y; }
  }

  // The lamp table (shader.rs LAMP_*): every lamp source of the map (two
  // texels each: position and radius, colour), given again when the sources
  // grow (they do as the self-lit textures arrive, early in the battle).
  writeLamps() {
    const local = this.graphics.localLights;
    if (!local || !this.graphics.settings.get("lampLighting")) return;
    const sources = local.sources;
    if (sources.length === this.lampSources) return;
    const data = new Float32Array(8 * sources.length);
    for (let i = 0; i < sources.length; i++) {
      const s = sources[i];
      data.set([s.position.x, s.position.y, s.position.z, s.radius, s.color.x, s.color.y, s.color.z, 0], 8 * i);
      this.lampIndex.set(s, i);
    }
    this.lampSources = sources.length;
    if (sources.length) this.rust.set_lamps(data);
  }

  // The lamps a pass's draw of `object` has this frame (four lamp table
  // indices, -1 for none, and a count), as three's onBeforeRender of a
  // DYNAMIC zero material selects them (local-lights.js select, with its
  // cache and occlusion rays); viewModel: a first-person part's. The object
  // returned is reused.
  lamps(object, viewModel = false) {
    const out = this.lampOut, lights = this.graphics.localLights?.select(object, viewModel) ?? [];
    out.count = Math.min(4, lights.length);
    for (let k = 0; k < 4; k++) out.indices[k] = k < out.count ? this.lampIndex.get(lights[k]) ?? -1 : -1;
    return out;
  }


  // The scene's onBeforeRender, as three starts a render of it (before it
  // lists and sorts it): each pass's beforeSort(renderer, scene, camera,
  // target, added), which can register what is new and hide it from three
  // before three lists it, so that the renderer draws it from its first
  // frame. added: for the frame's own render, what was added to the scene
  // since the last one (the scene itself the first time; every object of it
  // is told of its children added), else null.
  watchScene(scene) {
    const previous = scene.onBeforeRender, watched = new WeakSet();
    let added = null;
    const onAdded = ({ child }) => { added?.push(child); };
    const watch = (root) => root.traverse((o) => { if (!watched.has(o)) { watched.add(o); o.addEventListener("childadded", onAdded); } });
    scene.onBeforeRender = (...args) => {
      previous?.apply(scene, args);
      if (this.contextLost) return;
      const [renderer, , camera, target] = args;
      let roots = null;
      if (camera === this.graphics.camera && target === this.graphics.sceneTarget) {
        roots = added ?? [scene];
        added = [];
        for (const root of roots) watch(root);
      }
      if (roots?.length) this.registerItems(renderer, scene, camera, roots);
      for (const pass of this.passes) pass.beforeSort?.(...args, roots);
    };
  }

  // Discover scene resources without asking three to submit a draw. Keep
  // candidates between frames; the near walk supplies the camera's order.
  registerItems(renderer, scene, camera, roots) {
    const list = this.sceneItems ??= { opaque: [], transmissive: [], transparent: [] };
    const known = this.knownItems ??= new WeakSet();
    const fresh = [];
    for (const root of roots) root.traverse((object) => {
      if (!(object.isMesh || object.isSprite) || object === this.graphics.background || known.has(object)) return;
      known.add(object);
      const material = object.material;
      if (!material || Array.isArray(material) || !material.visible) return;
      if (!renderer.properties.get(material).currentProgram) fresh.push([object, material]);
      if (material.transparent) list.transparent.push({ object, material, geometry: object.geometry, group: null });
    });
    if (fresh.length) compileFor(renderer, fresh, camera, scene);
    this.byName.effects.listed({ renderer, scene, camera });
    const transparent = this.byName.transparent;
    for (const item of list.transparent) {
      if (transparent.taken.has(item.object) || !transparent.candidate(item)) continue;
      const entry = item.material.isShaderMaterial && "WORLD_SPACE" in (item.material.defines || {}) ? transparent.registerBucket(item) : transparent.registerObject(item, true);
      if (entry) transparent.taken.set(item.object, entry);
    }
  }



  // A phase of the frame, at its point in three's: the frame block written
  // for its view, its passes in order, then the renderer's call that draws
  // them. Three's state is reset after it, and the target it was drawing
  // into bound again.
  // defer: three draws nothing right after this phase (a scene's phase drawn
  // in place of three's render, between three's renders): its state is reset
  // only when three is next used (settle), so that phases drawn one after
  // another reset it once. walked: the transparent list's items as near.js
  // walked the scene for three's render it draws in place of (ctx.walked).
  // values: more of the phase's values (ctx), for its passes and its call.
  phase(name, renderer, scene, camera, defer = false, walked = null, values = null) {
    if (this.contextLost) return;


    const phase = PHASES[name], target = renderer.getRenderTarget();
    if (phase.view !== null) this.writeFrame(scene, camera, phase.view);
    // What the passes have written into the renderer's input so far: records
    // and bones, and where each skeleton's bones are.
    const ctx = { renderer, scene, camera, records: 0, bones: 0, skeletons: new Map(), defer, walked };
    if (values) Object.assign(ctx, values);
    for (const pass of this.phases[name]) pass.draw?.(ctx);
    if (name === "near" && this.recording?.keepFrames > 0) {
      this.recording.keepFrames--;
      this.views();
      this.recording.frames.push({ records: ctx.records, input: this.input.slice(0, this.recordsAt + ctx.records * this.stride), bones: this.bones.slice(0, ctx.bones * 16) });
    }
    if (ctx.priming) { this.stale = true; return; }
    const t0 = performance.now();
    const stats = this.phaseStats[name] = phase.draw(this.rust, ctx);
    this.phaseMs[name] = performance.now() - t0;
    // A phase that made no GL call left three's state as it was, unless a
    // pass registered something with GL calls of its own (ctx.reset).
    if (stats?.[4] !== 0 || ctx.reset) {
      if (defer) this.stale = true;
      else {
        this.stale = false;
        renderer.resetState();
        renderer.setRenderTarget(target);
      }
    }
    for (const pass of this.passes) pass.afterPhase?.(name, ctx);
  }

  // Three's state cache, out of date after a deferred phase, made good before
  // three is next used: every entry point of three's renderer that uses it
  // (render, targets, clears, reads, uploads, compiles, viewport) settles
  // first. Settling resets three's state and binds again the target three
  // believes bound (a phase leaves three's own idea of it unchanged).
  watchThree() {
    const renderer = this.renderer;
    if (renderer.__rustSettles) return;
    renderer.__rustSettles = true;
    const resetState = renderer.resetState?.bind(renderer), setRenderTarget = renderer.setRenderTarget?.bind(renderer), getRenderTarget = renderer.getRenderTarget?.bind(renderer);
    this.settleThree = () => {
      if (!this.stale) return;
      this.stale = false;
      const target = getRenderTarget();
      resetState();
      setRenderTarget(target);
    };
    for (const name of SETTLES) {
      const f = renderer[name];
      if (typeof f !== "function") continue;
      const bridge = this;
      renderer[name] = function (...args) {
        if (name === "setRenderTarget" && args[0]) {
          const target = args[0];
          bridge.textureTargets.set(target.texture, target);
          if (target.depthTexture) bridge.textureTargets.set(target.depthTexture, target);
        }
        bridge.settleThree?.();
        return f.apply(this, args);
      };
    }
  }

  settle() {
    this.settleThree?.();
  }

  // A scene's phase drawn in place of three's render of it (graphics.js
  // renders the scene with three only when this returns false): what three's
  // render does first (the scene's world matrices, where it keeps them up to
  // date itself, and its onBeforeRender, which packs its point lights), then
  // the phase. True when every pass of the phase drew all of the scene that
  // three would have (each sets `complete` as it draws); then what three's
  // render does as it ends is done too: the target resolved (when it is
  // multisampled), the scene's onAfterRender.
  cover(name, renderer, scene, camera) {
    if (this.contextLost) return true;

    const target = renderer.getRenderTarget();
    try {
      if (scene.matrixWorldAutoUpdate) scene.updateMatrixWorld();
      scene.onBeforeRender(renderer, scene, camera, target);
      const passes = this.phases[name];
      for (const pass of passes) pass.complete = false;
      this.phase(name, renderer, scene, camera, true);
      const complete = passes.length > 0 && passes.every((pass) => pass.complete === true);
      if (complete) {
        this.resolve(renderer.getRenderTarget());
        if (scene.isScene) scene.onAfterRender(renderer, scene, camera);
      }
      if (!complete) throw new Error(`Incomplete Rust phase ${name}: ${passes.filter((p) => !p.complete).map((p) => p.name).join(", ")}`);
      return true;
    } catch (error) {
      this.fail(error, renderer, target);
      return false;
    }
  }

  // Keep the failure visible and stop this battle. No other drawer takes over.
  fail(error, renderer = this.renderer, target = renderer.getRenderTarget()) {
    const failure = rendererFailure(renderer, error);
    this.failure = failure;
    this.errors.push(failure.message);
    const hud = globalThis.document?.querySelector("#hud");
    if (hud) hud.textContent = failure.message;
    throw failure;
  }

  // A multisampled target's samples into its texture, as three's
  // updateMultisampleRenderTarget blits them (r170, one colour texture; the
  // depth too where the target resolves it), with its state cache kept.
  // With three's state stale (a deferred phase), the binds are made directly
  // (three's cache is reset before its next use anyway).
  resolve(target) {
    if (!target || !(target.samples > 0)) return;
    const gl = this.gl, p = this.renderer.properties.get(target);
    if (!p.__webglMultisampledFramebuffer) return;
    const state = this.stale ? { bindFramebuffer: (t, f) => gl.bindFramebuffer(t, f) } : this.renderer.state;
    let mask = gl.COLOR_BUFFER_BIT;
    if (target.resolveDepthBuffer) {
      if (target.depthBuffer) mask |= gl.DEPTH_BUFFER_BIT;
      if (target.stencilBuffer && target.resolveStencilBuffer) mask |= gl.STENCIL_BUFFER_BIT;
    }
    state.bindFramebuffer(gl.READ_FRAMEBUFFER, p.__webglMultisampledFramebuffer);
    state.bindFramebuffer(gl.DRAW_FRAMEBUFFER, p.__webglFramebuffer);
    gl.blitFramebuffer(0, 0, target.width, target.height, 0, 0, target.width, target.height, mask, gl.NEAREST);
    state.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    state.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    state.bindFramebuffer(gl.DRAW_FRAMEBUFFER, p.__webglMultisampledFramebuffer);
  }

  // graphics.js: the frame's post passes (Graphics.postSteps), drawn by the
  // post pass when it can draw every one; true when it did.
  post(renderer, steps) {
    if (this.contextLost) return true;
    const target = renderer.getRenderTarget();
    try { if (!this.byName.post.cover(renderer, steps)) this.fail(new Error("Incomplete Rust post chain")); return true; } catch (error) { this.fail(error, renderer, target); return false; }
  }

  // graphics.js: the near scene, drawn in place of three's render of it when
  // nothing in it is left for three (near.js); true when it was.
  coverNear(renderer, scene, camera) {
    if (this.contextLost) return true;
    if (!this.byName.near.cover(renderer, scene, camera)) this.fail(new Error(`Incomplete Rust near scene: ${this.byName.near.stopped?.name || this.byName.near.stopped?.type || "configuration"}`));
    return true;
  }

  // graphics.js renderCascades: a shadow cascade's pass (shadow.js), drawn by
  // the shadow pass when it can draw all of it; true when it was.
  coverShadow(renderer, light, scene, camera, from) {
    if (this.contextLost) return true;
    const target = renderer.getRenderTarget();
    try { if (!this.byName.shadow.cover(renderer, light, scene, camera, from)) this.fail(new Error(`Incomplete Rust shadow: ${this.byName.shadow.why}`)); return true; } catch (error) { this.fail(error, renderer, target); return false; }
  }

  // water.js: a water mirror's render of the near scene, drawn in place of
  // three's render of it when nothing in it is left for three (mirror.js);
  // true when it was.
  coverMirror(renderer, scene, camera) {
    if (this.contextLost) return true;
    if (!this.byName.mirrorCover.cover(renderer, scene, camera)) this.fail(new Error("Incomplete Rust mirror scene"));
    return true;
  }

  // hud.js: the battle HUD's batch, drawn by the HUD pass; true when it was.
  coverHud(renderer, hud) {
    if (this.contextLost) return true;
    const target = renderer.getRenderTarget();
    try { if (!this.byName.hud.cover(renderer, hud)) this.fail(new Error(`Incomplete Rust HUD: ${this.byName.hud.why || "batch"}`)); return true; } catch (error) { this.fail(error, renderer, target); return false; }
  }

  // The near phase's stats and the time its renderer call took, as the
  // measurement tools read them, and what they read of the passes.
  get stats() { return this.phaseStats.near ?? null; }
  get frameMs() { return this.phaseMs.near; }
  get worldCount() { return this.byName.world.count; }
  get worldPending() { return this.byName.world.pending; }
  get bodyRecords() { return this.byName.instances.bodyRecords; }
  get meshRecords() { return this.byName.instances.meshRecords; }

}
