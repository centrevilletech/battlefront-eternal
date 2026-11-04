// Exact store/clock records from two independent WASM instances. The native
// context test consumes this stream and checks each callback before replying.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { putWorldClockInput, putWorldClockArgs, putWorldClockReply, putWorldClockState } from '../src/sim/gen/world.js';
import { putEcho } from '../src/sim/gen/store.js';
import { IMPORT_TABLE, COMMAND_TABLE, KIND_BOLT } from '../src/sim/gen/index.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  values.forEach((v, i) => bytes.writeDoubleLE(v, i * 8));
  return bytes.toString('hex');
};
const record = (b, put, n, value) => { const row = Array(n).fill(0); put(b, value, row, 0, {}); return words(row); };
const code = name => IMPORT_TABLE.find(x => x?.name === name).code;
const command = name => COMMAND_TABLE.find(x => x?.name === name).code;
const emit = async value => { if (!process.stdout.write(JSON.stringify(value) + '\n')) await once(process.stdout, 'drain'); };
function block(b, ptr) {
  b.views(); const at = ptr / 8;
  return words(Array.from(b.f.slice(at + 5, at + b.f[at + 2] + b.f[at + 3])));
}
const state = source => ({ ...source });
function run(source, op, seconds, limited, fail) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'performance');
  let time = 0, caught = null, value;
  const error = fail === null ? null : Error(fail);
  Object.defineProperty(globalThis, 'performance', { configurable: true, value: { now: () => time } });
  try {
    if (op !== 'advance') source[op]();
    else value = source.advance(seconds, (dt, tick) => {
      assert.equal(dt, source.dt);
      time += [0, 2, 18, 50, 81.125][tick % 5];
      if (error && tick === 2) { source.maxDebt = 4; throw error; }
    }, limited);
  } catch (e) { caught = e; if (error) assert.equal(e, error); }
  finally { Object.defineProperty(globalThis, 'performance', descriptor); }
  return { value, error: caught?.message ?? null, state: state(source) };
}
const lanes = [];
for (let cx = 0; cx < 2; cx++) {
  const instance = await loadSim(), b = bridgeFor(instance), world = createWorldSim(instance);
  const strings = [cx ? 'context B\udc00' : 'context A\ud800', 'invalid elapsed time', 'tick failed', ''];
  strings.forEach(s => b.strings.id(s));
  const lane = { cx, b, world, strings, active: null };
  const original = b.handlers.worldClock;
  b.handlers.worldClock = args => {
    const entry = { code: code('worldClock'), args: record(b, putWorldClockArgs, 16, args) };
    lane.active.calls.push(entry);
    try {
      const result = original(args);
      entry.reply = record(b, putWorldClockReply, 14, result);
      return result;
    } catch (error) {
      entry.error = error.message;
      entry.partial = record(b, putWorldClockReply, 14, { state: lane.active.source, time: 0 });
      throw error;
    }
  };
  b.register({ ask(args) {
    const entry = { code: code('ask'), args: record(b, putEcho, 8, args) };
    lane.active.calls.push(entry);
    // Copy every argument before growth or nested commands reuse mirrors.
    const out = { ...args, at: args.at.clone(), value: args.value + (cx ? 20 : 10) };
    b.x.memory.grow(1);
    entry.reply = record(b, putEcho, 8, out);
    return out;
  } });
  lanes.push(lane);
  await emit({ kind: 'init', cx, strings: strings.map(s => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i))) });
}
assert.notEqual(lanes[0].b.memory, lanes[1].b.memory);
async function clockCase(lane, source, js, op, seconds, limited, fail = null) {
  const { cx, b } = lane, calls = [];
  const input = record(b, putWorldClockInput, 16, { op, state: source, seconds, limited });
  const expected = run(js, op, seconds, limited, fail);
  lane.active = { calls, source };
  const actual = run(source, op, seconds, limited, fail);
  assert.deepEqual(actual, expected);
  const failed = calls.some(c => Object.hasOwn(c, 'error'));
  await emit({ kind: 'clock', cx, input, calls, output: failed ? null : block(b, b.x.sim_calls()),
    state: record(b, putWorldClockState, 13, source), error: actual.error });
}
let clocks = 0, commands = 0;
for (const options of [{ hz: 30, maxSteps: 3 }, { hz: 60, budget: 0.035, maxDebt: 8 }, { hz: 144, budget: 0.002, maxDebt: 0 }]) {
  const sources = lanes.map(l => l.world.clock(new FixedClock(options))), originals = lanes.map(() => new FixedClock(options));
  for (let frame = 0; frame < 240; frame++) for (const lane of lanes) {
    const op = frame === 90 ? 'rebase' : frame === 170 ? 'reset' : 'advance';
    await clockCase(lane, sources[lane.cx], originals[lane.cx], op, [1 / 240, 0.075, 0, 1 / 144, 1 / 60][(frame + lane.cx) % 5], frame % 3 !== 0);
    clocks++;
    if (frame % 20 === 0) {
      const { b, cx } = lane;
      for (const name of ['echo', 'relay']) {
        const args = { value: frame ? 0.1 + 0.2 : -0, at: { x: -0, y: Infinity, z: Number.MIN_VALUE }, count: 4294967295, note: lane.strings[0], events: 2 };
        const input = record(b, putEcho, 8, args), calls = [];
        lane.active = { calls }; const result = b.command(name, args);
        assert.equal(result.note, lane.strings[0]);
        const output = block(b, b.x.sim_calls());
        b.views(); const at = b.x.sim_events() / 8;
        const events = words(Array.from(b.f.slice(at, at + b.f[at] + 1)));
        await emit({ kind: 'command', cx, code: command(name), input, calls, output, events });
        b.x.sim_events_clear(); commands++;
      }
    }
  }
}
for (const lane of lanes) {
  for (const fail of ['tick failed', '']) {
    const source = lane.world.clock(new FixedClock()), js = new FixedClock();
    await clockCase(lane, source, js, 'advance', 0.05, false, fail);
    await clockCase(lane, source, js, 'advance', 0, false);
    clocks += 2;
  }
  for (const seconds of [NaN, Infinity, -Infinity, -1, -0, Number.MIN_VALUE]) {
    await clockCase(lane, lane.world.clock(new FixedClock()), new FixedClock(), 'advance', seconds, false);
    clocks++;
  }
}
// The same local entity slot belongs to independent stores; retiring it in
// one instance cannot retire or renumber the other instance's row.
for (const lane of lanes) lane.b.command('fill', { kind: KIND_BOLT, count: 1 });
for (const lane of lanes) { lane.b.views(); assert.equal(lane.b.f[lane.b.x.sim_export(KIND_BOLT, 1) / 8], 1); }
const a = lanes[0].b, b = lanes[1].b;
a.views(); const start = a.x.sim_export(KIND_BOLT, 1) / 8;
assert.equal(a.x.sim_retire(KIND_BOLT, a.f[start + 5], a.f[start + 6]), 0);
b.views(); assert.equal(b.f[b.x.sim_export(KIND_BOLT, 1) / 8], 1);
console.error(JSON.stringify({ contexts: 2, clocks, commands, isolatedRows: true }));
