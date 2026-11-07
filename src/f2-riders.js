import {BlockHeaderLayout as BLOCK} from './sim/gen/boundary.js';
// Vehicles.syncCrews publishes animation and death-drop facts from Rust.
// Hit shapes stay in the native unit and vehicle owners.
import { putF2RiderBirth, F2RiderOutMirror, syncF2RiderOut } from './sim/gen/f2-riders.js';
import { KIND_UNIT } from './sim/gen/index.js';

// riderSample's imported animation aliases. Resolve these during loading, then
// retain numeric clip handles for both native samples and page presentation.
const standins = { man_minigun: ['human_minigun_9pose', 4], drivesnowspeeder: ['human_drive', 0], drivesnowspeedergunner: ['human_drive', 0] };
export class F2Riders {
  constructor(phase) {
    this.phase = phase; this.bridge = phase.bridge;
    this.sample = { clip: null, pitch: 0, turn: 0, time: null };
    this.known = new WeakMap(); this.rows = new WeakMap(); this.clips = [];
    this.birthArgs = { words: phase.x.f2_rider_birth_words(), put: putF2RiderBirth };
  }
  // Native boarding never calls the page facade. Resolve every possible
  // holder at UnitFactory construction, including dormant pad variants.
  ensureAll(unit) {
    for (const holder of this.phase.vehicles.vehicles) this.ensure(unit, holder);
    for (const holder of this.phase.prepared) this.ensure(unit, holder);
    for (const holder of this.phase.seats.crews?.turrets ?? []) this.ensure(unit, holder);
  }
  // PlayerBodies retains the viewpoint identity while exchanging prepared
  // class resources. Mirrors follow that cold resource transfer too.
  alias(from, to) {
    const known = this.known.get(from), row = this.rows.get(from);
    if (known) this.known.set(to, known); else this.known.delete(to);
    if (row) this.rows.set(to, row); else this.rows.delete(to);
  }
  // riderSample resolves names only during construction or external boarding.
  ensure(unit, holder) {
    const animator = unit?.animator;
    if (!animator || !holder) return;
    // Class resources identify the native sampler. No bone hierarchy or
    // bind-length copy crosses the unit/vehicle ownership boundary.
    let known = this.known.get(unit);
    if (!known || known.animator !== animator || known.life !== unit.generation) {
      known = { animator, life: unit.generation, holders: new WeakSet() }; this.known.set(unit, known);
    }
    if (known.holders.has(holder)) return;
    const bindings = [], seats = holder.seats ?? [null];
    for (let i = 0; i < seats.length; i++) {
      const rider = seats[i]?.view.rider ?? holder.rider;
      let clip = rider && (animator.clips.get(rider.clip) || animator.clips.get(rider.anim));
      // riderSample keeps the original named stand-ins and their fixed frame.
      // Quirk: a timed stand-in does not use the ordinary nine-pose weights.
      const standin = rider && !clip && standins[rider.anim];
      if (standin) clip = animator.clips.get(standin[0]);
      const turretSample = !holder.seats ? holder.riderSample(animator.clips, this.sample) : null;
      if (!holder.seats) clip = turretSample?.clip ?? null;
      // The native unit already imported its immutable clip source. Share
      // that handle instead of loading the same tracks for the vehicle owner.
      const id = clip ? unit.f3.anim.clip(clip) : -1;
      if (clip) this.clips[id] = clip;
      bindings.push({ hull: holder.seats ? holder : null, turret: holder.seats ? null : holder, index: i,
        clip: id, nine: holder.seats ? Boolean(rider?.ninePose) : turretSample?.time == null, time: holder.seats ? (standin && clip ? standin[1] / clip.fps : null) : turretSample?.time ?? null });
    }
    // Native riderBody owns the geometry from this point. The output mirror
    // persists with the unit, including across a move to another holder.
    const a = { unit, life: unit.generation, scale: unit.rootScale ?? 1, clips: bindings };
    this.bridge.callArgs(this.birthArgs, a); this.phase.x.f2_rider_birth();
    known.holders.add(holder);
    if (!this.rows.has(unit)) {
      const row = new F2RiderOutMirror(); this.rows.set(unit, row);
    }
  }
  // Vehicles.syncCrews publishes only changed samples. Decode into retained
  // rows; unchanged riders neither copy a skeleton nor rebuild hit shapes.
  sync(ptr) {
    if (!ptr) return;
    const b = this.bridge, f = b.f, base = (ptr / 8) | 0;
    if (this.phase.counters) this.phase.counters.outputBlock(f,base);
    const count = f[base] | 0, stride = f[base + BLOCK.stride] | 0, heap = base + (f[base + BLOCK.heap] | 0);
    for (let i = 0; i < count; i++) {
      const at = base + BLOCK.words + i * stride, unit = b.refs[KIND_UNIT].at(f[at], f[at + 1]);
      syncF2RiderOut(b, f, at, heap, this.rows.get(unit));
    }

  }
  // Vehicles.vacate consumes the completed riderDropPoint on death. A late
  // packet for another life cannot move the current actor's corpse.
  drop(unit, out) {
    const row = this.rows.get(unit);
    return row && row.life === unit.generation && row.drop ? out.copy(row.drop) : null;
  }
  // The rider's actor and animation hooks remain presentation. Native rows
  // supply the sample; no world-phase node read or rig math happens here.
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
