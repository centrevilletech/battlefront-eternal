import * as THREE from 'three';
import { KIND_ITEM, F3ItemClassMirror, F3DispenserBindingMirror, F3UnitItemsMirror, F3ItemTargetMirror,
  putF3ItemClass, putF3DispenserBinding, putF3UnitItems, putF3ItemTarget } from './sim/gen/index.js';

// Item snapshots use the same persistent mirrors as F1. Scalars and vectors
// are sampled at mutation, then copied after the native span has returned.
const SCALARS = ['def','mine','yaw','landed','sinking','radius','owner','team','age','life','health','gone'];
const VECTORS = ['position','previous','velocity','up'];
const HEALTH_GAIN = ['PersonHealth','AnimalHealth','DroidHealth','VehicleHealth','BuildingHealth'];
const AMMO_GAIN = ['PersonAmmo','AnimalAmmo','DroidAmmo','VehicleAmmo','BuildingAmmo'];
// F1 owns the rows; this object owns only content bindings and the ordinary
// lists read by AI and capture. It never imports Item mirrors back into Rust.
export class F3Items {
  constructor(phase, items) {
    this.phase = phase;this.items = items;
    this.groups = new Map();this.keys = new Map();this.projected = new Map();this.lists = new Map();
    this.classRow = new F3ItemClassMirror();this.binding = new F3DispenserBindingMirror();
    this.classArgs = {words:phase.layout.itemClassWords,put:putF3ItemClass};
    this.bindingArgs = {words:phase.layout.dispenserWords,put:putF3DispenserBinding};
    this.unitRow = new F3UnitItemsMirror();
    this.unitArgs = {words:phase.layout.unitItemsWords,put:putF3UnitItems};
    this.units = new WeakSet();
    this.pending = new Array(1024);this.count = 0;
  }
  // Game.onDeath resolves drops through the soldier's side. Do this once
  // with class construction, including the entries naming absent classes.
  prepareUnit(unit) {
    if (unit.itemTarget) this.bindTarget(unit.itemTarget,unit);
    if (this.units.has(unit.classDef)) return;
    this.units.add(unit.classDef);
    const row = this.unitRow, classes = this.phase.factory.sides?.[unit.side]?.items;
    row.class = unit.classDef;row.drops.length = 0;
    row.addHealth = Number(unit.classDef.AddHealth);
    row.healthEffect = unit.classDef.HealthGainEffect || 'Powerup_blue';
    row.ammoEffect = unit.classDef.AmmoGainEffect || 'Powerup_red';
    row.explosion = unit.classDef.explosion ? this.phase.factory.context.ordnance.sim.explosionId(unit.classDef.explosion) : null;
    for (const drop of unit.classDef.drops ?? []) {
      // A missing side class stays in the roll and ends it without a drop.
      // Removing that entry here would change which later class is chosen.
      const def = classes?.[drop.class] ?? null;
      if (def) this.items.sim.defId(def);
      const words = this.items.sim.rounds.fieldWords(drop.probability);
      row.drops.push({def,probabilityTag:words[0],probability:words[1]});
    }
    this.phase.bridge.callArgs(this.unitArgs,row);this.phase.x.f3_unit_items();
  }
  // ItemsWorld.model measures immutable class geometry once. Pool grouping,
  // default life and health become numeric facts before a shot can spawn it.
  prepare(def, model) {
    let group = this.groups.get(def.base);
    if (group === undefined) { group = this.groups.size;this.groups.set(def.base,group); }
    const row = this.classRow;
    row.def = def;row.base = group;row.pool = Number(this.items.pools[def.base] ?? Infinity);
    row.mine = def.base === 'mine';row.life = Number(def.LifeSpan ?? Infinity);
    row.health = Number(def.MaxHealth ?? Infinity);row.model = model;
    for (let i = 0; i < HEALTH_GAIN.length; i++) {
      row.healthGain[i] = Number(def[HEALTH_GAIN[i]] || 0);
      row.ammoGain[i] = Number(def[AMMO_GAIN[i]] || 0);
    }
    this.phase.bridge.callArgs(this.classArgs,row);this.phase.x.f3_item_class();
  }
  // Game's mineTargets retains target membership and order. Resolve those
  // stable objects cold, then write qualified handles beside the F1 entry.
  // No health, weapon clocks or geometry travel back into the owning store.
  bindTarget(target, owner, hull = false) {
    const row = new F3ItemTargetMirror();
    if (hull) row.hull = owner; else row.unit = owner;
    target.f3ItemTarget = row;
  }
  writeTargets(targets) {
    const p = this.phase, b = p.bridge;
    b.views(); let at = p.layout.input / 8;
    for (let i = 0; i < targets.length; i++) {
      const row = targets[i].f3ItemTarget;
      if (!row) throw Error('unbound owned item target');
      row.life = row.unit ? row.unit.generation : row.hull.aiTarget.generation;
      putF3ItemTarget(b,row,b.f,at,null); at += p.layout.itemTargetWords;
    }
    p.counters.inputFields += targets.length * p.layout.itemTargetWords;
  }
  // A new life can be published during an aimer. Its names/buffers are
  // construction work and wait until the outer return, never a nested export.
  schedule(binding) {
    if (!this.items || binding.itemPending) return;
    binding.itemPending = true;this.pending[this.count++] = binding;
  }
  flush() {
    // The shared call block may still hold a native command's answer. Flush
    // only after that answer is decoded and the outer invocation has ended.
    if (this.phase.active || !this.count) return;
    const count = this.count;this.count = 0;
    for (let index = 0; index < count; index++) {
      const binding = this.pending[index];binding.itemPending = false;
      const unit = binding.unit, weapons = unit.combat.arsenal.weapons;
      const previous = binding.itemKeys ??= new Array(64);
      for (let i = 0; i < (binding.itemKeyCount ?? 0); i++) {
        const key = previous[i], projected = this.projected.get(key);
        if (projected === undefined || !this.items.owned.has(projected)) {
          this.keys.delete(key);this.projected.delete(key);this.lists.delete(key);
        }
      }
      binding.itemKeyCount = 0;
      for (let i = 0; i < weapons.length; i++) {
        const weapon = weapons[i];
        for (let level = 0; level < weapon.levels.length; level++) {
          const def = weapon.levels[level];if (!def.item) continue;
          const name = def.class ?? '';
          const key = `${name}:body:${binding.view.key.slot}:${binding.view.key.generation}:${unit.generation}`;
          // The private key is independent of owner numbering. Do not consume
          // ItemsWorld.ownerIds until the first actual throw is delivered.
          this.keys.set(key,{owner:unit,life:unit.generation,name});
          previous[binding.itemKeyCount++] = key;
          const row = this.binding;
          row.key = binding.view.key;row.weapon = weapon.levels[0];row.level = level;
          row.name = name;row.ownedKey = key;
          this.phase.bridge.callArgs(this.bindingArgs,row);
          if (!this.phase.x.f3_dispenser_binding()) throw Error('stale dispenser construction');
          this.phase.counters.wasmCalls++;this.phase.counters.lifecycleCalls++;
          this.phase.counters.inputFields += this.bindingArgs.words;
        }
      }
    }
  }
  snapshot(event) {
    // Copy into the existing objects. Aliasing an event vector would let the
    // next decoded notice move an item that did not change this time.
    const item = event.item, state = event.state;
    for (let i = 0; i < SCALARS.length; i++) { const key = SCALARS[i];item[key] = state[key]; }
    for (let i = 0; i < VECTORS.length; i++) { const key = VECTORS[i];item[key].copy(state[key]); }
    // Model presence cannot change during an item's life. These objects are
    // made only for a newly constructed mirror, after the outer phase return.
    if (state.center) { item.center ??= new THREE.Vector3();item.center.copy(state.center); }
    else item.center = null;
    if (state.turn) { item.turn ??= new THREE.Quaternion();item.turn.copy(state.turn); }
    else item.turn = null;
    if (event.made) this.items.items.push(item);
  }
  retired(event) {
    // MirrorPool.retire keeps the old object's terminal position intact.
    // A later native slot generation receives a different JS item object.
    const item = this.phase.bridge.refs[KIND_ITEM].retire(event.slot,event.generation);
    const at = this.items.items.indexOf(item);
    if (at >= 0) this.items.items.splice(at,1);
  }
  // Project only the changed list. The source map's first actual insertion
  // determines the owner's lazy number, even when another unit was built first.
  owned(event) {
    const meta = this.keys.get(event.key);
    let key = this.projected.get(event.key);
    if (event.removed) {
      if (key !== undefined) this.items.owned.delete(key);
      const list = this.lists.get(event.key);if (list) list.length = 0;
      // An old life's drained list cannot be used by this pooled body again.
      // Release its projection metadata and its references to retired items.
      if (meta.life !== meta.owner.generation || meta.owner.removed) {
        this.keys.delete(event.key);this.projected.delete(event.key);this.lists.delete(event.key);
      }
      return;
    }
    if (key === undefined) {
      // ownedBy can number an owner before its first throw. Read that same
      // WeakMap so native construction order never consumes a lazy number.
      const world = this.items.world, owner = meta.owner;
      let number = world.ownerIds.get(owner);
      if (number === undefined) { number = world.nextOwnerId++;world.ownerIds.set(owner,number); }
      key = `${meta.name}:unit:${number}:${meta.life}`;
      this.projected.set(event.key,key);
    }
    let list = this.lists.get(event.key);
    if (!list) { list = [];this.lists.set(event.key,list); }
    list.length = event.items.length;
    for (let i = 0; i < list.length; i++) list[i] = event.items[i];
    this.items.owned.set(key,list);
  }
}
