import { ownedWorldProgram } from "./world-programs.js";
// The far pass (far.rs): the far scene (graphics.farScene, seen from the
// far camera into graphics.farTarget), drawn by the renderer in place of
// three's render of it. graphics.js asks the core to cover the far phase
// first (index.js cover(): three's start of render, the phase, three's end);
// when this pass drew it all, three doesn't render the far scene. A water's
// mirror (water.js reflect) asks the same for its far scene (the farMirror
// phase: the mirror camera, its sky pass and then the rest drawn over it).
//
// Each frame (draw(), in the far phase) it walks the far scene as three's
// projectObject does (what is shown, on the camera's layers) and finds each
// mesh three would draw among its items, registering one the first time its
// material has a program (graphics.warmup compiles them at load; else three
// draws it once first). The far terrain is one item for all its runs. When
// every mesh three would draw is an item, the renderer clears the target as
// three would and draws them in three's order (complete); else (a mesh it
// can't draw yet) it draws nothing and three draws the frame. Nothing is
// hidden from three: whichever renderer draws each render of the far scene
// (the eye's, a water mirror's, the sky probe's), the other has it all.
//
// A material that reads only the frame's shared values gets a program with
// the frame block; any other (the water's, which keeps a time of its own) a
// plain one, all of whose uniforms the page writes each frame. The map's
// zero materials' own values are fixed at registration; any other
// material's are written each frame.
import * as THREE from "three";
import { compileFor, unbuilt } from "./instances.js";
import { ownedEffectProgram } from "./effect-material.js";
import { ownsEffectSource } from "./programs.js";
import { lighting } from "../materials.js";
import { VIEW_FAR } from "./index.js";


// add_program's kinds (web/mod.rs).
const WORLD = 0, PLAIN = 3;
// The far input's layout and the uniform sources and item flags (far.rs).
const CLEAR = 0, DRAW = 4, KEEP = 5, MIPMAPS = 6, NEAR = 7, SLOTS = 8;
const SRC = { FLOATS: 0, INPUT: 1, TEXTURE: 2, TEXTURE_INPUT: 3, PROJECTION: 4, VIEW: 5, EYE: 6, MODEL: 7, MODEL_VIEW: 8, NORMAL: 9, CUBE_INPUT: 10 };
const F = { DEPTH_TEST: 4, DEPTH_WRITE: 8, COLOR_WRITE: 16, A2C: 32, TRANSPARENT: 64, PLACED: 128, CULLED: 256, BLEND: 512 };
// The frame block's members (shader.rs FRAME_MEMBERS); those three takes from
// the camera, and the object's matrices.
const FRAME_NAMES = new Set(["projectionMatrix", "viewMatrix", "cameraPosition", "time", "sunDirection", "bump", "sunColor", "shadowStrength",
  "ambient", "aaSamples", "ambientTop", "terrainShade", "ambientBottom", "farAaSamples", "characterAmbient", "cascadeNear", "fogColor",
  "cascadeFar", "fade", "resolution", "fogRange"]);
const CAMERA = { projectionMatrix: SRC.PROJECTION, viewMatrix: SRC.VIEW, cameraPosition: SRC.EYE };
const OBJECT = { modelMatrix: SRC.MODEL, modelViewMatrix: SRC.MODEL_VIEW, normalMatrix: SRC.NORMAL };
// GL uniform types the renderer sets, and their floats an element.
const SAMPLER_2D = 0x8B5E, SAMPLER_CUBE = 0x8B60;
const FLOATS = { 0x1406: 1, 0x8B50: 2, 0x8B51: 3, 0x8B52: 4, 0x1404: 1, 0x8B56: 1, 0x8B5B: 9, 0x8B5C: 16, [SAMPLER_2D]: 1, [SAMPLER_CUBE]: 1 };
const IDENTITY = new THREE.Matrix4();
// The layer a near-scene item the pass draws goes on (no camera renders it:
// graphics.js uses 1, 2 and 4, sky-probe.js 3, water.js 5; instances.js's
// flying parts this one too).
const HIDDEN_LAYER = 30;
// core.program's policy for a program with the frame block: every uniform it
// reads besides the block is the far pass's to set (none through the core's
// table).
const ANY = { uniforms: { has: () => true }, material: { has: () => false } };

// A uniform's value as the floats three would upload, appended to `out`;
// false when it isn't one.
function floatsOf(value, out) {
  if (typeof value === "number") out.push(value);
  else if (typeof value === "boolean") out.push(value ? 1 : 0);
  else if (value?.isVector2) out.push(value.x, value.y);
  else if (value?.isVector3) out.push(value.x, value.y, value.z);
  else if (value?.isVector4 || value?.isQuaternion) out.push(value.x, value.y, value.z, value.w);
  else if (value?.isColor) out.push(value.r, value.g, value.b);
  else if (value?.isMatrix3 || value?.isMatrix4) for (const v of value.elements) out.push(v);
  else if (Array.isArray(value) || ArrayBuffer.isView(value)) { for (const v of value) if (!floatsOf(v, out)) return false; }
  else return false;
  return true;
}

// A material's blending as three's setMaterial sets it: [equation rgb,
// alpha, source rgb, destination rgb, source alpha, destination alpha], null
// for none, undefined for what the renderer doesn't set (a constant colour).
function blendOf(material, gl) {
  if (material.blending === THREE.NoBlending || (material.blending === THREE.NormalBlending && !material.transparent)) return null;
  const add = gl.FUNC_ADD, pre = material.premultipliedAlpha;
  switch (material.blending) {
    case THREE.NormalBlending: return pre ? [add, add, gl.ONE, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA] : [add, add, gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA];
    case THREE.AdditiveBlending: return pre ? [add, add, gl.ONE, gl.ONE, gl.ONE, gl.ONE] : [add, add, gl.SRC_ALPHA, gl.ONE, gl.SRC_ALPHA, gl.ONE];
    case THREE.SubtractiveBlending: return [add, add, gl.ZERO, gl.ONE_MINUS_SRC_COLOR, gl.ZERO, gl.ONE];
    case THREE.MultiplyBlending: return pre ? [add, add, gl.ZERO, gl.SRC_COLOR, gl.ZERO, gl.SRC_ALPHA] : [add, add, gl.ZERO, gl.SRC_COLOR, gl.ZERO, gl.SRC_COLOR];
    case THREE.CustomBlending: {
      const equations = { [THREE.AddEquation]: gl.FUNC_ADD, [THREE.SubtractEquation]: gl.FUNC_SUBTRACT, [THREE.ReverseSubtractEquation]: gl.FUNC_REVERSE_SUBTRACT, [THREE.MinEquation]: gl.MIN, [THREE.MaxEquation]: gl.MAX };
      const factors = { [THREE.ZeroFactor]: gl.ZERO, [THREE.OneFactor]: gl.ONE, [THREE.SrcColorFactor]: gl.SRC_COLOR, [THREE.SrcAlphaFactor]: gl.SRC_ALPHA,
        [THREE.SrcAlphaSaturateFactor]: gl.SRC_ALPHA_SATURATE, [THREE.DstColorFactor]: gl.DST_COLOR, [THREE.DstAlphaFactor]: gl.DST_ALPHA,
        [THREE.OneMinusSrcColorFactor]: gl.ONE_MINUS_SRC_COLOR, [THREE.OneMinusSrcAlphaFactor]: gl.ONE_MINUS_SRC_ALPHA,
        [THREE.OneMinusDstColorFactor]: gl.ONE_MINUS_DST_COLOR, [THREE.OneMinusDstAlphaFactor]: gl.ONE_MINUS_DST_ALPHA };
      const b = [equations[material.blendEquation], equations[material.blendEquationAlpha || material.blendEquation],
        factors[material.blendSrc], factors[material.blendDst], factors[material.blendSrcAlpha || material.blendSrc], factors[material.blendDstAlpha || material.blendDst]];
      return b.every((v) => v !== undefined) ? b : undefined;
    }
    default: return undefined;
  }
}

// three's depth functions (WebGLState).
const DEPTH = { [THREE.NeverDepth]: 0x0200, [THREE.AlwaysDepth]: 0x0207, [THREE.LessDepth]: 0x0201, [THREE.LessEqualDepth]: 0x0203,
  [THREE.EqualDepth]: 0x0202, [THREE.GreaterEqualDepth]: 0x0206, [THREE.GreaterDepth]: 0x0204, [THREE.NotEqualDepth]: 0x0205 };

class FarPass {
  constructor(core) {
    this.core = core;
    this.name = "far";
    this.phase = "far";
    this.items = new Map(); // object -> its items (one a material and program)
    this.all = []; // every item
    this.tables = new Map(); // Rust program -> its uniform table, or null
    this.plainPrograms = new Map(); // three's program -> a plain Rust program, or null
    this.failed = new WeakMap(); // object -> the material it couldn't be an item with
    // The map's far buckets that share a vertex array (share()): mesh -> {
    // its layout's key, where its index starts }; layout -> its geometry.
    this.sharing = null;
    this.layouts = new Map();
    this.nextSlot = SLOTS;
    // This frame's meshes ({ object, material, item }), the first `count`.
    this.found = [];
    this.count = 0;
    this.scratch = [];
    this.color = new THREE.Color();
    this.frustum = new THREE.Frustum();
    this.matrix = new THREE.Matrix4();
    // Ownership diagnostics: an unclaimed visible far mesh makes the
    // required phase incomplete.
    this.complete = false;
    this.mode = "incomplete";
    this.unclaimed = 0;
    // The near scene's items (drawNear): mesh -> its own layers' mask, while
    // it is hidden from three.
    this.hidden = new Map();
  }

  register() {
    this.core.addView("farInput", Float64Array, () => this.core.rust.far_input_ptr(), () => this.core.rust.far_input_len());
    this.core.addView("farVertices", Float32Array, () => this.core.rust.far_vertices_ptr(), () => this.core.rust.far_vertices_len());
  }

  // Register new resources and write the frame inputs. Every visible item
  // needs a Rust draw; incomplete coverage fails the battle.
  draw(ctx) {
    this.frame(ctx);
  }

  // The same resource table serves main far views, reflected views and probes.
  // Only frame/camera inputs change; registration must not depend on which view
  // happened to draw an object first. Every selected item remains Rust-owned.
  frame(ctx) {
    const { renderer, scene, camera } = ctx, core = this.core;
    this.near = false;
    this.count = 0;
    this.unclaimed = 0;
    this.camera = camera;
    this.scene = scene;
    // A sky probe's face (sky-probe.js): which one (far_probe_frame).
    const cube = camera.parent?.type === "CubeCamera" ? camera.parent : null;
    ctx.face = cube ? Math.max(0, cube.children.indexOf(camera)) : 0;
    // Its cube's mipmaps, made after the face drawn now as three's render
    // makes them (updateRenderTargetMipmap: whenever the texture asks for
    // them; CubeCamera.update holds them back until the last face): 1 + the
    // texture's id, else 0. Registered here, before the input is written
    // (registering can move the module's memory).
    const texture = cube?.renderTarget?.texture;
    this.mipmaps = texture?.generateMipmaps ? 1 + core.texture(texture) : 0;
    if (!this.sharing) this.share(scene);
    this.walk(scene, camera, 0);
    this.frustum.setFromProjectionMatrix(this.matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    for (let i = 0; i < this.count; i++) {
      const f = this.found[i];
      if (f.item) continue;
      if (f.object.frustumCulled && !f.object.isGroup && !this.frustum.intersectsObject(f.object)) continue;
      this.unclaimed++;
    }
    const complete = this.unclaimed === 0;
    if (complete) this.prepare(renderer, scene, camera);
    this.write(renderer, complete);
    this.complete = complete;
    this.mode = complete ? "full" : "incomplete";
  }

  // The near phase's part (a mirror's too): the near scene's meshes the pass
  // draws in three's place, the Enhanced ocean's surface (ocean-surface.js:
  // its near mesh, which shares its material with the far one), drawn after
  // the near scene's opaque passes (far.rs draw_near). Registered as any far
  // item; hidden from three by its layers from the next frame on (three
  // listed it in this render already: it draws it this once), its own layers
  // kept and tested against the camera's as three would.
  // The near ocean shares the far machinery but has the near camera range.
  // Register it before drawing rather than forcing a native first-use pass.
  // Its transparent ordering still belongs to the authored scene material.
  adoptNear(scene, camera) {
    const sea = this.core.graphics.scene.userData.water?.oceanSurface?.near;
    if (!sea || this.hidden.has(sea)) return;
    this.camera = camera;
    this.scene = scene;
    this.count = 0;
    this.find(sea, sea.material, 0);
    if (!this.found[0].item) this.core.fail(new Error("Unregistered ocean surface"));
    this.hidden.set(sea, sea.layers.mask);
    sea.layers.set(HIDDEN_LAYER);
  }

  drawNear(ctx) {
    const { renderer, scene, camera } = ctx, core = this.core;
    this.near = true;
    this.count = 0;
    this.unclaimed = 0;
    this.camera = camera;
    this.scene = scene;
    this.mipmaps = 0;
    const sea = core.graphics.scene.userData.water?.oceanSurface?.near ?? null;
    let draw = false;
    if (sea) {
      let shown = Boolean(sea.material?.visible || ownsEffectSource(sea.material));
      for (let n = sea; shown && n && n !== scene; n = n.parent) if (!n.visible) shown = false;
      if (shown && (this.ownLayers(sea) & camera.layers.mask) !== 0) {
        this.find(sea, sea.material, 0);
        const item = this.found[this.count - 1].item;
        if (item) {
          if (!this.hidden.has(sea)) { this.hidden.set(sea, sea.layers.mask); sea.layers.set(HIDDEN_LAYER); }
          draw = true;
        }
      }
    }
    if (draw) this.prepare(renderer, scene, camera);
    this.write(renderer, draw);
  }

  // A mesh's own layers' mask (kept while it is hidden).
  ownLayers(mesh) {
    return this.hidden.get(mesh) ?? mesh.layers.mask;
  }


  // The far scene as three's projectObject walks it: each mesh it would draw
  // (shown, on the camera's layers), with its group order; the terrain whole.
  // Mirror/probe traversal preserves visibility and parent render order.
  // A resource missed here cannot be silently left to the browser renderer;
  // validation reports the unsupported object before the frame is submitted.
  walk(object, camera, groupOrder) {
    if (object.visible === false) return;
    if (object.layers.test(camera.layers)) {
      if (object.isGroup) groupOrder = object.renderOrder;
      if (object.userData.patches) { this.find(object, object.children[0]?.material, groupOrder); return; }
      if (object.isMesh || object.isLine || object.isPoints || object.isSprite) {
        const material = object.material;
        if (Array.isArray(material) ? material.some((m) => m?.visible || ownsEffectSource(m)) : material?.visible || ownsEffectSource(material)) this.find(object, material, groupOrder);
      }
    }
    const children = object.children;
    for (let i = 0; i < children.length; i++) this.walk(children[i], camera, groupOrder);
  }

  // Its item for the material and program it has now, registered if it can be.
  find(object, material, groupOrder) {
    if (material && !ownsEffectSource(material) && material.userData.bfeFamily !== "world" && !Array.isArray(material) && unbuilt(this.core.renderer, material)) {
      const mesh = object.userData.patches ? object.children[0] : object;
      compileFor(this.core.renderer, [[mesh, material]], this.camera, this.scene);
    }
    const program = ownsEffectSource(material) ? material.userData.bfeProgram : material?.userData?.bfeFamily === "world" ? ownedWorldProgram(this.core, material, this.scene, this.camera) : material && !Array.isArray(material) ? this.core.renderer.properties.get(material).currentProgram : null;
    let item = null;
    for (const i of this.items.get(object) || []) if (i.material === material && i.threeProgram === program) { item = i; break; }
    if (!item && program && this.failed.get(object) !== material) {
      // null: never (as it is); undefined: not yet (a texture still loading).
      item = this.registerItem(object, material, program, groupOrder, this.camera);
      if (item) { let list = this.items.get(object); if (!list) this.items.set(object, list = []); list.push(item); this.all.push(item); } else if (item === null) this.failed.set(object, material);
      item ||= null;
    }
    // This frame's meshes, in entries kept from frame to frame.
    const f = this.found[this.count] ??= {};
    f.object = object; f.material = material; f.item = item;
    this.count++;
  }

  // An item for a mesh of the far scene (or the far terrain's group); null
  // when the renderer can't draw it as three would, undefined when it can't
  // yet (a texture three hasn't made yet).
  // Import the material program once, including its actual active uniforms.
  // Some far shaders consume the shared frame block; others need object/camera
  // values supplied separately. Keep that distinction instead of zeroing an
  // unrecognised uniform merely because the object is far from the camera.
  registerItem(object, material, threeProgram, groupOrder, camera) {
    const core = this.core, rust = core.rust, gl = core.gl;
    // The terrain's group: its patches (terrain-patches.js) and level.
    const terrain = object.userData.patches ? { patches: object.userData.patches, reduced: object.userData.reduced } : null;
    const mesh = terrain ? object.children[0] : object;
    if (!mesh?.isMesh || mesh.isSkinnedMesh || mesh.isBatchedMesh || mesh.morphTargetInfluences) return null;
    // An instanced mesh three culls by its instances' bounds: not here.
    const instanced = Boolean(mesh.isInstancedMesh);
    if (instanced && mesh.frustumCulled) return null;
    if (Array.isArray(material) || !(material.isShaderMaterial || ownsEffectSource(material) || material.userData?.bfeFamily === "world") || material.wireframe || material.stencilWrite || material.polygonOffset || material.clippingPlanes?.length) return null;
    // three draws a two-sided blended material in two passes.
    if (material.transparent && material.side === THREE.DoubleSide && !material.forceSinglePass) return null;
    if (mesh.matrixWorld.determinant() < 0) return null;
    const geometry = mesh.geometry;
    if (geometry.groups.length || Object.keys(geometry.morphAttributes).length) return null;
    // Unindexed (a full-screen triangle, the storm's bolt): drawn through an
    // index of the pass's own, 0, 1, 2..., over the draw range three draws
    // each frame.
    const unindexed = !geometry.index;
    if (unindexed && (instanced || !geometry.attributes.position || geometry.attributes.position.isInterleavedBufferAttribute)) return null;
    const blend = blendOf(material, gl);
    if (blend === undefined || DEPTH[material.depthFunc] === undefined) return null;
    // Its uniforms: the frame block's when every frame value it reads is
    // the shared one, else each its own.
    const active = [];
    let shared, program;
    if (ownsEffectSource(material)) {
      let root = object;
      while (root.parent) root = root.parent;
      program = ownedEffectProgram(core, material, [WORLD, PLAIN], material.side, root === core.graphics.scene && core.shadowsOn());
      shared = program.kind === "World";
      const types = { 1: gl.FLOAT, 2: gl.FLOAT_VEC2, 3: gl.FLOAT_VEC3, 4: gl.FLOAT_VEC4, 5: gl.INT, 9: gl.FLOAT_MAT3, 16: gl.FLOAT_MAT4, 32: gl.SAMPLER_2D, 33: gl.SAMPLER_3D, 34: gl.SAMPLER_2D_SHADOW, 35: gl.SAMPLER_CUBE };
      for (const u of program.uniforms) active.push({ name: u.name, type: types[u.type], size: u.size });
    } else {
      const p = threeProgram.program;
      if (threeProgram.owned) active.push(...threeProgram.inputs.map(u => ({...u, name:u.name.replace(/\[0\]$/, "")})));
      for (let i = 0, n = threeProgram.owned ? 0 : gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
        const u = gl.getActiveUniform(p, i);
        active.push({ name: u.name.replace(/\[0\]$/, ""), type: u.type, size: u.size });
      }
      shared = active.every(({ name }) => !FRAME_NAMES.has(name) || name in CAMERA || material.uniforms?.[name] === lighting[name]);
      program = threeProgram.owned ? threeProgram : shared ? core.program(threeProgram, WORLD, material, ANY) : this.plainProgram(threeProgram);
    }
    if (!program) return null;
    const own = shared ? active.filter(({ name }) => !FRAME_NAMES.has(name)) : active;
    // A zero material's values are its own and never change (materials.js):
    // fixed at registration. Any other's are written each frame.
    const fixed = shared && material.name?.startsWith("zero:");
    let table = this.tables.get(program.id);
    if (table === undefined) {
      if (own.some((u) => FLOATS[u.type] === undefined)) table = null;
      else {
        const id = rust.far_table(program.id, own.map((u) => u.name).join(","), new Uint32Array(own.map((u) => u.type)), new Uint32Array(own.map((u) => u.size)));
        table = id === undefined ? null : { id };
      }
      this.tables.set(program.id, table);
    }
    if (!table) return null;
    // Where each comes from: the camera and the object's matrices; else the
    // material's value, fixed (a zero material's) or written each frame
    // into the input (a plain one's).
    const placed = own.some((u) => u.name in OBJECT) || !mesh.matrixWorld.equals(IDENTITY) || (mesh.matrixAutoUpdate && !terrain);
    const slot = this.nextSlot;
    let at = slot + 1 + (placed ? 16 : 0) + (terrain ? 1 : 0);
    const sources = [], dynamic = [], watched = [];
    for (let index = 0; index < own.length; index++) {
      const { name, type, size } = own[index];
      if (type === SAMPLER_CUBE) {
        // A cube map (the Enhanced sky probe: sky-probe.js), named each
        // frame; none (the Classic flyers') is never read.
        if (!(name in (material.uniforms || {}))) return null;
        dynamic.push({ name, at, sampler: true, cube: true, id: -1 });
        sources.push([index, SRC.CUBE_INPUT, null, at]);
        at += 1;
        continue;
      }
      if (name in CAMERA) { sources.push([index, CAMERA[name], null, 0]); continue; }
      if (name in OBJECT) { sources.push([index, OBJECT[name], null, 0]); continue; }
      if (name === "isOrthographic") { sources.push([index, SRC.FLOATS, [camera.isOrthographicCamera ? 1 : 0], 0]); continue; }
      if (!(name in (material.uniforms || {}))) return null;
      const value = material.uniforms[name].value;
      if (type === SAMPLER_2D) {
        if (fixed) {
          // None: three binds its empty texture (a texel of zeros) there.
          if (value === null || value === undefined) {
            const id = core.texture(this.emptyTexture());
            if (id < 0) return undefined;
            sources.push([index, SRC.TEXTURE, null, id]);
            continue;
          }
          if (!value?.isTexture) return null;
          const id = core.texture(value);
          if (id < 0) return undefined;
          sources.push([index, SRC.TEXTURE, null, id]);
          // A data texture's contents can change (three uploads them before
          // its next draw).
          if (value.isDataTexture) watched.push(value);
        } else {
          dynamic.push({ name, at, sampler: true, id: -1 });
          sources.push([index, SRC.TEXTURE_INPUT, null, at]);
          at += 1;
        }
        continue;
      }
      const floats = [];
      const want = FLOATS[type] * Math.max(1, size);
      if (!floatsOf(value, floats) || floats.length !== want) return null;
      if (fixed) sources.push([index, SRC.FLOATS, floats, 0]);
      else {
        dynamic.push({ name, at, floats: want, sampler: false });
        sources.push([index, SRC.INPUT, null, at]);
        at += want;
      }
    }
    // An instanced mesh's per-instance attributes (its matrices, its colours,
    // its geometry's instanced ones), each written each frame after the
    // instance count; the geometry's others are the core's.
    const perInstance = (name) => instanced && (name === "instanceMatrix" || name === "instanceColor" || Boolean(geometry.attributes[name]?.isInstancedBufferAttribute));
    const instances = [];
    let countAt = -1;
    if (instanced) {
      const max = mesh.instanceMatrix.count;
      countAt = at++;
      for (const [name, location] of program.attributes) {
        if (!perInstance(name)) continue;
        const attribute = name === "instanceMatrix" ? mesh.instanceMatrix : name === "instanceColor" ? mesh.instanceColor : geometry.attributes[name];
        if (!attribute?.array || attribute.isInterleavedBufferAttribute || (attribute.meshPerAttribute ?? 1) !== 1 || !(attribute.array instanceof Float32Array)) return null;
        const size = attribute.itemSize;
        if (size > 4 && size !== 16) return null;
        instances.push({ attribute, location, columns: size > 4 ? 4 : 1, rows: size > 4 ? 4 : size, max, at, size });
        at += max * size;
      }
    }
    // An unindexed geometry's range (first index, count), written each frame;
    // the buffers of its attributes three uploads anew (DynamicDrawUsage: the
    // storm's bolt) refreshed when their versions move: their values in the
    // pass's vertices, and the slot saying they were written anew.
    const rangeAt = unindexed ? at : -1;
    if (unindexed) at += 2;
    const share = this.sharing?.get(mesh);
    const made = unindexed ? this.unindexedGeometry(geometry, program) : null;
    const g = share ? this.layoutGeometry(share.key, geometry, program) : made ? made.g
      : core.geometry(geometry, instanced ? { id: program.id, attributes: program.attributes.filter(([name]) => !perInstance(name)) } : program);
    if (g === null || g === undefined) return null;
    const refresh = [];
    let refreshAt = -1;
    for (const { attribute, slot } of made?.buffers ?? []) {
      if (attribute.usage !== THREE.DynamicDrawUsage) continue;
      if (refreshAt < 0) refreshAt = at++;
      const from = this.nextVertex ?? 0, floats = attribute.array.length;
      if (from + floats > core.rust.far_vertices_len()) return null;
      this.nextVertex = from + floats;
      refresh.push({ attribute, slot, at: from, floats, version: -1 });
    }
    if (at > core.rust.far_input_len()) return null;
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    const s = geometry.boundingSphere;
    const flags = material.side | (material.depthTest ? F.DEPTH_TEST : 0) | (material.depthWrite ? F.DEPTH_WRITE : 0) | (material.colorWrite ? F.COLOR_WRITE : 0)
      | (material.alphaToCoverage ? F.A2C : 0) | (material.transparent ? F.TRANSPARENT : 0) | (placed ? F.PLACED : 0)
      | (mesh.frustumCulled && !terrain ? F.CULLED : 0) | (blend ? F.BLEND : 0);
    const id = rust.far_item(program.id, table.id, g, flags, DEPTH[material.depthFunc], new Uint32Array(blend || [0, 0, 0, 0, 0, 0]),
      new Float64Array([groupOrder, (terrain ? mesh : object).renderOrder, material.id, mesh.id]), new Float64Array([s.center.x, s.center.y, s.center.z, s.radius]), slot);
    for (const [index, kind, floats, from] of sources) rust.far_source(id, index, kind, new Float32Array(floats || []), from);
    for (const i of instances) rust.far_instanced(id, i.location, i.columns, i.rows, i.max, countAt, i.at);
    if (share) rust.far_base(id, share.base);
    for (const r of refresh) rust.far_refresh(id, r.slot, r.at, r.floats, refreshAt);
    if (unindexed) {
      rust.far_range_input(id, rangeAt);
    } else if (terrain) {
      // Its levels, as terrain-patches.js keeps them (the reduced one made
      // the first time it is drawn).
      this.level(id, terrain.patches.full, 0);
      if (terrain.patches.reduced) this.level(id, terrain.patches.reduced, 1);
    } else if (mesh.userData.cellLights) {
      // The positions alone (the map's vertices are interleaved).
      const position = geometry.attributes.position, positions = new Float32Array(position.count * 3);
      for (let i = 0; i < position.count; i++) { positions[3 * i] = position.getX(i); positions[3 * i + 1] = position.getY(i); positions[3 * i + 2] = position.getZ(i); }
      const index = geometry.index.array;
      rust.far_cells(id, positions, index instanceof Uint32Array ? index : Uint32Array.from(index), 48);
    } else {
      // Its own index's range (a shared geometry's whole index is others' too).
      const range = geometry.drawRange, count = geometry.index.count;
      const start = Math.max(0, range.start), end = Math.min(count, range.start + range.count);
      if (share || start !== 0 || end !== count) rust.far_range(id, start, Math.max(0, end - start));
    }
    this.nextSlot = at;
    // Its hooks that set uniforms (the far water's range), for one whose
    // values are written each frame.
    const hooks = !fixed && [mesh.onBeforeRender !== THREE.Object3D.prototype.onBeforeRender ? "object" : null,
      material.onBeforeRender !== THREE.Material.prototype.onBeforeRender ? "material" : null].filter(Boolean);
    return { id, object, mesh, material, threeProgram, slot, placed, terrain, levels: terrain ? [true, Boolean(terrain.patches.reduced)] : null, level: 0,
      dynamic, watched, hooks: hooks || [], instances, countAt, rangeAt, refresh, refreshAt };
  }

  // An unindexed geometry's copy for the renderer: each attribute the
  // program reads (one buffer each, its slot there), and an index 0, 1,
  // 2... over every vertex. { g, buffers: [{ attribute, slot }] }, or null.
  // Non-indexed sky geometry still needs a retained VAO and explicit vertices.
  // The renderer must preserve its triangles rather than hand it back to Three.
  // Registration is cold; the resulting geometry id is reused by every view.
  unindexedGeometry(geometry, program) {
    const rust = this.core.rust, position = geometry.attributes.position, count = position.count;
    const buffers = [];
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      if (!a || a.isInterleavedBufferAttribute || !(a.array instanceof Float32Array) || a.count !== count) return null;
      buffers.push({ attribute: a, location });
    }
    const g = rust.add_geometry();
    for (const b of buffers) {
      const array = b.attribute.array;
      b.slot = rust.geometry_buffer(g, new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
      rust.geometry_pointer(g, b.slot, b.location, b.attribute.itemSize, 0x1406, b.attribute.normalized, 0, 0);
    }
    const index = new Uint32Array(count);
    for (let i = 0; i < count; i++) index[i] = i;
    rust.geometry_index(g, new Uint8Array(index.buffer), 0x1405);
    return { g, buffers };
  }

  // The map's far buckets (zero materials, their vertices in the world) that
  // read the same attributes laid out alike share one vertex array: their
  // vertices in one buffer and their indices in one, each bucket's offset to
  // its vertices and starting where the last ended (far.rs base), so a row
  // of them in three's order binds nothing between draws. Made once, for
  // every bucket with a program when the far pass first draws (warmup made
  // them); a bucket that comes later keeps a geometry of its own.
  // Terrain levels and placed meshes may share one backing geometry. Cache
  // that allocation while retaining each item's independent range and bounds.
  // Otherwise a camera's last culling result could become another view's base.
  share(scene) {
    this.sharing = new Map();
    const renderer = this.core.renderer, groups = new Map();
    scene.traverse((o) => {
      const m = o.material;
      if (!o.isMesh || o.isInstancedMesh || o.parent?.userData.patches || !m || Array.isArray(m) || !m.name?.startsWith("zero:")) return;
      if (!("FAR" in m.defines) || !("WORLD_SPACE" in m.defines)) return;
      const program = renderer.properties.get(m).currentProgram;
      const key = program && this.layoutKey(o.geometry, program);
      if (!key) return;
      let list = groups.get(key);
      if (!list) groups.set(key, list = []);
      list.push(o);
    });
    for (const [key, list] of groups) {
      if (list.length < 2) continue;
      const stride = list[0].geometry.attributes.position.data.stride;
      let vertices = 0, indices = 0;
      for (const o of list) { vertices += o.geometry.attributes.position.data.count; indices += o.geometry.index.count; }
      const vertex = new Float32Array(vertices * stride), index = new Uint32Array(indices);
      let v = 0, i = 0;
      for (const o of list) {
        const data = o.geometry.attributes.position.data, own = o.geometry.index.array;
        vertex.set(data.array, v * stride);
        for (let k = 0; k < own.length; k++) index[i + k] = own[k] + v;
        this.sharing.set(o, { key, base: i });
        v += data.count;
        i += own.length;
      }
      this.layouts.set(key, { vertex, index, stride, g: null });
    }
  }

  // The layout a bucket's program reads its attributes in (each from one
  // interleaved buffer of floats: name, location, size, offset), or null.
  layoutKey(geometry, threeProgram) {
    const gl = this.core.gl, p = threeProgram.program, parts = [];
    let data = null;
    if (!(geometry.index?.array instanceof Uint32Array) || geometry.groups.length) return null;
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) {
      const name = gl.getActiveAttrib(p, i).name, a = geometry.attributes[name];
      if (!a?.isInterleavedBufferAttribute || !(a.data.array instanceof Float32Array) || (data && a.data !== data)) return null;
      data = a.data;
      parts.push(`${name}:${gl.getAttribLocation(p, name)}:${a.itemSize}:${a.offset}:${a.normalized}`);
    }
    return data ? `${data.stride}|${parts.sort().join(",")}` : null;
  }

  // A layout's geometry, made the first time a bucket of it registers.
  // Attribute layouts belong to the linked program, not just the BufferGeometry.
  // The same source mesh can be compiled with different active attributes.
  // A cached VAO is reusable only for the same explicit layout key.
  layoutGeometry(key, geometry, program) {
    const layout = this.layouts.get(key), rust = this.core.rust;
    if (layout.g !== null) return layout.g;
    const g = rust.add_geometry();
    const buffer = rust.geometry_buffer(g, new Uint8Array(layout.vertex.buffer));
    for (const [name, location] of program.attributes) {
      const a = geometry.attributes[name];
      rust.geometry_pointer(g, buffer, location, a.itemSize, 0x1406, a.normalized, layout.stride * 4, a.offset * 4);
    }
    rust.geometry_index(g, new Uint8Array(layout.index.buffer), 0x1405);
    layout.g = g;
    layout.vertex = layout.index = null;
    return g;
  }

  // A terrain level's index and cells.
  level(id, level, n) {
    const cells = new Float64Array(level.cells.length * 8);
    level.cells.forEach((cell, i) => {
      const { min, max } = cell.box;
      cells.set([cell.start, cell.end, min.x, min.y, min.z, max.x, max.y, max.z], i * 8);
    });
    const index = level.index.array;
    this.core.rust.far_terrain(id, n, index instanceof Uint32Array ? index : Uint32Array.from(index), cells);
  }

  // What a sampler three is given no texture for reads: three's empty
  // texture, one texel of zeros.
  emptyTexture() {
    if (!this.empty) {
      this.empty = new THREE.DataTexture(new Uint8Array(4), 1, 1);
      this.empty.needsUpdate = true;
    }
    return this.empty;
  }

  // A plain program (Kind::Plain) for three's: its GLSL as it is.
  plainProgram(threeProgram) {
    let entry = this.plainPrograms.get(threeProgram);
    if (entry !== undefined) return entry;
    entry = null;
    const core = this.core, gl = core.gl, p = threeProgram.program;
    const attributes = [];
    for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_ATTRIBUTES); i < n; i++) {
      const a = gl.getActiveAttrib(p, i);
      attributes.push(`${a.name}:${gl.getAttribLocation(p, a.name)}`);
    }
    const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
    try {
      entry = { id: core.rust.add_program(PLAIN, gl.getShaderSource(vs), gl.getShaderSource(fs), attributes.join(","), ""),
        attributes: attributes.map((a) => { const [name, location] = a.split(":"); return [name, Number(location)]; }) };
    } catch (error) {
      core.fail(error);
    }
    this.plainPrograms.set(threeProgram, entry);
    return entry;
  }

  // What this frame's items read that can register anything (and so grow
  // the module's memory): the hooks that set their values (the far water's
  // range), the textures they read now, the terrain's level.
  // Prepare programs/textures before writing views into WASM memory: creating
  // a retained resource may grow the heap and detach an earlier typed view.
  // No draw is needed to initialise these compiler and texture dependencies.
  prepare(renderer, scene, camera) {
    const core = this.core;
    for (let i = 0; i < this.count; i++) {
      const item = this.found[i].item;
      if (!item) continue;
      for (const hook of item.hooks) {
        if (hook === "object") item.mesh.onBeforeRender(renderer, scene, camera, item.mesh.geometry, item.material, null);
        else item.material.onBeforeRender(renderer, scene, camera, item.mesh.geometry, item.mesh, null);
      }
      for (const texture of item.watched) this.upload(texture);
      if (item.terrain) item.level = this.terrainLevel(item);
      const uniforms = item.material.uniforms;
      for (const d of item.dynamic) {
        if (!d.sampler) continue;
        const value = uniforms[d.name].value;
        if (value?.isTexture) this.upload(value);
        // None: three's empty texture, as for a fixed one (a cube sampler
        // given none is never read: none).
        d.id = d.cube ? (value?.isTexture ? core.texture(value) : -1) : core.texture(value?.isTexture ? value : this.emptyTexture());
      }
    }
  }

  // This frame's input: whether the renderer draws, the clear colour, then
  // each item's slots.
  // Write current values into the registered sources, preserving sampler ids.
  // The native texture owner can replace a handle after upload/filter changes;
  // Rust must observe that replacement without allocating a different item.
  // Frame writes never choose a different rendering implementation.
  write(renderer, draw) {
    const core = this.core;
    core.views();
    const input = core.farInput;
    input[DRAW] = draw ? 1 : 0;
    if (!draw) return;
    // WebGLBackground's clear (none with autoClear off: a water mirror's
    // far scene drawn over its sky): the colour (a render target's: the
    // working space), premultiplied by its alpha as the context is.
    input[KEEP] = this.near || !renderer.autoClear ? 1 : 0;
    input[MIPMAPS] = this.mipmaps;
    input[NEAR] = this.near ? 1 : 0;
    renderer.getClearColor(this.color);
    const alpha = renderer.getClearAlpha(), pre = renderer.getContextAttributes()?.premultipliedAlpha !== false ? alpha : 1;
    input[CLEAR] = this.color.r * pre; input[CLEAR + 1] = this.color.g * pre; input[CLEAR + 2] = this.color.b * pre; input[CLEAR + 3] = alpha;
    for (const item of this.all) input[item.slot] = 0;
    const floats = this.scratch;
    for (let i = 0; i < this.count; i++) {
      const item = this.found[i].item;
      if (!item) continue;
      const slot = item.slot;
      input[slot] = 1;
      if (item.placed) input.set(item.mesh.matrixWorld.elements, slot + 1);
      if (item.terrain) input[slot + 1] = item.level;
      if (item.instances.length) {
        const count = Math.min(item.mesh.count, item.instances[0].max);
        input[item.countAt] = count;
        for (const i of item.instances) input.set(i.attribute.array.subarray(0, count * i.size), i.at);
      }
      if (item.rangeAt >= 0) {
        // renderBufferDirect's range of an unindexed geometry.
        const range = item.mesh.geometry.drawRange, vertices = item.mesh.geometry.attributes.position.count;
        const start = Math.max(0, range.start), end = Math.min(vertices, range.start + range.count);
        input[item.rangeAt] = start; input[item.rangeAt + 1] = Math.max(0, end - start);
      }
      if (item.refreshAt >= 0) {
        input[item.refreshAt] = 0;
        if (item.refresh.some((r) => r.attribute.version !== r.version)) {
          for (const r of item.refresh) { core.farVertices.set(r.attribute.array, r.at); r.version = r.attribute.version; }
          input[item.refreshAt] = 1;
        }
      }
      const uniforms = item.material.uniforms;
      for (const d of item.dynamic) {
        if (d.sampler) { input[d.at] = d.id; continue; }
        floats.length = 0;
        floatsOf(uniforms[d.name].value, floats);
        for (let k = 0; k < d.floats; k++) input[d.at + k] = floats[k] ?? 0;
      }
    }
  }

  // The terrain's level this frame (terrain-patches.js reduced()), made and
  // registered the first time it is drawn.
  terrainLevel(item) {
    const terrain = item.terrain, reduced = Boolean(terrain.reduced?.());
    if (reduced && !item.levels[1]) {
      this.level(item.id, terrain.patches.level(true), 1);
      item.levels[1] = true;
    }
    return reduced ? 1 : 0;
  }

  // A texture's new contents, uploaded as three would before a draw.
  upload(texture) {
    const renderer = this.core.renderer, properties = renderer.properties.get(texture);
    if (texture.version > 0 && properties.__version !== texture.version) renderer.initTexture(texture);
  }
}

export function farPass(core) {
  return new FarPass(core);
}

// The far pass's items from the near scene (FarPass.drawNear), a pass of the
// near phase (and so a mirror's) of their own.
export function seaPass(core) {
  const far = () => core.byName.far;
  return {
    name: "sea",
    phase: "near",
    beforeSort: (renderer, scene, camera) => { if (scene === core.graphics.scene) far().adoptNear(scene, camera); },
    draw: (ctx) => far().drawNear(ctx),
  };
}
