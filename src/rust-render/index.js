// The Rust battle renderer (rust/bfe-render, built to src/rust-pkg) behind
// the `rust` query parameter: an experiment in drawing the near scene's
// static world and its soldiers with far fewer WebGL calls than three makes
// for them. Three.js still draws everything else and stays the default.
//
// It draws inside the near scene's pass, first of its opaque objects (from
// an anchor at the start of three's opaque list; the far scene's image,
// graphics.background, is drawn last of them): the map's opaque buckets,
// then every visible soldier's body and held gun, instanced. Three's state
// is reset after it (renderer.resetState) and three draws the rest of the
// scene. What it draws three does not (each such material is set
// invisible, which three's render lists respect), but three's scene update
// still composes their matrices and bones, which the page's other code
// reads. It draws them with programs made from the GLSL three compiled for
// them (shader.rs), so they light as three's do.
//
// Only what it supports moves; everything else stays with three, item by
// item: a program that reads what the renderer can't give it yet (the
// Enhanced lamps, a shadow receiver of a pass that doesn't feed shadows)
// stays three's (program()). The shadow receivers read the cascades from a
// block of their own (BfeShadow), written once a frame.
//
// It is made of passes, a file each here as in the renderer (renderer.rs):
// the world (world.js), the instanced soldiers, guns and rigs
// (instances.js), and, empty for now, the far scene, terrain, the
// transparent pass, effects, foliage, the first-person view and the post
// passes. Each draws in a phase of the frame: far, near, transparent, view,
// post (and mirror: the near phase's passes from a mirror's camera;
// farMirror and probe: the far phase's, from a water mirror's camera and for
// a sky probe's faces). A
// scene whose passes draw all of it is left out of three's frame (cover).
// This file is their core: the WASM renderer and what every pass
// registers through it (textures, programs, geometries), the frame block,
// typed views of the renderer's memory, and the registry that runs each
// phase's passes and then the renderer's call that draws them.
import * as THREE from "three";
import { lighting } from "../materials.js";
import { packed } from "../point-lights.js";
import { worldPass } from "./world.js";
import { instancesPass } from "./instances.js";
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
  transparent: { view: VIEW_NEAR, draw: (rust, ctx) => rust.transparent_frame(ctx.three ?? 0) },
  view: { view: VIEW_FIRST_PERSON, draw: (rust, ctx) => rust.view_frame(ctx.records) },
  post: { view: null, draw: (rust, ctx) => rust.post_frame(ctx.steps ?? 0) },
  hud: { view: null, draw: (rust) => rust.hud_frame() },
  // The near phase's passes from a mirror camera (water.js's reflection).
  mirror: { view: VIEW_SPARE, draw: (rust, ctx) => rust.mirror_frame(ctx.records) },
  // The transparent phase's passes from that mirror camera.
  mirrorTransparent: { view: VIEW_SPARE, draw: (rust, ctx) => rust.mirror_transparent_frame(ctx.three ?? 0) },
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
      if (/frame$|_segment$|_(ptr|len)$|^(records_at|record_stride)$/.test(String(name))) return value.bind(target);
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
    this.textures = new Map(); // WebGLTexture -> Rust texture
    this.farTexture = null;
    this.phaseStats = {};
    this.phaseMs = {};
    this.enabled = true;
    this.active = !new URLSearchParams(location.search).has("rustOff");
    this.errors = [];
    // The Enhanced configuration the battle started in (supported()), and
    // the cascades' maps last given to the renderer.
    this.config = null;
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
    // A lost context takes every object the renderer made with it (three
    // makes its own again when it is restored): three draws everything.
    this.lost = false;
    this.renderer.domElement?.addEventListener?.("webglcontextlost", () => { this.lost = true; this.fail(new Error("WebGL context lost")); });
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
    for (const make of [worldPass, instancesPass, farPass, terrainPass, transparentPass, effectsPass, foliagePass, viewPass, postPass, hudPass, waterPass, nearPass, canopiesPass, seaPass, motionPass]) this.addPass(make(this));
    // The near phase's hook: first in three's opaque list of the near scene
    // (graphics.background, the far scene's image, is drawn last of it).
    this.nearAnchor = graphics.scene ? this.anchor(graphics.scene, { renderOrder: -1e9 }, (renderer, scene, camera) => this.pass(renderer, scene, camera)) : null;
  }

  // A pass: { name, phase, register?(), draw?(ctx), setActive?(on),
  // restore?(), afterPhase?(name, ctx) }. register() runs once, as it is
  // added; draw(ctx) in each of its phase's frames, in the order the passes
  // were added, to register what it draws (the first time three has drawn
  // it, which gives its material a program) and write its inputs;
  // setActive(on) moves what it registered back to three (off) or to the
  // renderer (on); restore() gives three anything it hid outside the near
  // scene, for good; afterPhase(name, ctx) runs for every pass after any
  // phase's renderer call (a pass that draws later in the same render of
  // three's, such as the transparent list, arms itself there).
  addPass(pass) {
    if (!this.phases[pass.phase]) throw new Error(`rust-render: pass ${pass.name} has no phase ${pass.phase}`);
    this.passes.push(pass);
    this.phases[pass.phase].push(pass);
    this.byName[pass.name] = pass;
    pass.register?.();
  }

  // The Enhanced configuration: the shadow cascades (0: none), lamps, the
  // temporal upscaler.
  configKey() {
    const graphics = this.graphics;
    const cascades = this.renderer.shadowMap.enabled ? graphics.csm?.lights.length ?? -1 : 0;
    return `${cascades}:${graphics.settings.get("lampLighting") ? 1 : 0}:${graphics.upscaling === "temporal" ? 1 : 0}`;
  }

  // The configuration the renderer covers: the one the battle started in.
  // Within it each pass leaves to three, item by item, what it can't draw
  // (program()); a change of it gives everything back to three for the rest
  // of the battle (programs registered for the old one would be stale).
  supported() {
    const key = this.configKey();
    this.config ??= key;
    return key === this.config;
  }

  // Whether the frame has shadow receivers' inputs to give (C2: the cascades'
  // block and maps).
  shadowsOn() {
    return this.renderer.shadowMap.enabled && Boolean(this.graphics.csm) && lighting.cascadeMaps.value.length > 0;
  }

  // Three's own renders that must see what the passes took from it (by its
  // material's visibility or its layers): three's shadow pass, whose casters
  // include buckets the world pass draws (graphics.js renderCascades), and the
  // temporal upscaler's motion pass, which draws the first-person view again
  // for its mask (upscaler.drawMotion). fn runs with them shown to three
  // (each pass's reveal(on)).
  reveal(fn) {
    if (!this.enabled || !this.active) return fn();
    for (const pass of this.passes) pass.reveal?.(true);
    try {
      return fn();
    } finally {
      for (const pass of this.passes) pass.reveal?.(false);
    }
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
      if (!ok) { this.errors.push(`p${threeProgram.id} ${name}${why}`); break; }
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
        this.errors.push(`p${threeProgram.id}: ${String(error).slice(0, 300)}`);
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

  // A point inside three's render lists, for a pass that must draw there
  // (the start of the transparent list...): a mesh three draws with no
  // fragments (one degenerate triangle, neither tested nor written, never
  // culled), whose onAfterRender calls back with (renderer, scene, camera).
  // transparent: in the transparent list, else the opaque one; renderOrder:
  // its place in that list, which three sorts by first.
  anchor(scene, { transparent = false, renderOrder = 0 } = {}, callback) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
    const material = new THREE.MeshBasicMaterial({ transparent, colorWrite: false, depthWrite: false, depthTest: false });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = "rust-render anchor";
    mesh.frustumCulled = false;
    mesh.renderOrder = renderOrder;
    mesh.onAfterRender = (renderer, scene, camera) => callback(renderer, scene, camera);
    scene.add(mesh);
    return mesh;
  }

  // The scene's onBeforeRender, as three starts a render of it (before it
  // lists and sorts it): each pass's beforeSort(renderer, scene, camera,
  // target), which can register what is new and hide it from three before
  // three lists it, so that the renderer draws it from its first frame.
  watchScene(scene) {
    const previous = scene.onBeforeRender;
    scene.onBeforeRender = (...args) => {
      previous?.apply(scene, args);
      if (!this.enabled || !this.active || this.lost || !this.supported()) return;
      for (const pass of this.passes) pass.beforeSort?.(...args);
    };
  }

  // Off: three draws everything again (for comparisons in one battle); on:
  // what is registered moves back.
  setActive(on) {
    on = Boolean(on);
    if (on === this.active) return;
    this.active = on;
    for (const pass of this.passes) pass.setActive?.(on);
  }

  // The near anchor's onAfterRender: the near scene's pass, at the start of
  // three's opaque list.
  pass(renderer, scene, camera) {
    if (camera === this.graphics.camera && renderer.getRenderTarget() === this.graphics.sceneTarget) this.phase("near", renderer, scene, camera);
    // A mirror's render of the near scene (camera.userData.mirror: water.js)
    // draws what the renderer took from three too.
    else if (camera.userData.mirror) this.phase("mirror", renderer, scene, camera);
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
  phase(name, renderer, scene, camera, defer = false, walked = null) {
    if (!this.enabled || !this.active) return;
    if (!this.supported()) { this.restore(); return; }
    const phase = PHASES[name], target = renderer.getRenderTarget();
    if (phase.view !== null) this.writeFrame(scene, camera, phase.view);
    // What the passes have written into the renderer's input so far: records
    // and bones, and where each skeleton's bones are.
    const ctx = { renderer, scene, camera, records: 0, bones: 0, skeletons: new Map(), defer, walked };
    for (const pass of this.phases[name]) pass.draw?.(ctx);
    if (name === "near" && this.recording?.keepFrames > 0) {
      this.recording.keepFrames--;
      this.views();
      this.recording.frames.push({ records: ctx.records, input: this.input.slice(0, this.recordsAt + ctx.records * this.stride), bones: this.bones.slice(0, ctx.bones * 16) });
    }
    const t0 = performance.now();
    const stats = this.phaseStats[name] = phase.draw(this.rust, ctx);
    this.phaseMs[name] = performance.now() - t0;
    // A phase that made no GL call left three's state as it was.
    if (stats?.[4] !== 0) {
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
      renderer[name] = function (...args) { bridge.settleThree?.(); return f.apply(this, args); };
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
    if (!this.enabled || !this.active) return false;
    const target = renderer.getRenderTarget();
    try {
      if (scene.matrixWorldAutoUpdate) scene.updateMatrixWorld();
      scene.onBeforeRender(renderer, scene, camera, target);
      const passes = this.phases[name];
      for (const pass of passes) pass.complete = false;
      this.phase(name, renderer, scene, camera, true);
      const complete = this.enabled && passes.length > 0 && passes.every((pass) => pass.complete === true);
      if (complete) {
        this.resolve(renderer.getRenderTarget());
        if (scene.isScene) scene.onAfterRender(renderer, scene, camera);
      }
      return complete;
    } catch (error) {
      this.fail(error, renderer, target);
      return false;
    }
  }

  // An error in a pass: kept, and everything given back to three for good
  // (restore), three's state reset and the target it was drawing into bound
  // again, so the frame goes on with three.
  fail(error, renderer = this.renderer, target = renderer.getRenderTarget()) {
    this.errors.push(`failed: ${String(error?.stack || error).slice(0, 400)}`);
    console.warn("Rust renderer failed, three draws everything:", error);
    try { this.restore(); } catch (e) { this.errors.push(`restore: ${String(e).slice(0, 200)}`); }
    renderer.resetState();
    renderer.setRenderTarget(target);
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
    const target = renderer.getRenderTarget();
    try { return this.byName.post?.cover(renderer, steps) ?? false; } catch (error) { this.fail(error, renderer, target); return false; }
  }

  // graphics.js: the near scene, drawn in place of three's render of it when
  // nothing in it is left for three (near.js); true when it was.
  coverNear(renderer, scene, camera) {
    return this.byName.near?.cover(renderer, scene, camera) ?? false;
  }

  // hud.js: the battle HUD's batch, drawn by the HUD pass; true when it was.
  coverHud(renderer, hud) {
    const target = renderer.getRenderTarget();
    try { return this.byName.hud?.cover(renderer, hud) ?? false; } catch (error) { this.fail(error, renderer, target); return false; }
  }

  // The near phase's stats and the time its renderer call took, as the
  // measurement tools read them, and what they read of the passes.
  get stats() { return this.phaseStats.near ?? null; }
  get frameMs() { return this.phaseMs.near; }
  get worldCount() { return this.byName.world.count; }
  get worldPending() { return this.byName.world.pending; }
  get bodyRecords() { return this.byName.instances.bodyRecords; }
  get meshRecords() { return this.byName.instances.meshRecords; }

  // Everything back to three (the configuration left what it covers): the
  // near scene's hidden materials, then whatever a pass hid elsewhere.
  restore() {
    this.enabled = false;
    this.errors.push("unsupported configuration: three draws everything");
    this.graphics.scene.traverse((o) => { for (const m of [].concat(o.material || [])) if (m && m.visible === false && m.isShaderMaterial) m.visible = true; });
    for (const pass of this.passes) pass.restore?.();
  }
}
