// The effects pass (effects.rs): the battle's effects, their simulation
// left here in the page, drawn as items of the near scene's transparent
// list (transparent.js: the renderer sorts every source's items with what
// three still draws, as three would sort them all). So far:
//
// - The particle batches: particles.js's (the Classic bolts and their glow,
//   the effects' CPU particles) and effects-gpu.js's analytic ones, every
//   particle renderer's of the near scene (particles.js particleRenderers:
//   the ordnance's, the effects', the map's environment effects').
// - The Classic arc caster's lightning (lightning.js): one mesh rebuilt each
//   frame, drawn twice as three draws a double-sided transparent material.
//
// Each is registered the first time three has drawn it (which gives its
// material a program), then marked external: three is shown it no more and
// uploads it nothing, and each frame this pass writes what it draws into the
// renderer's memory (a CPU batch's instances, an analytic batch's from the
// first that changed, the lightning's vertices and indices as far as they
// are used), with the key three would have sorted it by: group order,
// render order, the centre of its bounding sphere (through its world
// matrix, as three's projectObject takes it) and its id.
//
// What the Enhanced looks draw (blaster-bolts.js, sniper-trace.js, the
// arcs, the destruction and jetpack effects) stays with three for now, as
// do the muzzle flashes and the effects' models.
import * as THREE from "three";
import { particleRenderers } from "../particles.js";
import { classicLightning } from "../lightning.js";
import { particleDepth } from "../particle-depth.js";
import { blendOf, stateOf, sidesOf } from "./transparent.js";

// add_program's kind for these programs (web/mod.rs: the identity for a
// model matrix, read as modelViewMatrix).
const EFFECT = 4;
// The material values a batch's program reads, set by the renderer from
// the frame (soft particles) and the batch's owner (its rows).
const MATERIAL = new Set(["softParticles", "depthResolution", "cameraRange", "trackRows", "emitterRows"]);
const POLICY = { uniforms: new Set(["modelViewMatrix", "map", "sceneDepth", "tracks", "emitters", ...MATERIAL]), material: MATERIAL };
// A basic material's values (three's refreshUniformsCommon), as the page
// writes them for a mesh: name, size.
const BASIC = [["diffuse", 3], ["opacity", 1], ["mapTransform", 9]];
const MESH_POLICY = { uniforms: new Set(["modelViewMatrix", "map", ...BASIC.map(([name]) => name)]), material: new Set() };
// The instance attributes of each kind of batch, in the order this pass
// writes them; and the lightning's vertex attributes.
const KINDS = {
  cpu: [["iPosition", 3], ["iColor", 4], ["iShape", 4], ["iAxis", 3], ["iAxis2", 3], ["iFrame", 4]],
  analytic: [["spawnPosition", 3], ["spawnVelocity", 3], ["spawnColor", 4], ["spawnShape", 4], ["spawnMeta", 4]],
};
const LIGHTNING = [["position", 3], ["uv", 2], ["color", 4]];
// The frame's inputs (effects.rs IN_*, OWNER_*, DRAW_*).
const IN_DRAWS = 0, IN_OWNERS = 1, IN_SOFT = 2;
const OWNER_AT = 8, OWNER_STRIDE = 4, MAX_OWNERS = 8;
const DRAW_AT = OWNER_AT + MAX_OWNERS * OWNER_STRIDE, DRAW_STRIDE = 12, DRAW_KEY = 6, MAX_DRAWS = 512;
const KIND_BATCH = 0, KIND_MESH = 1;
// Floats of an emitter row (effects-gpu.js).
const EMITTER_FLOATS = 12;
const center = new THREE.Vector4();

// What kind of batch a particle renderer's batch is (null: one this pass
// doesn't draw, such as the Enhanced bolts').
function kindOf(batch) {
  const a = batch.arrays, m = batch.material;
  if (!a || !m?.isShaderMaterial || !batch.mesh || !batch.geometry?.isInstancedBufferGeometry) return null;
  if (a.iShape && a.iPosition) return "cpu";
  if (a.spawnMeta && a.spawnPosition) return "analytic";
  return null;
}

// The order of the nearest Group above an object, as three's projectObject
// passes it down (0 without one).
function groupOrderOf(object) {
  for (let o = object.parent; o; o = o.parent) if (o.isGroup) return o.renderOrder;
  return 0;
}

// three's program for a material drawn with `side` (0 front, 1 back): the
// one it last drew with for that side among the material's programs (they
// differ by FLIP_SIDED). The same as transparent.js's, which keeps its own
// private.
function programForSide(renderer, material, side) {
  const props = renderer.properties.get(material), current = props.currentProgram;
  if (!current) return null;
  const gl = renderer.getContext();
  const vertexOf = (program) => {
    const [vs] = gl.getAttachedShaders(program.program).filter((s) => gl.getShaderParameter(s, gl.SHADER_TYPE) === gl.VERTEX_SHADER);
    return gl.getShaderSource(vs);
  };
  const flipped = (source) => /^#define FLIP_SIDED$/m.test(source);
  const strip = (source) => source.replace(/^#define (FLIP|DOUBLE)_SIDED\n/gm, "");
  const want = side === 1;
  const currentSource = vertexOf(current);
  if (flipped(currentSource) === want && !/^#define DOUBLE_SIDED$/m.test(currentSource)) return current;
  const base = strip(currentSource);
  for (const program of props.programs?.values() || []) {
    if (program === current) continue;
    const source = vertexOf(program);
    if (flipped(source) === want && !/^#define DOUBLE_SIDED$/m.test(source) && strip(source) === base) return program;
  }
  return null;
}

class EffectsPass {
  constructor(core) {
    this.core = core;
    this.name = "effects";
    this.phase = "transparent";
    this.records = []; // registered batches
    this.byBatch = new Map(); // batch -> its record, or null (three draws it)
    this.owners = new Map(); // GpuEffects -> { id }
    this.meshes = new Map(); // the lightning's material -> its record, or null
    this.batchCount = -1;
    this.pending = false;
    this.frame = 0;
    this.active = true;
    // What the measurement tools read: last frame's draws and instances.
    this.draws = 0;
    this.instances = 0;
    core.addView("fxInput", Float64Array, () => core.rust.fx_input_ptr(), () => core.rust.fx_input_len());
    core.addView("fxData", Float32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
    core.addView("fxBits", Uint32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
  }

  // The near scene's particle renderers' batches, looked over again when
  // one was added: new ones registered (or left to three).
  batches() {
    const renderers = particleRenderers.get(this.core.graphics.scene) || [];
    let n = 0;
    for (const r of renderers) n += r.batches.size;
    if (n === this.batchCount && !this.pending) return;
    this.batchCount = n;
    this.pending = false;
    for (const r of renderers) {
      for (const batch of r.batches.values()) {
        if (this.byBatch.has(batch)) continue;
        const kind = kindOf(batch);
        if (!kind) { this.byBatch.set(batch, null); continue; }
        const record = this.registerBatch(batch, kind, r);
        if (record === undefined) this.pending = true;
        else this.byBatch.set(batch, record);
      }
    }
  }

  // A batch's record, or null when the renderer can't draw it; undefined
  // while three has not drawn it yet.
  registerBatch(batch, kind, renderer) {
    const core = this.core, material = batch.material;
    const threeProgram = core.renderer.properties.get(material).currentProgram;
    if (!threeProgram) return undefined;
    const fail = (why) => { core.errors.push(`effects ${kind}: ${why}`); return null; };
    const blend = blendOf(material), state = stateOf(material);
    // A particle material: depth tested less-or-equal, not written, double-sided.
    if (!blend || !state || state.func !== 0x0203 || state.flags !== 5 || material.side !== THREE.DoubleSide) return fail("state");
    const program = core.program(threeProgram, EFFECT, material, POLICY);
    if (!program) return fail("program");
    const at = new Map(program.attributes);
    const attributes = KINDS[kind];
    for (const [name, size] of attributes) if (!at.has(name) || batch.geometry.getAttribute(name)?.itemSize !== size) return fail(`attribute ${name}`);
    if (!at.has("corner") || batch.geometry.index?.array.constructor !== Uint16Array) return fail("quad");
    const map = material.uniforms.map?.value ?? null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return undefined; // not uploaded yet: next time
    let owner = -1;
    if (kind === "analytic") {
      const gpu = renderer.gpu;
      if (!gpu || material.uniforms.tracks !== gpu.uniforms.tracks) return fail("owner");
      owner = this.owner(gpu);
      if (owner < 0) return fail("tracks");
    }
    const id = core.rust.fx_add_batch(program.id, texture, new Uint32Array(blend), at.get("corner"), new Uint32Array(attributes.flatMap(([name, size]) => [at.get(name), size])), owner);
    const record = { batch, kind, id, attributes, mesh: batch.mesh, map, handle: this.handle(map), synced: false, capacity: 0, from: this.frame };
    batch.external = true;
    batch.mesh.visible = false;
    this.records.push(record);
    return record;
  }

  // An owner of analytic batches (effects-gpu.js GpuEffects), registered once.
  owner(gpu) {
    let o = this.owners.get(gpu);
    if (!o) {
      const tracks = this.core.texture(gpu.uniforms.tracks.value);
      if (tracks < 0) return -1;
      o = { id: this.core.rust.fx_add_owner(tracks, gpu.uniforms.trackRows.value) };
      this.owners.set(gpu, o);
    }
    return o.id;
  }

  // The Classic lightning of the near scene, while its material is one the
  // renderer has (registered once three drew it); null otherwise. While it
  // wears a material three has not drawn yet, three draws it.
  lightning() {
    const lightning = classicLightning.get(this.core.graphics.scene);
    if (!lightning || lightning.isEnhanced()) return null;
    const material = lightning.mesh.material;
    if (!material) return null;
    let record = this.meshes.get(material);
    if (record === undefined) {
      record = this.registerMesh(lightning.mesh, material, LIGHTNING);
      if (record !== undefined) this.meshes.set(material, record);
    }
    lightning.external = Boolean(record);
    return record ? { lightning, record } : null;
  }

  // A mesh drawn as three draws it (each of its sides in turn); undefined
  // while three has not drawn it as it is yet, null when the renderer can't.
  registerMesh(mesh, material, attributes) {
    const core = this.core, renderer = core.renderer;
    const props = renderer.properties.get(material);
    if (!props.currentProgram || props.__version !== material.version) return undefined;
    const fail = (why) => { core.errors.push(`effects mesh: ${why}`); return null; };
    const blend = blendOf(material), state = stateOf(material);
    if (!blend || !state) return fail("state");
    const sides = sidesOf(material);
    const programs = sides.map((side) => { const p = programForSide(renderer, material, side); return p && core.program(p, EFFECT, material, MESH_POLICY); });
    if (programs.some((p) => !p)) return fail("program");
    const locations = programs.map((p) => { const at = new Map(p.attributes); return attributes.map(([name]) => at.get(name)); });
    if (locations.some((l) => l.some((v) => v === undefined))) return fail("attributes");
    for (const [name, size] of attributes) if (mesh.geometry.getAttribute(name)?.itemSize !== size) return fail(`attribute ${name}`);
    if (!(mesh.geometry.index?.array instanceof Uint32Array)) return fail("index");
    const map = material.map ?? null;
    const texture = map ? core.texture(map) : -1;
    if (map && texture < 0) return undefined;
    const values = BASIC.filter(([name]) => programs.some((p) => p.names.has(name)));
    const id = core.rust.fx_add_mesh(texture, new Uint32Array(blend), state.flags & 3, state.func, new Uint32Array(attributes.map(([, size]) => size)), values.map(([name, size]) => `${name}:${size}`).join(","));
    sides.forEach((side, i) => core.rust.fx_mesh_pass(id, programs[i].id, side, new Uint32Array(locations[i])));
    return { id, mesh, material, map, handle: this.handle(map), values, attributes, from: this.frame };
  }

  // A texture's GL handle, uploaded first if three has a newer version of
  // it than it last uploaded (three uploads one only as it draws with it).
  handle(map) {
    if (!map) return null;
    const renderer = this.core.renderer, props = renderer.properties.get(map);
    if (props.__webglTexture && props.__version !== map.version) renderer.initTexture(map);
    return props.__webglTexture ?? null;
  }

  // Shown as three would show it: in the scene, it and its parents visible,
  // in the camera's layers.
  shown(mesh, camera) {
    const scene = this.core.graphics.scene;
    if (!mesh.layers.test(camera.layers)) return false;
    let o = mesh.parent;
    for (; o && o !== scene; o = o.parent) if (!o.visible) return false;
    return o === scene;
  }

  // three's sort key for an object, into a draw entry: group order, render
  // order, its bounding sphere's centre through its world matrix, its id.
  key(input, e, object) {
    const geometry = object.geometry;
    if (!geometry.boundingSphere) geometry.computeBoundingSphere();
    center.copy(geometry.boundingSphere.center).applyMatrix4(object.matrixWorld);
    const k = e + DRAW_KEY;
    input[k] = groupOrderOf(object); input[k + 1] = object.renderOrder;
    input[k + 2] = center.x; input[k + 3] = center.y; input[k + 4] = center.z; input[k + 5] = object.id;
  }

  // The transparent phase, before the renderer's call: the frame's draws.
  draw(ctx) {
    const core = this.core;
    this.frame++;
    if (!this.active) {
      // Off (three draws them): none of last frame's draws again.
      core.views();
      core.fxInput[IN_DRAWS] = 0;
      return;
    }
    this.batches();
    const bolt = this.lightning();
    // Textures three uploaded anew, or made anew (a new handle: registered
    // again). Registering may move the renderer's memory: views after.
    for (const record of this.records) {
      const handle = this.handle(record.map);
      if (handle && handle !== record.handle) { record.handle = handle; core.rust.fx_batch_texture(record.id, core.texture(record.map)); }
    }
    if (bolt) {
      const record = bolt.record, handle = this.handle(record.map);
      if (handle && handle !== record.handle) { record.handle = handle; core.rust.fx_mesh_texture(record.id, core.texture(record.map)); }
    }
    // Soft particles: the scene's depth, taken before the first of them as
    // three's batches take it (particle-depth.js).
    const soft = particleDepth.enabled.value > 0.5;
    if (soft) particleDepth.capture(ctx.renderer, ctx.scene, ctx.camera);
    const depth = soft && particleDepth.map.value ? core.texture(particleDepth.map.value) : -1;
    // What three would have drawn this frame, registered before it (three
    // drew what was registered this frame itself).
    const drawn = this.records.filter((record) => record.from !== this.frame && record.batch.count > 0 && record.batch.material.visible !== false && this.shown(record.mesh, ctx.camera));
    const lit = bolt && bolt.record.from !== this.frame && bolt.lightning.index > 0 && bolt.record.material.visible !== false && this.shown(bolt.lightning.mesh, ctx.camera) ? bolt : null;
    // The floats this frame needs; the renderer's memory grows first if need be.
    let need = 0;
    for (const record of drawn) need += record.batch.count * (record.kind === "cpu" ? 21 : 18);
    for (const gpu of this.owners.keys()) need += Math.min(gpu.count, gpu.uniforms.emitterRows.value) * EMITTER_FLOATS;
    if (lit) need += lit.lightning.vertex * 9 + lit.lightning.index + 13;
    core.views();
    if (need > core.fxData.length) {
      core.rust.fx_reserve(Math.max(need * 2, core.fxData.length * 2));
      core.buffer = null;
      core.views();
    }
    const input = core.fxInput, data = core.fxData, bits = core.fxBits;
    let at = 0, owners = 0, draws = 0, instances = 0;
    for (const [gpu, o] of this.owners) {
      if (owners === MAX_OWNERS) break;
      const rows = gpu.uniforms.emitterRows.value, written = Math.min(gpu.count, rows);
      const e = OWNER_AT + owners++ * OWNER_STRIDE;
      input[e] = o.id; input[e + 1] = rows; input[e + 2] = written; input[e + 3] = at;
      data.set(gpu.data.subarray(0, written * EMITTER_FLOATS), at);
      at += written * EMITTER_FLOATS;
    }
    for (const record of drawn) {
      if (draws === MAX_DRAWS) break;
      const b = record.batch, count = b.count;
      // An analytic batch's instances from the first that changed since it
      // was last read (effects-gpu.js unread), when the renderer holds the
      // rest; all of them otherwise.
      let from = 0;
      if (record.kind === "analytic") {
        if (record.synced && record.capacity === b.capacity) from = Math.min(Math.max(b.unread ?? 0, 0), count);
        b.unread = Infinity;
      }
      const start = at;
      for (const [name, size] of record.attributes) {
        data.set(b.arrays[name].subarray(from * size, count * size), at);
        at += (count - from) * size;
      }
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_BATCH; input[e + 1] = record.id; input[e + 2] = count; input[e + 3] = from; input[e + 4] = start; input[e + 5] = b.capacity;
      this.key(input, e, record.mesh);
      record.synced = true;
      record.capacity = b.capacity;
      instances += count;
    }
    if (lit && draws < MAX_DRAWS) {
      const { lightning, record } = lit, geometry = lightning.geometry, material = record.material;
      const vertices = lightning.vertex, indices = lightning.index, start = at;
      for (const [name, size] of record.attributes) {
        data.set(geometry.attributes[name].array.subarray(0, vertices * size), at);
        at += vertices * size;
      }
      bits.set(geometry.index.array.subarray(0, indices), at);
      at += indices;
      for (const [name] of record.values) {
        if (name === "diffuse") { data[at++] = material.color.r; data[at++] = material.color.g; data[at++] = material.color.b; }
        else if (name === "opacity") data[at++] = material.opacity;
        else if (name === "mapTransform") {
          const map = material.map;
          if (map?.matrixAutoUpdate) map.updateMatrix();
          data.set(map ? map.matrix.elements : [1, 0, 0, 0, 1, 0, 0, 0, 1], at);
          at += 9;
        }
      }
      const e = DRAW_AT + draws++ * DRAW_STRIDE;
      input[e] = KIND_MESH; input[e + 1] = record.id; input[e + 2] = vertices; input[e + 3] = indices; input[e + 4] = start; input[e + 5] = 0;
      this.key(input, e, lightning.mesh);
    }
    input[IN_DRAWS] = draws;
    input[IN_OWNERS] = draws ? owners : 0;
    input[IN_SOFT] = particleDepth.enabled.value;
    input[IN_SOFT + 1] = particleDepth.resolution.value.x; input[IN_SOFT + 2] = particleDepth.resolution.value.y;
    input[IN_SOFT + 3] = particleDepth.cameraRange.value.x; input[IN_SOFT + 4] = particleDepth.cameraRange.value.y;
    input[IN_SOFT + 5] = depth;
    this.draws = draws;
    this.instances = instances;
  }

  // Off: three draws everything again, everything uploaded anew; on: it
  // comes back here, written in full the first frame.
  setActive(on) {
    this.active = on;
    for (const record of this.records) {
      const b = record.batch;
      b.external = on;
      record.synced = false;
      if (on) { b.mesh.visible = false; continue; }
      b.mesh.visible = b.count > 0;
      for (const [name] of record.attributes) {
        const a = b.geometry.getAttribute(name);
        a.clearUpdateRanges();
        a.addUpdateRange(0, b.count * a.itemSize);
        a.needsUpdate = true;
      }
    }
    const lightning = classicLightning.get(this.core.graphics.scene);
    if (lightning && !on && lightning.external) {
      lightning.external = false;
      lightning.mesh.visible = lightning.index > 0 && !lightning.isEnhanced();
      for (const name of ["position", "uv", "color"]) lightning.geometry.attributes[name].needsUpdate = true;
      lightning.geometry.index.needsUpdate = true;
    }
    if (!on) this.draws = this.instances = 0;
  }

  restore() {
    this.setActive(false);
  }
}

export function effectsPass(core) {
  return new EffectsPass(core);
}
