// The mounted weapon boundary: definitions and emitter identities at birth,
// frozen foreign input at latch, publication and ordered presentation at end.
import { Vector3 } from 'three';
import { rigNodes } from './vehicle-phase-data.js';
import { registerWeaponClass } from './weapon-content-sim.js';
import { seatLockTarget } from './weapon-targeting.js';
import { categoryOf } from './ai/classic/weapons-ai.js';
import { putFfBirth, FfOutputMirror, syncFfOutput } from './sim/gen/f2-firing.js';

const zero = new Vector3();
const mutable = ['deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated', 'floor', 'kick',
  'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime', 'released', 'hidden', 'charging', 'held',
  'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait', 'salvoSeed', 'salvoCount', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const num = x => Number.isFinite(parseFloat(x)) ? parseFloat(x) : 0;
export class F2Firing {
  constructor(phase) {
    this.phase = phase; this.bridge = phase.bridge; this.vs = phase.vehicles;
    this.output = { words: FfOutputMirror.words, Mirror: FfOutputMirror, sync: syncFfOutput };
    this.aim = new Vector3();
    this.bridge.register({
      f2WeaponDraw: a => ({ value: a.hull.seats[a.seat].weapons[a.weapon].state.random() }),
      f2TargetFacts: a => this.targetFacts(a),
      // AI and other state-changing listeners retain their synchronous slot.
      f2Fired: a => { const shot = this.shot(a); phase.hook(() => this.vs.world.fired(a.hull, a.seat, shot, a.owner, a.origin.clone(), a.direction.clone())); },
    });
  }
  adopt(v) {
    const b = this.bridge, nodes = rigNodes(v), node = n => n ? nodes.indexOf(n) : -1;
    const named = name => v.byName[(name || '').toLowerCase()];
    const seats = v.seats.map(s => ({ weapons: s.weapons.map(w => {
      registerWeaponClass(b, w.state.levels[0]);
      if (w.def.ordnance) this.vs.ordnance.sim.defId(w.def.ordnance);
      const state = { ...w.state, level: w.state.levels.indexOf(w.state.def), admits: false, hasRandom: Boolean(w.state.random), melee: null };
      return { state, ordnance: w.def.ordnance ?? null, tow: w.def.ordnance?.base === 'towcable', range: Math.max(20, w.def.LockOnRange || 0),
        cableLength: Math.max(10, w.def.ordnance?.CableLength || 60), channel: w.state.channel, effect: w.def.MuzzleFlashEffect || null,
        weaponNode: node(named(w.def.FirePointName)), fallbackNode: node(named(w.aimers[0]?.FirePointName || w.aimers[0]?.node || w.def.FirePointName)),
        directionNodes: w.aimers.map(a => node(named(a.FirePointName))), outside: w.aimers.map(a => num(a.FireOutsideLimits ?? s.settings?.FireOutsideLimits)),
        emitter: (w.muzzles.length ? w.emitter : w.aimers[0]?.emitter ?? w.emitter) ?? s.emitter,
        muzzles: w.muzzles.map(m => ({ emitter: m.emitter ?? w.emitter ?? s.emitter })) };
    }) }));
    b.callArgs({ words: this.phase.x.f2_firing_words(0), put: putFfBirth }, { hull: v, life: v.aiTarget.generation, legTop: v.legTop, legs: v.legs, seats });
    this.phase.x.f2_firing_adopt();
  }
  input(out) {
    for (const v of this.vs.vehicles) for (const seat of v.seats) {
      const l = seat.lease, input = l?.input, picked = seatLockTarget(l);
      const index = picked === undefined ? -2 : picked === null ? -1 : this.vs.units.all().indexOf(picked);
      out.push(input?.reload ? 1 : 0, index);
      for (let c = 0; c < seat.weapons.length; c++) {
        const w = seat.weapons[c].state;
        out.push(input?.pressed[c] ? 1 : 0, input?.held[c] ? 1 : 0, w.random ? 1 : 0,
          w.floor, w.spreadScale, w.sniperScale, w.yawSpreadOverride === null ? 0 : 1, w.yawSpreadOverride ?? 0);
      }
    }
  }
  sync(ptr) {
    if (!ptr) return;
    const b = this.bridge; b.views();
    const f = b.f, base = ptr / 8;
    const value = syncFfOutput(b, f, base + 5, base + f[base + 2], new FfOutputMirror());
    this.phase.counters.outputFields += f[base + 1] + f[base + 3];
    for (const row of value.hulls) {
      const v = row.hull;
      if (v.aiTarget.generation !== row.life) continue;
      for (let i = 0; i < row.seats.length; i++) for (let j = 0; j < row.seats[i].weapons.length; j++) {
        const w = v.seats[i].weapons[j], state = row.seats[i].weapons[j];
        for (const key of mutable) w.state[key] = state.state[key];
        w.state.def = w.state.levels[state.state.level]; w.state.level = state.state.level;
        w.barrel = state.barrel;
        for (let k = 0; k < w.muzzles.length; k++) if (state.kicks[k] !== undefined) w.muzzles[k].kickAt = state.kicks[k];
        if (row.seats[i].consumed && v.seats[i].lease) {
          v.seats[i].lease.input.reload = false; v.seats[i].lease.input.pressed[j] = false;
        }
      }
    }
    for (const row of value.cables) {
      let t = this.vs.tethers.get(row.source);
      if (!t) {
        t = { source: row.source, line: null, start: new Vector3(), end: new Vector3() };
        this.vs.tethers.set(row.source, t);
      }
      Object.assign(t, { target: row.target, def: row.source.seats[row.seat].weapons[row.weapon].def, seat: row.seat, length: row.length });
      t.start.copy(row.start); t.end.copy(row.end);
      if (row.source.towTarget < 0 && t.line) t.line.visible = false;
    }
  }
  targetFacts(a) {
    this.phase.sync();
    const t = this.vs.units?.all()[a.index], viewer = a.viewer;
    if (!t) return { eligible: false, relation: 0, category: 0, center: zero, radius: 0, aim: zero, body: -1 };
    const eligible = t !== viewer.aiTarget && t.alive && !t.seat && (t.health ?? t.combat?.health ?? 1) > 0 && t.kind !== 10;
    // The same lazy read as lockSpan. No eager unit-shape invalidation.
    const sphere = eligible ? t.sphere : null;
    const center = sphere?.center?.clone() ?? t.position?.clone() ?? zero;
    if (eligible && t.aimPoint) t.aimPoint(this.aim); else this.aim.copy(center);
    return { eligible: Boolean(eligible), relation: this.vs.units.relation(viewer.team, t.team), category: categoryOf(t),
      center, radius: Math.max(0, sphere?.radius ?? 0), aim: this.aim, body: this.phase.bodySlot(t.vehicle?.body ?? t.turret?.body ?? t.building?.body) };
  }
  shot(a) {
    return { weapon: a.hull.seats[a.seat].weapons[a.weapon].state, yaw: a.yaw, pitch: a.pitch, charge: a.charge,
      targetId: a.targetId, targetGeneration: a.targetGeneration };
  }
  drain() {
    this.bridge.drain({
      f2Shot: a => {
        const shot = this.shot(a), w = this.vs.world;
        w.sound('turretFire', a.hull, shot);
        if (shot.weapon.def.MuzzleFlashEffect) w.effect(shot.weapon.def.MuzzleFlashEffect, a.origin.clone());
        w.flash(a.origin.clone());
      },
      f2Tow: a => {
        const def = a.source.seats[a.seat].weapons[a.weapon].def;
        this.vs.world.sound(a.reason === 'attached' ? 'towAttach' : 'towRelease', a.source, def, ...(a.reason === 'attached' ? [] : [a.reason]));
      },
    });
  }
}
