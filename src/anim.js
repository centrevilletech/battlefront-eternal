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

// Weighted pose accumulator. Add any number of clip samples, then apply.
export class Pose {
  constructor(boneCount, rootIndex) {
    this.count = boneCount;
    this.root = rootIndex;
    this.q = new Float32Array(boneCount * 4);
    this.t = new Float32Array(boneCount * 3);
    this.w = new Float32Array(boneCount);
  }

  clear() {
    this.q.fill(0);
    this.t.fill(0);
    this.w.fill(0);
  }

  // time in seconds. Looping clips wrap on their duration; others hold the ends.
  add(clip, time, weight, loop = true) {
    if (!clip || weight <= 1e-4) return;
    const frames = clip.frames;
    let f = 0;
    if (frames > 1) {
      const span = frames - 1;
      f = time * clip.fps;
      f = loop ? ((f % span) + span) % span : Math.min(Math.max(f, 0), span);
    }
    const i0 = Math.floor(f);
    const i1 = Math.min(i0 + 1, frames - 1);
    const a = f - i0;
    const d = clip.data;
    for (let k = 0; k < clip.tracks.length; k++) {
      const bone = clip.tracks[k];
      if (bone < 0) continue;
      const p0 = (k * frames + i0) * STRIDE;
      const p1 = (k * frames + i1) * STRIDE;
      let x1 = d[p1];
      let y1 = d[p1 + 1];
      let z1 = d[p1 + 2];
      let w1 = d[p1 + 3];
      if (d[p0] * x1 + d[p0 + 1] * y1 + d[p0 + 2] * z1 + d[p0 + 3] * w1 < 0) {
        x1 = -x1;
        y1 = -y1;
        z1 = -z1;
        w1 = -w1;
      }
      let qx = d[p0] + (x1 - d[p0]) * a;
      let qy = d[p0 + 1] + (y1 - d[p0 + 1]) * a;
      let qz = d[p0 + 2] + (z1 - d[p0 + 2]) * a;
      let qw = d[p0 + 3] + (w1 - d[p0 + 3]) * a;
      // Not Math.hypot: with four arguments V8 allocates per call.
      const len = Math.sqrt(qx * qx + qy * qy + qz * qz + qw * qw) || 1;
      qx /= len;
      qy /= len;
      qz /= len;
      qw /= len;
      const qi = bone * 4;
      const acc = this.q;
      if (acc[qi] * qx + acc[qi + 1] * qy + acc[qi + 2] * qz + acc[qi + 3] * qw < 0) {
        qx = -qx;
        qy = -qy;
        qz = -qz;
        qw = -qw;
      }
      acc[qi] += qx * weight;
      acc[qi + 1] += qy * weight;
      acc[qi + 2] += qz * weight;
      acc[qi + 3] += qw * weight;
      const ti = bone * 3;
      // Physics moves the soldier; the clip's own ground travel is dropped.
      if (bone !== this.root || !clip.rootMotion) {
        this.t[ti] += (d[p0 + 4] + (d[p1 + 4] - d[p0 + 4]) * a) * weight;
        this.t[ti + 1] += (d[p0 + 5] + (d[p1 + 5] - d[p0 + 5]) * a) * weight;
        this.t[ti + 2] += (d[p0 + 6] + (d[p1 + 6] - d[p0 + 6]) * a) * weight;
      }
      this.w[bone] += weight;
    }
  }

  // Bones no clip touched keep their bind pose.
  apply(bones) {
    for (let i = 0; i < this.count; i++) {
      const w = this.w[i];
      if (w <= 1e-6) continue;
      const qi = i * 4;
      const ti = i * 3;
      const bone = bones[i];
      bone.quaternion.set(this.q[qi], this.q[qi + 1], this.q[qi + 2], this.q[qi + 3]).normalize();
      bone.position.set(this.t[ti] / w, this.t[ti + 1] / w, this.t[ti + 2] / w);
    }
  }
}
