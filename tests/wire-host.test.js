import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireHostBridge, createWireStaggerBridge } from '../src/runtime/wire-host.js';
import { readFileSync } from 'node:fs';
import { WorldFieldStagger } from '../src/runtime/snapshot-records.js';
import { EntityBudget } from '../src/runtime/network-entity-budget.js';
import { encodeWordDelta, encodeWorldSection } from '../src/runtime/snapshot-codec.js';

function random(seed) { return () => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) / 4294967296; }; }
const field = (schema, id, payload) => ({ kind: 'summary', generation: 1, schema, id, payload: new TextEncoder().encode(payload) });

test('native shared baseline choices preserve greedy costs and tie order', async () => {
  const host = createWireHostBridge(bridgeFor(await loadSim()));
  const cases = JSON.parse(readFileSync(new URL('./fixtures/wire-baselines.json', import.meta.url), 'utf8'));
  for (const [n, { tick, acks, max, expected }] of cases.entries()) {
    assert.deepEqual(host.chooseSharedBaselines(tick, acks, max === 'NaN' ? NaN : max), expected, `frozen baseline ${n}`);
  }
  host.dispose();
});

test('native client cadence, acknowledgements, own baselines and budgets survive gaps', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), host = createWireHostBridge(bridge, { budget: 20000, staleAfterMs: 5000, epoch: 17 });
  const a = host.add('a', 0), b = host.add('b', 100);
  assert.equal(host.due(1), false); assert.equal(host.due(2), true); host.captured(2);
  assert.equal(host.due(3), false); assert.equal(host.due(5), true); host.captured(5);
  assert.equal(host.due(6), false); assert.equal(host.due(7), true);
  a.started = true; a.ready = true; b.started = true;
  assert.equal(host.eligible(a, 5000, {}), true);
  assert.equal(host.eligible(a, 5000.01, {}), false); assert.equal(a.metrics.staleSkips, 1);
  assert.equal(host.eligible(b, 99999, {}), false); assert.equal(b.metrics.staleSkips, 0);
  assert.deepEqual(host.staleClients(5100, 5), ['a', 'b']);
  a.started = false; assert.deepEqual(host.staleClients(5000, 5), []); a.started = true;
  host.chargeBytes(a, 100, 19999); assert.equal(host.delivery(a, 100), 2);
  host.chargeBytes(a, 101, 1); assert.equal(host.delivery(a, 101), 1); a.ownSentAt = 101;
  assert.equal(host.delivery(a, 200.999), 0); assert.equal(host.delivery(a, 201), 1); assert.equal(a.metrics.budgetSkips, 3);
  assert.equal(host.budgetUsed(a, 1100), 1); assert.equal(host.budgetUsed(a, 1101), 0);
  host.chargeBytes(a, 1500, 3); host.chargeBytes(a, 1490, 4);
  assert.equal(host.budgetUsed(a, 2495), 7); assert.equal(host.budgetUsed(a, 2500), 0);
  let oldPayload = null, oldSummaries = new Map();
  for (let tick = 6; tick <= 310; tick += 2) {
    host.captured(tick);
    const correction = tick % 6 ? field(1, 1, JSON.stringify([tick, 'own'])) : null;
    const summaries = new Map([['2:4', field(2, 4, JSON.stringify([tick % 12]))]]);
    const result = host.prepareOwn(a, { tick, baselineTick: a.baselineTick, correction, summaries });
    assert.deepEqual(result.correctionDelta, correction ? encodeWordDelta(correction.payload, oldPayload) : null);
    assert.deepEqual(result.summariesSection, encodeWorldSection(summaries, oldSummaries));
    assert.equal(host.acceptAck(a, { epoch: 18, tick }), false);
    assert.equal(host.acceptAck(b, { epoch: 17, tick }), false);
    assert.equal(host.acceptAck(a, { epoch: 17, tick }), true);
    assert.equal(host.acceptAck(a, { epoch: 17, tick }), false);
    oldPayload = correction?.payload ?? oldPayload; oldSummaries = summaries;
    if (tick === 120) instance.exports.memory.grow(2);
  }
  assert.deepEqual(host.heldAcks(a, 310), [310, 308, 306, 304, 302, 300, 298, 296]);
  assert.deepEqual(host.heldAcks(a, 438), []);
  host.resync(a); assert.equal(a.baselineTick, 0); host.remove('a'); host.remove('b'); host.dispose();
  assert.throws(() => host.due(1), /disposed/);
  const local = createWireHostBridge(bridge, { local: true });
  for (const n of [0, -1, 2.9, 17, NaN, Infinity, -Infinity]) {
    local.setLocalInterval(n); assert.equal(local.interval, Math.max(1, Math.min(3, Math.floor(n) || 1)));
  }
  local.dispose();
});

test('native world cadence retains record identity, order and state-team JSON semantics', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), r = random(973);
  const payloads = ['[]', '[0,"idle",0,1]', '[0,"dead",0,1]', '[0,"dead",0,2]', '[0,null,0,null]', '[0,[1,null,3],0,{}]', '[0,"\\ud800",0,"\\udfff"]', '[0,1e300,0,-0]', '[0,{"toString":7},0,0]', '{"a":1}', 'broken'];
  for (const cadence of [undefined, { 1: 0, 2: 2.5, 7: 3, 9: 17, 10: 0 }]) {
    const js = new WorldFieldStagger(cadence), native = createWireStaggerBridge(bridge, cadence);
    const originals = new Map(); let tick = 0;
    for (let round = 0; round < 1600; round++) {
      tick += Math.floor(r() * 5); const offered = [];
      const schemas = [1, 2, 4, 5, 6, 7, 8, 9, 10];
      for (const schema of schemas) {
        for (const id of [0, 1, 13, 0xffffffff, 9007199254740000]) {
          const key = `${schema}:${id}`;
          if (r() < 0.16) continue;
          let record = originals.get(key);
          if (!record || r() < 0.55) { record = field(schema, id, payloads[Math.floor(r() * payloads.length)]); originals.set(key, record); }
          offered.push(record);
        }
      }
      const complete = new Set(schemas.filter(() => r() < 0.25));
      js.take(tick, offered, complete); native.take(tick, offered, complete);
      assert.deepEqual(native.values(), js.values(), `values ${round}`);
      assert.deepEqual(native.world(), js.world(), `world ${round}`);
      const inspect = new Set(schemas.filter(() => r() < 0.5));
      assert.equal(native.anyDue(tick + 3, inspect), js.anyDue(tick + 3, inspect));
      assert.equal(native.schemaDue(tick + 3, inspect), js.schemaDue(tick + 3, inspect));
      for (const record of offered.slice(0, 5)) {
        for (const state of [null, 'idle:1', 'dead:2', '\ud800:\udfff']) assert.equal(native.held(tick + 1, record.schema, record.id, state), js.held(tick + 1, record.schema, record.id, state));
        assert.equal(native.isFieldDue(tick + 1, record.schema, record.id), js.isFieldDue(tick + 1, record.schema, record.id));
        assert.equal(native.isDue(tick + 1, `${record.schema}:${record.id}`), js.isDue(tick + 1, `${record.schema}:${record.id}`));
      }
      if (round === 60) instance.exports.memory.grow(2);
    }
    native.dispose(); assert.throws(() => native.world(), /disposed/);
  }
});

test('native entity budget matches fractional replenishment, backwards clocks and refunds', async () => {
  const bridge = bridgeFor(await loadSim()), r = random(182);
  for (const rate of [1, 9876.3, 100000, 123456789]) {
    const options = { bytesPerSecond: rate, burstBytes: 7777, now: 25 }, js = new EntityBudget(options), native = new EntityBudget({ ...options, bridge });
    let now = 25;
    for (let n = 0; n < 2000; n++) {
      now += (r() - 0.2) * 10; const size = 1 + Math.floor(r() * 1100), op = ['available', 'take', 'refund'][n % 3];
      assert.equal(op === 'available' ? native[op](now) : native[op](size, now), op === 'available' ? js[op](now) : js[op](size, now));
      for (const key of ['tokens', 'at', 'offeredBytes', 'denied']) assert.equal(native[key], js[key], `${rate} ${n} ${key}`);
    }
    for (const [size, at] of [[0, NaN], [1101, 1], [1.5, 3], ['1', 2], [1, Infinity], [1, '3']]) {
      let expected; try { js.take(size, at); } catch (e) { expected = e.message; }
      assert.throws(() => native.take(size, at), e => e.message === expected);
    }
    native.dispose();
  }
});
