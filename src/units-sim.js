// A world's units on the simulation's module (rust/sim/src/units/module.rs),
// behind its switch: createGame's ?units=rust, or under Node BFE_UNITS=rust
// (world-hash --units rust). Off by default until units and combat move
// over together, with ?combat=rust.
//
// The step's loop over the units (game.js: each one's tick, a soldier's
// water, a droideka's deep water, the death regions) runs in Rust. The Unit
// objects stay the battle's: the phase writes every unit's rows into the
// store before it (the units' import fields: his life, intent, controls,
// body, hit shapes...), the module ticks them there, and the rows of the
// units it changed are synced back into the same objects after it, so
// everything else reads and writes them as before. A unit's combat is not
// in his rows: with combat on the module the store holds it and its facade
// (src/combat-sim.js) mirrors it; otherwise it stays the PlayerCombat's, and
// the phase is given what it reads of it (UnitsCombatView).
//
// What the Rust asks of the battle comes back as the module's imports
// (rust/sim/schema/units.mjs): the AI's aimer, a droideka's body (its clips
// and gun rig are the battle's JavaScript's for now), a network player's
// recon droid, a JavaScript combat, a unit taken off the field. Inside the
// phase the battle's JavaScript sees the units as the phase has left them:
// an import that reads the battle has the units the phase changed synced
// first (`reading`), and one that runs the battle's code (`hook`: a death,
// a round fired) has them all written back into the store after, as the
// code may have changed any of them (P7's hull phase does the same).
//
// A world only: a page draws soldiers from their records.
import * as THREE from "three";
import { bridgeFor } from "./sim/runtime.js";
import { KIND_BODY, KIND_UNIT, UNIT_EXPORT_STRIDE, putUnitsCombatViews, putUnitsPhase, syncCombat, syncUnit } from "./sim/gen/index.js";
import { giveMapRules } from "./vehicles-sim.js";
import { droidekaWaterDepth, waterDamageRule } from "./vehicle-water.js";
import { unitWatch } from "./units.js";

// A block's header words (rust/sim/src/store/block.rs).
const HEADER = 5;

// The units' phase on the battle's instance of the module, through its one
// bridge (bridgeFor). `game`: the battle's factory, combats on the module
// (src/combat-sim.js, or null), collision world (built on the ray kernel),
// water (its WaterState, or null), map ({ killY, deathRegions, scripted }),
// presentEffect, and aimReads() (whether the AI's aimer reads more of the
// battle than the unit it aims: the Enhanced aimer reads its target).
export function attachUnitsSim(instance, classes, game) {
  return new UnitsSim(bridgeFor(instance, { classes }), game);
}

export class UnitsSim {
  constructor(bridge, { factory, combatSim = null, collision, water = null, map, presentEffect, aimReads = () => false }) {
    if (!collision.rays) throw Error("the units' phase needs the collision's ray kernel");
    this.bridge = bridge;
    this.x = bridge.x;
    this.factory = factory;
    this.combatSim = combatSim;
    this.collision = collision;
    this.water = water;
    this.presentEffect = presentEffect;
    this.aimReads = aimReads;
    // Whether the phase runs (a hook of the unit it ticks goes its way).
    this.active = false;
    this.bodiesVersion = -1;
    this.named = false;
    this.args = { dt: 0, level: NaN, units: [] };
    this.infos = [];
    this.waterRules = new WeakMap(); // a droideka class -> its DroidekaWater, or null
    this.views = { list: [] };
    this.viewList = [];
    this.phaseRecord = { words: this.x.units_words(0), put: putUnitsPhase };
    this.viewsRecord = { words: this.x.units_words(1), put: putUnitsCombatViews };
    this.combatWords = this.x.units_words(2);
    this.aim = { aim: null };
    // The units the phase ticks (every built one on the field: one taken
    // off it does nothing in the loop, and is neither written nor synced).
    this.live = [];
    // A droideka's body ticking in JavaScript (unitBodyTick), and whether
    // its blow-apart ran the battle's code meanwhile.
    this.body = null;
    this.bodyHooked = false;
    this.wrapped = new WeakSet();
    this.one = [];
    this.x.units_setup(combatSim ? 1 : 0, collision.rayWorld);
    giveMapRules(bridge, water, map);
    if (combatSim) combatSim.unitsSim = this;
    bridge.register(this.handlers());
  }

  // ---- the phase ----

  // The step's loop over the units (game.js), on the module.
  phase(dt) {
    const built = this.factory.built, n = built.length, live = this.live;
    live.length = 0;
    for (let i = 0; i < n; i++) if (!built[i].removed) live.push(built[i]);
    this.active = true;
    try {
      this.writeAll();
      this.bodiesIn();
      const a = this.args, units = a.units;
      a.dt = dt;
      a.level = this.water ? this.water.level : NaN;
      units.length = live.length;
      for (let i = 0; i < live.length; i++) {
        const u = live[i], info = (this.infos[i] ??= { unit: 0, remote: false, edges: null, edge: null, droidekaWater: null });
        info.unit = u.id;
        info.remote = typeof u.remoteStep === "function";
        // A network player's presses, which a combat on the module reads
        // (a JavaScript combat reads them itself).
        info.edges = this.combatSim && Array.isArray(u.networkShots) ? u.networkShots : null;
        info.edge = this.combatSim ? u.networkShot ?? null : null;
        info.droidekaWater = null;
        if (u.walker) {
          info.droidekaWater = this.droidekaWater(u.soldier.def);
          this.watch(u.soldier);
        }
        units[i] = info;
      }
      this.bridge.callArgs(this.phaseRecord, a);
      this.x.units_phase();
      this.syncDirty();
      if (built.length !== n) throw Error("a unit was built inside the units' phase");
    } finally {
      this.active = false;
    }
  }

  // A droideka's blow-apart (onExploded: the battle's blast) while its body
  // ticks in JavaScript: the battle's code runs there, so the other units
  // as the phase has left them first, and every unit back into the store
  // after the tick (unitBodyTick).
  watch(body) {
    if (this.wrapped.has(body)) return;
    this.wrapped.add(body);
    const exploded = body.onExploded;
    body.onExploded = (...args) => {
      if (this.body === body && !this.bodyHooked) {
        this.syncDirty(1);
        this.bodyHooked = true;
      }
      return exploded.apply(body, args);
    };
  }

  // A droideka class's water rule (game.js droidekaWater: waterDamageRule
  // and droidekaWaterDepth), or null when its water deals nothing.
  droidekaWater(def) {
    let rule = this.waterRules.get(def);
    if (rule !== undefined) return rule;
    const r = waterDamageRule(def);
    rule = r ? { interval: r.interval, amount: r.amount, upright: droidekaWaterDepth(def, false), ball: droidekaWaterDepth(def, true) } : null;
    this.waterRules.set(def, rule);
    return rule;
  }

  // The units the phase ticks into the store (their import fields), every
  // other entity the store holds left as it is (writeSome), and with a
  // JavaScript combat what the phase reads of theirs.
  writeAll() {
    const live = this.live;
    this.bridge.writeSome(KIND_UNIT, live);
    if (!this.combatSim) {
      const list = this.viewList;
      list.length = live.length;
      for (let i = 0; i < live.length; i++) {
        const u = live[i], c = u.combat, v = (list[i] ??= { unit: 0, dead: false, pivot: null, origin: null });
        v.unit = u.id; v.dead = c.dead; v.pivot = c.pivot; v.origin = c.origin;
      }
      this.views.list = list;
      this.bridge.callArgs(this.viewsRecord, this.views);
      this.x.units_combat_views();
    }
  }

  // The rows of the units the phase changed into their objects (and, with
  // combat on the module, each one's combat into its facade's mirror; the
  // water step's gun under water onto his combat). `which` (units_sync): 0
  // every one changed since the last time, 1 all of them but the one being
  // ticked, 2 the one being ticked alone.
  syncDirty(which = 0) {
    const b = this.bridge, ptr = this.x.units_sync(which);
    b.views();
    const f = b.f, base = ptr / 8, rows = f[base], stride = f[base + 1], h = base + f[base + 2];
    const refs = b.refs[KIND_UNIT], combat = Boolean(this.combatSim);
    for (let i = 0; i < rows; i++) {
      const at = base + HEADER + i * stride, u = refs.objects[f[at]];
      syncUnit(b, f, at, h, u);
      // (Which of his arrays his hit shapes are: a rider's set has more
      // than two.)
      u.hitSet = u.shapeCount > 2 && u.riderHits ? u.riderHits : u.shapes;
      let o = at + UNIT_EXPORT_STRIDE;
      if (combat) {
        const c = u.combat;
        syncCombat(b, f, o, h, c.state);
        o += this.combatWords;
        c.kept.gunUnderwater = f[o] !== 0;
      } else u.combat.gunUnderwater = f[o] !== 0;
    }
  }

  // The battle's bodies as the soldiers' movement in the module meets
  // them, before the phase and after a hook: the kernel's order of the
  // bodies, each one's enabled flag by its place in it, and the numbers the
  // store knows each by (the body a soldier falls through).
  bodiesIn() {
    const c = this.collision, b = this.bridge;
    c.rayBodies();
    const bodies = c.bodies;
    if (!this.named) {
      // The module's copy of the collision (the hull phase's too, which
      // sets its vehicle barriers when it starts).
      this.x.veh_collision(c.rayWorld, -1);
      this.named = true;
    }
    if (this.bodiesVersion !== c.bodiesVersion) {
      const refs = b.refs[KIND_BODY];
      const ptr = this.x.veh_input(bodies.length * 2);
      b.views();
      const f = b.f, at = ptr / 8;
      for (let i = 0; i < bodies.length; i++) { f[at + 2 * i] = refs.slotOf(bodies[i]); f[at + 2 * i + 1] = bodies[i].raySlot; }
      this.x.veh_bodies(bodies.length);
      this.bodiesVersion = c.bodiesVersion;
    }
    const flags = this.x.movement_flags_ptr();
    b.views();
    const u8 = b.u8;
    for (let i = 0; i < bodies.length; i++) u8[flags + i] = bodies[i].enabled ? 1 : 0;
  }

  // An import that runs the battle's code inside the phase: the units as
  // the phase has left them first, and every unit back into the store
  // after (the code may have changed any of them, and a body's flag).
  hook(run) {
    this.syncDirty();
    try {
      return run();
    } finally {
      this.writeAll();
      this.bodiesIn();
    }
  }

  // An import that reads the battle inside the phase: the units as the
  // phase has left them first; the hit shapes it made of any unit (lazily,
  // as the JavaScript does on first asking in a tick) go back into the
  // store after. (The unit being ticked, if the module has him out
  // mid-combat, is left as he was last synced.)
  reading(run) {
    this.syncDirty();
    const touched = (unitWatch.touched = []);
    try {
      return run();
    } finally {
      unitWatch.touched = null;
      if (touched.length) {
        const out = this.x.units_taken();
        const some = out < 0 ? touched : touched.filter((u) => u.id !== out);
        if (some.length) this.bridge.writeSome(KIND_UNIT, some);
      }
    }
  }

  // Whether `id` is the unit the phase ticks now (a hook of his, not of a
  // command the battle's code started for someone else).
  ticking(id) {
    return this.active && this.x.units_current() === id;
  }

  // ---- what the phase asks of the battle (the module's imports) ----
  // The arguments object is the bridge's, reused for the next call of the
  // same import: each handler reads all of it before it runs code that can
  // come back into the module, and copies a vector it hands on.

  handlers() {
    const unit = (id) => this.bridge.refs[KIND_UNIT].objects[id];
    const view = { unit: 0, dead: false, pivot: null, origin: null };
    const viewOf = (u) => {
      const c = u.combat;
      view.unit = u.id; view.dead = c.dead; view.pivot = c.pivot; view.origin = c.origin;
      return view;
    };
    return {
      unitAim: (a) => {
        const u = unit(a.unit), dt = a.dt;
        if (this.aimReads()) this.reading(() => u.onMoved(dt));
        else {
          // The Classic aimer reads of the battle only his body's aim
          // after it moved (unit.forward) and what JavaScript keeps.
          const s = u.soldier;
          s.aimYaw = a.aimYaw;
          s.aimPitch = a.aimPitch;
          u.onMoved(dt);
        }
        this.aim.aim = u.intent.aim;
        return this.aim;
      },
      unitBodyTick: (a) => {
        // A droideka's body reads and moves only itself (its clips, the
        // collision world), unless it blows apart (watch): his rows out
        // and back in, or every unit's after a blast.
        const u = unit(a.unit), dt = a.dt, one = this.one;
        this.syncDirty(2);
        this.body = u.soldier;
        this.bodyHooked = false;
        try {
          u.soldier.tick(u.input, dt);
        } finally {
          this.body = null;
          if (this.bodyHooked) { this.writeAll(); this.bodiesIn(); } else { one[0] = u; this.bridge.writeSome(KIND_UNIT, one); }
        }
      },
      unitSampleMuzzles: (a) => {
        // Its hidden gun rig posed from its body: his rows alone.
        const u = unit(a.unit), dt = a.dt, one = this.one;
        this.syncDirty(2);
        u.soldier.sampleMuzzles(dt);
        one[0] = u;
        this.bridge.writeSome(KIND_UNIT, one);
      },
      unitRemoteStep: (a) => {
        const u = unit(a.unit), dt = a.dt, active = a.active, r = u.combat.remotes;
        // With no droid of his out, nothing flying and no strike called,
        // the step only counts the tick (src/runtime/network-remote-world.js).
        if (r && !r.droids.length && !r.strikes.length && !r.piloted) u.remoteStep(dt, active ? u.intent : null, u.controls);
        else this.hook(() => u.remoteStep(dt, active ? u.intent : null, u.controls));
      },
      unitCombat: (a) => {
        const u = unit(a.unit), dt = a.dt, stick = a.stick;
        this.hook(() => {
          // (Unit.tick sets it after the aimer, before anything reads it.)
          u.combat.aimOverride = u.alive ? u.intent.aim : null;
          u.combat.update(dt, u.controls, stick);
          u.combat.flushMuzzles();
        });
        return viewOf(u);
      },
      unitDamage: (a) => {
        const u = unit(a.unit), amount = a.amount;
        this.hook(() => u.combat.damage(amount));
        return viewOf(u);
      },
      unitRemoved: (a) => {
        // Unit.remove after its life's flags: his Character lets go of
        // him, nothing of him is drawn, the factory takes his body back.
        const u = unit(a.unit);
        if (u.character?.unit === u) u.character.unit = null;
        u.character = null;
        u.undraw();
        u.onRemoved(u);
      },
      unitSplash: (a) => {
        this.presentEffect("watersplash_md", new THREE.Vector3(a.at.x, a.at.y, a.at.z));
      },
    };
  }
}
