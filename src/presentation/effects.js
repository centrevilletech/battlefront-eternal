import * as THREE from "three";
import { EffectResources } from "../effects.js";
import { presentationProfile } from "../presentation-profile.js";
import { PresentationCapture } from "./capture.js";
import { effectContent } from "./content.js";
import {
  VERSION,
  Frame as F,
  Command as C,
  CommandKind as K,
  Header as H,
  Effect as E,
  Draw as D,
  DrawKind as DK,
} from "./layout.js";

import {presentationModule} from './module.js';
export async function loadPresentation() { return presentationModule(); }
// Only mutable host commands remain on this handle. No particle or emitter
// clock is mirrored on the page. Generation belongs to the birth record.
export class EffectHandle {
  constructor(defs, position, quaternion, velocity, id, generation, seed) {
    this.prewarm = 0;
    this.id = id;
    this.generation = generation;
    this.seed = seed;
    this.fresh = true;
    this.dirty = true;
    this.nozzleDirty = false;
    this.position = position.clone();
    this.birthPosition = position.clone();
    this.quaternion = quaternion.clone();
    this.velocity = velocity?.clone() || new THREE.Vector3();
    this.tint = 1;
    this.scale = 1;
    this.inherit = 0;
    this.stopped = false;
    this.killed = false;
    this.done = false;
    this.time = 0;
    this.emitters = defs.map((def) => ({ def }));
  }
  move(position, quaternion, velocity) {
    if (
      !this.position.equals(position) ||
      (quaternion && !this.quaternion.equals(quaternion)) ||
      (velocity && !this.velocity.equals(velocity))
    )
      this.dirty = true;
    this.position.copy(position);
    if (quaternion) this.quaternion.copy(quaternion);
    if (velocity) this.velocity.copy(velocity);
  }
  get tint() {
    return this._tint;
  }
  set tint(v) {
    if (v !== this._tint) this.dirty = true;
    this._tint = v;
  }
  get scale() {
    return this._scale;
  }
  set scale(v) {
    if (v !== this._scale) this.dirty = true;
    this._scale = v;
  }
  get inherit() {
    return this._inherit;
  }
  set inherit(v) {
    if (v !== this._inherit) this.dirty = true;
    this._inherit = v;
  }
  get stopped() {
    return this._stopped;
  }
  set stopped(v) {
    if (v !== this._stopped) this.dirty = true;
    this._stopped = v;
  }
  get killed() {
    return this._killed;
  }
  set killed(v) {
    if (v !== this._killed) this.dirty = true;
    this._killed = v;
  }
  stop() {
    this.stopped = true;
  }
  kill() {
    this.stopped = true;
    this.killed = true;
  }
  // Outlet selection is a host resource hook; nozzle matrices enter the
  // same batch as movement, without an emitter call into WebAssembly.
  attachNozzles(frames) {
    if (!frames?.length) return;
    for (const em of this.emitters) {
      const box = em.def.spawner.offset;
      const offset = (em.offset ||= new THREE.Vector3());
      offset
        .set((box[0][0] + box[0][1]) * 0.5, (box[1][0] + box[1][1]) * 0.5, (box[2][0] + box[2][1]) * 0.5)
        .applyQuaternion(this.quaternion);
      const authored = (em.authored ||= new THREE.Vector3());
      authored.copy(offset).add(this.position);
      let frame = frames[0];
      for (let i = 1; i < frames.length; i++)
        if (authored.distanceToSquared(frame.position) >= authored.distanceToSquared(frames[i].position))
          frame = frames[i];
      const axis = (em.axis ||= new THREE.Vector3());
      axis.set(-1, 0, 0).applyQuaternion(this.quaternion);
      em.nozzleTurn ||= new THREE.Quaternion();
      em.nozzleTurn.setFromUnitVectors(axis, frame.direction);
      em.nozzleOffset ||= new THREE.Vector3();
      em.nozzleOffset.copy(frame.position).sub(this.position).sub(offset.applyQuaternion(em.nozzleTurn));
    }
    this.nozzleDirty = true;
  }
}

// What a refused start returns: a handle that is already finished and
// ignores every call. It never enters the batch, so its id (0) names nothing.
const REFUSED = new EffectHandle([], new THREE.Vector3(), new THREE.Quaternion(), null, 0, 0, 0);
REFUSED.done = true;

export class PageEffects extends EffectResources {
  constructor(scene, library, options, loaded) {
    super(scene, library, options);
    this.loaded = loaded;
    this.content = effectContent(this.library, this.gpu);
    this.kernel = new loaded.module.PagePresentation(JSON.stringify(this.content.data));
    const k = this.kernel,
      buffer = loaded.memory.buffer;
    this.input = new Float64Array(buffer, k.input_ptr(), k.input_len());
    this.header = new Float64Array(buffer, k.header_ptr(), H.STRIDE);
    this.effectOutput = new Float64Array(buffer, k.effects_ptr(), k.effects_len());
    this.drawOutput = new Float64Array(buffer, k.draws_ptr(), k.draws_len());
    this.eventOutput = new Float64Array(buffer, k.events_ptr(), k.events_len());
    // Ids come from the kernel's own table, so the page can never hold more
    // live handles than the kernel can name.
    const ids = k.handles_len();
    this.handles = new Array(ids).fill(null);
    this.generations = new Uint32Array(ids);
    this.available = new Uint32Array(ids - 1);
    for (let i = 0; i < ids - 1; i++) this.available[i] = ids - 1 - i;
    this.freeCount = ids - 1;
    this.refusedHandles = 0;
    this.reported = false;
    this.sequence = 0;
    this.frame = 0;
    this.tick = 0;
    this.epoch = 0;
    this.inputClock = 0;
    this.playoutTick = 0;
    this.cpuBatches = new Array(this.content.source.length).fill(null);
    this.batches = new Array(this.content.source.length).fill(null);
    this.modelPools = new Array(this.content.source.length).fill(null);
    for (let i = 0; i < this.content.source.length; i++) {
      const g = this.content.source[i].geometry;
      if (g.type !== "geometry") continue;
      const make = this.model?.(g.model);
      if (!make) continue;
      let pool = this.meshes.get(g.model);
      if (!pool) {
        pool = { list: [], used: 0, make };
        this.meshes.set(g.model, pool);
      }
      this.modelPools[i] = pool;
    }
    this.axisScratch = [0, 0, 0];
    this.axis2Scratch = [0, 0, 0];
    this.frameScratch = [0, 0, 1, 1];
    this.capture = options.record ? new PresentationCapture(options.captureCapacity) : null;
    this.calls = 0;
  }
  start(defs, position, quaternion, velocity, seed = null) {
    const content = this.content.handles.get(defs);
    if (content === undefined) throw Error("unregistered presentation effect content");
    // Every id in use: the old page would have grown its list. Refuse the
    // birth with a finished handle rather than failing the frame.
    if (!this.freeCount) {
      this.refusedHandles++;
      return REFUSED;
    }
    const id = this.available[--this.freeCount],
      generation = ++this.generations[id];
    const h = new EffectHandle(
      defs,
      position,
      quaternion,
      velocity,
      id,
      generation,
      seed ?? (Math.random() * 4294967296) >>> 0,
    );
    h.content = content;
    this.handles[id] = h;
    this.effects.push(h);
    return h;
  }
  command(h, kind, row) {
    const a = this.input,
      at = F.STRIDE + row * C.STRIDE;
    a[at + C.SEQUENCE] = ++this.sequence;
    a[at + C.KIND] = kind;
    a[at + C.ID] = h.id;
    a[at + C.GENERATION] = h.generation;
    a[at + C.TICK] = this.tick;
    a[at + C.CONTENT] = h.content;
    a[at + C.SEED] = h.seed;
    h.position.toArray(a, at + C.POSITION);
    h.quaternion.toArray(a, at + C.QUATERNION);
    h.velocity.toArray(a, at + C.VELOCITY);
    a[at + C.TINT] = h.tint;
    a[at + C.SCALE] = h.scale;
    a[at + C.INHERIT] = h.inherit;
    a[at + C.STOPPED] = +h.stopped;
    a[at + C.KILLED] = +h.killed;
    return at;
  }
  refresh() {
    const k = this.kernel,
      b = this.loaded.memory.buffer;
    if (this.input.buffer === b) return;
    this.input = new Float64Array(b, k.input_ptr(), k.input_len());
    this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
    this.effectOutput = new Float64Array(b, k.effects_ptr(), k.effects_len());
    this.drawOutput = new Float64Array(b, k.draws_ptr(), k.draws_len());
    this.eventOutput = new Float64Array(b, k.events_ptr(), k.events_len());
  }
  update(dt) {
    this.refresh();
    const a = this.input;
    a[F.VERSION] = VERSION;
    a[F.FRAME] = ++this.frame;
    a[F.EPOCH] = this.epoch;
    a[F.TICK] = this.tick;
    a[F.INPUT_CLOCK] = this.inputClock;
    a[F.PLAYOUT_TICK] = this.playoutTick;
    a[F.DT] = dt;
    a[F.PRESENT] = 1;
    a[F.FROZEN] = 0;
    let commands = 0;
    const room = (a.length - F.STRIDE) / C.STRIDE;
    for (let i = 0; i < this.effects.length; i++) {
      const effect = this.effects[i];
      // A full command block (a long stall with many births) leaves the rest
      // dirty for the next frame, whole: an effect's commands never split.
      const needed =
        (effect.dirty ? (effect.fresh && !effect.birthPosition.equals(effect.position) ? 2 : 1) : 0) +
        (effect.prewarm ? 1 : 0) +
        (effect.nozzleDirty ? effect.emitters.length : 0);
      if (commands + needed > room) break;
      if (effect.dirty) {
        const at = this.command(effect, effect.fresh ? K.SPAWN : K.SYNC, commands++);
        if (effect.fresh) {
          effect.birthPosition.toArray(a, at + C.POSITION);
          if (!effect.birthPosition.equals(effect.position)) this.command(effect, K.SYNC, commands++);
        }
        effect.fresh = false;
        effect.dirty = false;
      }
      if (effect.prewarm) {
        const at = this.command(effect, K.PREWARM, commands++);
        a[at + C.SECONDS] = effect.prewarm;
        effect.prewarm = 0;
      }
      if (!effect.nozzleDirty) continue;
      for (let j = 0; j < effect.emitters.length; j++) {
        const em = effect.emitters[j];
        if (!em.nozzleTurn) continue;
        const at = this.command(effect, K.NOZZLE, commands++);
        a[at + C.EMITTER] = j;
        em.nozzleTurn.toArray(a, at + C.NOZZLE_TURN);
        em.nozzleOffset.toArray(a, at + C.NOZZLE_OFFSET);
      }
      effect.nozzleDirty = false;
    }
    a[F.COMMANDS] = commands;
    this.kernel.present_frame();
    this.calls++;
    // Only a page built against another kernel layout fails; full stores
    // refuse births and are reported once.
    if (this.header[H.STATUS]) throw Error(`presentation kernel layout mismatch: ${this.header[H.STATUS]}`);
    if (!this.reported) this.reportRefusals();
    const started = presentationProfile?.start();
    this.publish();
    presentationProfile?.finish("outputPublication", started);
    this.capture?.record(a, this.header, this.effectOutput, this.drawOutput, this.eventOutput);
  }
  reportRefusals() {
    const h = this.header,
      refused = h[H.REFUSED_EFFECTS] + h[H.REFUSED_PARTICLES] + h[H.SKIPPED_DRAWS] + h[H.SKIPPED_COMMANDS];
    if (!refused && !this.refusedHandles) return;
    this.reported = true;
    console.warn(
      `presentation stores full: ${h[H.REFUSED_EFFECTS] + this.refusedHandles} effect and ` +
        `${h[H.REFUSED_PARTICLES]} particle births refused, ${h[H.SKIPPED_DRAWS]} draws and ` +
        `${h[H.SKIPPED_COMMANDS]} commands skipped`,
    );
  }
  // Map sources are born already flowing. This queues their age inside the
  // next environment batch; it never enters WebAssembly for one emitter.
  step(effect, seconds) {
    effect.prewarm += seconds;
  }
  get particleCount() {
    return this.header[H.PARTICLES];
  }
  beginCapture(capacity) {
    if (this.frame) throw Error("presentation recording must start before the first frame; use presentRecord=1");
    this.capture = new PresentationCapture(capacity);
  }
  endCapture() {
    const capture = this.capture;
    this.capture = null;
    return capture
      ? {
          version: VERSION,
          frames: capture.frames,
          full: capture.full,
          content: this.content.data,
          records: capture.export(),
        }
      : null;
  }
  publish() {
    this.renderer.begin();
    this.gpu?.begin();
    for (const pool of this.meshes.values()) pool.used = 0;
    const effects = this.effectOutput,
      draws = this.drawOutput;
    for (let i = 0; i < this.header[H.EFFECTS]; i++) {
      const at = i * E.STRIDE,
        id = effects[at + E.ID],
        handle = this.handles[id];
      // Rows carry the generation; a reused id never updates an older handle.
      if (handle && handle.generation === effects[at + E.GENERATION]) {
        handle.time = effects[at + E.TIME];
        handle.done = !!effects[at + E.DONE];
      }
      if (!effects[at + E.KILLED]) this.gpu?.registerRecord(effects, at);
    }
    for (let i = 0; i < this.header[H.DRAWS]; i++) {
      const at = i * D.STRIDE,
        index = draws[at + D.DEFINITION],
        def = this.content.source[index],
        geometry = def.geometry;
      const kind = draws[at + D.KIND];
      if (kind === DK.MODEL) {
        const pool = this.modelPools[index];
        if (!pool) continue;
        let mesh = pool.list[pool.used];
        if (!mesh) {
          // Effects.drawModel chose this when a pool slot was first used,
          // so definitions sharing a model retain birth-order blend choices.
          mesh = pool.make(geometry.blend === "additive");
          pool.list.push(mesh);
          this.scene.add(mesh);
        }
        pool.used++;
        mesh.visible = true;
        mesh.position.fromArray(draws, at + D.POSITION);
        mesh.quaternion.fromArray(draws, at + D.TURN);
        mesh.scale.setScalar(draws[at + D.SIZE]);
        mesh.userData.material.color.setRGB(draws[at + D.COLOR], draws[at + D.COLOR + 1], draws[at + D.COLOR + 2]);
        mesh.userData.material.opacity = draws[at + D.COLOR + 3];
        continue;
      }
      const batches = kind === DK.ANALYTIC ? this.batches : this.cpuBatches;
      let batch = batches[index];
      if (!batch) {
        const texture = geometry.texture ? this.emitterTexture(def) : null;
        if (!texture) continue;
        batch = batches[index] =
          kind === DK.ANALYTIC
            ? this.gpu.batch(def, texture)
            : this.renderer.batch(texture, geometry.blend === "additive" ? "additive" : "normal");
      }
      if (kind === DK.ANALYTIC) {
        const slot = draws[at + D.EFFECT];
        batch.pushRecord(draws, at, slot, this.gpu.definitions.get(def), this.gpu.epochs[slot]);
      } else {
        for (let j = 0; j < 3; j++) {
          this.axisScratch[j] = draws[at + D.AXIS + j];
          this.axis2Scratch[j] = draws[at + D.AXIS2 + j];
        }
        for (let j = 0; j < 4; j++) this.frameScratch[j] = draws[at + D.FRAME + j];
        batch.push(
          draws[at + D.POSITION],
          draws[at + D.POSITION + 1],
          draws[at + D.POSITION + 2],
          draws[at + D.COLOR],
          draws[at + D.COLOR + 1],
          draws[at + D.COLOR + 2],
          draws[at + D.COLOR + 3],
          draws[at + D.SIZE],
          draws[at + D.ROTATION],
          draws[at + D.SHAPE],
          draws[at + D.LENGTH],
          this.axisScratch,
          this.axis2Scratch,
          this.frameScratch,
        );
      }
    }
    this.renderer.end();
    this.gpu?.end();
    for (const pool of this.meshes.values())
      for (let i = pool.used; i < pool.list.length; i++) pool.list[i].visible = false;
    let kept = 0;
    for (let i = 0; i < this.effects.length; i++) {
      const effect = this.effects[i];
      if (effect.done) {
        this.handles[effect.id] = null;
        this.available[this.freeCount++] = effect.id;
      } else this.effects[kept++] = effect;
    }
    this.effects.length = kept;
  }
}
