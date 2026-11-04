import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { createReplaySim, createReplayBridge } from '../src/runtime/replay-sim.js';
import { replayTicks } from '../src/runtime/reconcile.js';
import { replayCases, replayOrdinaryCases, replayMutationCases, replayObservableCases, replayBoundaryCases, replayOpaqueCases, replayNestedCases, runReplayCase } from './fixtures/prediction-replay-cases.js';

const scenario = name => replayCases().find(c => c.name === name);
const entry = (frame, key) => frame.command.entries.find(([name]) => name === key)?.[1];
const numeric = traces => traces.filter(e => typeof e.op === 'number');
const command = (tick, sequence = tick) => ({ tick, sequence, generation: 1, down: ['KeyW'], actions: [{ sequence }], look: [[sequence, -sequence]] });
function twin(c, native) {
  const actual = runReplayCase(c, native), expected = runReplayCase(c);
  assert.deepEqual(actual.result, expected.result, c.name);
  if (c.native) assert.ok(actual.calls > 0, `${c.name} delegates`);
  assert.equal(native.activeSessions, 0, `${c.name} releases all owners`);
  assert.deepEqual(native.handles, []); return actual.result;
}
function caught(fn) {
  try { return { thrown: false, value: fn() }; } catch (value) { return { thrown: true, value }; }
}

test('native replay owns begin, per-frame assembly and advance while preserving opaque aliases', async () => {
  const traces = [], native = createReplaySim(await loadSim(), { trace: e => { traces[traces.length] = e; } });
  try {
    for (const c of replayOrdinaryCases()) {
      const at = traces.length, result = twin(c, native), events = traces.slice(at), ops = numeric(events).map(e => e.op);
      assert.ok(ops.includes(0), `${c.name} native begin`);
      assert.equal(events.filter(e => e.op === 'create').length, 1); assert.equal(events.filter(e => e.op === 'drop').length, 1);
      if (c.noFrames) assert.ok(!ops.includes(1) && !ops.includes(2));
      else { assert.equal(ops.filter(op => op === 1).length, result.frames.length); assert.equal(ops.filter(op => op === 2).length, result.frames.length); }
    }
    const blocked = twin(scenario('ordinary:future-foreign-command-blocks-later-life'), native);
    assert.equal(blocked.returned, 5); assert.equal(blocked.frames.length, 5);
    assert.equal(entry(blocked.frames[4], 'any'), false);
    const fractional = twin(scenario('ordinary:fractional-endpoint-copies-view-without-frame'), native);
    assert.equal(fractional.returned, 10.75); assert.deepEqual(fractional.events, ['view', 'view']);
    const noLife = twin(scenario('ordinary:no-life-ignores-view-and-callback'), native);
    assert.ok(Object.is(noLife.returned, -0)); assert.deepEqual(noLife.events, []);
  } finally { native.dispose(); }
});

test('callback mutation retains the old endpoint and switches once at the live cursor', async () => {
  const traces = [], native = createReplaySim(await loadSim(), { trace: e => { traces[traces.length] = e; } });
  try {
    for (const c of replayMutationCases()) twin(c, native);
    const at = traces.length, late = twin(scenario('mutation:late-accessor'), native), ops = numeric(traces.slice(at));
    assert.equal(late.returned, 7); assert.equal(late.frames.length, 7); assert.equal(late.final.viewReads, 2);
    assert.equal(ops.filter(e => e.op === 0).length, 1); assert.equal(ops.filter(e => e.op === 1).length, 1);
    assert.equal(ops.filter(e => e.op === 2).length, 1, 'the first callback commits before the one-way handoff');
    assert.deepEqual(late.events.filter(e => Array.isArray(e) && e[0] === 'apply').map(e => e[1]), [1, 2, 3, 4, 5, 6, 7]);
    const beyond = twin(scenario('mutation:append-beyond'), native);
    assert.equal(beyond.returned, 7); assert.equal(beyond.frames.length, 7);
    const aliases = twin(scenario('mutation:held-samples-and-view-object-aliases'), native);
    assert.ok(aliases.events.some(e => Array.isArray(e) && e[0] === 'bindings' && e.slice(1).every(Boolean)));
    assert.equal(aliases.final.freshActions, 3); assert.equal(aliases.final.freshLook, 3);
    assert.equal(entry(aliases.retained[0], 'down').ref, 'replacement-down');
    assert.equal(entry(aliases.retained[1], 'down').ref, 'shared-down');
    assert.equal(entry(aliases.retained[0], 'gamepad').ref, 'replacement-gamepad');
    assert.equal(entry(aliases.retained[1], 'gamepad').ref, 'shared-context');
    for (const name of ['mutation:installed-view-setter', 'mutation:installed-array-push']) {
      const start = traces.length; twin(scenario(name), native);
      assert.equal(numeric(traces.slice(start)).filter(e => e.op === 1).length, 1, 'changed output operations keep the remaining original loop');
    }
  } finally { native.dispose(); }
});

test('original scans, getters, iterator closure and bounded unusual ticks retain their exact order', async () => {
  const traces = [], native = createReplaySim(await loadSim(), { trace: e => { traces[traces.length] = e; } });
  try {
    for (const c of [...replayObservableCases(), ...replayBoundaryCases()]) twin(c, native);
    const scans = twin(scenario('observable:different-second-scan'), native);
    assert.equal(scans.returned, 0); assert.equal(scans.frames.length, 0); assert.ok(!scans.events.includes('view'));
    assert.equal(scans.events.filter(e => Array.isArray(e) && e[0] === 'iterator').length, 2);
    const slots = twin(scenario('observable:tick-and-slot-double-reads'), native);
    assert.equal(slots.returned, 2); assert.equal(slots.frames.length, 2);
    assert.deepEqual(slots.events.slice(0, 7), [['slot', 1], ['tick', 1], ['tick', 2], ['slot', 2], ['slot', 3], ['tick', 3], ['apply', 1, true]]);
    const view = twin(scenario('observable:double-view-reads-presence-and-insertion-order'), native);
    assert.ok(view.frames[0].keys.includes('aspect')); assert.equal(entry(view.frames[0], 'aspect'), undefined);
    assert.deepEqual(view.frames[1].keys.slice(7), ['aspect', 'gamepad', 'mouseScale', 'invertMouse']);
    assert.equal(entry(view.frames[1], 'mouseScale'), undefined);
    for (const name of ['observable:indexed-prototype-array', 'observable:indexed-prototype-object']) {
      const at = traces.length, result = twin(scenario(name), native);
      assert.ok(result.events.some(e => Array.isArray(e) && e[0] === 'indexed-setter'));
      assert.equal(traces.length, at, 'entry eligibility performs no observable private array writes');
    }
    const actionThrow = twin(scenario('observable:action-body-throw-closes-iterator-with-original-value'), native);
    assert.ok(actionThrow.thrown && actionThrow.exception.same);
    assert.deepEqual(actionThrow.events, ['actions-iterator', 'actions-next', 'push-throw', 'actions-return']);
    for (const c of replayBoundaryCases().filter(c => /from-(Infinity|-Infinity|9007199254740992)$/.test(c.name))) {
      const at = traces.length, result = twin(c, native);
      assert.equal(result.frames.length, 3); assert.ok(result.thrown && result.exception.same);
      assert.equal(numeric(traces.slice(at)).length, 0, 'the original nonadvancing loop throws through its callback without a gameplay clamp');
    }
    for (const c of replayBoundaryCases().filter(c => c.name.startsWith('boundary:throw-'))) {
      const at = traces.length, result = twin(c, native), ops = numeric(traces.slice(at));
      assert.ok(result.thrown && result.exception.same); assert.equal(ops.filter(e => e.op === 1).length, 1);
      assert.equal(ops.filter(e => e.op === 2).length, 0, 'a thrown apply skips Advance');
    }
  } finally { native.dispose(); }
});

test('opaque values preserve signed zero and NaN payload bits through each arena and output container', async () => {
  const trace = [], native = createReplaySim(await loadSim(), { trace: e => trace.push(e) });
  try {
    for (const c of replayOpaqueCases()) {
      const at = trace.length, result = twin(c, native), frames = numeric(trace.slice(at)).filter(e => e.op === 1).length;
      const pattern = BigInt(`0x${c.name.split('-').at(-1)}`), bytes = Buffer.alloc(8); bytes.writeBigUInt64LE(pattern);
      assert.equal(entry(result.frames[0], 'down').bits, bytes.toString('hex'), c.name);
      for (const key of ['mouseScale', 'aspect', 'gamepad', 'flightSettings']) assert.equal(entry(result.frames[0], key).bits, bytes.toString('hex'), `${c.name} ${key}`);
      assert.equal(frames, c.name.startsWith('opaque:typed-') ? 1 : result.frames.length);
    }
  } finally { native.dispose(); }
});

test('nested owners share one bridge safely across trace mutation and memory growth', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge), events = [], live = new Set();
  let nested = false, grew = false, native;
  native = createReplayBridge(bridge, { trace(event) {
    events.push({ session: event.session, op: event.op });
    if (event.op === 'create') { assert.ok(!live.has(event.session)); live.add(event.session); }
    if (event.op === 'drop') assert.ok(live.delete(event.session));
    event.input.fill(-123); event.output.fill(999);
    if (event.op !== 1 || nested) return;
    nested = true;
    try {
      const inner = [];
      const result = replayTicks([command(1, 9)], 0, 1, [], (tick, c) => inner.push([tick, c.actions[0].sequence]), {}, { native });
      assert.equal(result, 1); assert.deepEqual(inner, [[1, 9]]);
      wire.encodeWordDelta(new Uint8Array([1, 2, 3, 4]));
      if (!grew) { instance.exports.memory.grow(1); grew = true; }
    } finally { nested = false; }
  } });
  try {
    for (const c of replayNestedCases()) twin(c, native);
    assert.ok(grew); assert.equal(live.size, 0); assert.ok(new Set(events.filter(e => e.op === 'create').map(e => e.session)).size > 2);
  } finally { native.dispose(); }
});

test('disposal during callbacks defers active owners and rejects new entries without losing outputs', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  for (const phase of ['create', 'frame', 'apply']) {
    const ticks = [], events = []; let native, disposed = false;
    const close = () => {
      if (disposed) return; disposed = true;
      assert.equal(native.activeSessions, 1); const held = native.handles; assert.equal(held.length, 1); held.fill(0);
      assert.ok(native.handles[0] > 0); native.dispose(); assert.equal(native.activeSessions, 1);
      assert.throws(() => replayTicks([command(1)], 0, 1, [], () => {}, {}, { native }), /disposed/);
    };
    native = createReplayBridge(bridge, { trace(e) { events.push(e.op); if (phase === 'create' && e.op === 'create' || phase === 'frame' && e.op === 1) close(); } });
    const result = replayTicks([command(2)], 0, 1, [], tick => { ticks.push(tick); if (phase === 'apply') close(); }, {}, { native });
    assert.equal(result, 2); assert.deepEqual(ticks, [1, 2]); assert.equal(native.activeSessions, 0); assert.deepEqual(native.handles, []);
    assert.equal(events.filter(e => e === 'create').length, 1); assert.equal(events.filter(e => e === 'drop').length, 1); native.dispose();
  }
  let warm = 0;
  for (let i = 0; i < 48; i++) {
    const native = createReplayBridge(bridge); assert.equal(replayTicks([command(2)], 0, 1, [], () => {}, {}, { native }), 2);
    assert.equal(native.activeSessions, 0); assert.deepEqual(native.handles, []); native.dispose(); native.dispose();
    if (i === 4) warm = instance.exports.memory.buffer.byteLength;
  }
  assert.ok(instance.exports.memory.buffer.byteLength <= warm + 2 * 65536);
});

test('create, frame, apply and drop trace failures free owners and preserve arbitrary original throws', async () => {
  const instance = await loadSim();
  for (const phase of ['create', 'frame', 'apply', 'drop']) for (const thrown of [undefined, null, Symbol('trace-stop'), { reason: 'trace-stop' }]) {
    const cleanup = Symbol('cleanup-stop'), events = []; let applied = 0;
    const native = createReplaySim(instance, { trace(e) {
      events.push(e.op);
      if (phase === 'create' && e.op === 'create' || phase === 'frame' && e.op === 1 || phase === 'drop' && e.op === 'drop') throw thrown;
      if (e.op === 'drop') throw cleanup;
    } });
    const result = caught(() => replayTicks([command(2)], 0, 1, [], () => { applied++; if (phase === 'apply') throw thrown; }, {}, { native }));
    assert.ok(result.thrown); assert.equal(result.value, thrown, phase);
    assert.equal(applied, phase === 'drop' ? 2 : phase === 'apply' ? 1 : 0);
    assert.equal(native.activeSessions, 0); assert.deepEqual(native.handles, []);
    assert.equal(events.filter(e => e === 'create').length, 1); assert.equal(events.filter(e => e === 'drop').length, 1);
    native.dispose();
  }
});
