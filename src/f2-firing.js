import {BlockHeaderLayout as BLOCK} from './sim/gen/boundary.js';
import {NO_RANDOM_STREAM} from './sim/gen/boundary.js';
// The mounted weapon boundary: definitions and emitter identities at birth,
// frozen foreign input at latch, publication and ordered presentation at end.
import { EVENT_TABLE } from './sim/gen/index.js';
import { Vector3 } from 'three';
import { FfPhaseLayout } from './sim/gen/f2-firing.js';
import { Weapon } from './weapons.js';
import { rigNodes, copyMountedWeapon } from './vehicle-phase-data.js';
import { registerWeaponClass } from './weapon-content-sim.js';
import { seatLockTarget } from './weapon-targeting.js';
import { categoryOf, weaponAI } from './weapon-policy.js';
import { putFfBirth, putFfSupply, putFfWeaponInput, putFfWeaponChange, FfInputsLayout, FfWeaponInputLayout, FfWeaponChangeLayout, FfHullOutMirror, FfSeatOutMirror, FfWeaponOutMirror, syncFfHullOut, FfCableOutMirror, syncFfCableOut, FfSupplyResultMirror, syncFfSupplyResult, FfPhaseMirror, syncFfPhase } from './sim/gen/f2-firing.js';

const zero = new Vector3();
const num = x => Number.isFinite(parseFloat(x)) ? parseFloat(x) : 0;
// Vehicle.fireSeat now runs entirely in Rust. This facade supplies immutable
// weapon/rig bindings, changed AI modifiers and the existing random stream.
// It never executes Weapon.update or routes a round back through JS fire.
export class F2Firing {
  // Definitions and mirror shapes are registered before the phase starts.
  // Keep one decoder per event type, since presentation drains repeatedly over
  // the same reserved event buffer in a warm battle.
  constructor(phase) {
    this.phase = phase; this.bridge = phase.bridge; this.vs = phase.vehicles;
    this.categories = new WeakMap();
    this.aim = new Vector3(); this.number = { value: 0 };
    this.facts = { alive: false, eligible: false, relation: 0, category: 0, center: new Vector3(), radius: 0, aim: this.aim, body: -1 };
    this.shotRow = {weapon:null,yaw:0,pitch:0,charge:0,targetId:0,targetGeneration:0};
    this.phaseFields = new FfPhaseMirror();
    this.eventRows = [];
    for (const e of EVENT_TABLE) if (e && (e.name === 'f2Shot' || e.name === 'f2Tow')) this.eventRows[e.code ?? EVENT_TABLE.indexOf(e)] = new e.record.Mirror();
    this.drainedWords = 0;
    this.rows = []; this.cableRow = new FfCableOutMirror();
    this.inputWords = FfWeaponInputLayout.words;
    this.change = { hull: null, life:0, seat: 0, weapon: 0, input: null };
    this.lives = new WeakMap();
    this.inputScratch = new Float64Array(this.inputWords); this.inputCache = new WeakMap();
    this.frozen = { channel: 0, target: -2, hasRandom: false, randomBinding:NO_RANDOM_STREAM, floor: 0, spreadScale: 1, sniperScale: 0, yawSpreadOverride: null };
    this.supplyArgs = { words: phase.x.f2_firing_words(3), put: putFfSupply };
    this.supplyResult = new FfSupplyResultMirror();
    this.supplyResultType = { words: phase.x.f2_firing_words(4), Mirror: FfSupplyResultMirror, sync: syncFfSupplyResult };
    this.supplyRow = { hull: null, life: 0, amount: 0 };
    this.birthArgs = { words: phase.x.f2_firing_words(0), put: putFfBirth };
    this.bridge.register({
      f2Posed: () => { phase.commitForeign(); phase.sync(); },
      f2WeaponDraw: a => this.draw(a),
      f2TargetFacts: a => this.targetFacts(a),
      // AI and other state-changing listeners retain their synchronous slot.
      // VehiclesWorld.fired can change AI or trigger damage immediately. Keep
      // that listener synchronous with the shot; sound, flash and cable visuals
      // use the ordered presentation journal instead.
      f2Fired: notice => {
        // Vehicle.emitShot journals a tow transition and its visuals before
        // the synchronous fired listener journals the shot. Drain this prefix
        // now; the phase-end drain resumes after it, including nested shots.
        const a = notice.shot;
        const w = a.hull.seats[a.seat].weapons[a.weapon].state;
        copyMountedWeapon(w, a.state);
        w.def = w.levels[a.state.level]; w.level = a.state.level;
        this.drain(notice.events);
        this.vs.world.fired(a.hull, a.seat, this.shot(a), a.owner, a.origin.clone(), a.direction.clone());
      },
    });
  }
  // PowerupStation.serve changes canonical ammo, including a shot in the same
  // tick. Only the supplied hull's weapon rows are published. The scalar
  // command rejects both a retired handle and an earlier gameplay life.
  supply(v, amount) {
    const a = this.supplyRow; a.hull = v; a.life = v.aiTarget.generation; a.amount = amount;
    const previous = this.bridge.x; this.bridge.x = this.phase.x;
    try { this.bridge.callArgs(this.supplyArgs, a); } finally { this.bridge.x = previous; }

    const ptr = this.phase.x.f2_firing_supply();
    const result = this.bridge.callResult(this.supplyResultType, ptr, this.supplyResult);
    this.sync(result.output, 0); return result.changed;
  }
  // Weapon draws still use the world's existing stream. Retaining this
  // per-shot seam preserves the draw order across hulls and turrets while
  // avoiding an import for idle weapons or ordinary weapon clocks.
  draw(a) {
    const random = a.hull.seats[a.seat].weapons[a.weapon].state.random;
    this.number.value = random(); return this.number;
  }
  // Vehicle construction and respawn resolve nodes once, in rigNodes order.
  // Store actual emitter nodes and aimer indices, including the fallback beam
  // emitter whose frame may differ from the current muzzle.
  adopt(v) {
    const b = this.bridge, nodes = rigNodes(v), node = n => n ? nodes.indexOf(n) : -1;
    const named = name => v.byName[(name || '').toLowerCase()];
    const seats = v.seats.map(s => ({ weapons: s.weapons.map((w, index) => {
      registerWeaponClass(b, w.state.levels[0]);
      if (w.def.ordnance) this.vs.ordnance.sim.defId(w.def.ordnance);
      const emitter = (w.muzzles.length ? w.emitter : w.aimers[0]?.emitter ?? w.emitter) ?? s.emitter;
      const state = { ...w.state, level: w.state.levels.indexOf(w.state.def), admits: false, hasRandom: Boolean(w.state.random), melee: null };
      return { state, ordnance: w.def.ordnance ?? null, tow: w.def.ordnance?.base === 'towcable', range: Math.max(20, w.def.LockOnRange || 0),
        cableLength: Math.max(10, w.def.ordnance?.CableLength || 60), channel: w.state.channel ?? index, relationMask: weaponAI(w.def).relationMask, categoryMask: weaponAI(w.def).categoryMask,
        weaponNode: node(named(w.def.FirePointName)), fallbackNode: node(named(w.aimers[0]?.FirePointName || w.aimers[0]?.node || w.def.FirePointName)),
        directionNodes: w.aimers.map(a => node(named(a.FirePointName))), outside: w.aimers.map(a => num(a.FireOutsideLimits ?? s.settings?.FireOutsideLimits)),
        emitter, emitterNode: node(emitter?.node), emitterAimer: w.aimers.indexOf(emitter?.aimer),
        muzzles: w.muzzles.map(m => { const e = m.emitter ?? w.emitter ?? s.emitter; return { emitter: e, node: node(e?.node), aimer: w.aimers.indexOf(e?.aimer) }; }) };
    }) }));
    b.callArgs(this.birthArgs, { hull: v, life: v.aiTarget.generation, commandWalker: v.base === 'commandwalker', legTop: v.legTop, legs: v.legs, seats });
    this.phase.x.f2_firing_adopt();
    const row = new FfHullOutMirror();
    for (const seat of v.seats) {
      const s = new FfSeatOutMirror();
      for (const weapon of seat.weapons) {
        const w = new FfWeaponOutMirror(); w.kicks.length = weapon.muzzles.length; s.weapons.push(w);
      }
      row.seats.push(s);
    }
    this.rows[v.simSlot] = row;
    this.lives.set(v,v.aiTarget.generation);
    for (const seat of v.seats) for (const weapon of seat.weapons) { const cache = new Float64Array(this.inputWords); cache.fill(NaN); this.inputCache.set(weapon, cache); }
  }
  // AI settings remain foreign scalar writes. Resolve a changed callback
  // once, then send its numeric stream id beside the changed modifiers.
  modifiers(weapon, out) {
    out.channel = weapon.channel ?? 0; out.hasRandom = Boolean(weapon.random);
    out.randomBinding = NO_RANDOM_STREAM;
    if (weapon.f3RandomDraw !== weapon.random) {
      weapon.f3RandomDraw = weapon.random;
      weapon.f3RandomId = this.random.register(weapon.random);
    }
    out.randomBinding = weapon.f3RandomId ?? NO_RANDOM_STREAM;
    out.floor = weapon.floor; out.spreadScale = weapon.spreadScale;
    out.sniperScale = weapon.sniperScale; out.yawSpreadOverride = weapon.yawSpreadOverride;
  }
  // Native SeatUnits tune the canonical weapon and freeze their target in
  // F3. Importing their mirrors would send owned facts back to the owner.
  // Other seats retain explicit foreign modifiers and lock hints.
  // The cache is committed only after native latch reads the pending batch.
  input(at) {
    const start = at, out = this.frozen, b = this.bridge;
    at += FfInputsLayout.words; let count = 0;
    for (let vi = 0; vi < this.vs.vehicles.length; vi++) {
      const v = this.vs.vehicles[vi];
      for (let si = 0; si < v.seats.length; si++) {
        const seat = v.seats[si], l = seat.lease;
        if (l?.controllable?.native) continue;
        const picked = seatLockTarget(l);
        out.target = picked === undefined ? -2 : picked === null ? -1 : this.vs.units.all().indexOf(picked);
        for (let c = 0; c < seat.weapons.length; c++) {
          const w = seat.weapons[c].state;
          this.modifiers(w,out);
          putFfWeaponInput(b, out, this.inputScratch, 0, null);
          const cache = this.inputCache.get(seat.weapons[c]); let changed = false;
          for (let j = 0; j < this.inputWords; j++) {
            const old = cache[j], next = this.inputScratch[j];
            if (old === next ? old === 0 && 1 / old !== 1 / next : old === old || next === next) changed = true;
          }
          if (changed) {
            const a = this.change; a.hull = v; a.life = v.aiTarget.generation; a.seat = si; a.weapon = c; a.input = out;
            putFfWeaponChange(b, a, b.f, at, null); at += FfWeaponChangeLayout.words; count++;
          }
        }
      }
    }
    b.f[start + FfInputsLayout.count] = count;
    return at - start;
  }
  // A lifecycle callback may have rebuilt the batch before latch. Accept only
  // those submitted values; post-fire mirrors may already contain later weapon
  // state that must be compared and sent on the following tick.
  commitInput(start) {
    const f = this.bridge.f, n = f[start + FfInputsLayout.count] | 0;
    for (let i = 0; i < n; i++) {
      const at = start + FfInputsLayout.words + i * FfWeaponChangeLayout.words;
      const v = this.phase.pool.objects[f[at + FfWeaponChangeLayout.hull]], seat = f[at + FfWeaponChangeLayout.seat], weapon = f[at + FfWeaponChangeLayout.weapon];
      const cache = this.inputCache.get(v.seats[seat].weapons[weapon]);
      for (let j = 0; j < cache.length; j++) cache[j] = f[at + FfWeaponChangeLayout.input + j];
    }
  }
  syncDescriptor(ptr) {
    if (!ptr) return;
    const b = this.bridge; b.views();
    const p = syncFfPhase(b, b.f, (ptr / 8) | 0, 0, this.phaseFields);
    if (this.phase.counters) this.phase.counters.outputFields += FfPhaseLayout.words;
    this.sync(p.weapons, p.cables);
  }
  // fireCrews publishes changed mounted state, preserving the JS Weapon
  // objects held by AI and capture. Barrel and kick metadata belong to the same
  // native weapon life, rather than a new wrapper made for each shot.
  sync(ptr, cables) {
    const b = this.bridge; b.views();
    const f = b.f;
    if (this.phase.counters) {
      this.phase.counters.outputBlock(f,(ptr / 8) | 0);
      this.phase.counters.outputBlock(f,(cables / 8) | 0);
    }
    if (ptr) {
      const base = (ptr / 8) | 0, n = f[base] | 0, stride = f[base + BLOCK.stride] | 0, heap = base + (f[base + BLOCK.heap] | 0);

      for (let ri = 0; ri < n; ri++) {
        const at = base + BLOCK.words + ri * stride;
        const row = syncFfHullOut(b, f, at, heap, this.rows[f[at]]), v = row.hull;
        if (v.aiTarget.generation !== row.life) continue;
        if (this.lives.get(v) !== row.life) {
          // Vehicle.spawn replaces each JS Weapon once per hull life. This
          // facade construction follows the native reset; it never imports a
          // new weapon row, rig or modifier batch back into the owned phase.
          this.lives.set(v,row.life);
          for (let i = 0; i < v.seats.length; i++) {
            const weapons = v.seats[i].weapons;
            for (let j = 0; j < weapons.length; j++) weapons[j].state = new Weapon(weapons[j].def);
          }
        }
        for (let i = 0; i < row.seats.length; i++) for (let j = 0; j < row.seats[i].weapons.length; j++) {
          const w = v.seats[i].weapons[j], state = row.seats[i].weapons[j];
          copyMountedWeapon(w.state, state.state);
          // The constructor's first publication precedes stream binding.
          // Its untouched weapons already hold their construction callbacks.
          if (this.random) {
            w.state.random = this.random.callback(state.randomBinding);
            w.state.f3RandomDraw = w.state.random; w.state.f3RandomId = state.randomBinding;
          }
          w.state.def = w.state.levels[state.state.level]; w.state.level = state.state.level;
          w.barrel = state.barrel;
          for (let k = 0; k < w.muzzles.length; k++) if (state.kicks[k] !== undefined) w.muzzles[k].kickAt = state.kicks[k];

        }
      }
    }
    // updateTethers owns cable endpoints. JS retains line objects for visual
    // presentation only; a release event hides the existing line rather than
    // leaving an old cable attached to a respawned hull.
    if (cables) {
      const base = (cables / 8) | 0, n = f[base] | 0, stride = f[base + BLOCK.stride] | 0, heap = base + (f[base + BLOCK.heap] | 0);

      for (let i = 0; i < n; i++) {
        const row = syncFfCableOut(b, f, base + BLOCK.words + i * stride, heap, this.cableRow);
        let t = this.vs.tethers.get(row.source);
        if (!t) {
          t = { source: row.source, line: null, start: new Vector3(), end: new Vector3() };
          this.vs.tethers.set(row.source, t);
        }
        t.target = row.target; t.def = row.source.seats[row.seat].weapons[row.weapon].def; t.seat = row.seat; t.length = row.length;
        t.start.copy(row.start); t.end.copy(row.end);
        if (row.source.towTarget < 0 && t.line) t.line.visible = false;
      }
    }
  }
  // weapon-targeting.js requests lazy bounds only after eligibility passes.
  // Do not eagerly ask every unit's sphere while rebuilding input, because
  // that would change when exposed rider hit shapes become current.
  targetFacts(a) {
    const t = this.vs.units?.all()[a.index], viewer = a.viewer;
    const out = this.facts;
    if (!t) { out.alive = out.eligible = false; return out; }
    const eligible = t !== viewer.aiTarget && t.alive && !t.seat && (t.health ?? t.combat?.health ?? 1) > 0 && t.kind !== 10;
    // lockSpan asks lazy unit bounds only after the candidate passes filters.
    const sphere = eligible && a.shapes ? t.sphere : null;
    out.center.copy(sphere?.center ?? t.position ?? zero);
    if (a.aim && t.aimPoint) t.aimPoint(this.aim); else this.aim.copy(out.center);
    out.alive = Boolean(t.alive); out.eligible = Boolean(eligible);
    out.relation = this.vs.units.relation(viewer.aiTarget.team, t.team);
    const def = t.classDef;
    if (Number.isInteger(t.category)) out.category = t.category;
    else if (def) {
      let category = this.categories.get(def);
      if (category === undefined) { category = categoryOf(t); this.categories.set(def, category); }
      out.category = category;
    } else out.category = categoryOf(t);
    out.radius = Math.max(0, sphere?.radius ?? 0); out.body = this.phase.bodySlot(t.vehicle?.body ?? t.turret?.body ?? t.building?.body);
    return out;
  }

  shot(a) {
    const shot = this.shotRow;
    shot.weapon = a.hull.seats[a.seat].weapons[a.weapon].state;
    shot.yaw = a.yaw; shot.pitch = a.pitch; shot.charge = a.charge;
    shot.targetId = a.targetId; shot.targetGeneration = a.targetGeneration;
    return shot;
  }
  // The event buffer is ordered across shots and cable transitions. Reuse
  // decoders, while effects that retain a position receive their own event
  // value. These are shot/lifecycle allocations, not idle phase scratch.
  drain(ptr = this.phaseFields.events) {
    const b = this.bridge; b.views();
    const f = b.f, base = (ptr / 8) | 0, end = base + 1 + f[base];
    for (let at = base + 1 + this.drainedWords; at < end;) {
      // Advance before a listener can reenter and drain the same journal.
      const next = at + 2 + f[at + 1]; this.drainedWords = next - base - 1;
      const e = EVENT_TABLE[f[at]], r = e.record;
      // Unit-owned combat shares this journal with fleet presentation. Its
      // outer drain owns those other records and their later notification slot.
      if (e.name !== 'f2Shot' && e.name !== 'f2Tow') {
        at = base + 1 + this.drainedWords;
        continue;
      }
      const a = r.sync(b, f, at + 2, at + 2 + r.words, this.eventRows[f[at]]);

      if (e.name === 'f2Shot') this.presentShot(a);
      else if (e.name === 'f2Tow') this.presentTow(a);
      at = base + 1 + this.drainedWords;
    }
  }
  // Vehicle.emitShot emits sound and flash even when no projectile survives
  // a synchronous launch listener. Preserve that slot and use the copied event
  // origin, which cannot move with the hull before presentation reads it.
  presentShot(a) {
    const shot = this.shot(a), w = this.vs.world;
    w.sound('turretFire', a.hull, shot);
    if (shot.weapon.def.MuzzleFlashEffect) w.effect(shot.weapon.def.MuzzleFlashEffect, a.origin.clone());
    w.flash(a.origin.clone());
  }
  presentTow(a) {
    const def = a.source.seats[a.seat].weapons[a.weapon].def;
    if (a.reason === 'attached') this.vs.world.sound('towAttach', a.source, def);
    else this.vs.world.sound('towRelease', a.source, def, a.reason);
  }
}
