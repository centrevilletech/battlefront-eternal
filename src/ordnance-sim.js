import {BlockHeaderLayout as BLOCK} from './sim/gen/boundary.js';
// The rounds in flight on the simulation's module (rust/sim/src/ordnance):
// once a battle's world hands them over (attachOrdnanceSim), Rust owns the
// bolts, beams, grenades and rounds and flies them, and the Ordnance's
// lists hold mirrors of them (src/sim/runtime.js MirrorPool), refreshed
// after each step and each round fired, which the rest of the battle reads
// as it read the rounds themselves. Everything the Rust asks of the battle
// comes back here as the module's imports (rust/sim/schema/ordnance.mjs)
// and goes to the same OrdnanceWorld (src/ordnance-world.js) the
// JavaScript rounds call, with the same objects: a round handed to the
// capture's observers is its mirror, made current first; a unit's hit gets
// the very vector the JavaScript hands it (a damage event keeps it).
//
// A world only: a page draws rounds from the records it is sent, and the
// presentation (trails, bolt looks, the sniper trace) stays the page's.
import * as THREE from "three";
import { IMPORT_TABLE, KIND_BEAM, KIND_BOLT, KIND_GRENADE, KIND_ORDNANCE, KIND_ROUND, KIND_TABLE, putFireArgs, putFlightMissileArgs } from "./sim/gen/index.js";
import { DefTable } from "./sim/runtime.js";

// A block's header words (rust/sim/src/store/block.rs).
const HEADER = BLOCK.words;
const TYPES = ["round", "grenade", "beam"];
// The kinds of round Rust owns, in the order their mirrors are refreshed
// (each kind before the lists that name it).
const OWNED = [KIND_BEAM, KIND_BOLT, KIND_GRENADE, KIND_ROUND];
// An ordnance class's and an explosion's fields the Rust reads, in the
// order ord_def and ord_explosion take them (rust/sim/src/ordnance/defs.rs).
const ORDNANCE_FIELDS = ["base", "Velocity", "LifeSpan", "Gravity", "LaserLength", "GlowLength", "BlurLength", "Damage", "ScaleTime", "TurnRate", "WaverTurn", "WaverRate", "StickTerrain", "StickBuilding", "ExplosionImpact", "ExplosionExpire"];
const EXPLOSION_FIELDS = ["Damage", "DamageRadiusInner", "DamageRadiusOuter", "PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];
const ZERO = Object.freeze(new THREE.Vector3());

// The plain objects among a round's owners and a beam's sources, told
// apart by shape as the replay harness's capture tells them: a hull's or
// seat's beam source (src/vehicles.js beamSource, a getter for its frame)
// and what src/ordnance-owner.js froze of a unit.
export const ordnanceTests = {
  emitter: (v) => Object.getPrototypeOf(v) === Object.prototype && typeof v.viewDirection === "function" && typeof Object.getOwnPropertyDescriptor(v, "matrix")?.get === "function",
  ownerSnapshot: (v) => Object.isFrozen(v) && Object.getPrototypeOf(v) === Object.prototype && Number.isSafeInteger(v.id) && Number.isSafeInteger(v.generation) && "className" in v,
};

// Hands a battle's rounds to its instance of the module, through `bridge`
// (a SimBridge on it, given the battle's Unit and Vehicle classes and the
// tests above); from then on ordnance.fire, update and flightMissile run
// in Rust.
export function attachOrdnanceSim(ordnance, bridge) {
  ordnance.sim = new OrdnanceSim(ordnance, bridge);
  return ordnance.sim;
}

export class OrdnanceSim {
  constructor(ordnance, bridge) {
    if (ordnance.bolts.length || ordnance.beams.length || ordnance.grenades.length || ordnance.rounds.length) throw Error("the rounds go to the module before any is fired");
    this.ordnance = ordnance;
    this.bridge = bridge;
    this.x = bridge.x;
    for (const kind of OWNED) bridge.own(kind);
    // Definitions the module has been given, by sort (ordnance classes in
    // the bridge's own table, which the rows' definition words index).
    this.ordnanceDefs = bridge.defs.ordnance;
    this.sent = 0;
    // Fleet content loads before the unit owner binds live world services.
    // These cold observers collect no state; binding replays prepared classes.
    // Entering flight before binding is an error, never a second authority.
    this.nativeServices = {ordnance(){},explosion(){},item(){},
      owner(){throw Error('unit world services are not bound');},
      beforeStep(){throw Error('unit world services are not bound');},
      afterStep(){throw Error('unit world services are not bound');}};
    this.water = false;
    this.explosions = new DefTable("explosion");
    this.fireArgs = { words: this.x.ord_args_words(0), put: putFireArgs };
    this.missileArgs = { words: this.x.ord_args_words(1), put: putFlightMissileArgs };
    this.fire$ = { def: 0, origin: null, direction: null, owner: null, visual: null, ignoreBody: null, emitter: null, channel: 0, targetId: undefined, targetGeneration: undefined };
    this.missile$ = { round: null, flightClass: undefined, life: 0, speed: 0 };
    this.rowAt = Object.fromEntries(["projectile", "launch", "impact", "grenadeContact"].map((name) => [name, IMPORT_TABLE.find((i) => i?.name === name).args.words]));
    // Answers, written whole before each is handed back (the bridge copies
    // an answer into the module before any other import runs).
    this.castAnswer = { found: false, distance: 0, point: ZERO, normal: ZERO, terrain: false, body: null };
    this.unitAnswer = { found: false, unit: null, distance: 0, point: ZERO, normal: ZERO, part: null };
    this.mountAnswer = { found: false, vehicle: null, boundRadius: 0, body: null };
    this.nearAnswer = { found: false, units: [] };
    this.sphereAnswer = { found: false, center: ZERO, radius: 0 };
    this.flag = { value: false };
    this.number = { value: 0 };
    this.tick = { value: null };
    this.pointAnswer = { found: false, point: ZERO };
    this.poseAnswer = { position: null, direction: null };
    this.seekAnswer = { velocity: null, targetId: 0 };
    this.target = new THREE.Vector3();
    bridge.register(this.handlers());
  }

  // ---- what JavaScript asks of the rounds ----

  // ordnance.js fireRound: the round made (its mirror), for a bolt, rocket,
  // missile, shell or fat ray; undefined for a beam or grenade.
  fire(def, origin, direction, owner, visual, ignoreBody, emitter, channel, targetId, targetGeneration) {
    this.waterKnown();
    const a = this.fire$;
    a.def = def;
    this.defId(def);
    a.origin = origin;
    a.direction = direction;
    a.owner = owner ?? null;
    a.visual = visual ?? null;
    a.ignoreBody = ignoreBody ?? null;
    a.emitter = emitter ?? null;
    a.channel = channel;
    a.targetId = targetId ?? undefined;
    a.targetGeneration = targetGeneration ?? undefined;
    this.nativeServices.owner(a.owner);
    this.bridge.callArgs(this.fireArgs, a);
    a.owner = a.visual = a.ignoreBody = a.emitter = a.origin = a.direction = null;
    this.nativeServices.beforeStep();
    let shot;
    try { shot = this.answered(this.x.ord_fire()); }
    finally { this.nativeServices.afterStep(); }
    this.bridge.sync(KIND_ORDNANCE, this.ordnance);
    return shot;
  }

  // ordnance.js flightMissile, on the round's own state.
  flightMissile(round, flightClass, life, speed) {
    const a = this.missile$;
    a.round = round;
    a.flightClass = flightClass ?? undefined;
    a.life = life;
    a.speed = speed;
    this.bridge.callArgs(this.missileArgs, a);
    a.round = null;
    this.answered(this.x.ord_flight_missile());
  }

  // ordnance.js update (a world's): the rounds' flight, then every mirror.
  update(dt) {
    this.waterKnown();
    this.nativeServices.beforeStep();
    let kinds;
    try { kinds = this.x.ord_update(dt); }
    finally { this.nativeServices.afterStep(); }
    // Ordnance.update delivers impacts while the launched object still has
    // its identity. Retiring mirrors first would make a queued impact resolve
    // a new object and lose CombatCapture's launch-to-terminal association.
    this.refresh(kinds);
  }

  // Whether the battle has water, told to the module when it changes (a
  // world sets ordnance.water once it is made): with none, a round's water
  // crossing is null without asking.
  waterKnown() {
    const has = Boolean(this.ordnance.water);
    if (has !== this.water) this.x.ord_water(0, (this.water = has) ? 1 : 0);
  }

  // The rounds' mirrors and the lists: each kind's retired first (their
  // last state), then the rest, then the Ordnance's lists in place. `kinds`
  // (ord_update's answer): a bit for each kind of OWNED with anything to
  // sync; none, and nothing changed.
  refresh(kinds = 0xf) {
    if (!kinds) return;
    const b = this.bridge;
    for (let i = 0; i < OWNED.length; i++) if (kinds & (1 << i)) b.sync(OWNED[i]);
    b.sync(KIND_ORDNANCE, this.ordnance);
  }

  // A command's answer (ord_fire's, ord_flight_missile's): the round's
  // mirror, made current, or undefined.
  answered(ptr) {
    const b = this.bridge;
    b.views();
    const f = b.f, base = ptr / 8, at = base + HEADER, kind = f[at];
    if (!kind) return undefined;
    return this.mirror(kind, f, at + 1, base + f[base + BLOCK.heap]);
  }

  // The mirror of the round whose row is at `at` (heap from `h`), made
  // current.
  mirror(kind, f, at, h) {
    const m = this.bridge.refs[kind].at(f[at], f[at + 1]);
    KIND_TABLE[kind].sync(this.bridge, f, at, h, m);
    return m;
  }

  // The round an import hands over: its row after the import's own words.
  handed(name, kind) {
    const b = this.bridge;
    b.views();
    const f = b.f, base = b.exchange;
    return this.mirror(kind, f, base + HEADER + this.rowAt[name], base + f[base + BLOCK.heap]);
  }

  // An ordnance class's index, given to the module the first time.
  defId(def) {
    const table = this.ordnanceDefs, id = table.id(def);
    while (this.sent < table.list.length) this.send(this.sent++);
    return id;
  }

  send(id) {
    const def = this.ordnanceDefs.at(id);
    this.writeFields(def, ORDNANCE_FIELDS);
    this.x.ord_def(id);
    // Whole-phase owners can prepare native hit policy when a definition
    // enters F1. This construction seam never runs from a projectile tick.
    this.nativeServices.ordnance(id, def);
  }

  explosionId(def) {
    let id = this.explosions.ids.get(def);
    if (id !== undefined) return id;
    id = this.explosions.register(def);
    this.writeFields(def, EXPLOSION_FIELDS);
    this.x.ord_explosion(id);
    this.nativeServices.explosion(id, def);
    return id;
  }

  // A definition's fields as (tag, value) word pairs into the call block
  // (rust/sim/src/ordnance/module.rs `field`). An explosion a class names
  // is registered (its own words written and taken) before the class's
  // words go in.
  writeFields(def, names) {
    const words = new Float64Array(2 * names.length);
    for (let i = 0; i < names.length; i++) [words[2 * i], words[2 * i + 1]] = this.fieldWords(def[names[i]]);
    const b = this.bridge, ptr = this.x.sim_call_room(words.length, 0);
    b.views();
    b.f.set(words, ptr / 8 + HEADER);
  }

  // A value as a definition's field's two words: [tag, value].
  fieldWords(v) {
    if (v === undefined) return [0, 0];
    if (v === null) return [1, 0];
    if (v === false) return [2, 0];
    if (v === true) return [3, 0];
    if (typeof v === "number") return [4, v];
    if (typeof v === "string") return [5, this.bridge.strings.id(v)];
    if (typeof v === "object") return [6, this.explosionId(v)];
    return [7, 0];
  }

  // A field's value back from its words (BlastArgs).
  fieldValue(tag, value) {
    switch (tag) {
      case 0: return undefined;
      case 1: return null;
      case 2: return false;
      case 3: return true;
      case 4: return value;
      case 5: return this.bridge.strings.at(value);
      case 6: return this.explosions.at(value);
      default: throw Error("an explosion's definition the module cannot hand back");
    }
  }

  // A beam's copy of its class (made with the beam: the projectile
  // handler) holding this step's share of the Damage (ordnance.js stepBeam).
  beamDef(beam, damage) {
    const def = (beam.damageDef ??= { ...beam.def });
    def.Damage = damage;
    return def;
  }

  // ---- what the rounds ask of the battle (the module's imports) ----

  handlers() {
    const o = this.ordnance, w = () => o.world;
    const cast = (hit) => {
      const a = this.castAnswer;
      if (!hit) { a.found = false; a.distance = 0; a.point = ZERO; a.normal = ZERO; a.terrain = false; a.body = null; return a; }
      a.found = true; a.distance = hit.distance; a.point = hit.point; a.normal = hit.normal; a.terrain = hit.terrain; a.body = hit.body ?? null;
      return a;
    };
    const point = (p) => {
      const a = this.pointAnswer;
      a.found = Boolean(p);
      a.point = p || ZERO;
      return a;
    };
    const flag = (value) => { this.flag.value = value; return this.flag; };
    const number = (value) => { this.number.value = value; return this.number; };
    return {
      cast: (a) => cast(w().cast(a.origin, a.direction, a.far, a.behind ? o.behind : o.hit, a.ignoreBody, a.staticOnly)),
      castMoving: (a) => cast(w().castMoving(a.origin, a.direction, a.far, a.behind ? o.behind : o.hit, a.ignoreBody)),
      raycastUnits: (a) => {
        const hit = w().raycastUnits(a.origin, a.direction, a.far, a.owner, a.historyTick, a.ignoreBody, a.mount), r = this.unitAnswer;
        if (!hit) { r.found = false; r.unit = null; r.distance = 0; r.point = ZERO; r.normal = ZERO; r.part = null; return r; }
        r.found = true; r.unit = hit.unit; r.distance = hit.distance; r.point = hit.point; r.normal = hit.normal; r.part = hit.part;
        return r;
      },
      riderMount: (a) => {
        const m = w().riderMount(a.body), r = this.mountAnswer;
        if (!m) { r.found = false; r.vehicle = null; r.boundRadius = 0; r.body = null; return r; }
        r.found = true; r.vehicle = m.vehicle; r.boundRadius = m.boundRadius; r.body = m.body;
        return r;
      },
      fatQuery: (a) => {
        const near = w().fatQuery(a.center, a.radius, a.owner, a.struck, a.ignoreBody), r = this.nearAnswer;
        r.found = Boolean(near);
        r.units = near || [];
        return r;
      },
      unitSphere: (a) => {
        const s = w().unitSphere(a.unit), r = this.sphereAnswer;
        r.found = Boolean(s);
        r.center = s ? s.center : ZERO;
        r.radius = s ? s.radius : 0;
        return r;
      },
      isDroid: (a) => flag(w().isDroid(a.unit)),
      gone: (a) => flag(w().gone(a.subject)),
      beamLocalOrigin: (a) => point(w().beamLocalOrigin(a.source, a.origin)),
      beamPose: (a) => {
        w().beamPose(a.source, a.localOrigin, a.position, a.direction);
        this.poseAnswer.position = a.position;
        this.poseAnswer.direction = a.direction;
        return this.poseAnswer;
      },
      shotTick: (a) => { this.tick.value = w().shotTick(a.owner, a.channel); return this.tick; },
      historyTick: () => number(w().historyTick()),
      shooter: (a) => w().shooter(a.owner, a.channel),
      missileTarget: (a) => point(w().missileTarget(a.targetId, a.targetGeneration, this.target) ? this.target : null),
      navigateMissile: (a) => {
        const r = this.seekAnswer;
        r.targetId = w().navigateMissile(a.position, a.velocity, a.flightClass, a.targetId, a.targetGeneration, a.dt);
        r.velocity = a.velocity;
        return r;
      },
      waterCrossing: (a) => point(w().waterCrossing(a.from, a.to, o.wet)),
      waver: () => number(w().waver()),
      spin: () => number(w().spin()),
      // (Vectors handed on are copies: the arguments object is read into
      // again at the next call, and the code they go to may keep them.)
      explosion: (a) => w().explosion(this.fieldValue(a.defTag, a.def), a.position.clone(), a.owner),
      hit: (a) => w().hit(a.body, a.beam ? this.beamDef(a.beam, a.damage) : a.def, a.owner, a.direction.clone()),
      hitUnit: (a) => {
        const def = a.beamDef ? this.beamDef(a.beam, a.damage) : a.def;
        // The vector the JavaScript hands (rust/sim/src/ordnance/world.rs Point).
        let at;
        if (a.at === 1) at = (a.beam.end ??= new THREE.Vector3()).copy(a.point);
        else if (a.at === 2) at = o.unitHit.point.copy(a.point);
        else if (a.at === 3) at = o.start.copy(a.point);
        else at = a.point.clone();
        w().hitUnit(a.unit, def, at, a.owner, a.part, a.direction.clone());
      },
      projectile: (a) => {
        const round = this.handed("projectile", a.kind);
        // A beam's copy of its class, as ordnance.js beam makes it.
        if (a.kind === KIND_BEAM) round.damageDef = { ...round.def };
        w().projectile(round, TYPES[a.type]);
      },
      launch: (a) => w().launch(this.handed("launch", a.kind), a.tick),
      impact: (a) => {
        const round = this.handed("impact", a.kind), s = a.struck;
        w().impact(round, s ? { point: s.point.clone(), normal: s.normal.clone(), terrain: s.terrain, body: s.body, unit: s.unit, part: s.part, surface: a.surface } : { point: a.point.clone(), normal: a.normal.clone(), surface: a.surface });
      },
      grenadeContact: (a) => w().grenadeContact(this.handed("grenadeContact", a.kind)),
      bounce: (a) => w().bounce(a.def, a.position.clone(), a.speed),
    };
  }
}
