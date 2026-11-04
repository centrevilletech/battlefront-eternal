// A world's combats on the simulation's module (rust/sim/src/combat/
// module.rs), behind its switch: createGame's ?combat=rust, or under Node
// BFE_COMBAT=rust (world-hash --combat rust). Off by default until units
// and combat move over together.
//
// Each unit the factory builds gets a CombatFacade instead of a
// src/combat.js PlayerCombat: the same fields, getters and methods the rest
// of the battle uses, over a mirror of the store's combat record (synced
// after every command), whose methods (update, flushMuzzles, damage,
// explode, reset, the arsenal's select and next) and setters (health,
// zoomed, a weapon's reserve and kick...) are the module's exports. What
// the battle's JavaScript sets on a combat (whether he mans a gun, his
// team, his aimer's direction, whether his gun is under water, the
// generators, the hooks) stays on the facade and goes in with each
// command, as do his intent, controls, body and trigger edges while units
// are the JavaScript's; what combat writes into the unit (the presses it
// took, his aim's kick, a blast's push, a droideka's shield) comes back in
// each answer. What combat asks of the battle comes back here as the
// module's imports (rust/sim/schema/combat.mjs), served with the same
// CombatWorld (src/combat-world.js) PlayerCombat calls; an import that runs
// the battle's code first syncs the facade and the unit from the store and
// gives the unit's view back after (CombatFacade.current, feed), so the
// hook finds both as the JavaScript would leave them, and the Rust after it
// sees what the hook did.
//
// A world only: a page draws soldiers from their records, and its own
// player keeps his PlayerCombat.
import * as THREE from "three";
import { valueWords, weaponClasses, registerWeaponClass } from "./weapon-content-sim.js";
import { bridgeFor } from "./sim/runtime.js";
import { CbOutMirror, CombatMirror, KIND_UNIT, putCbDamageIn, putCbExplodeIn, putCbIn, syncCbOut, syncCombat } from "./sim/gen/index.js";
import { BLAST_BODY, PlayerCombat } from "./combat.js";
import { CombatWorld } from "./combat-world.js";
import { blastDistance, blastShare, falloff, HEALTH_SCALES } from "./ordnance.js";
import { emitSound } from "./audio/events.js";
import { legacyRandom } from "./ai/random.js";

// A block's header words (rust/sim/src/store/block.rs).
const HEADER = 5;
// The legacy streams a combat or weapon with no generator draws from
// (Math.random's, as src/combat.js and src/weapons.js name them).
const legacySpread = legacyRandom("weapon-spread");
const legacyChunks = legacyRandom("blow-apart");
const PRESENT = ["weaponAction", "endWeaponAction", "lightning"];

// What tells apart the objects a combat's unions hold that are no class's
// instances: what src/ordnance-owner.js froze of a unit whose life ended
// (src/ordnance-sim.js tells it so too).
export const combatTests = {
  ownerSnapshot: (v) => Object.isFrozen(v) && Object.getPrototypeOf(v) === Object.prototype && Number.isSafeInteger(v.id) && Number.isSafeInteger(v.generation) && "className" in v,
};

// The battle's combats on its instance of the module, through its one
// bridge (bridgeFor: made here, or joined if the rounds made it).
// `classes`: Unit, Vehicle, Soldier, Droideka, Building, Turret.
export function attachCombatSim(instance, classes) {
  return new CombatSim(bridgeFor(instance, { classes, tests: combatTests }));
}

export class CombatSim {
  constructor(bridge) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.facades = [];
    this.weaponsSent = weaponClasses(bridge); // a weapon class -> its index
    this.classesSent = new Set();
    this.explosions = new Map(); // an explosion -> its index
    this.out = new CbOutMirror();
    const words = (i) => this.x.cmb_words(i);
    this.inRecord = { words: words(0), put: putCbIn };
    this.outWords = words(1);
    this.damageRecord = { words: words(3), put: putCbDamageIn };
    this.explodeRecord = { words: words(4), put: putCbExplodeIn };
    this.damage$ = { in: null, amount: 0, onShield: 0, source: null, attacker: null, explosion: false };
    // The units' phase on the module, when it runs too (src/units-sim.js).
    this.unitsSim = null;
    this.explode$ = { in: null, explosion: 0, position: null, owner: null, scale: 1 };
    bridge.register(this.handlers());
  }

  // A facade for a unit the factory builds (PlayerCombat's options).
  make(options) {
    return new CombatFacade(this, options);
  }

  // The content a combat reads, given to the module the first time.
  send(words, call) {
    const b = this.bridge, ptr = this.x.sim_call_room(words.length, 0);
    b.views();
    b.f.set(words, ptr / 8 + HEADER);
    call();
  }

  weaponClass(def) {
    return registerWeaponClass(this.bridge, def);
  }

  unitClass(def) {
    const id = this.bridge.defs.unitClass.id(def);
    if (this.classesSent.has(def)) return id;
    for (const weapon of def.weapons || []) this.weaponClass(weapon);
    this.classesSent.add(def);
    const view = { AddHealth: def.AddHealth, base: def.base, weapons: def.weapons || [] };
    this.send(valueWords(view, this.bridge.strings, [], this.weaponsSent), () => this.x.cmb_class(id));
    return id;
  }

  explosion(def) {
    let id = this.explosions.get(def);
    if (id !== undefined) return id;
    id = this.explosions.size;
    this.explosions.set(def, id);
    this.send(valueWords(def, this.bridge.strings, []), () => this.x.cmb_explosion(id));
    return id;
  }

  // ---- what combat asks of the battle (the module's imports) ----
  // The arguments object is the bridge's (perhaps reused for the next call
  // of the same import): each handler reads all of it before it runs code
  // that can come back into the module, and copies a vector it hands on.

  handlers() {
    const at = (a) => this.facades[a.unit];
    const v = (p) => new THREE.Vector3(p.x, p.y, p.z);
    const flag = { value: false }, number = { value: 0 }, text = { value: "" };
    const hit = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    const droid = { found: false, droid: 0 };
    const admit = { kind: 0, token: 0 };
    const target = { target: null };
    const pick = { pick: 0, id: 0, generation: 0, alive: false };
    const gun = { found: false, point: new THREE.Vector3(), gun: 0, hasLast: false, lastIndex: 0, lastBarrel: 0 };
    const point = { found: false, point: new THREE.Vector3() };
    const arcs = { targets: [] };
    // An import that runs the battle's code: the unit's view out of the
    // store and back in after it (current, feed); for the unit the units'
    // phase is ticking (src/units-sim.js), the phase's way: every unit it
    // changed synced first and every unit written back after.
    const changing = (c, run) => {
      const units = this.unitsSim;
      if (units?.ticking(c.owner.id)) return units.hook(run);
      c.current();
      try { run(); } finally { c.feed(); }
    };
    // An import that reads the battle: inside the units' phase, the units
    // it changed synced first.
    const reading = (run) => {
      const units = this.unitsSim;
      return units?.active ? units.reading(run) : run();
    };
    return {
      combatCast: (a) => {
        const c = at(a), origin = v(a.origin), direction = v(a.direction), far = a.far, staticOnly = a.staticOnly;
        const h = c.world.cast(origin, direction, far, c.hit, null, staticOnly);
        hit.found = Boolean(h);
        if (h) { hit.distance = h.distance; hit.point.copy(h.point); hit.normal.copy(h.normal); }
        return hit;
      },
      combatDraw: (a) => {
        const c = at(a), weapon = a.weapon, legacy = a.legacy;
        if (weapon < 0) number.value = legacy ? legacyChunks() : c.random();
        else number.value = legacy ? legacySpread() : c.arsenal.weapons[weapon].random();
        return number;
      },
      combatAdmit: (a) => {
        const c = at(a), r = c.admitRemoteLaunch(c.arsenal.weapons[a.weapon]);
        admit.kind = r === true ? 0 : r === false ? 1 : 2;
        admit.token = typeof r === "number" ? r : 0;
        return admit;
      },
      combatRemoteOf: (a) => {
        const c = at(a), d = c.world.remoteOf(c.arsenal.weapons[a.weapon]);
        droid.found = Boolean(d);
        droid.droid = d ? c.droidNumber(d) : 0;
        return droid;
      },
      combatRemoteEnter: (a) => { const c = at(a), d = c.droids[a.droid]; changing(c, () => c.world.remoteEnter(d)); },
      combatRemoteKill: (a) => { const c = at(a), d = c.droids[a.droid]; changing(c, () => c.world.remoteKill(d)); },
      combatRemoteLaunch: (a) => {
        const c = at(a), weapon = a.weapon, velocity = c.fireDir.set(a.velocity.x, a.velocity.y, a.velocity.z), yaw = a.yaw, team = a.team, admission = a.admission;
        changing(c, () => {
          c.world.remoteLaunch(c.arsenal.weapons[weapon], c.origin, velocity, yaw, team, admission);
          if (c.firstPerson) c.view?.play("throw", c.arsenal.weapons[weapon]);
        });
      },
      combatLockOn: (a) => {
        const c = at(a), w = c.arsenal.weapons[a.weapon];
        target.target = c.world.lockOn(v(a.eye), v(a.dir), w.levels[a.level], a.current, a.held, a.team);
        return target;
      },
      combatLockKey: (a) => { text.value = at(a).world.lockKey(a.target); return text; },
      combatRepair: (a) => {
        const c = at(a), w = c.arsenal.weapons[a.weapon], t = a.target, team = a.team;
        changing(c, () => { flag.value = Boolean(c.world.repair(w.def, t, team)); });
        return flag;
      },
      combatLockPick: (a) => {
        const c = at(a), w = c.arsenal.weapons[a.weapon], m = w.w;
        // The launcher as it is now (its mirror has it after the command).
        m.level = a.level; m.lockComplete = a.lockComplete; m.lockTargetId = a.lockTargetId; m.lockGeneration = a.lockGeneration;
        const origin = v(a.origin), dir = v(a.dir);
        const picked = reading(() => c.world.lockPick(w, origin, dir));
        pick.pick = picked === undefined ? 0 : picked === null ? 1 : 2;
        pick.id = picked ? picked.unit.id : 0;
        pick.generation = picked ? picked.unit.generation ?? 0 : 0;
        pick.alive = picked ? Boolean(picked.unit.alive) : false;
        return pick;
      },
      combatFireRound: (a) => {
        const c = at(a.shot), shot = c.shot(a.shot), fromOrigin = a.fromOrigin, bind = a.bind;
        const from = fromOrigin ? null : v(a.from), dir = c.fireDir.set(a.dir.x, a.dir.y, a.dir.z);
        const selected = a.selected ? { index: a.selectedIndex, barrel: a.selectedBarrel } : null;
        changing(c, () => c.world.fireRound(shot.weapon.def.ordnance, fromOrigin ? c.origin : from, dir, shot, bind, selected));
      },
      combatDispense: (a) => {
        const c = at(a), w = c.arsenal.weapons[a.weapon], most = a.most, velocity = c.fireDir.set(a.velocity.x, a.velocity.y, a.velocity.z), yaw = a.yaw, team = a.team;
        changing(c, () => {
          const d = w.def;
          flag.value = Boolean(c.world.dispense(d.class, most, d.item, c.origin, velocity, yaw, c.owner, team));
          if (c.firstPerson) c.view?.play("throw", w);
        });
        return flag;
      },
      combatMeleeStrike: (a) => { const c = at(a); changing(c, () => c.world.meleeStrike()); },
      combatArcTargets: (a) => {
        const c = at(a), list = reading(() => c.world.arcTargets());
        arcs.targets = list.map((t) => ({ origin: t.origin, center: t.center ?? null, radius: t.radius ?? null }));
        return arcs;
      },
      combatHurtArc: (a) => { const c = at(a), index = a.index, amount = a.amount, from = v(a.from); changing(c, () => c.world.hurtArcTarget(index, amount, from)); },
      combatLightning: (a) => { flag.value = Boolean(this.lightningBolt(a.value)); return flag; },
      combatFired: (a) => {
        const c = at(a), shot = c.shot(a);
        changing(c, () => {
          emitSound(c.player ? "fire" : "actorFire", c, shot);
          c.world.fired(shot);
        });
      },
      combatDamaged: (a) => {
        const c = at(a), amount = a.amount, hurt = c.hurts[c.hurts.length - 1] ?? { source: null, attacker: a.attacker, info: null, blast: false };
        // A blast's hurt carries combat's own info; any other, its caller's.
        const info = a.explosion && hurt.blast ? { kind: "explosion", amount: a.infoAmount, typed: a.typed, generic: a.generic, known: false, point: null } : hurt.info;
        changing(c, () => {
          if (c.player) emitSound(c.health > 0 ? "hurt" : "death", c, amount);
          else emitSound(c.health > 0 ? "actorHurt" : "actorDeath", c, amount);
          c.world.damaged(amount, hurt.attacker, { ...info, source: hurt.source });
        });
      },
      combatDied: (a) => {
        const c = at(a);
        changing(c, () => {
          if (!c.blowApart) c.animator.die?.();
          c.updateHeld();
          c.world.died();
        });
      },
      combatBlownApart: (a) => { const c = at(a), health = a.health; changing(c, () => c.world.blownApart(health)); },
      combatRespawn: (a) => { const c = at(a); changing(c, () => c.world.respawn()); },
      combatPresent: (a) => {
        const c = at(a), kind = PRESENT[a.kind];
        if (kind === "weaponAction") {
          const action = a.action, bank = a.bank, posture = a.posture, prepare = a.prepare;
          c.animator.weaponAction?.(action, bank, posture);
          c.world.presentation({ kind, action, bank, posture, prepare });
        } else if (kind === "endWeaponAction") {
          const action = a.action ?? undefined;
          c.animator.endWeaponAction?.(action);
          c.world.presentation({ kind, action: action ?? null });
        } else {
          const name = a.name, from = v(a.from), to = v(a.to), charge = a.charge, channel = a.channel;
          c.lightning?.spawn(this.lightningBolt(name), from, to, charge, channel === null ? null : (out) => c.drawnGunOrigin(out));
          c.world.presentation({ kind, name, from: from.toArray(), to: to.toArray(), charge, ...(channel === null ? {} : { channel }) });
        }
      },
      combatNextGun: (a) => {
        const s = at(a).owner.soldier, p = s.nextGun(gun.point);
        gun.found = Boolean(p);
        gun.gun = s.gun;
        gun.hasLast = Boolean(s.lastGun);
        gun.lastIndex = s.lastGun?.index ?? 0;
        gun.lastBarrel = s.lastGun?.barrel ?? 0;
        return gun;
      },
      combatGunAt: (a) => {
        const s = at(a).owner.soldier, p = s.gunAt(point.point, { index: a.index, barrel: a.barrel });
        point.found = Boolean(p);
        return point;
      },
      combatShieldHit: (a) => {
        const c = at(a), s = c.owner.soldier, taken = a.taken, hurt = c.hurts[c.hurts.length - 1] ?? { source: null, info: null, blast: a.explosion };
        // As the JavaScript's absorb calls it: the shield's value taken, not
        // yet down.
        if (s.shield) s.shield.value = a.left;
        s.onShieldHit?.(taken, hurt.source, hurt.blast ? "explosion" : hurt.info?.kind ?? null);
      },
    };
  }

  // The battle's lightning bolt of a name (effects.json), if it has one
  // (every combat's effects are the battle's).
  lightningBolt(name) {
    for (const c of this.facades) if (c) return c.world.lightning(name);
    return undefined;
  }
}

// A weapon of a combat on the module (src/weapons.js Weapon's fields and
// getters), one a weapon and life, as each life's arsenal is new.
class WeaponFacade {
  constructor(combat, index, def) {
    this.combat = combat;
    this.index = index;
    // The class at each charge level, as Weapon keeps them.
    this.levels = [def, ...(def.charges || []).map((level) => ({ ...def, ...level }))];
    this.name = def.class;
    this.channel = def.channel;
    this.thrown = def.base === "grenade" || def.base === "dispenser" || def.base === "remote";
    this.repair = def.base === "repair";
    this.bank = def.AnimationBank || "rifle";
    this.rounds = def.RoundsPerClip;
    // What the battle sets on a weapon (the AI's settings, its generator):
    // its generator and spread settings are written into the store as they
    // are set (the units' phase reads them there); its sniper scale only
    // the AI reads.
    this.kept = { random: null, spreadScale: 1, yawSpreadOverride: null };
    this.sniperScale = 0;
    this.shownAtWork = 0;
    this.meleeView = def.base === "melee" ? new MeleeView(this) : null;
  }

  // Its mirror (the store's weapon record).
  get w() { return this.combat.state.arsenal.weapons[this.index]; }
  get random() { return this.kept.random; }
  set random(value) { const had = Boolean(this.kept.random); this.kept.random = value; if (had !== Boolean(value)) this.keep(5, value ? 1 : 0); }
  get spreadScale() { return this.kept.spreadScale; }
  set spreadScale(value) { if (Object.is(this.kept.spreadScale, value)) return; this.kept.spreadScale = value; this.keep(6, value); }
  get yawSpreadOverride() { return this.kept.yawSpreadOverride; }
  set yawSpreadOverride(value) {
    if (Object.is(this.kept.yawSpreadOverride, value)) return;
    this.kept.yawSpreadOverride = value;
    this.keep(value === null ? 8 : 7, value === null ? 0 : value);
  }
  get def() { return this.levels[this.w.level]; }
  get level() { return this.w.level; }
  get admitShot() { return this.w.admits ? this.combat.admitRemoteLaunch : null; }
  get deployed() { return this.w.deployed; }
  get firing() { return this.w.firing; }
  get timer() { return this.w.timer; }
  get bottomless() { return this.w.bottomless; }
  get perShot() { return this.w.perShot; }
  get clip() { return this.w.clip; }
  set clip(value) { this.set(2, "clip", value); }
  get reserve() { return this.w.reserve; }
  set reserve(value) { this.set(0, "reserve", value); }
  get heat() { return this.w.heat; }
  set heat(value) { this.set(3, "heat", value); }
  get overheated() { return this.w.overheated; }
  get floor() { return this.w.floor; }
  set floor(value) { this.set(4, "floor", value); }
  get kick() { return this.w.kick; }
  set kick(value) { this.set(1, "kick", value); }
  get wait() { return this.w.wait; }
  get reloadLeft() { return this.w.reloadLeft; }
  get throwTime() { return this.w.throwTime; }
  get released() { return this.w.released; }
  get hidden() { return this.w.hidden; }
  get charging() { return this.w.charging; }
  get held() { return this.w.held; }
  get charge() { return this.w.charge; }
  get limit() { return this.w.limit; }
  get pattern() { return this.w.pattern; }
  get salvosLeft() { return this.w.salvosLeft; }
  get salvoWait() { return this.w.salvoWait; }
  get salvoSeed() { return this.w.salvoSeed; }
  get salvoCount() { return this.w.salvoCount; }
  get lockTargetId() { return this.w.lockTargetId; }
  get lockGeneration() { return this.w.lockGeneration; }
  get lockElapsed() { return this.w.lockElapsed; }
  get lockComplete() { return this.w.lockComplete; }
  get melee() { return this.w.melee ? this.meleeView : null; }
  get hasRandom() { return Boolean(this.random); }
  get admits() { return this.w.admits; }
  get handEmpty() { return this.hidden; }
  get reloading() { return this.reloadLeft > 0; }
  get busy() { return this.throwTime >= 0 || this.firing || this.salvosLeft > 0; }
  get loaded() { return this.bottomless ? Infinity : Math.round(this.clip * this.rounds); }
  get spare() { return this.reserve === Infinity ? Infinity : Math.round(this.reserve * Math.max(this.rounds, 1)); }
  get canShoot() { return this.bottomless || this.clip >= this.perShot - 1e-6; }
  get empty() { return !this.canShoot && this.clip + this.reserve < this.perShot - 1e-6; }

  // A field the battle writes (a recharge droid's or a power-up's ammo, the
  // Enhanced AI's spread floor and kick): the module's, and the mirror's at
  // once.
  set(field, name, value) {
    const c = this.combat;
    c.sim.x.cmb_weapon_set(c.owner.id, this.index, field, value);
    this.w[name] = value;
  }

  // What the battle keeps on the weapon, into the store's record of it: a
  // weapon of this life's arsenal only (one the AI still holds from an
  // earlier life is nobody's).
  keep(field, value) {
    const c = this.combat;
    if (c.made && c.arsenal?.weapons[this.index] === this) c.sim.x.cmb_weapon_set(c.owner.id, this.index, field, value);
  }
}

// A melee weapon's swing (src/heroes.js MeleeSwing) as its mirror has it.
class MeleeView {
  constructor(weapon) { this.weapon = weapon; }
  get state() { return this.weapon.w.melee; }
  get time() { return this.state.time; }
  get index() { return this.state.index; }
  get active() { return this.state.active; }
  captureState() { const s = this.state; return { time: s.time, index: s.index, active: s.active }; }
}

// A combat's arsenal (src/weapons.js Arsenal), new each life.
class ArsenalFacade {
  constructor(combat, defs) {
    this.combat = combat;
    this.weapons = defs.map((def, i) => new WeaponFacade(combat, i, def));
    this.channels = [0, 1].map((c) => this.weapons.filter((w) => w.channel === c));
  }

  get selected() { return this.combat.state.arsenal.selected; }

  current(channel) {
    return this.channels[channel][this.selected[channel]] || null;
  }

  // Arsenal.select (the AI's weapon choice), refused while the channel's
  // weapon is busy; one already in the hand asks nothing.
  select(weapon) {
    const list = this.channels[weapon?.channel] || [];
    if (list.indexOf(weapon) < 0) return false;
    if (this.current(weapon.channel) === weapon) return true;
    const c = this.combat;
    return c.answer(c.sim.x.cmb_select(c.owner.id, weapon.index), false);
  }

  next(channel, direction = 1) {
    const c = this.combat;
    return c.answer(c.sim.x.cmb_next(c.owner.id, channel, direction), false);
  }
}

// A world's combat on the module: src/combat.js PlayerCombat's shape for
// the rest of the battle, its state a mirror of the store's.
export class CombatFacade {
  constructor(sim, { soldier, def, actor, animator, collision, camera = null, ordnance, lights, owner, player = false }) {
    this.sim = sim;
    this.owner = owner;
    this.worldBoundary = new CombatWorld(this);
    this.aimOverride = null;
    this.onDamaged = () => {};
    this.soldier = soldier;
    this.def = def;
    this.actor = actor;
    this.animator = animator;
    this.collision = collision;
    this.camera = camera;
    this.ordnance = ordnance;
    this.lights = lights;
    this.hud = null;
    this.onDeath = () => {};
    this.onRespawn = () => {};
    this.onBlowApart = () => {};
    this.items = null;
    this.live = null;
    // What the battle's JavaScript keeps on a combat that the module reads
    // (the units' phase reads them from the store): written into the store
    // as they are set, once the combat has its record.
    this.kept = { mounted: false, gunUnderwater: false, team: 0, player, deferMuzzles: false, random: undefined, remotes: null };
    this.admitRemoteLaunch = (weapon) => this.world.admitRemote(weapon);
    this.lightning = null;
    this.glow = null;
    this.arcTargets = () => [];
    this.effects = null;
    this.firstPerson = false;
    this.view = null;
    this.held = new Map(); // no weapon models in a world
    this.shown = null;
    this.droideka = def.base === "walkerdroid";
    // What the class gives the store (PlayerCombat's constructor).
    this.maxHealthOf = def.MaxHealth || 300;
    this.chunksOf = def.chunks?.length && (def.ChunkFrequency ?? -1) > 0 ? def.ChunkFrequency : 0;
    this.scaleOf = HEALTH_SCALES[(def.HealthType || "person").toLowerCase()] || "PersonScale";
    // The store's record (synced after every command) and the arsenal's
    // facade (new each life).
    this.state = new CombatMirror();
    this.arsenal = null;
    this.made = false;
    // The hurts under way (damage and explode, innermost last): what
    // combat's hooks hand the battle of the caller's own objects.
    this.hurts = [];
    this.droids = [];
    this.in = {};
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.fireDir = new THREE.Vector3();
    this.muzzlePoint = new THREE.Vector3();
    this.worldPoint = new THREE.Vector3();
  }

  get world() {
    return (this.worldBoundary ??= new CombatWorld(this));
  }

  // Its record in the store, made once the unit has its id (the factory
  // calls this when the registry has numbered it).
  attach() {
    if (this.made) return;
    const sim = this.sim, id = this.owner.id;
    sim.facades[id] = this;
    // (The store names him by his id: the bridge knows his object by it.)
    sim.bridge.refs[KIND_UNIT].slotOf(this.owner);
    const cls = sim.unitClass(this.def);
    this.arsenal = new ArsenalFacade(this, this.def.weapons || []);
    this.made = true;
    this.answer(sim.x.cmb_new(id, cls, this.player ? 1 : 0, this.maxHealthOf, this.chunksOf, sim.bridge.strings.id(this.scaleOf)), false);
    // What the battle set on the combat before it had its record.
    const k = this.kept;
    for (const [field, value] of [[5, k.mounted], [6, k.gunUnderwater], [7, k.team], [9, k.deferMuzzles], [10, k.random], [11, k.remotes]]) sim.x.cmb_set(id, field, Number(field >= 10 ? Boolean(value) : value));
    this.state.mounted = k.mounted; this.state.gunUnderwater = k.gunUnderwater; this.state.team = k.team; this.state.deferMuzzles = k.deferMuzzles;
    this.state.hasRandom = Boolean(k.random); this.state.hasRemotes = Boolean(k.remotes);
  }

  // ---- what the battle keeps on the combat (cmb_set's 5 to 11) ----
  get mounted() { return this.kept.mounted; }
  set mounted(value) { this.keep(5, "mounted", value, Boolean(value)); }
  get gunUnderwater() { return this.kept.gunUnderwater; }
  set gunUnderwater(value) { this.keep(6, "gunUnderwater", value, Boolean(value)); }
  get team() { return this.kept.team; }
  set team(value) { this.keep(7, "team", value, value); }
  get player() { return this.kept.player; }
  set player(value) { this.keep(8, "player", value, Boolean(value)); }
  get deferMuzzles() { return this.kept.deferMuzzles; }
  set deferMuzzles(value) { this.keep(9, "deferMuzzles", value, Boolean(value)); }
  get random() { return this.kept.random; }
  set random(value) { this.kept.random = value; if (this.made) { this.sim.x.cmb_set(this.owner.id, 10, value ? 1 : 0); this.state.hasRandom = Boolean(value); } }
  get remotes() { return this.kept.remotes; }
  set remotes(value) { this.kept.remotes = value; if (this.made) { this.sim.x.cmb_set(this.owner.id, 11, value ? 1 : 0); this.state.hasRemotes = Boolean(value); } }

  // A field the battle keeps here, into the store and the mirror at once
  // (`stored`: as the store's record has it).
  keep(field, name, value, stored) {
    if (Object.is(this.kept[name], value)) return;
    this.kept[name] = value;
    if (!this.made) return;
    this.sim.x.cmb_set(this.owner.id, field, Number(stored));
    this.state[name] = stored;
  }

  // ---- the mirror ----
  get maxHealth() { return this.state.maxHealth; }
  get health() { return this.state.health; }
  set health(value) { this.set(0, "health", value); }
  get dead() { return this.state.dead; }
  set dead(value) { this.set(2, "dead", Boolean(value)); }
  get deathTime() { return this.state.deathTime; }
  set deathTime(value) { this.set(3, "deathTime", value); }
  get lastAttacker() { return this.state.lastAttacker; }
  get repairTarget() { return this.state.repairTarget; }
  get repairLock() { return this.state.repairLock; }
  get blowApart() { return this.state.blowApart; }
  set blowApart(value) { this.set(4, "blowApart", Boolean(value)); }
  get chunks() { return this.state.chunks; }
  get zoomStep() { return this.state.zoomStep; }
  set zoomStep(value) { this.set(1, "zoomStep", value); }
  get kick() { return this.state.kick; }
  get scale() { return this.state.scale; }
  get pendingMuzzles() { return this.state.pendingShots; }
  get origin() { return this.state.origin; }
  get target() { return this.state.target; }
  get aimDir() { return this.state.aimDir; }
  get shotDir() { return this.state.shotDir; }
  get left() { return this.state.left; }
  get up() { return this.state.up; }
  get pivot() { return this.state.pivot; }
  get zoomed() { return this.zoomStep > 0; }
  set zoomed(on) { if (!on && this.zoomStep !== 0) this.zoomStep = 0; }
  get zoom() {
    const weapon = this.arsenal?.current(0);
    if (!this.zoomStep || !weapon) return 1;
    return PlayerCombat.zooms(weapon.def)[this.zoomStep] || 1;
  }
  get usesEyeView() { return Boolean(this.firstPerson || (this.zoom > 1 && this.arsenal.current(0)?.def.ZoomFirstPerson)); }
  // What the store's capture reads of a combat (src/combat.js's getters).
  get hasRandom() { return Boolean(this.random); }
  get hasRemotes() { return Boolean(this.remotes); }
  get seenList() { return this.state.seenList; }
  get pendingShots() { return this.state.pendingShots; }

  // A field the battle writes: the module's, and the mirror's at once.
  set(field, name, value) {
    this.sim.x.cmb_set(this.owner.id, field, Number(value));
    this.state[name] = value;
  }

  // ---- the commands ----

  // The unit's view and the fields the battle keeps here (CbIn).
  fill() {
    const u = this.owner, i = this.in;
    i.unit = u.id; i.generation = u.generation; i.classDef = u.classDef; i.networkHuman = u.networkHuman; i.seated = Boolean(u.seat);
    i.remotesActive = this.remotes?.active; i.intent = u.intent; i.controls = u.controls; i.soldier = u.soldier;
    i.edges = Array.isArray(u.networkShots) ? u.networkShots : null; i.edge = u.networkShot ?? null;
    i.aimOverride = this.aimOverride; i.mounted = this.mounted; i.gunUnderwater = this.gunUnderwater; i.team = this.team; i.player = this.player;
    i.deferMuzzles = this.deferMuzzles; i.random = Boolean(this.random); i.hasRemotes = Boolean(this.remotes); i.weapons = this.arsenal.weapons;
    return i;
  }

  // A command's answer (module.rs `answer`): the record, into the mirror,
  // and what it wrote into the unit, into his objects (`unit`: a command
  // that had his view; one that did not, as a weapon chosen between ticks,
  // writes nothing of his); gives its flag.
  answer(ptr, unit = true) {
    const b = this.sim.bridge;
    b.views();
    const f = b.f, base = ptr / 8, at = base + HEADER, h = base + f[base + 2];
    syncCombat(b, f, at + 1 + this.sim.outWords, h, this.state);
    if (unit) {
      syncCbOut(b, f, at + 1, h, this.sim.out);
      this.written(this.sim.out);
    }
    return f[at] !== 0;
  }

  // What combat wrote into the unit: a press or intent flag it took (only
  // one it took: one it left keeps its value), his aim's pitch, a blast's
  // push on a soldier, a droideka's shield.
  written(o) {
    const u = this.owner, c = u.controls, i = u.intent, s = u.soldier;
    if (c.pressed[0] && !o.pressed0) c.pressed[0] = false;
    if (c.pressed[1] && !o.pressed1) c.pressed[1] = false;
    if (i.reload && !o.reload) i.reload = false;
    if (i.nextPrimary && !o.nextPrimary) i.nextPrimary = false;
    if (i.previousPrimary && !o.previousPrimary) i.previousPrimary = false;
    if (i.nextSecondary && !o.nextSecondary) i.nextSecondary = false;
    if (i.zoom && !o.zoom) i.zoom = false;
    s.aimPitch = o.aimPitch;
    if (this.droideka) {
      const shield = s.shield;
      if (shield) { shield.on = o.shieldOn; shield.value = o.shieldValue; }
    } else {
      s.velocity.copy(o.velocity);
      s.air = o.air;
      s.stateTime = o.stateTime;
      s.grounded = o.grounded;
    }
  }

  // Before an import that runs the battle's code: the facade and the unit
  // as the store has them now; after it, the unit's view back.
  current() {
    this.answer(this.sim.x.cmb_current(this.owner.id));
  }

  feed() {
    this.sim.bridge.callArgs(this.sim.inRecord, this.fill());
    this.sim.x.cmb_feed();
  }

  update(dt, controls, stick) {
    this.sim.bridge.callArgs(this.sim.inRecord, this.fill());
    this.answer(this.sim.x.cmb_update(dt, stick));
    this.updateHeld();
  }

  flushMuzzles() {
    if (!this.state.pendingShots?.length) return;
    this.sim.bridge.callArgs(this.sim.inRecord, this.fill());
    this.answer(this.sim.x.cmb_flush());
  }

  reset() {
    this.clearCharge();
    this.arsenal = new ArsenalFacade(this, this.def.weapons || []);
    this.sim.bridge.callArgs(this.sim.inRecord, this.fill());
    this.answer(this.sim.x.cmb_reset());
    this.updateHeld();
  }

  damage(amount, onShield = amount, source = null, attacker = null, info = null) {
    if (this.dead || amount <= 0) return;
    const sim = this.sim, a = sim.damage$;
    a.in = this.fill(); a.amount = amount; a.onShield = onShield; a.source = source; a.attacker = attacker; a.explosion = info?.kind === "explosion";
    this.hurts.push({ source, attacker, info, blast: false });
    try {
      sim.bridge.callArgs(sim.damageRecord, a);
      a.in = a.source = a.attacker = null;
      this.answer(sim.x.cmb_damage());
    } finally { this.hurts.pop(); }
  }

  explode(explosion, position, owner = null, scale = 1) {
    // PlayerCombat.explode's way out before it changes anything, here, so a
    // blast out of his reach asks the module nothing.
    const s = this.soldier, wasDead = this.dead;
    if (wasDead && (this.blowApart || this.def.base === "walkerdroid")) return;
    const [middle, radius] = BLAST_BODY[wasDead ? "prone" : s.air ? "stand" : s.posture] || BLAST_BODY.stand;
    const center = this.worldPoint.copy(s.position);
    center.y += middle;
    const distance = blastDistance(position, center, radius);
    const push = (explosion.Push || 0) * falloff(distance, explosion.PushRadiusInner || 0, explosion.PushRadiusOuter || 0);
    const damage = scale * (explosion.Damage || 0) * blastShare(explosion, distance), typed = explosion[this.scale] ?? 1;
    const hurts = !wasDead && (damage > 0 || !(damage * typed <= 0));
    if (!(push > 0) && !hurts) return;
    const sim = this.sim, a = sim.explode$;
    a.in = this.fill(); a.explosion = sim.explosion(explosion); a.position = position; a.owner = owner; a.scale = scale;
    this.hurts.push({ source: position, attacker: owner, info: null, blast: true });
    try {
      sim.bridge.callArgs(sim.explodeRecord, a);
      a.in = a.position = a.owner = null;
      this.answer(sim.x.cmb_explode());
    } finally { this.hurts.pop(); }
  }

  // A shot as the JavaScript's weapons make it (src/weapons.js, heroes.js
  // MeleeSwing), from its record.
  shot(a) {
    const weapon = this.arsenal.weapons[a.weapon];
    if (a.kind === 1) return { weapon, repair: true };
    if (a.kind === 2) {
      const swings = weapon.def.swings?.length ? weapon.def.swings.slice(0, 3) : null;
      return { weapon, meleeStart: true, swing: a.swing, sound: swings ? swings[a.swing - 1].sound : null };
    }
    return { weapon, yaw: a.yaw, pitch: a.pitch, charge: a.charge, targetId: a.targetId, targetGeneration: a.targetGeneration, ...(a.admission === undefined ? {} : { launchAdmission: a.admission }) };
  }

  // A recon droid's number, for the module.
  droidNumber(droid) {
    let n = this.droids.indexOf(droid);
    if (n < 0) { n = this.droids.length; this.droids.push(droid); }
    return n;
  }

  // ---- what only draws (PlayerCombat's, without models in a world) ----

  updateHeld() {
    if (!this.arsenal) return;
    const thrown = this.arsenal.current(1);
    const weapon = thrown?.busy || thrown?.shownAtWork > 0 ? thrown : this.arsenal.current(0);
    this.shown = this.dead || this.mounted ? null : weapon;
    this.animator.setBank?.(weapon?.bank || "rifle");
  }

  presentProjectileShot() {}

  presentCharge() {}

  clearCharge() {
    this.glow?.kill();
    this.glow = null;
  }

  updateFlashes() {}

  firePoint() {
    return null;
  }

  drawnGunOrigin() {
    return null;
  }

  roundMuzzle(round) {
    return this.firePoint(round.weapon)?.position ?? null;
  }

  bindRoundMuzzle(round, weapon, selected = null) {
    round.weapon = weapon;
    round.muzzleGeneration = this.owner?.generation ?? null;
    round.muzzleIndex = selected?.index ?? -1;
    round.muzzleBarrel = selected?.barrel ?? -1;
    round.visualMuzzle = () => this.roundMuzzle(round);
  }

  tryMeleeBlock(direction, point) {
    if (this.dead || this.mounted) return false;
    const weapon = this.arsenal.current(0);
    if (weapon?.def.base !== "melee" || !Number(weapon.def.CanDeflect)) return false;
    this.effects?.play?.("lightsabre_deflect", point);
    this.world.presentation({ kind: "effect", name: "lightsabre_deflect", position: point.toArray() });
    return true;
  }

  static zooms(def) {
    return PlayerCombat.zooms(def);
  }
}
