import {NO_RANDOM_STREAM} from './sim/gen/boundary.js';
import { randomBinding, randomRevision } from './ai/random.js';
import { F3RandomEpochLayout, F3RandomStateMirror, F3RandomGlobalsMirror, putF3RandomState, syncF3RandomState, putF3RandomGlobals } from './sim/gen/index.js';

const NO_STREAM = NO_RANDOM_STREAM;

// ClassicRandom's callbacks remain ordinary JS functions for AI decisions.
// Rust draws from the same state between boundaries, without a per-draw import.
export class F3Random {
  constructor(phase) {
    this.phase = phase;
    this.entries = [];
    this.nativeEpoch = -1;
    this.jsEpoch = -1;
    this.ids = new WeakMap();
    this.row = new F3RandomStateMirror();
    this.globals = new F3RandomGlobalsMirror();
    this.globals.gameplay = this.globals.spin = this.globals.drop = this.globals.seat = NO_STREAM;
  }

  // Loading a named binding must not call RandomStreams.get. That map's lazy
  // insertion order is an observed part of the authority's random-state hash.
  register(draw) {
    if (!draw) return NO_STREAM;
    const binding = randomBinding(draw);
    if (!binding) throw Error('F3 random callback has no generator binding');
    const source = binding.streams ?? binding.random;
    // A bare AI generator is keyed by object identity, even when another
    // generator has the same seed. Names distinguish a registry's streams.
    let names = this.ids.get(source);
    if (!names) { names = new Map(); this.ids.set(source, names); }
    const name = binding.name;
    let id = names.get(name);
    if (id !== undefined) return id;
    id = this.entries.length;
    if (id >= this.phase.layout.randomCapacity) throw Error('F3 random binding capacity exceeded');
    names.set(name, id);
    this.nativeEpoch = -1;
    this.jsEpoch = -1;
    this.entries.push({
      binding, draw,
      present: false,
      seed: binding.streams?.seed,
      initial: binding.streams ? binding.streams.initialState(name) : 0,
      row: new F3RandomStateMirror(),
    });
    return id;
  }

  configure(gameplay, spin, drop, seat) {
    // These are content-time bindings for F1 and drops. Preparing a channel
    // does not draw from it or create a named stream in the world's map.
    this.globals.gameplay = this.register(gameplay);
    this.globals.spin = this.register(spin);
    this.globals.drop = this.register(drop);
    this.globals.seat = this.register(seat);
    const p = this.phase, b = p.bridge;
    b.views(); putF3RandomGlobals(b, this.globals, b.f, p.layout.randomGlobals / 8, b.heap);
    p.counters.inputFields += p.layout.randomGlobalWords;
  }

  // Cold adoption visits the complete loadout once. Live setters publish
  // only their changed binding, including true-to-true callback replacement.
  refresh(binding) {
    const c = binding.unit.combat;
    this.combat(binding);
    const weapons = c.arsenal.weapons;
    for (let i = 0; i < weapons.length; i++) this.weapon(binding, i, weapons[i]);
  }
  combat(binding) {
    const draw = binding.unit.combat.random;
    if (binding.randomDraw === draw) return;
    binding.randomDraw = draw;
    this.bind(binding, 0, draw);
  }
  weapon(binding, index, weapon) {
    const draw = weapon.random;
    if (weapon.f3RandomDraw === draw) return;
    weapon.f3RandomDraw = draw;
    this.bind(binding, index + 1, draw);
  }
  bind(binding, index, draw) {
    // Index zero is combat's chunks generator; the arsenal starts at one.
    // The ordinary qualified command also works as an aimer return delta.
    const p = this.phase;
    const id = this.register(draw);
    p.begin(binding, 'randomBinding'); p.change.index = index; p.change.value = id;
    p.commit();
  }

  // Mounted publications retain the original callback identity. Returning
  // this function neither creates a stream nor draws from it on the JS side.
  callback(id) { return id === NO_STREAM ? null : this.entries[id].draw; }

  // Only generator words enter the phase. Bodies and weapon tables stay in
  // their owned slabs; the AI's private Enhanced stream stays a separate id.
  push() {
    // Unit.onMoved usually draws no JS random value. An unchanged revision
    // means every registered row already has the same native words.
    if (this.jsEpoch === randomRevision) return;
    this.jsEpoch = randomRevision;
    const p = this.phase, b = p.bridge;
    b.views(); let at = p.layout.randomStates / 8;
    for (let i = 0; i < this.entries.length; i++, at += p.layout.randomWords) {
      const entry = this.entries[i], source = entry.binding;
      const random = source.random;
      // RandomStreams.reset keeps the callback but clears its generator. Only
      // that between-battle change needs to hash the stream's name again.
      if (source.streams && entry.seed !== source.streams.seed) {
        entry.seed = source.streams.seed;
        entry.initial = source.streams.initialState(source.name);
      }
      // Seed zero is a valid live generator. Presence follows the object,
      // while an absent lazy name starts at its mixed seed without insertion.
      entry.present = Boolean(random);
      const state = random ? random.state : entry.initial;
      // A preceding pull already retained the native words. Most callbacks
      // touch no generator; only JS draws/reset need another import write.
      if (entry.row.state !== state || entry.row.present !== entry.present) {
        entry.row.state = state;
        entry.row.present = entry.present;
        putF3RandomState(b, entry.row, b.f, at, b.heap);
        p.counters.inputFields += p.layout.randomWords;
      }
    }
  }

  pull() {
    const p = this.phase, b = p.bridge;
    b.views();
    // Most Unit.onMoved imports follow no native draw. A single epoch word
    // replaces the full stream scan while preserving first-use ordering when
    // weapons, drops or hulls actually advance a registered generator.
    const epoch = b.f[p.layout.randomEpoch / 8 + F3RandomEpochLayout.revision];
    p.counters.outputFields++;
    if (epoch === this.nativeEpoch) return;
    this.nativeEpoch = epoch;
    let at = p.layout.randomStates / 8, materialize = false;
    for (let i = 0; i < this.entries.length; i++, at += p.layout.randomWords) {
      const entry = this.entries[i];
      syncF3RandomState(b, b.f, at, 0, entry.row);
      if (entry.present) entry.binding.random.state = entry.row.state;
      else if (entry.row.present) materialize = true;
    }
    // Read all use stamps before inserting any name. A first draw can come
    // from a later binding, for example grenade spin before gameplay waver.
    // Several lazy names can be drawn in one native span. Materialize them
    // in their first-use sequence, not their construction binding order.
    while (materialize) {
      let next = null;
      for (let i = 0; i < this.entries.length; i++) {
        const entry = this.entries[i];
        if (!entry.present && entry.row.present && (!next || entry.row.sequence < next.row.sequence)) next = entry;
      }
      if (!next) break;
      next.binding.streams.get(next.binding.name).state = next.row.state;
      next.present = true;
    }
    this.jsEpoch = randomRevision;
    // Existing generators were updated during the first pass. Newly used
    // names above still materialize in native first-draw order, independently
    // of registration order, preserving the world's random-state digest.
    p.counters.outputFields += this.entries.length * p.layout.randomWords;
  }
}
