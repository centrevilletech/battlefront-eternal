// Record every AnimatedProp.step in the existing synthetic conformance
// tests, then replay the file with replay_animated. The test fixtures stay
// independent of the native implementation and retain all their asserts.
import fs from 'node:fs';
import path from 'node:path';
import { register } from 'node:module';
import { Handle, MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
register('./sim-capture/hooks.mjs', import.meta.url);
const { AnimatedProp } = await import('../src/props.js');
const { observeSounds } = await import('../src/audio/events.js');
const { animatedOwn, animatedSettings, animatedBody } = await import('./sim-capture/animated.mjs');
const args = process.argv.slice(2), at = args.indexOf('--out'), out = at < 0 ? null : args[at + 1];
if (!out) throw Error('animated-twin requires --out DIRECTORY');
const module = args.includes('--module') ? await import('../src/props-sim.js') : null;
const instances = module ? await Promise.all(Array.from({ length: 32 }, async () => (await import('../src/sim/module.js')).loadSim())) : [];
const native = new WeakMap();
fs.mkdirSync(out, { recursive: true });
const file = fs.openSync(path.join(out, 'animated.bfecap'), 'w'); fs.writeSync(file, MAGIC);
const identities = new Map(), defs = new Map(), pending = [], wrapped = new WeakSet();
const writer = new Writer({ identify(value) {
  if (identities.has(value)) return identities.get(value);
  if (value?.base === 'animation' && typeof value.class === 'string') {
    if (!defs.has(value)) { defs.set(value, `${value.class}:${defs.size}`); pending.push(value); }
    return new Handle('def', defs.get(value));
  }
} });
const write = (type, tick, name, value) => {
  writer.record(type, tick, name, value);
  while (pending.length) { const def = pending.shift(); writer.whole.add(def); writer.record(RECORD.def, tick, defs.get(def), def); writer.whole.clear(); }
  fs.writeSync(file, writer.bytes.subarray(0, writer.length)); writer.length = 0;
};
write(RECORD.header, 0, 'capture', { format: 1, v8: process.versions.v8, node: process.version, battle: { synthetic: true } });
const game = { live: { animated: [] } };
let current = null, depth = 0, frames = 0, callbacks = 0;
function callback(name, args, body, fn) {
  if (!current || depth) return fn();
  const { p, tick, id } = current;
  const before = body ? animatedBody(body, p, game) : null, propBefore = body ? animatedOwn(p, game) : null;
  write(RECORD.frame, tick, 'AnimatedProp.step', { k: 'call', f: id, n: `animated.${name}`, a: args, d: [] });
  callbacks++; depth++; let result;
  try { result = fn(); return result; }
  finally {
    depth--;
    write(RECORD.frame, tick, 'AnimatedProp.step', { k: 'ret', f: id, n: `animated.${name}`, r: result, o: args.map(a => a && typeof a === 'object' ? a : undefined), i: [],
      x: body ? { before, after: animatedBody(body, p, game), propBefore, propAfter: animatedOwn(p, game) } : {} });
  }
}
globalThis.__bfePropsRider = (...a) => callback('rider', a, null, () => {});
observeSounds((...a) => callback('sound', a, null, () => {}));
function wrap(unit) {
  if (wrapped.has(unit)) return;
  wrapped.add(unit); identities.set(unit, new Handle('unit', identities.size));
  for (const method of ['letGo', 'remove']) if (typeof unit[method] === 'function') {
    const base = unit[method]; unit[method] = function (...a) { return callback(method, a, unit, () => base.apply(this, a)); };
  }
  if (unit.combat?.damage) {
    const base = unit.combat.damage;
    unit.combat.damage = function (...a) { return callback('damage', a, unit, () => base.apply(this, a)); };
  }
}
const step = AnimatedProp.prototype.step;
AnimatedProp.prototype.step = function (...args) {
  const p = this;
  if (!game.live.animated.includes(p)) { game.live.animated.push(p); identities.set(p, new Handle('animated', game.live.animated.indexOf(p))); }
  if (module && !native.has(p)) {
    const instance = instances.pop(); if (!instance) throw Error('animated twin instance pool exhausted');
    module.attachAnimatedPropsSim(instance, { animated: [p] });
    native.set(p, p.step); delete p.step;
  }
  const order = args[2] ?? [], bodies = [...new Set([...order, ...(p.victim ? [p.victim.unit] : [])])];
  bodies.forEach(wrap);
  const state = () => ({ prop: animatedOwn(p, game), settings: animatedSettings(p), bodies: bodies.map(u => animatedBody(u, p, game)),
    order: order.map(u => bodies.indexOf(u)), positions: (args[1] ?? []).map(p => [p.x, p.y, p.z]),
    positionOwners: (args[1] ?? []).map(position => bodies.findIndex(u => u.position === position)) });
  current = { p, tick: ++frames, id: frames };
  write(RECORD.frame, frames, 'AnimatedProp.step', { k: 'enter', f: frames, p: null, t: p, a: args, s: state() });
  const result = (native.get(p) ?? step).apply(p, args);
  write(RECORD.frame, frames, 'AnimatedProp.step', { k: 'exit', f: frames, d: [], r: result, s: state() });
  current = null; return result;
};
process.once('exit', () => { fs.closeSync(file); console.error(JSON.stringify({ animatedTwin: { frames, callbacks } })); });
await import('../tests/animated-props.test.js');
