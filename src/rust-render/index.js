// The Rust battle renderer (rust/bfe-render, built to src/rust-pkg) behind
// the `rust` query parameter: an experiment in drawing the near scene's
// static world and its soldiers with far fewer WebGL calls than three makes
// for them. Three.js still draws everything else and stays the default.
//
// It draws inside the near scene's pass, right after the far scene's image
// (graphics.background): the map's opaque buckets, then every visible
// soldier's body and held gun, instanced. Three's state is reset after it
// (renderer.resetState) and three draws the rest of the scene. What it draws
// three does not (each such material is set invisible, which three's render
// lists respect), but three's scene update still composes their matrices
// and bones, which the page's other code reads. It draws them with programs
// made from the GLSL three compiled for them (shader.rs), so they light as
// three's do.
//
// Only what it supports moves; everything else stays with three, as does
// everything in a configuration it does not cover (shadows, lamp lighting).
//
// It is made of passes, a file each here as in the renderer (renderer.rs):
// the world (world.js), the instanced soldiers, guns and rigs
// (instances.js), and, empty for now, the far scene, terrain, the
// transparent pass, effects, foliage, the first-person view and the post
// passes. Each draws in a phase of the frame: far, near, transparent, view,
// post. This file is their core: the WASM renderer and what every pass
// registers through it (textures, programs, geometries), the frame block,
// typed views of the renderer's memory, and the registry that runs each
// phase's passes and then the renderer's call that draws them.
import * as THREE from "three";
import { lighting } from "../materials.js";
import { packed } from "../point-lights.js";
import { worldPass } from "./world.js";
import { instancesPass } from "./instances.js";
import { farPass } from "./far.js";
import { terrainPass } from "./terrain.js";
import { transparentPass } from "./transparent.js";
import { effectsPass } from "./effects.js";
import { foliagePass } from "./foliage.js";
import { viewPass } from "./view.js";
import { postPass } from "./post.js";

// The views a frame draws, each with a frame block of its own (core.rs
// VIEW_*). This file imports the passes, so a pass reads these inside its
// functions, not as it loads.
export const VIEW_NEAR = 0, VIEW_FAR = 1, VIEW_FIRST_PERSON = 2, VIEW_SPARE = 3;
// The frame block's members (shader.rs FRAME_MEMBERS), in its float layout.
const FRAME = [
  ["projectionMatrix", 0, 16], ["viewMatrix", 16, 16], ["cameraPosition", 32, 3], ["time", 35, 1],
  ["sunDirection", 36, 3], ["bump", 39, 1], ["sunColor", 40, 3], ["shadowStrength", 43, 1],
  ["ambient", 44, 3], ["aaSamples", 47, 1], ["ambientTop", 48, 3], ["terrainShade", 51, 1],
  ["ambientBottom", 52, 3], ["farAaSamples", 55, 1], ["characterAmbient", 56, 3], ["cascadeNear", 59, 1],
  ["fogColor", 60, 3], ["cascadeFar", 63, 1], ["fade", 64, 2], ["resolution", 66, 2], ["fogRange", 68, 2],
];
const FRAME_NAMES = new Set(FRAME.map(([name]) => name));
const LIGHT_COUNT = 70, LIGHTS = 72, MAX_LIGHTS = 16;
const GL_TYPES = { Float32Array: 0x1406, Uint16Array: 0x1403, Uint8Array: 0x1401, Int16Array: 0x1402, Int8Array: 0x1400, Uint32Array: 0x1405 };
// The phases of a frame, in the order three draws them: the view whose frame
// block a phase's passes read (written before they draw; null: none) and
// the renderer's call that draws them (renderer.rs).
const PHASES = {
  far: { view: VIEW_FAR, draw: (rust) => rust.far_frame() },
  near: { view: VIEW_NEAR, draw: (rust, ctx) => rust.frame(ctx.records) },
  transparent: { view: null, draw: (rust) => rust.transparent_frame() },
  view: { view: VIEW_FIRST_PERSON, draw: (rust) => rust.view_frame() },
  post: { view: null, draw: (rust) => rust.post_frame() },
  // The effects at a place of their own in the near scene's transparent
  // list (effects.js), until the transparent pass draws them among its items.
  effects: { view: VIEW_NEAR, draw: (rust) => rust.effects_frame() },
};

// The renderer with each registration call it is given kept (typed arrays
// copied), so another build of it can be given the same scene. The phases'
// calls and the memory layout's are not registration.
function recorder(rust, recording) {
  return new Proxy(rust, {
    get(target, name) {
      const value = target[name];
      if (typeof value !== "function") return value;
      if (/frame$|_(ptr|len)$|^(records_at|record_stride)$/.test(String(name))) return value.bind(target);
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
    // The renderer's inputs: the frame block's values, then the passes'
    // records (f64), where they start and a record's size (instances.rs);
    // and the bones of the rigs drawn (f32).
    this.recordsAt = this.rust.records_at();
    this.stride = this.rust.record_stride();
    this.addView("input", Float64Array, () => this.rust.input_ptr(), () => this.rust.input_len());
    this.addView("bones", Float32Array, () => this.rust.bones_ptr(), () => this.rust.bones_len());
    // The passes, in the renderer's order.
    this.passes = [];
    this.phases = Object.fromEntries(Object.keys(PHASES).map((name) => [name, []]));
    this.byName = {};
    for (const make of [worldPass, instancesPass, farPass, terrainPass, transparentPass, effectsPass, foliagePass, viewPass, postPass]) this.addPass(make(this));
  }

  // A pass: { name, phase, register?(), draw?(ctx), setActive?(on),
  // restore?() }. register() runs once, as it is added; draw(ctx) in each
  // of its phase's frames, in the order the passes were added, to register
  // what it draws (the first time three has drawn it, which gives its
  // material a program) and write its inputs; setActive(on) moves what it
  // registered back to three (off) or to the renderer (on); restore() gives
  // three anything it hid outside the near scene, for good.
  addPass(pass) {
    if (!this.phases[pass.phase]) throw new Error(`rust-render: pass ${pass.name} has no phase ${pass.phase}`);
    this.passes.push(pass);
    this.phases[pass.phase].push(pass);
    this.byName[pass.name] = pass;
    pass.register?.();
  }

  // Three's config the renderer covers.
  supported() {
    const settings = this.graphics.settings;
    return !this.renderer.shadowMap.enabled && !settings.get("lampLighting") && this.graphics.upscaling !== "temporal";
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
  program(threeProgram, kind, material, policy) {
    let entry = this.programs.get(threeProgram);
    if (entry !== undefined) return entry;
    entry = null;
    const gl = this.gl, p = threeProgram.program;
    const names = [];
    let ok = true;
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
      const u = gl.getActiveUniform(p, i), name = u.name.replace(/\[0\]$/, "");
      names.push(name);
      if (FRAME_NAMES.has(name)) {
        // A frame value this material keeps of its own can't come from the block.
        if (!["projectionMatrix", "viewMatrix", "cameraPosition"].includes(name) && material.uniforms?.[name] !== lighting[name]) ok = false;
      } else if (!policy.uniforms.has(name)) ok = false;
      if (!ok) { this.errors.push(`p${threeProgram.id} ${name}`); break; }
    }
    if (ok) {
      const attributes = [];
      for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) {
        const a = gl.getActiveAttrib(p, i);
        attributes.push(`${a.name}:${gl.getAttribLocation(p, a.name)}`);
      }
      const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
      try {
        entry = { id: this.rust.add_program(kind, gl.getShaderSource(vs), gl.getShaderSource(fs), attributes.join(","), names.filter((name) => policy.material.has(name)).join(",")),
          attributes: attributes.map((a) => { const [name, location] = a.split(":"); return [name, Number(location)]; }), names: new Set(names) };
      } catch (error) {
        this.errors.push(`p${threeProgram.id}: ${String(error).slice(0, 300)}`);
      }
    }
    this.programs.set(threeProgram, entry);
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
    if (view !== VIEW_FAR) {
      const far = this.renderer.properties.get(this.graphics.farTarget.texture).__webglTexture;
      if (far !== this.farTexture) { this.farTexture = far; this.rust.set_far_map(this.texture(this.graphics.farTarget.texture)); }
    }
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

  // Off: three draws everything again (for comparisons in one battle); on:
  // what is registered moves back.
  setActive(on) {
    on = Boolean(on);
    if (on === this.active) return;
    this.active = on;
    for (const pass of this.passes) pass.setActive?.(on);
  }

  // graphics.background's onAfterRender: the near scene's pass, the far
  // scene's image just drawn behind it.
  pass(renderer, scene, camera) {
    if (camera !== this.graphics.camera || renderer.getRenderTarget() !== this.graphics.sceneTarget) return;
    this.phase("near", renderer, scene, camera);
  }

  // A phase of the frame, at its point in three's: the frame block written
  // for its view, its passes in order, then the renderer's call that draws
  // them. Three's state is reset after it, and the target it was drawing
  // into bound again.
  phase(name, renderer, scene, camera) {
    if (!this.enabled || !this.active) return;
    if (!this.supported()) { this.restore(); return; }
    const phase = PHASES[name], target = renderer.getRenderTarget();
    if (phase.view !== null) this.writeFrame(scene, camera, phase.view);
    // What the passes have written into the renderer's input so far: records
    // and bones, and where each skeleton's bones are.
    const ctx = { renderer, scene, camera, records: 0, bones: 0, skeletons: new Map() };
    for (const pass of this.phases[name]) pass.draw?.(ctx);
    if (name === "near" && this.recording?.keepFrames > 0) {
      this.recording.keepFrames--;
      this.views();
      this.recording.frames.push({ records: ctx.records, input: this.input.slice(0, this.recordsAt + ctx.records * this.stride), bones: this.bones.slice(0, ctx.bones * 16) });
    }
    const t0 = performance.now();
    this.phaseStats[name] = phase.draw(this.rust, ctx);
    this.phaseMs[name] = performance.now() - t0;
    renderer.resetState();
    renderer.setRenderTarget(target);
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
