// The battle's state through the entity store, against a real battle
// (docs/sim-store.md), as a tools/world-hash.mjs plugin: the battle runs as
// world-hash runs it, and on sampled ticks, after the step, the battle's
// rules and Characters, the command posts and the command vehicles' posts,
// and the Classic spawn manager (rust/sim/schema/battle.mjs):
//   - hold no field their schema does not list (state a replay would miss);
//   - go into the store of an instance of the simulation's module of its
//     own (every field) and come back the same, bit for bit (Object.is on
//     every number, each reference the very object, every Map's and
//     object's keys in their order).
//
//   BFE_CONTENT_DIR=<game data> node tools/world-hash.mjs --map nab2 --side rep \
//     --plugin tools/battle-store-check.mjs [--store-every 7] [--store-report FILE] [--store-strict]
//
// It reads plain fields and calls no game code, so the battle's
// fingerprints are the same with it and without it.
import fs from 'node:fs';
import { loadSim } from '../src/sim/module.js';
import { SimBridge, undeclared } from '../src/sim/runtime.js';
import { KIND_TABLE, KIND_BATTLE, KIND_CHARACTER, KIND_COMMAND_POSTS, KIND_POST, KIND_MOBILE_POSTS, KIND_MOBILE_POST, KIND_SPAWNING } from '../src/sim/gen/index.js';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const every = Number(opt('store-every', 7));
const reportFile = opt('store-report', null), strict = argv.includes('--store-strict');
if (!(Number.isInteger(every) && every >= 1)) throw Error(`--store-every ${opt('store-every')}`);

const instance = await loadSim();
let game = null, label = '';
const stats = { ticks: 0, records: {}, words: 0 };
const failures = [];
const unlisted = new Map();
const fail = (what) => { if (failures.length < 20) failures.push(`${label} ${what}`); };

// How the unions' kinds are told apart here (Node runs the sources as they
// are, so class names hold).
const named = (name) => (o) => o?.constructor?.name === name;
const tests = () => ({ post: named('Post'), mobilePost: named('MobilePost'), vehicle: named('Vehicle'), building: (o) => typeof o?.health === 'number' && 'desc' in o, unit: (o) => typeof o.id === 'number' && game.units.byId(o.id) === o });

export function battle({ game: g, options, index }) {
  game = g;
  label = `${options.map} ${options.side} seed ${options.seed}${index ? ` battle ${index + 1}` : ''}`;
}

// The battle's objects by kind, as the store lists them.
function state() {
  const mobile = game.posts.mobile ?? { posts: [] };
  const lists = [[KIND_CHARACTER, game.battle.characters], [KIND_POST, game.posts.posts], [KIND_MOBILE_POST, mobile.posts]];
  const singletons = [[KIND_BATTLE, game.battle], [KIND_COMMAND_POSTS, game.posts], [KIND_MOBILE_POSTS, mobile]];
  if (game.ai?.spawning) singletons.push([KIND_SPAWNING, game.ai.spawning]);
  return { lists, singletons };
}

export function ticked(tick) {
  if (tick % every) return;
  stats.ticks++;
  const { lists, singletons } = state();
  for (const [kind, objects] of [...lists, ...singletons.map(([k, o]) => [k, [o]])]) {
    const keys = KIND_TABLE[kind].keys;
    for (const o of objects) for (const key of undeclared(o, keys)) {
      const name = `${KIND_TABLE[kind].name}.${key}`;
      unlisted.set(name, (unlisted.get(name) ?? 0) + 1);
    }
  }
  // A store of its own each time (its identities hold the objects they
  // are given), every field in and back out.
  instance.exports.sim_reset();
  const b = new SimBridge(instance, { tests: tests(), replace: true });
  try {
    // The singletons first: the kinds homed in their lists are listed in
    // those lists' order.
    for (const [kind, o] of singletons) b.write(kind, [o], true);
    for (const [kind, objects] of lists) b.write(kind, objects, true);
    for (const [kind, objects] of lists) {
      const k = KIND_TABLE[kind], back = b.read(kind);
      if (back.length !== objects.length) { fail(`tick ${tick}: ${k.name}: ${back.length} back of ${objects.length}`); continue; }
      back.forEach(({ slot, gen, object }, i) => {
        if (b.refs[kind].at(slot, gen) !== objects[i]) fail(`tick ${tick}: ${k.name}[${i}] is not in its place`);
        const d = k.same(objects[i], object);
        if (d) fail(`tick ${tick}: ${k.name}[${i}]${d}`);
      });
      stats.records[k.name] = (stats.records[k.name] ?? 0) + objects.length;
      stats.words += objects.length * k.fullStride;
    }
    for (const [kind, o] of singletons) {
      const k = KIND_TABLE[kind];
      const [{ object }] = b.read(kind, () => ({}));
      const d = k.same(o, object);
      if (d) fail(`tick ${tick}: ${k.name}${d}`);
      stats.records[k.name] = (stats.records[k.name] ?? 0) + 1;
      stats.words += k.fullStride;
    }
  } catch (error) {
    fail(`tick ${tick}: ${error.stack}`);
  }
}

export function finish() {
  const kinds = Object.entries(stats.records).map(([k, n]) => `${n} ${k}`).join(', ');
  console.error(`battle-store-check ${label}: ${stats.ticks} ticks sampled, ${kinds}, ${stats.words} row words through the store, ${failures.length} differences`);
  if (unlisted.size) console.error(`battle-store-check ${label}: fields no schema lists: ${[...unlisted].map(([k, n]) => `${k} (${n})`).join(', ')}`);
  for (const f of failures) console.error(`battle-store-check ${f}`);
  if (reportFile) fs.writeFileSync(reportFile, JSON.stringify({ label, ...stats, differences: failures, unlisted: Object.fromEntries(unlisted) }, null, 1));
  if (strict && (failures.length || unlisted.size)) throw Error(`battle-store-check: ${failures.length} differences, ${unlisted.size} fields no schema lists`);
}
