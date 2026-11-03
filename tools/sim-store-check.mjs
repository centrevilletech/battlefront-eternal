// The entity store against a real battle (docs/sim-store.md), as a
// tools/world-hash.mjs plugin: the battle runs exactly as world-hash runs
// it, and on sampled ticks, after the step, every round in flight (each
// bolt, beam, grenade and rocket the battle's Ordnance holds):
//   - has no field its schema does not list (state a replay would miss);
//   - goes into the store of an instance of the simulation's module of its
//     own (every field) and comes back the same, bit for bit (Object.is on
//     every number, each reference the very object);
//   - with --store-values FILE, is exported as the replay harness records
//     it (the harness's handles for what the store only refers to) and
//     written there in its format, for rust/sim/tests/replay_values.rs to
//     read into the store and write back (BFE_SIM_STORE_VALUES=FILE cargo
//     test -p sim --features replay).
//
//   BFE_CONTENT_DIR=<game data> node tools/world-hash.mjs --map nab2 --side rep \
//     --plugin tools/sim-store-check.mjs [--store-every 7] [--store-values FILE]
//
// It reads plain fields and never calls game code, so the battle's
// fingerprints are the same with it and without it. The first difference
// fails the run (finish() throws), with its path.
import fs from 'node:fs';
import { loadSim } from '../src/sim/module.js';
import { SimBridge, ExportContext, exportState, undeclared } from '../src/sim/runtime.js';
import { KIND_TABLE, KIND_BOLT, KIND_BEAM, KIND_GRENADE, KIND_ROUND, KIND_ORDNANCE } from '../src/sim/gen/index.js';
import { resolveOrdnanceOwner } from '../src/ordnance-owner.js';
import { Writer, Handle, RECORD, MAGIC } from './sim-capture/format.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const every = Number(opt('store-every', 7));
const valuesFile = opt('store-values', null);
if (!(Number.isInteger(every) && every >= 1)) throw Error(`--store-every ${opt('store-every')}`);

const instance = await loadSim();
const LISTS = [[KIND_BOLT, 'bolts'], [KIND_BEAM, 'beams'], [KIND_GRENADE, 'grenades'], [KIND_ROUND, 'rounds']];
let game = null, label = '';
const stats = { ticks: 0, records: 0, fields: 0, byKind: {} };
const failures = [];
const unlisted = new Map(); // "kind.key" -> times
const fail = (what) => { if (failures.length < 20) failures.push(`${label} ${what}`); };

// How a union's kinds are told apart here (Node runs the sources as they
// are, so class names hold; a battle wires its own constructors).
function tests() {
  const named = (name) => (o) => o.constructor?.name === name;
  return {
    unit: (o) => typeof o.id === 'number' && game.units.byId(o.id) === o,
    vehicle: named('Vehicle'),
    turret: (o) => named('Turret')(o),
    weapon: named('Weapon'),
    emitter: (o) => typeof o.viewDirection === 'function' && 'matrix' in o,
    ownerSnapshot: (o) => resolveOrdnanceOwner(o) !== o,
  };
}

// The replay harness's handles (tools/sim-capture/plugin.mjs names units,
// hulls, bodies and definitions so), and numbers by kind for the rest.
const numbered = new Map(), defKeys = new WeakMap(), defCounts = new Map();
function handle(kind, object) {
  if (kind === 'unit') return new Handle('unit', object.id);
  if (kind === 'body' && Number.isInteger(object.offset)) return new Handle('body', object.offset / 0x1000000);
  if (kind === 'vehicle') { const i = game.vehicles?.vehicles.indexOf(object) ?? -1; if (i >= 0) return new Handle('vehicle', i); }
  if (!numbered.has(kind)) numbered.set(kind, { ids: new WeakMap(), next: 0 });
  const k = numbered.get(kind);
  let id = k.ids.get(object);
  if (id === undefined) { id = k.next++; k.ids.set(object, id); }
  return new Handle(kind, kind === 'vehicle' ? `gone#${id}` : id);
}
function defHandle(table, def) {
  let key = defKeys.get(def);
  if (key === undefined) {
    const n = (defCounts.get(def.class) ?? 0) + 1;
    defCounts.set(def.class, n);
    key = n === 1 ? def.class : `${def.class}#${n}`;
    defKeys.set(def, key);
  }
  return new Handle('def', key);
}

let writer = null, fd = null;
if (valuesFile) { fd = fs.openSync(valuesFile, 'w'); fs.writeSync(fd, MAGIC); writer = new Writer(); }

export function battle({ game: g, options, index }) {
  game = g;
  label = `${options.map} ${options.side} seed ${options.seed}${index ? ` battle ${index + 1}` : ''}`;
}

export function ticked(tick) {
  if (tick % every) return;
  const o = game.ordnance;
  stats.ticks++;
  for (const [kind, list] of LISTS) {
    const keys = KIND_TABLE[kind].keys;
    for (const r of o[list]) for (const key of undeclared(r, keys)) {
      const name = `${KIND_TABLE[kind].name}.${key}`;
      unlisted.set(name, (unlisted.get(name) ?? 0) + 1);
    }
  }
  // A store of its own each time (its identities hold the objects they
  // are given), every field in and back out.
  instance.exports.sim_reset();
  const b = new SimBridge(instance, { tests: tests() });
  try {
    for (const [kind, list] of LISTS) b.write(kind, o[list], true);
    b.write(KIND_ORDNANCE, [o], true);
    for (const [kind, list] of LISTS) {
      const k = KIND_TABLE[kind], back = b.read(kind), original = o[list];
      if (back.length !== original.length) { fail(`tick ${tick}: ${list}: ${back.length} back of ${original.length}`); continue; }
      back.forEach(({ object }, i) => {
        if (b.refs[kind].at(back[i].slot, back[i].gen) !== original[i]) fail(`tick ${tick}: ${list}[${i}] is not in its place`);
        const d = k.same(original[i], object);
        if (d) fail(`tick ${tick}: ${list}[${i}]${d}`);
      });
      stats.records += original.length;
      stats.byKind[list] = (stats.byKind[list] ?? 0) + original.length;
      stats.fields += original.length * k.fullStride;
    }
    const [{ object: system }] = b.read(KIND_ORDNANCE, () => ({}));
    for (const [, list] of LISTS) if (system[list].length !== o[list].length || system[list].some((x, i) => x !== o[list][i])) fail(`tick ${tick}: ordnance.${list} came back otherwise`);
  } catch (error) {
    fail(`tick ${tick}: ${error.message}`);
  }
  if (writer) {
    const cx = new ExportContext({ tests: tests(), ref: handle, def: defHandle });
    writer.record(RECORD.frame, tick, 'ordnance', exportState('ordnance', o, cx));
    fs.writeSync(fd, writer.take());
  }
}

export function finish() {
  if (fd !== null) fs.closeSync(fd);
  const kinds = Object.entries(stats.byKind).map(([k, n]) => `${n} ${k}`).join(', ');
  console.error(`sim-store-check ${label}: ${stats.ticks} ticks sampled, ${stats.records} records (${kinds}), ${stats.fields} row words through the store`);
  if (unlisted.size) failures.unshift(`fields no schema lists: ${[...unlisted].map(([k, n]) => `${k} (${n})`).join(', ')}`);
  if (failures.length) throw Error(`sim-store-check: ${failures.length} problem(s):\n  ${failures.join('\n  ')}`);
}
