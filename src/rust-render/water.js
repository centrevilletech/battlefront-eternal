// The water pass (water.rs): the near scene's terrain water (water.js) as
// items of the transparent list: the surface drawn with the look's textures
// (Low), or at High and Ultra in two passes (the tint over the opaque
// scene's copy, then the reflection), the rings where things meet the
// water, at High and Ultra the shore's lapping and splashes (water-shore.js,
// one quad instanced at each shore point), and the Enhanced ocean's crest
// spray (ocean-storm.js, one card instanced at each of its seeds).
// Each is registered the first time three has a program for its material
// (made as three compiled it: Kind::Plain), then hidden from three; each
// frame, in the transparent phase, the pass writes whether three would have
// drawn it (its object's and parents' visibility, the camera's layers), its
// sort key as three's projectObject makes it, its draw range and every
// uniform its program reads (the waders, an array, apart). The rings'
// vertices that changed (their update ranges) are copied into the
// renderer's mirror of them, and the shore's instances whenever three would
// upload them.
//
// The tint pass copies the opaque scene's colour and depth as three draws
// it (its onBeforeRender, water.js captureSurface). It is the first item of
// the near scene's transparent list (the lowest render order there, -20),
// so the copy is made here at the start of the transparent phase, before
// anything of the list is drawn, as three makes it. The Ultra mirror's
// render of the near scene hides the water, and the eye under the surface
// draws the underwater pass (three's, which copies the scene again). The
// spray reads the scene's depth: three copies it in the spray's
// onBeforeRender (particle-depth.js), after whatever transparent item before
// it writes depth; the pass asks for that copy before the spray as three
// starts the near scene's render (particleDepth.request: a copier three draws
// just before it), and runs the spray's hook ahead of its turn.
import * as THREE from "three";
import { particleDepth } from "../particle-depth.js";
import { uniformType, writeUniform } from "./post.js";
import { blendOf, sidesOf, stateOf } from "./transparent.js";

const PLAIN = 3;
// water.rs's layout.
const UNIFORM = 18, MAX_UNIFORMS = 32, ARRAY_FLOATS = 32;
const S = { SHOWN: 0, PROGRAM: 1, KEY: 2, CENTER: 5, WORLD: 8, RANGE: 24, UNIFORMS: 26, FIRST: 27, ARRAY: 27 + MAX_UNIFORMS * UNIFORM };
const SURFACE = S.ARRAY + 3 + ARRAY_FLOATS, MAX_SURFACES = 8;
const RING_UPLOAD = MAX_SURFACES * SURFACE, SETS = RING_UPLOAD + 2;
const RINGS = [["position", "waterRingsPosition", 3, "water_rings_position"], ["uv", "waterRingsUv", 2, "water_rings_uv"], ["alpha", "waterRingsAlpha", 1, "water_rings_alpha"]];
const frustum = new THREE.Frustum(), viewProjection = new THREE.Matrix4();
const BASE_BEFORE = THREE.Object3D.prototype.onBeforeRender;
const point = new THREE.Vector3();

// Rust's program for a water material's three program, as three compiled
// it, with its uniforms ({ name, type } in slot order) and its one uniform
// array ({ name, width, size, slot }, after them), or null when it reads one
// the renderer can't set; undefined until three has made it.
const programs = new WeakMap();
function surfaceProgram(core, material, attributes) {
  const threeProgram = core.renderer.properties.get(material).currentProgram;
  if (!threeProgram) return undefined;
  let entry = programs.get(threeProgram);
  if (entry !== undefined) return entry;
  entry = null;
  const gl = core.gl, p = threeProgram.program, uniforms = [], arrays = [];
  for (let i = 0, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); i < n; i++) {
    const u = gl.getActiveUniform(p, i), name = u.name.replace(/\[0\]$/, ""), type = uniformType(gl, u.type);
    if (u.size === 1 && type) uniforms.push({ name, type, size: 1 });
    else if (u.size > 1 && type >= 1 && type <= 4) arrays.push({ name, width: type, size: u.size });
    else arrays.push(null);
  }
  const array = arrays[0] ?? null;
  if (arrays.length <= 1 && (!array || array.width * array.size <= ARRAY_FLOATS) && uniforms.length <= MAX_UNIFORMS) {
    const [vs, fs] = gl.getAttachedShaders(p).sort((a) => (gl.getShaderParameter(a, gl.SHADER_TYPE) === gl.VERTEX_SHADER ? -1 : 1));
    try {
      const names = [...uniforms.map((u) => u.name), ...(array ? [array.name] : [])];
      entry = { id: core.rust.add_program(PLAIN, gl.getShaderSource(vs), gl.getShaderSource(fs), attributes, names.join(",")), uniforms, array: array && { ...array, slot: uniforms.length } };
    } catch (error) {
      core.errors.push(`water p${threeProgram.id}: ${String(error).slice(0, 300)}`);
    }
  } else core.errors.push(`water p${threeProgram.id}: ${arrays.length} arrays`);
  programs.set(threeProgram, entry);
  return entry;
}

class WaterPass {
  constructor(core) {
    this.core = core;
    this.name = "water";
    this.phase = "transparent";
    this.water = null;
    // Registered meshes: { mesh, material, program, id, kind, set }, in
    // water.rs's order.
    this.surfaces = [];
    // The shore's instances: geometry -> { id, attributes, versions, at }.
    this.sets = new Map();
    this.tried = new WeakSet();
    this.input = false;
    // The renderer's copy of the rings needs every vertex.
    this.full = true;
    // The scene's depth asked for before the spray, as three starts the
    // frame's render of the near scene (particle-depth.js requester).
    this.depthKey = new Float64Array(6);
    this.unrequest = core.graphics?.scene ? particleDepth.requester(core.graphics.scene, (camera) => {
      try { this.requestDepth(camera); } catch (error) { core.errors.push(`water depth request failed: ${String(error?.stack || error).slice(0, 400)}`); }
    }) : null;
  }

  // The copy of the scene's depth before the crest spray, when the renderer
  // draws it this frame: its key as three's projectObject will make it
  // (group order, render order, its geometry's sphere centre in the world,
  // its id). Not required: three's own copy for it is taken only with soft
  // particles on (its shader reads the depth only then).
  requestDepth(camera) {
    const core = this.core;
    for (const surface of this.surfaces) surface.requested = false;
    if (this.on === false || !core.enabled || !core.active) return;
    const spray = this.water?.ocean?.waves?.spray;
    const surface = spray && this.surfaces.find((s) => s.mesh === spray);
    if (!surface || !this.shown(surface, camera, core.graphics.scene, false)) return;
    const mesh = surface.mesh, key = this.depthKey;
    let groupOrder = 0;
    for (let n = mesh.parent; n; n = n.parent) if (n.isGroup) { groupOrder = n.renderOrder; break; }
    const c = point.copy(mesh.geometry.boundingSphere.center).applyMatrix4(mesh.matrixWorld);
    key[0] = groupOrder; key[1] = mesh.renderOrder; key[2] = c.x; key[3] = c.y; key[4] = c.z; key[5] = mesh.id;
    surface.requested = particleDepth.request(key, false);
  }

  draw(ctx) {
    const core = this.core, water = core.graphics.scene.userData.water;
    if (!water) return;
    if (water !== this.water) { this.water = water; this.surfaces = []; this.sets = new Map(); this.tried = new WeakSet(); this.full = true; }
    for (const mesh of [water.lowPass, water.tintPass, water.mirrorPass]) if (mesh && !this.tried.has(mesh)) this.registerMesh(mesh, "surface");
    if (water.rings && !this.tried.has(water.rings)) this.registerMesh(water.rings, "rings");
    for (const mesh of water.shore?.meshes ?? []) if (!this.tried.has(mesh)) this.registerMesh(mesh, "instanced");
    const spray = water.ocean?.waves?.spray;
    if (spray && !this.tried.has(spray)) this.registerMesh(spray, "instanced");
    if (!this.surfaces.length) return;
    core.views();
    const input = core.waterInput, camera = ctx.camera;
    // Off (this pass alone): three draws the water.
    if (this.on === false) { this.surfaces.forEach((surface, s) => { input[s * SURFACE + S.SHOWN] = 0; }); return; }
    frustum.setFromProjectionMatrix(viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    input[RING_UPLOAD] = 0; input[RING_UPLOAD + 1] = 0;
    // The tint pass's copy of the opaque scene (its onBeforeRender), before
    // the list's first item: the renderer draws the tint pass first.
    // Each shown surface's own onBeforeRender, as three calls it before the
    // draw (the tint's copy; the spray's soft particle depth), before any
    // item of the list is drawn.
    for (const surface of this.surfaces) {
      if (surface.mesh.onBeforeRender === BASE_BEFORE || !this.shown(surface, camera, ctx.scene)) continue;
      const hook = () => surface.mesh.onBeforeRender(ctx.renderer, ctx.scene, camera, surface.mesh.geometry, surface.material, null);
      // The spray's copy of the scene's depth: the copier's, before it, when
      // it was asked for (else taken now, as before).
      particleDepth.ahead(hook, surface.requested === true);
    }
    core.views();
    this.surfaces.forEach((surface, s) => this.write(surface, s, input, camera, ctx.scene));
    for (const set of this.sets.values()) this.instances(set, input);
  }

  // A water mesh the renderer can draw: one pass (single sided, or both
  // sides at once), a blending and state it knows, a program with at most
  // one uniform array; the rings and the shore with their own arrays.
  registerMesh(mesh, kind) {
    const core = this.core, material = mesh.material;
    const geometry = mesh.geometry, set = kind === "instanced" ? this.instanceSet(geometry) : null;
    if (kind === "instanced" && !set) { this.tried.add(mesh); return; }
    const attributes = kind === "rings" ? "position:0,uv:1,alpha:2" : kind === "instanced" ? `position:0,uv:1,${set.attributes.map((a, k) => `${a.name}:${2 + k}`).join(",")}` : "position:0,uv:1";
    const program = surfaceProgram(core, material, attributes);
    if (program === undefined) return; // until three has made it
    this.tried.add(mesh);
    const blend = blendOf(material), state = stateOf(material), sides = sidesOf(material);
    if (!program || !blend || !state || sides.length !== 1 || !geometry.index || geometry.groups.length || this.surfaces.length >= MAX_SURFACES) return;
    let id;
    if (kind === "rings") {
      if (!this.input) this.views();
      const index = geometry.index.array;
      id = core.rust.water_rings(index instanceof Uint32Array ? index : Uint32Array.from(index), new Uint32Array(blend), state.func, state.flags, sides[0]);
    } else {
      const g = core.geometry(geometry, { id: program.id, attributes: [["position", 0], ["uv", 1]] });
      if (g === null || g === undefined) return;
      id = core.rust.water_surface(g, new Uint32Array(blend), state.func, state.flags, sides[0]);
      if (set) set.attributes.forEach((a, k) => core.rust.water_instanced(id, set.id, k, 2 + k));
    }
    if (!this.input) this.views();
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    this.surfaces.push({ mesh, material, program, id, kind, set });
    material.visible = !core.active;
  }

  // An instanced geometry's instances (the shore's two meshes share one): a
  // set of the renderer's for its instanced attributes, made once; null when
  // there is no room.
  instanceSet(geometry) {
    let set = this.sets.get(geometry);
    if (set !== undefined) return set;
    const core = this.core;
    const attributes = Object.entries(geometry.attributes).filter(([, a]) => a.isInstancedBufferAttribute)
      .map(([name, attribute]) => ({ name, attribute, size: attribute.itemSize }));
    const max = Math.min(...attributes.map((a) => a.attribute.count));
    set = null;
    if (attributes.length && attributes.every((a) => a.attribute.array instanceof Float32Array && (a.attribute.meshPerAttribute ?? 1) === 1)) {
      const id = core.rust.water_set(max, new Uint32Array(attributes.map((a) => a.size)));
      if (id !== undefined) {
        // Where each attribute's values start in the renderer's copy: after
        // every set made before.
        let at = this.nextInstance ?? 0;
        for (const a of attributes) { a.at = at; at += max * a.size; }
        this.nextInstance = at;
        set = { id, geometry, attributes, max, versions: attributes.map(() => -1) };
      }
    }
    this.sets.set(geometry, set);
    return set;
  }

  views() {
    const core = this.core;
    core.addView("waterInput", Float64Array, () => core.rust.water_input_ptr(), () => core.rust.water_input_len());
    for (const [, view, , binding] of RINGS) core.addView(view, Float32Array, () => core.rust[`${binding}_ptr`](), () => core.rust[`${binding}_len`]());
    core.addView("waterInstances", Float32Array, () => core.rust.water_instances_ptr(), () => core.rust.water_instances_len());
    this.input = true;
  }

  // Whether three would list it in this render: shown with every parent,
  // on the camera's layers, its material the same, in view when culled
  // (culled: the frustum this frame's phase made).
  shown(surface, camera, scene, culled = true) {
    const { mesh, material } = surface;
    for (let n = mesh; n && n !== scene; n = n.parent) if (!n.visible) return false;
    if (!mesh.layers.test(camera.layers) || mesh.material !== material) return false;
    return !(culled && mesh.frustumCulled && !frustum.intersectsObject(mesh));
  }

  // Its values this frame, as three would have listed and drawn it.
  write(surface, s, input, camera, scene) {
    const { mesh, material, program } = surface, o = s * SURFACE;
    input[o + S.SHOWN] = 0;
    if (!this.shown(surface, camera, scene)) return;
    const geometry = mesh.geometry;
    // The draw range, as renderBufferDirect clamps it to the index.
    const range = geometry.drawRange, count = geometry.index.count;
    const first = Math.max(0, range.start), last = Math.min(count, range.start + range.count);
    if (last <= first) return;
    input[o + S.RANGE] = first; input[o + S.RANGE + 1] = last - first;
    // Its sort key: the nearest group's render order, its own, its id; z
    // from its geometry's bounding sphere (as three last computed it).
    let groupOrder = 0;
    for (let n = mesh.parent; n; n = n.parent) if (n.isGroup) { groupOrder = n.renderOrder; break; }
    input[o + S.KEY] = groupOrder; input[o + S.KEY + 1] = mesh.renderOrder; input[o + S.KEY + 2] = mesh.id;
    const c = geometry.boundingSphere.center;
    input[o + S.CENTER] = c.x; input[o + S.CENTER + 1] = c.y; input[o + S.CENTER + 2] = c.z;
    input.set(mesh.matrixWorld.elements, o + S.WORLD);
    input[o + S.PROGRAM] = program.id;
    input[o + S.UNIFORMS] = program.uniforms.length;
    for (let k = 0; k < program.uniforms.length; k++) {
      if (!writeUniform(this.core, input, o + S.FIRST + k * UNIFORM, k, program.uniforms[k], material, mesh, camera)) return;
    }
    input[o + S.ARRAY] = -1;
    if (program.array) {
      const { slot, width, size, name } = program.array, values = material.uniforms?.[name]?.value;
      if (!Array.isArray(values)) return;
      input[o + S.ARRAY] = slot; input[o + S.ARRAY + 1] = width; input[o + S.ARRAY + 2] = size;
      for (let i = 0; i < size; i++) {
        const v = values[i], at = o + S.ARRAY + 3 + i * width;
        if (typeof v === "number") input[at] = v;
        else { input[at] = v?.x ?? 0; if (width > 1) input[at + 1] = v?.y ?? 0; if (width > 2) input[at + 2] = v?.z ?? 0; if (width > 3) input[at + 3] = v?.w ?? 0; }
      }
    }
    if (surface.kind === "rings") this.rings(geometry, input);
    input[o + S.SHOWN] = 1;
  }

  // The rings' vertices that changed (all of them the first time) into the
  // renderer's mirror.
  rings(geometry, input) {
    const core = this.core, position = geometry.attributes.position;
    let start = 0, end = position.count;
    if (!this.full) {
      start = Infinity; end = 0;
      for (const r of position.updateRanges) { start = Math.min(start, r.start / 3); end = Math.max(end, (r.start + r.count) / 3); }
      if (!(end > start)) return;
    }
    for (const [name, view, size] of RINGS) core[view].set(geometry.attributes[name].array.subarray(start * size, end * size), start * size);
    input[RING_UPLOAD] = start; input[RING_UPLOAD + 1] = end;
    this.full = false;
  }

  // A set's instances this frame: how many draw (three's instanceCount, at
  // most what its attributes hold), and their values whenever three would
  // upload them (an attribute's version moved).
  instances(set, input) {
    if (!set) return;
    const core = this.core, at = SETS + 2 * set.id, count = Math.min(set.geometry.instanceCount ?? 0, set.max);
    input[at + 1] = count;
    if (set.attributes.every((a, k) => a.attribute.version === set.versions[k])) return;
    set.attributes.forEach((a, k) => {
      set.versions[k] = a.attribute.version;
      core.waterInstances.set(a.attribute.array.subarray(0, set.max * a.size), a.at);
    });
    input[at] = 1;
  }

  setActive(on) {
    this.on = on;
    for (const surface of this.surfaces) surface.material.visible = !on;
    // Three's copy of the rings is whole: the renderer's needs every vertex
    // when it draws them again; three may have uploaded new instances.
    if (on) { this.full = true; for (const set of this.sets.values()) set?.versions.fill(-1); }
  }

  restore() {
    this.setActive(false);
    this.unrequest?.();
    this.unrequest = null;
  }
}

export function waterPass(core) {
  return new WaterPass(core);
}
