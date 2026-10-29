// Remote presentation from the snapshot ring: an entity is drawn between
// the two snapshots that bracket render time (slerp for vehicles,
// shortest-arc yaw for infantry). If the newer bracket is missing, it
// extrapolates at its last velocity for at most 100 ms (6 ticks), then
// holds. A `new` life or the discontinuity bit snaps: brackets never blend
// across them. No coasting brakes, wall rays or speed-inferred jumps.
import { Quaternion } from 'three';

const angle = (a, b, t) => a + Math.atan2(Math.sin(b - a), Math.cos(b - a)) * t;
const poseKinds = new Set(['soldier', 'vehicle', 'droideka']);
export class SnapshotTimeline {
  constructor({ maxSnapshots = 64, hz = 60, extrapolateTicks = 6 } = {}) {
    Object.assign(this, { maxSnapshots, hz, extrapolateTicks });
    this.qa = new Quaternion(); this.qb = new Quaternion();
    this.reset(0);
  }
  reset(epoch) { this.epoch = epoch; this.snapshots = []; this.fire = new Map(); }
  remove(id) { for (const snap of this.snapshots) snap.poses.delete(id); this.fire.delete(id); }
  // poses: Map<entityId, record-shaped pose> for one snapshot tick. A later
  // part of the same tick replaces it. Poses are copied on the way in.
  addSnapshot(tick, poses, epoch = this.epoch) {
    if (epoch < this.epoch) return false;
    if (epoch > this.epoch) this.reset(epoch);
    const copy = new Map();
    for (const [id, record] of poses) {
      if (!poseKinds.has(record.kind)) continue;
      const sample = { ...record, position: [...record.position], velocity: [...record.velocity] };
      if (record.quaternion) sample.quaternion = [...record.quaternion];
      // Unwrap the u8 fire counter to a monotonic count per life; a jump
      // over half the range is a wrap the other way and does not count.
      if (record.fireCounter !== undefined) {
        const key = `${id}:${record.generation}`, last = this.fire.get(key);
        if (!last || tick >= last.tick) {
          const total = last ? last.total + ((record.fireCounter - last.raw + 256) % 256 <= 128 ? (record.fireCounter - last.raw + 256) % 256 : 0) : record.fireCounter;
          this.fire.set(key, { tick, raw: record.fireCounter, total });
          sample.fireCounter = total;
        }
      }
      copy.set(id, sample);
    }
    const existing = this.snapshots.findIndex(s => s.tick === tick);
    if (existing >= 0) for (const [id, sample] of copy) this.snapshots[existing].poses.set(id, sample);
    else {
      this.snapshots.push({ tick, poses: copy });
      this.snapshots.sort((a, b) => a.tick - b.tick);
      while (this.snapshots.length > this.maxSnapshots) this.snapshots.shift();
    }
    return true;
  }
  sample(id, tick, out = {}) {
    if (!Number.isFinite(tick)) throw Error('invalid presentation tick');
    let a = null, aTick = 0, b = null, bTick = 0;
    for (const snap of this.snapshots) {
      const pose = snap.poses.get(id);
      if (!pose) continue;
      if (snap.tick <= tick) { a = pose; aTick = snap.tick; b = pose; bTick = snap.tick; }
      else { b = pose; bTick = snap.tick; break; }
    }
    if (!a) return null;
    // Never blend across a new life, a class change or a discontinuity:
    // hold the older bracket until render time reaches the newer one.
    if (b !== a && (b.generation !== a.generation || b.kind !== a.kind || b.disc)) b = a, bTick = aTick;
    const blend = bTick > aTick ? Math.max(0, Math.min(1, (tick - aTick) / (bTick - aTick))) : 0;
    const held = a === b && tick >= aTick;
    const extra = held && (a.health > 0 || a.state === 8) ? Math.min(this.extrapolateTicks, tick - aTick) / this.hz : 0;
    out.id = id; out.generation = a.generation; out.kind = a.kind;
    out.fromTick = aTick; out.toTick = bTick; out.extrapolatedTicks = extra * this.hz;
    out.segment = aTick; out.disc = !!a.disc;
    out.position ??= [0, 0, 0]; out.velocity ??= [0, 0, 0];
    out.pathVelocity ??= [0, 0, 0]; out.coastOrigin ??= [0, 0, 0];
    for (let i = 0; i < 3; i++) {
      out.velocity[i] = a.velocity[i] + (b.velocity[i] - a.velocity[i]) * blend;
      out.position[i] = a.position[i] + (b.position[i] - a.position[i]) * blend + a.velocity[i] * extra;
      out.pathVelocity[i] = bTick > aTick ? (b.position[i] - a.position[i]) * this.hz / (bTick - aTick) : (extra > 0 && tick - aTick < this.extrapolateTicks ? a.velocity[i] : 0);
      out.coastOrigin[i] = a.position[i];
    }
    for (const name of ['classId', 'team', 'health', 'state']) out[name] = a[name];
    if (a.kind === 'soldier' || a.kind === 'droideka') {
      out.yaw = angle(a.yaw, b.yaw, blend); out.pitch = a.pitch + (b.pitch - a.pitch) * blend;
      out.yawRate = bTick > aTick ? (angle(a.yaw, b.yaw, 1) - a.yaw) * this.hz / (bTick - aTick) : 0;
      out.pitchRate = bTick > aTick ? (b.pitch - a.pitch) * this.hz / (bTick - aTick) : 0;
      for (const name of ['posture', 'weapon', 'fireCounter']) out[name] = a[name];
      if (a.kind === 'droideka') {
        for (const name of ['mode', 'reverse', 'dead', 'shield', 'shieldOn']) out[name] = a[name];
        for (const name of ['time', 'roll', 'lean', 'legs']) out[name] = a.mode === b.mode ? a[name] + (b[name] - a[name]) * blend : a[name];
      }
      delete out.quaternion; delete out.power;
    } else {
      out.quaternion ??= [0, 0, 0, 1];
      this.qa.fromArray(a.quaternion); this.qb.fromArray(b.quaternion);
      this.qa.slerp(this.qb, blend).normalize().toArray(out.quaternion); out.power = a.power;
      for (const name of ['yaw', 'pitch', 'yawRate', 'pitchRate', 'posture', 'weapon', 'fireCounter']) delete out[name];
    }
    return out;
  }
}
// Binding layer only: the timeline's sample is what is drawn, with no
// late-bracket offsets or coast clamping of its own.
export class SnapshotPresentation {
  constructor(timeline, { maxEntities = 768 } = {}) {
    if (!timeline?.sample) throw Error('invalid snapshot presentation');
    this.timeline = timeline; this.maxEntities = maxEntities; this.epoch = timeline.epoch;
    this.bindings = new Map();
  }
  bindSoldier(id, generation, presentation) { return this.bind(id, generation, 'soldier', presentation); }
  bindVehicle(id, generation, vehicle) { return this.bind(id, generation, 'vehicle', vehicle); }
  bind(id, generation, kind, target) {
    if (!Number.isSafeInteger(id) || id < 0 || !Number.isSafeInteger(generation) || generation < 0 || !target) throw Error('invalid snapshot presentation binding');
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    const current = this.bindings.get(id);
    if (current?.generation === generation && current.kind === kind && current.target === target) return target;
    if (!this.bindings.has(id) && this.bindings.size >= this.maxEntities) throw Error('snapshot presentation capacity exceeded');
    this.unbind(id);
    this.bindings.set(id, { generation, kind, target, sample: {} });
    return target;
  }
  unbind(id) {
    const binding = this.bindings.get(id);
    if (binding) { binding.target.networkPose = null; binding.target.networkFire = null; }
    this.bindings.delete(id);
  }
  reset(epoch = this.timeline.epoch) { for (const id of [...this.bindings.keys()]) this.unbind(id); this.epoch = epoch; }
  update(tick, { playerId = null, vehicleId = null } = {}) {
    if (!Number.isFinite(tick)) throw Error('invalid snapshot presentation time');
    if (this.epoch !== this.timeline.epoch) this.reset(this.timeline.epoch);
    for (const [id, binding] of this.bindings) {
      const { target, sample } = binding;
      if (id === playerId || id === vehicleId) { target.networkPose = null; continue; }
      const pose = this.timeline.sample(id, tick, sample);
      if (!pose) { target.networkPose = null; continue; }
      if (pose.generation !== binding.generation || pose.kind !== binding.kind) { this.unbind(id); continue; }
      target.networkPose = pose;
    }
  }
}
