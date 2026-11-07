import {BlockHeaderLayout as BLOCK} from '../gen/boundary.js';
// The things lying about on the simulation's module (rust/sim/src/items):
// once a battle's world hands them over (attachItemsSim, with its rounds),
// Rust owns the mines and power-ups and steps them, and the Items' list and
// dispensers' lists hold mirrors of them, refreshed after each update and
// each command (a spawn, a throw, a drop, a blast, a hit). What the Rust
// asks of the battle comes back here as the module's imports
// (rust/sim/schema/items.mjs) and goes to the same ItemsWorld
// (src/ordnance-world.js) the JavaScript items call.
//
// A world only, as the rounds (src/ordnance-sim.js): a page flies the
// copies of the items it is sent with the JavaScript (network-entity-items.js).
import * as THREE from "three";
import { KIND_ITEM, putDispenseArgs, putDropArgs, putHurtArgs, putItemsBlastArgs, putSpawnArgs } from "../gen/index.js";

// A block's header words (rust/sim/src/store/block.rs).
const HEADER = BLOCK.words;
// An item class's fields the Rust reads, in the order items_def takes them
// (rust/sim/src/items/module.rs ITEM_FIELDS).
const ITEM_FIELDS = ["base", "Gravity", "Rebound", "Friction", "LifeSpan", "MaxHealth", "HealthType", "ExplosionDeath", "ExplosionTrigger", "ExplosionExpire", "TriggerRadius", "SuppressRadius"];
const ZERO = Object.freeze(new THREE.Vector3());

// Hands a battle's items to the module its rounds are on (`rounds`, the
// OrdnanceSim attachOrdnanceSim made): from then on Items.update, spawn,
// dispense, dropFor, blast and hurt run in Rust.
export function attachItemsSim(items, rounds) {
  items.sim = new ItemsSim(items, rounds);
  return items.sim;
}

export class ItemsSim {
  constructor(items, rounds) {
    if (items.items.length) throw Error("the items go to the module before any is made");
    this.items = items;
    this.rounds = rounds;
    const b = (this.bridge = rounds.bridge), x = (this.x = b.x);
    b.own(KIND_ITEM);
    this.itemDefs = b.defs.item;
    this.sent = 0;
    this.args = [putSpawnArgs, putDispenseArgs, putDropArgs, putItemsBlastArgs, putHurtArgs].map((put, i) => ({ words: x.items_args_words(i), put }));
    // A class's model, measured once (ItemsWorld.model): its size and middle
    // are the same for every item of the class, and the world draws none.
    this.models = new Map();
    this.flag = { value: false };
    this.number = { value: 0 };
    this.key = { key: "" };
    this.castAnswer = { found: false, distance: 0, point: ZERO, normal: ZERO, terrain: false, body: null };
    this.pointAnswer = { found: false, point: ZERO };
    this.modelAnswer = { mesh: false, radius: 0, center: null };
    this.sendPools();
    b.register(this.handlers());
  }

  // ---- what JavaScript asks of the items ----

  spawn(def, position, velocity, yaw, { owner = null, team = 0 } = {}) {
    this.defId(def);
    return this.command(0, this.x.items_spawn, { def, position, velocity, yaw, owner, team });
  }

  dispense(weapon, maxItems, def, position, velocity, yaw, owner, team) {
    this.defId(def);
    return this.command(1, this.x.items_dispense, { weapon, maxItems, def, position, velocity, yaw, owner: owner ?? null, team });
  }

  // A death's drop: the side's classes the list names, looked up first (a
  // name the side lacks is null, which ends the roll with nothing there).
  dropFor(drops, classes, position, velocity, yaw) {
    const entries = (drops || []).map((drop) => {
      const def = classes[drop.class] || null;
      if (def) this.defId(def);
      const [probabilityTag, probability] = this.rounds.fieldWords(drop.probability);
      return { def, probabilityTag, probability };
    });
    return this.command(2, this.x.items_drop_for, { drops: entries, position, velocity, yaw });
  }

  blast(explosion, position) {
    this.command(3, this.x.items_blast, { explosion: this.rounds.explosionId(explosion), position });
  }

  hurt(item, amount) {
    this.command(4, this.x.items_hurt, { item, amount });
  }

  // A command: its arguments into the call block, the export run, the item
  // it made (its mirror, made current) or null, then every mirror.
  command(which, run, args) {
    this.bridge.callArgs(this.args[which], args);
    const native = this.rounds.nativeServices;
    native.beforeStep();
    let item;
    try { const ptr = run();item = ptr ? this.rounds.answered(ptr) ?? null : undefined; }
    finally { native.afterStep(); }
    return item;
  }

  defId(def) {
    const table = this.itemDefs, id = table.id(def);
    while (this.sent < table.list.length) {
      const n = this.sent++;
      this.rounds.writeFields(table.at(n), ITEM_FIELDS);
      this.x.items_def(n);
      const def = table.at(n);
      this.rounds.nativeServices.item(def,this.model(def));
    }
    return id;
  }

  // The pools (how many of each base the world holds at most) as fields.
  sendPools() {
    const entries = Object.entries(this.items.pools), words = new Float64Array(3 * entries.length), b = this.bridge;
    entries.forEach(([base, size], i) => {
      words[3 * i] = b.strings.id(base);
      [words[3 * i + 1], words[3 * i + 2]] = this.rounds.fieldWords(size);
    });
    const ptr = this.x.sim_call_room(words.length, 0);
    b.views();
    b.f.set(words, ptr / 8 + HEADER);
    this.x.items_pools(entries.length);
  }

  model(def) {
    let m = this.models.get(def);
    if (!m) {
      const made = this.items.world.model(def);
      // Measured, not drawn: the world shows nothing.
      if (made.mesh) this.items.scene.remove(made.mesh);
      m = { mesh: Boolean(made.mesh), radius: made.radius, center: made.center ? made.center.clone() : null };
      this.models.set(def, m);
    }
    const a = this.modelAnswer;
    a.mesh = m.mesh;
    a.radius = m.radius;
    a.center = m.center;
    return a;
  }

  // ---- what the items ask of the battle (the module's imports) ----

  handlers() {
    const items = this.items, w = () => items.world;
    const cast = (hit) => {
      const a = this.castAnswer;
      if (!hit) { a.found = false; a.distance = 0; a.point = ZERO; a.normal = ZERO; a.terrain = false; a.body = null; return a; }
      a.found = true; a.distance = hit.distance; a.point = hit.point; a.normal = hit.normal; a.terrain = Boolean(hit.terrain); a.body = hit.body ?? null;
      return a;
    };
    const point = (p) => { const a = this.pointAnswer; a.found = Boolean(p); a.point = p || ZERO; return a; };
    const flag = (value) => { this.flag.value = value; return this.flag; };
    return {
      itemCast: (a) => cast(w().cast(a.origin, a.direction, a.far, items.hit)),
      blocked: (a) => flag(w().blocked(a.position, a.target)),
      itemWaterCrossing: (a) => point(w().waterCrossing(a.from, a.to, items.wet)),
      itemModel: (a) => this.model(a.def),
      ownerKey: (a) => { this.key.key = w().ownerKey(a.owner); return this.key; },
      dropRoll: () => { this.number.value = w().dropRoll(); return this.number; },
      // (Vectors handed on are copies: the arguments object is read into
      // again at the next call.)
      explode: (a) => w().explode(this.rounds.fieldValue(a.defTag, a.def), a.position.clone(), a.owner),
      pickup: (a) => w().pickup(a.def, a.position.clone()),
    };
  }
}
