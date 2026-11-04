// Run the existing JS/WASM differential fixtures unchanged, tracing their
// successful raw operations for the native context consumer tests. Callback
// failures are still checked by those fixtures; native failure/reentry tests
// exercise their typed partial-reply and cleanup contract separately.
import assert from 'node:assert/strict';
import { readFileSync, writeSync } from 'node:fs';
import { bridgeFor, Heap } from '../src/sim/runtime.js';
import { IMPORT_TABLE } from '../src/sim/gen/index.js';
import * as worldRecords from '../src/sim/gen/world.js';

const names = new Set(['input_actions_run', 'input_fire_run', 'input_mailbox_run', 'input_queue_run',
  'input_controls_run', 'input_controls_axis', 'world_events_run', 'world_local_run', 'world_step_run', 'world_network_run']);
const stateRecords = { world_step_run: ['WorldStepState', 23], world_network_run: ['WorldNetworkState', 14] };
const counts = Object.fromEntries([...names].map(n => [n, 0]));
const bridges = new WeakMap(); let next = 0, failures = 0, tests = 0;
const hex = values => { const bytes = Buffer.alloc(values.length * 8); values.forEach((v, i) => bytes.writeDoubleLE(v, i * 8)); return bytes.toString('hex'); };
const pipeWait = new Int32Array(new SharedArrayBuffer(4));
const emit = value => {
  const bytes = Buffer.from(JSON.stringify(value) + '\n');
  // Pipes may accept only a prefix of a large mailbox/queue record.
  for (let offset = 0; offset < bytes.length;) {
    try { offset += writeSync(1, bytes, offset, bytes.length - offset); }
    catch (error) {
      if (error.code !== 'EAGAIN') throw error;
      Atomics.wait(pipeWait, 0, 0, 1);
    }
  }
};
function block(b, ptr) {
  b.views(); const at = ptr / 8, n = b.f[at + 1], heap = at + b.f[at + 2];
  return { row: hex(Array.from(b.f.slice(at + 5, at + 5 + n))), heap: hex(Array.from(b.f.slice(heap, heap + b.f[at + 3]))) };
}
function encode(b, descriptor, value) {
  const row = Array(descriptor.words).fill(0), heap = new Heap();
  descriptor.put(b, value, row, 0, heap);
  return { row: hex(row), heap: hex(heap.words) };
}
function instrument(instance) {
  const b = bridgeFor(instance);
  if (bridges.has(b)) return;
  const cx = next++, frames = [];
  bridges.set(b, cx);
  const x = { ...b.x };
  for (const name of names) {
    const original = x[name];
    assert.equal(typeof original, 'function', name);
    x[name] = (...args) => {
      const strings = Array.from({ length: x.sim_str_count() }, (_, i) => {
        const s = b.strings.at(i); return Array.from({ length: s.length }, (_, n) => s.charCodeAt(n));
      });
      const frame = { cx, name, strings, input: block(b, x.sim_calls()), calls: [] };
      frames.push(frame);
      try {
        const value = original(...args);
        if (name === 'input_controls_axis') frame.scalar = hex([value]);
        else if (stateRecords[name]) {
          assert.ok(frame.state, 'a completed world operation must reply at least once');
          if (name === 'world_network_run') frame.scalar = hex([value]);
        } else frame.output = block(b, value);
        counts[name]++; emit(frame);
        return value;
      } catch (error) { failures++; throw error; }
      finally { assert.equal(frames.pop(), frame); }
    };
  }
  b.x = x;
  const register = b.register.bind(b);
  b.register = handlers => register(Object.fromEntries(Object.entries(handlers).map(([name, handler]) => {
    const imp = IMPORT_TABLE.find(i => i?.name === name);
    return [name, args => {
      const frame = frames.at(-1);
      if (!frame) return handler(args);
      const call = { code: imp.code, input: block(b, x.sim_import_args()) };
      frame.calls.push(call);
      const value = handler(args);
      call.output = encode(b, imp.result, value);
      const state = stateRecords[frame.name];
      if (state) frame.state = encode(b, { words: state[1], put: worldRecords[`put${state[0]}`] }, value.state);
      return value;
    }];
  })));
}

// The test files own their JS references, mutations and assertions. Evaluate
// their fixture definitions with only the factories instrumented; no core or
// runtime module is rewritten and no test bodies are duplicated here.
for (const file of ['input-sim.test.js', 'command-controls-sim.test.js', 'world-events-sim.test.js',
  'world-local-sim.test.js', 'world-step-sim.test.js', 'world-network-sim.test.js']) {
  const url = new URL(`../tests/${file}`, import.meta.url), tasks = [], env = {};
  let source = readFileSync(url, 'utf8');
  const imports = [...source.matchAll(/^import (.+?) from ['"](.+?)['"];$/gm)];
  for (const [line, binding, path] of imports) {
    const module = await import(path.startsWith('.') ? new URL(path, url).href : path);
    if (path === 'node:test') env.test = (name, run) => tasks.push([name, run]);
    else if (binding.startsWith('* as ')) env[binding.slice(5)] = module;
    else if (binding.startsWith('{')) for (const spec of binding.slice(1, -1).split(',')) {
      const [name, alias = name] = spec.trim().split(/\s+as\s+/);
      const original = module[name];
      env[alias] = ['createInputSim', 'createWorldSim'].includes(name)
        ? (instance, ...args) => { instrument(instance); return original(instance, ...args); } : original;
    } else env[binding] = module.default;
    source = source.replace(line, '');
  }
  source = source.replaceAll('import.meta.url', JSON.stringify(url.href));
  Function(...Object.keys(env), source)(...Object.values(env));
  assert.ok(tasks.length > 0, file);
  for (const [name, run] of tasks) { await run(); tests++; }
}
assert.ok(Object.values(counts).every(n => n > 0));
assert.ok(next > 2 && failures > 0 && tests > 20);
const summary = { summary: true, contexts: next, tests, callbackFailuresCheckedByJs: failures, counts };
emit(summary); console.error(JSON.stringify(summary));
