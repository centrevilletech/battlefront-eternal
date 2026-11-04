// Vehicles.syncCrews publishes bodies from Rust. Lazy hit-shape readers copy
// retained world samples here; they never sample a hull's JavaScript rig.
import { AnimKernel } from './anim-kernel.js';
import { putF2RiderBirth, F2RiderOutMirror, syncF2RiderOut } from './sim/gen/f2-riders.js';
import { KIND_UNIT } from './sim/gen/index.js';

const standins = { man_minigun: ['human_minigun_9pose', 4], drivesnowspeeder: ['human_drive', 0], drivesnowspeedergunner: ['human_drive', 0] };
export class F2Riders {
  constructor(phase) {
    this.phase = phase; this.bridge = phase.bridge; this.kernel = new AnimKernel(phase.x);
    this.sample = { clip: null, pitch: 0, turn: 0, time: null };
    this.known = new WeakMap(); this.rows = new WeakMap(); this.clips = [];
    this.birthArgs = { words: phase.x.f2_rider_birth_words(), put: putF2RiderBirth };
  }
  // riderSample resolves names only when a unit first takes this model's seat.
  ensure(unit, holder) {
    const animator = unit?.animator, bones = animator?.bones;
    if (!bones || !holder) return;
    let known = this.known.get(unit), fresh = !known || known.bones !== bones;
    if (!known || fresh || known.life !== unit.generation) {
      known = { bones, life: unit.generation, holders: new WeakSet() }; this.known.set(unit, known);
    }
    if (known.holders.has(holder)) return;
    const descriptions = fresh ? bones.map(b => ({ name: b.name, parent: bones.indexOf(b.parent), p: b.position, q: b.quaternion, s: b.scale,
      bind: animator.bindLengths?.find(pair => pair[0] === b)?.[1] ?? null })) : [];
    const bindings = [], seats = holder.seats ?? [null];
    for (let i = 0; i < seats.length; i++) {
      const rider = seats[i]?.view.rider ?? holder.rider;
      let clip = rider && (animator.clips.get(rider.clip) || animator.clips.get(rider.anim));
      const standin = rider && !clip && standins[rider.anim];
      if (standin) clip = animator.clips.get(standin[0]);
      const turretSample = !holder.seats ? holder.riderSample(animator.clips, this.sample) : null;
      if (!holder.seats) clip = turretSample?.clip ?? null;
      const id = clip ? this.kernel.clip(clip) : -1;
      if (clip) this.clips[id] = clip;
      bindings.push({ hull: holder.seats ? holder : null, turret: holder.seats ? null : holder, index: i,
        clip: id, nine: holder.seats ? Boolean(rider?.ninePose) : turretSample?.time == null, time: holder.seats ? (standin && clip ? standin[1] / clip.fps : null) : turretSample?.time ?? null });
    }
    const a = { unit, life: unit.generation, scale: unit.rootScale ?? 1, rootBind: animator.rootBind ?? null, bones: descriptions, clips: bindings };
    this.bridge.callArgs(this.birthArgs, a); this.phase.x.f2_rider_birth();
    known.holders.add(holder);
    if (!this.rows.has(unit)) {
      const row = new F2RiderOutMirror(); row.shapes.length = 0; this.rows.set(unit, row);
    }
  }
  sync(ptr) {
    if (!ptr) return;
    const b = this.bridge, f = b.f, base = (ptr / 8) | 0;
    const count = f[base] | 0, stride = f[base + 1] | 0, heap = base + (f[base + 2] | 0);
    for (let i = 0; i < count; i++) {
      const at = base + 5 + i * stride, unit = b.refs[KIND_UNIT].at(f[at], f[at + 1]);
      syncF2RiderOut(b, f, at, heap, this.rows.get(unit));
    }
    this.phase.counters.outputFields += count * stride + f[base + 3];
    this.phase.counters.outputRiders += count * stride + f[base + 3];
  }
  // Unit.updateSpheres owns when the samples become its hit shapes. Do not
  // touch spheresTick here, including across boarding and synchronous deaths.
  shapes(unit, out) {
    const row = this.rows.get(unit);
    if (!row || row.life !== unit.generation) return 0;
    for (let i = 0; i < row.shapes.length; i++) {
      const a = row.shapes[i], b = out[i]; b.type = a.capsule ? 'capsule' : 'sphere'; b.part = a.head ? 'head' : 'body';
      b.center.copy(a.center); b.radius = a.radius; b.yaw = 0;
      if (a.capsule) { b.halfSize.copy(a.half); b.half = b.halfSize; } else b.half = 0;
    }
    return row.shapes.length;
  }
  drop(unit, out) {
    const row = this.rows.get(unit);
    return row && row.life === unit.generation && row.drop ? out.copy(row.drop) : null;
  }
  present(unit) {
    const actor = unit.actor;
    if (!actor) return;
    const row = this.rows.get(unit), rider = unit.seat?.vehicle?.seats[unit.seat.index]?.view.rider;
    unit.presentation?.reset?.(); actor.root.position.copy(unit.soldier.position);
    actor.root.visible = row?.show ?? Boolean(rider?.node);
    const clip = row && row.clip >= 0 ? this.clips[row.clip] : null;
    if (!clip) unit.animator?.gunner?.(null);
    else if (row.time !== null) unit.animator?.gunner?.(clip, 0, 0, row.time);
    else unit.animator?.gunner?.(clip, row.pitch, row.turn);
  }
}
