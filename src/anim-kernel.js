// The shared pose sampler from rust/bfe-pose, exposed by the simulation
// module for retained vehicle, prop and rider consumers. A battle installs
// this binding before constructing those rigs; module load errors propagate.
// Unit display animation owns its blends and state inside bfe-presentation.
//
// The module holds a copy of each clip it samples (its frames and which
// bone each track drives), made the first time the clip is sampled, and
// one pose per Pose. A pose's q, t and w are read on this side where the
// JavaScript reads them (the animator's overlays, a rider's points), as
// views of the module's memory; a copy can grow that memory and detach
// the views, so they are made again after every call that can copy.
import { loadSim } from "./sim/module.js";

// The kernel of a battle, on its instance of the simulation's module (the
// one its other kernels use), or on an instance of its own: the module is
// compiled once per thread, and each battle gets its own instance, whose
// copies go with it when the battle ends.
export async function loadAnimKernel(instance = null) {
  return new AnimKernel((instance ?? await loadSim()).exports);
}

export class AnimKernel {
  constructor(exports) {
    this.x = exports;
    this.memory = exports.memory;
    this.clipIds = new WeakMap();
    // Each pose's bone count, by id, for its views' lengths.
    this.counts = [];
    // Each pose's views, kept until a copy grows the memory and detaches
    // them (a detached view reports no length, which is the signal).
    this.viewsByPose = [];
    this.outByPose = [];
  }

  // Cold construction returns records, never live node callbacks. The same
  // constructor serves the page and the world's compatibility shells.
  constructVehicle(data, batches = false) {
    const negativeZero = '~negative zero~';
    const json = JSON.stringify(data, (_,value) => Object.is(value,-0) ? negativeZero : value).replaceAll(`"${negativeZero}"`,'-0.0');
    const encoded = new TextEncoder().encode(json);
    const at = this.x.pose_construction_input(encoded.length);
    new Uint8Array(this.memory.buffer, at, encoded.length).set(encoded);
    const failed = batches ? this.x.pose_construct_batches() : this.x.pose_construct_vehicle();
    const text = new TextDecoder().decode(new Uint8Array(this.memory.buffer, this.x.pose_construction_output(), this.x.pose_construction_length()));
    if (failed) throw Error(`Vehicle resource: ${text}`);
    return JSON.parse(text);
  }

  // A clip's copy, made the first time it is sampled: its frames (f32, as
  // the extractor wrote them) and the bone each track drives, with -1
  // for one the body lacks. Copying can grow the memory.
  clip(clip) {
    let id = this.clipIds.get(clip);
    if (id === undefined) {
      id = this.x.clip_new(clip.frames, clip.tracks.length, clip.fps, clip.rootMotion ? 1 : 0);
      new Float32Array(this.memory.buffer, this.x.clip_data(id), clip.data.length).set(clip.data);
      new Int32Array(this.memory.buffer, this.x.clip_tracks(id), clip.tracks.length).set(clip.tracks);
      this.clipIds.set(clip, id);
    }
    return id;
  }

  newPose(count, root) {
    const id = this.x.pose_new(count, root);
    this.counts[id] = count;
    return id;
  }

  clear(id) {
    this.x.pose_clear(id);
  }

  add(id, clip, time, weight, loop) {
    this.x.pose_add(id, this.clip(clip), time, weight, loop ? 1 : 0);
  }

  // The pose's accumulators, as views of the memory (a later copy can
  // grow it and detach them; then they are made again here).
  views(id) {
    const kept = this.viewsByPose[id];
    if (kept && kept.q.byteLength) return kept;
    const count = this.counts[id];
    const buffer = this.memory.buffer;
    return this.viewsByPose[id] = {
      q: new Float32Array(buffer, this.x.pose_q(id), count * 4),
      t: new Float32Array(buffer, this.x.pose_t(id), count * 3),
      w: new Float32Array(buffer, this.x.pose_w(id), count),
    };
  }

  // The pose's apply finals (rust/sim/src/pose pose_out): count * 7 f64.
  applyOut(id) {
    const at = this.x.pose_out(id);
    const kept = this.outByPose[id];
    if (kept && kept.byteLength && kept.byteOffset === at) return kept;
    return this.outByPose[id] = new Float64Array(this.memory.buffer, at, this.counts[id] * 7);
  }
}
