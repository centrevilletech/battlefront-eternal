// The effects pass (effects.rs): the battle's effects, their simulation
// left here in the page. So far the particle batches: particles.js's (the
// Classic bolts and their glow, the effects' CPU particles) and
// effects-gpu.js's analytic ones, every particle renderer's of the near
// scene (particles.js particleRenderers: the ordnance's, the effects', the
// map's environment effects').
//
// A batch is registered the first time three has drawn it (which gives its
// material a program), then marked external: three is shown it no more and
// uploads it nothing (particles.js), and each frame this pass writes its
// instances into the renderer's memory, a CPU batch's all of them, an
// analytic batch's from the first that changed, and lists its draw. Every
// batch three would draw at the particles' place in its transparent list
// (renderOrder 10, the world's origin for a centre, so in the order they
// were made) is drawn there, until the transparent pass draws them among its
// items: from an anchor placed after everything else at that renderOrder.
//
// What the Enhanced looks draw (blaster-bolts.js, sniper-trace.js, the
// arcs, the destruction and jetpack effects) stays with three for now, as
// do the muzzle flashes and the effects' models (transparent meshes that
// sort among the map's own).
import * as THREE from "three";
import { particleRenderers } from "../particles.js";
import { particleDepth } from "../particle-depth.js";

// add_program's kind for these programs (web/mod.rs: the identity for a
// model matrix, read as modelViewMatrix).
const EFFECT = 3;
// The material values a batch's program reads, set by the renderer from
// the frame (soft particles) and the batch's owner (its rows).
const MATERIAL = new Set(["softParticles", "depthResolution", "cameraRange", "trackRows", "emitterRows"]);
const POLICY = { uniforms: new Set(["modelViewMatrix", "map", "sceneDepth", "tracks", "emitters", ...MATERIAL]), material: MATERIAL };
// The instance attributes of each kind of batch, in the order this pass
// writes them.
const KINDS = {
  cpu: [["iPosition", 3], ["iColor", 4], ["iShape", 4], ["iAxis", 3], ["iAxis2", 3], ["iFrame", 4]],
  analytic: [["spawnPosition", 3], ["spawnVelocity", 3], ["spawnColor", 4], ["spawnShape", 4], ["spawnMeta", 4]],
};
// Where the batches sit in three's transparent list (particles.js,
// effects-gpu.js).
const ORDER = 10;
// The frame's inputs (effects.rs IN_*, OWNER_*, DRAW_*).
const IN_DRAWS = 0, IN_OWNERS = 1, IN_SOFT = 2;
const OWNER_AT = 8, OWNER_STRIDE = 4, MAX_OWNERS = 8;
const DRAW_AT = OWNER_AT + MAX_OWNERS * OWNER_STRIDE, DRAW_STRIDE = 5, MAX_DRAWS = 512;
// Floats of an emitter row (effects-gpu.js).
const EMITTER_FLOATS = 12;

// Three's blending for a material as setBlending makes it: enabled, then the
// rgb and alpha equations, then the source and destination factors for rgb
// and for alpha (GL enums).
const GL = {
  ADD: 0x8006, SUBTRACT: 0x800a, REVERSE_SUBTRACT: 0x800b, MIN: 0x8007, MAX: 0x8008,
  ZERO: 0, ONE: 1, SRC_COLOR: 0x0300, ONE_MINUS_SRC_COLOR: 0x0301, SRC_ALPHA: 0x0302, ONE_MINUS_SRC_ALPHA: 0x0303,
  DST_ALPHA: 0x0304, ONE_MINUS_DST_ALPHA: 0x0305, DST_COLOR: 0x0306, ONE_MINUS_DST_COLOR: 0x0307, SRC_ALPHA_SATURATE: 0x0308,
  CONSTANT_COLOR: 0x8001, ONE_MINUS_CONSTANT_COLOR: 0x8002, CONSTANT_ALPHA: 0x8003, ONE_MINUS_CONSTANT_ALPHA: 0x8004,
};
const EQUATIONS = {
  [THREE.AddEquation]: GL.ADD, [THREE.SubtractEquation]: GL.SUBTRACT, [THREE.ReverseSubtractEquation]: GL.REVERSE_SUBTRACT,
  [THREE.MinEquation]: GL.MIN, [THREE.MaxEquation]: GL.MAX,
};
const FACTORS = {
  [THREE.ZeroFactor]: GL.ZERO, [THREE.OneFactor]: GL.ONE, [THREE.SrcColorFactor]: GL.SRC_COLOR, [THREE.OneMinusSrcColorFactor]: GL.ONE_MINUS_SRC_COLOR,
  [THREE.SrcAlphaFactor]: GL.SRC_ALPHA, [THREE.OneMinusSrcAlphaFactor]: GL.ONE_MINUS_SRC_ALPHA, [THREE.DstAlphaFactor]: GL.DST_ALPHA,
  [THREE.OneMinusDstAlphaFactor]: GL.ONE_MINUS_DST_ALPHA, [THREE.DstColorFactor]: GL.DST_COLOR, [THREE.OneMinusDstColorFactor]: GL.ONE_MINUS_DST_COLOR,
  [THREE.SrcAlphaSaturateFactor]: GL.SRC_ALPHA_SATURATE, [THREE.ConstantColorFactor]: GL.CONSTANT_COLOR,
  [THREE.OneMinusConstantColorFactor]: GL.ONE_MINUS_CONSTANT_COLOR, [THREE.ConstantAlphaFactor]: GL.CONSTANT_ALPHA,
  [THREE.OneMinusConstantAlphaFactor]: GL.ONE_MINUS_CONSTANT_ALPHA,
};
export function blendOf(m) {
  const { ADD, ZERO, ONE, SRC_ALPHA, ONE_MINUS_SRC_ALPHA, SRC_COLOR, ONE_MINUS_SRC_COLOR } = GL;
  const b = m.blending;
  if (b === THREE.NoBlending || (b === THREE.NormalBlending && !m.transparent)) return [0, ADD, ADD, ONE, ZERO, ONE, ZERO];
  if (b === THREE.CustomBlending) {
    const equation = m.blendEquation, src = m.blendSrc, dst = m.blendDst;
    return [1, EQUATIONS[equation], EQUATIONS[m.blendEquationAlpha || equation], FACTORS[src], FACTORS[dst], FACTORS[m.blendSrcAlpha || src], FACTORS[m.blendDstAlpha || dst]];
  }
  const pre = m.premultipliedAlpha;
  switch (b) {
    case THREE.NormalBlending: return pre ? [1, ADD, ADD, ONE, ONE_MINUS_SRC_ALPHA, ONE, ONE_MINUS_SRC_ALPHA] : [1, ADD, ADD, SRC_ALPHA, ONE_MINUS_SRC_ALPHA, ONE, ONE_MINUS_SRC_ALPHA];
    case THREE.AdditiveBlending: return pre ? [1, ADD, ADD, ONE, ONE, ONE, ONE] : [1, ADD, ADD, SRC_ALPHA, ONE, SRC_ALPHA, ONE];
    case THREE.SubtractiveBlending: return [1, ADD, ADD, ZERO, ONE_MINUS_SRC_COLOR, ZERO, ONE];
    case THREE.MultiplyBlending: return pre ? [1, ADD, ADD, ZERO, SRC_COLOR, ZERO, SRC_ALPHA] : [1, ADD, ADD, ZERO, SRC_COLOR, ZERO, SRC_COLOR];
    default: return null;
  }
}

// What kind of batch a particle renderer's batch is (null: one this pass
// doesn't draw, such as the Enhanced bolts').
function kindOf(batch) {
  const a = batch.arrays, m = batch.material;
  if (!a || !m?.isShaderMaterial || !batch.mesh || !batch.geometry?.isInstancedBufferGeometry) return null;
  if (a.iShape && a.iPosition) return "cpu";
  if (a.spawnMeta && a.spawnPosition) return "analytic";
  return null;
}

class EffectsPass {
  constructor(core) {
    this.core = core;
    this.name = "effects";
    this.phase = "effects";
    this.records = []; // registered batches, in three's order (their meshes' ids)
    this.byBatch = new Map(); // batch -> its record, or null (three draws it)
    this.owners = new Map(); // GpuEffects -> { id }
    this.batchCount = -1;
    this.frame = 0;
    this.anchor = null;
    this.active = true;
    // Stats the measurement tools read: draws and instances last frame.
    this.draws = 0;
    this.instances = 0;
    core.addView("fxInput", Float64Array, () => core.rust.fx_input_ptr(), () => core.rust.fx_input_len());
    core.addView("fxData", Float32Array, () => core.rust.fx_data_ptr(), () => core.rust.fx_data_len());
  }

  // The anchor three calls back from, after everything else at the
  // particles' renderOrder: the near view's draws only.
  register() {
    const core = this.core, graphics = core.graphics;
    this.anchor = core.anchor(graphics.scene, { transparent: true, renderOrder: ORDER + 1e-6 }, (renderer, scene, camera) => {
      if (camera !== graphics.camera || renderer.getRenderTarget() !== graphics.sceneTarget || !core.enabled || !core.active || !this.active) return;
      this.frame++;
      // Nothing to draw: no phase (three's state is left as it is).
      this.batches();
      if (this.records.some((record) => this.drawable(record))) core.phase("effects", renderer, scene, camera);
      else this.draws = this.instances = 0;
    });
  }

  // Whether a record draws this frame: registered before it (three drew
  // it as it was registered), with instances, shown as three would show it.
  drawable(record) {
    return record.from !== this.frame && record.batch.count > 0 && record.batch.material.visible !== false && this.shown(record.mesh);
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
    const blend = blendOf(material);
    const fail = (why) => { core.errors.push(`effects ${kind}: ${why}`); return null; };
    if (!blend || material.depthFunc !== THREE.LessEqualDepth || !material.depthTest || material.depthWrite || !material.colorWrite
      || material.side !== THREE.DoubleSide || material.stencilWrite || material.polygonOffset || material.alphaToCoverage) return fail("state");
    if (batch.mesh.renderOrder !== ORDER || batch.mesh.parent?.isGroup && batch.mesh.parent.renderOrder !== 0) return fail("order");
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
    const record = { batch, kind, id, attributes, mesh: batch.mesh, map, handle: map && core.renderer.properties.get(map).__webglTexture, synced: false, capacity: 0, from: this.frame };
    batch.external = true;
    batch.mesh.visible = false;
    this.records.push(record);
    this.records.sort((a, b) => a.mesh.id - b.mesh.id);
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

  // Shown as three would show it: in the scene, it and its parents visible.
  shown(mesh) {
    const scene = this.core.graphics.scene;
    let o = mesh.parent;
    for (; o && o !== scene; o = o.parent) if (!o.visible) return false;
    return o === scene;
  }

  // The frame's draws and their instances, in three's order.
  draw(ctx) {
    const core = this.core;
    this.batches();
    // A texture three made again (a new handle) is registered again; any
    // registration may move the renderer's memory, so this comes first.
    for (const record of this.records) {
      if (!record.map) continue;
      const handle = core.renderer.properties.get(record.map).__webglTexture;
      if (handle && handle !== record.handle) { record.handle = handle; core.rust.fx_batch_texture(record.id, core.texture(record.map)); }
    }
    // Soft particles: the scene's depth, taken before the first of them as
    // three's batches take it (particle-depth.js).
    const soft = particleDepth.enabled.value > 0.5;
    if (soft) particleDepth.capture(ctx.renderer, ctx.scene, ctx.camera);
    const depth = soft && particleDepth.map.value ? core.texture(particleDepth.map.value) : -1;
    core.views();
    let input = core.fxInput, data = core.fxData;
    // The floats this frame needs: every shown batch's written instances
    // and the owners' rows; the renderer's memory grows first if need be.
    let need = 0;
    for (const record of this.records) {
      const b = record.batch;
      if (b.count > 0) need += b.count * (record.kind === "cpu" ? 21 : 18);
    }
    for (const gpu of this.owners.keys()) need += Math.min(gpu.count, gpu.uniforms.emitterRows.value) * EMITTER_FLOATS;
    if (need > data.length) {
      core.rust.fx_reserve(Math.max(need * 2, data.length * 2));
      core.buffer = null;
      core.views();
      input = core.fxInput; data = core.fxData;
    }
    let at = 0, owners = 0;
    for (const [gpu, o] of this.owners) {
      const rows = gpu.uniforms.emitterRows.value, written = Math.min(gpu.count, rows);
      if (owners === MAX_OWNERS) break;
      const e = OWNER_AT + owners++ * OWNER_STRIDE;
      input[e] = o.id; input[e + 1] = rows; input[e + 2] = written; input[e + 3] = at;
      data.set(gpu.data.subarray(0, written * EMITTER_FLOATS), at);
      at += written * EMITTER_FLOATS;
    }
    let draws = 0, instances = 0;
    for (const record of this.records) {
      const b = record.batch, count = b.count;
      if (!this.drawable(record)) continue;
      if (draws === MAX_DRAWS) break;
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
      input[e] = record.id; input[e + 1] = count; input[e + 2] = from; input[e + 3] = start; input[e + 4] = b.capacity;
      record.synced = true;
      record.capacity = b.capacity;
      instances += count;
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

  // Off: three draws the batches again, everything they hold uploaded anew;
  // on: they come back here, written in full the first frame.
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
  }

  restore() {
    this.setActive(false);
  }
}

export function effectsPass(core) {
  return new EffectsPass(core);
}
