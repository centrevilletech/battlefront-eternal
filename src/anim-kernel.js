// The pose sampling of src/anim.js in WebAssembly (rust/sim/src/pose, in
// the simulation's module, src/sim/module.js), the page's default (?anim=js
// keeps the JavaScript in src/anim.js, which is also the fallback when the
// module cannot load); the module blends every clip as it does, to the
// last bit (rust/sim/src/pose says how), so the skeleton the page draws
// does not move and the rider poses the simulation reads keep their
// answers.
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
