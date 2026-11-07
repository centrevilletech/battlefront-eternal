// Soldier animation clips decoded from the game's zaa_ banks by the extractor.
// Each clip stores, per skeleton bone, frames of [qx, qy, qz, qw, tx, ty, tz]
// at 30 fps. Looping clips repeat their first frame as their last.

const STRIDE = 7;
const ROOT_MOTION = 1;

export class Clip {
  constructor(name, desc, floats, fps) {
    this.name = name;
    this.fps = fps;
    this.frames = desc.frames;
    this.tracks = desc.tracks;
    this.data = floats.subarray(desc.offset, desc.offset + desc.tracks.length * desc.frames * STRIDE);
    this.duration = Math.max(desc.frames - 1, 0) / fps;
    this.rootMotion = (desc.flags & ROOT_MOTION) !== 0;
    // Locomotion clips carry their ground travel on DummyRoot. That travel over
    // one cycle is the stride, and stride / duration is the cycle's own speed.
    this.stride = Math.hypot(desc.travel[0], desc.travel[2]);
    this.speed = this.duration > 0 ? this.stride / this.duration : 0;
  }
}

// boneNames: the skeleton the clips will pose. The bank's tracks follow the
// bone order of the skeleton they were extracted for (meta.bones); a body
// with the same bones in another order (the ARC trooper's) gets its tracks
// matched up by name, and tracks for bones it lacks are dropped.
export function loadClips(meta, buffer, boneNames = null) {
  const floats = new Float32Array(buffer);
  const clips = new Map();
  const lookup = boneNames ? new Map(boneNames.map((name, i) => [name.toLowerCase(), i])) : null;
  const remap = lookup && meta.bones ? meta.bones.map((name) => lookup.get(name.toLowerCase()) ?? -1) : null;
  for (const [name, desc] of Object.entries(meta.clips)) {
    const tracks = remap ? desc.tracks.map((i) => remap[i] ?? -1) : desc.tracks;
    clips.set(name, new Clip(name, { ...desc, tracks }, floats, meta.fps));
  }
  return clips;
}

// Vehicle, prop and rider consumers share the sampler installed before a
// battle constructs their rigs. Existing accumulators bind it when cleared.
// Unit display blending runs directly in rust/bfe-pose and does not use this
// host accumulator. The remaining consumers still need its clip/bone API.
let animKernel = null;
// The page's snapshot ring shares the battle's required native instance.
export function animationKernel() {
  if (!animKernel) throw Error("The native animation sampler is not ready");
  return animKernel;
}
// Rigs are cold resource records from the same required native instance.
export function vehicleConstruction(data, batches = false) {
  if (!animKernel) throw Error("The native animation sampler is not ready");
  return animKernel.constructVehicle(data, batches);
}
export function setAnimKernel(kernel) {
  if (!kernel) throw new Error("The native animation sampler is required");
  animKernel = kernel;
}

// Weighted pose accumulator. Add any number of clip samples, then apply.
export class Pose {
  constructor(boneCount, rootIndex) {
    this.count = boneCount;
    this.root = rootIndex;
    if (!animKernel) throw new Error("The native animation sampler is not ready");
    this.kernel = animKernel;
    this.kernelPose = this.kernel.newPose(this.count, this.root);
    this.refresh();
  }

  // The accumulators as the kernel holds them now: views of its memory,
  // which copying a clip in can grow and detach, so they are re-made
  // after every kernel call that can copy (clear and add here).
  refresh() {
    const views = this.kernel.views(this.kernelPose);
    this.q = views.q;
    this.t = views.t;
    this.w = views.w;
  }

  clear() {
    this.kernel.clear(this.kernelPose);
    this.refresh();
  }

  // time in seconds. Looping clips wrap on their duration; others hold the ends.
  add(clip, time, weight, loop = true) {
    if (!clip || weight <= 1e-4) return;
    this.kernel.add(this.kernelPose, clip, time, weight, loop);
    this.refresh();
  }

  // Bones no clip touched keep their bind pose.
  apply(bones) {
    // pose::apply_out normalizes the accumulated f32 values. Only the
    // drawn bone writes remain here; gameplay uses the native hidden rig.
    const out = this.kernel.applyOut(this.kernelPose);
    this.refresh();
    for (let i = 0; i < this.count; i++) {
      if (this.w[i] <= 1e-6) continue;
      const oi = i * 7;
      const bone = bones[i];
      bone.quaternion.set(out[oi], out[oi + 1], out[oi + 2], out[oi + 3]);
      bone.position.set(out[oi + 4], out[oi + 5], out[oi + 6]);
    }
  }
}
