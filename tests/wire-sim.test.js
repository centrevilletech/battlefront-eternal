import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createWireSim } from '../src/runtime/wire-sim.js';
import { encodeEntities, encodeWordDelta, emptySnapshot } from '../src/runtime/snapshot-codec.js';
import { wireCases } from './fixtures/wire-cases.js';

test('native snapshot encoders match the protocol twins byte for byte', async () => {
  const sim = createWireSim(await loadSim());
  wireCases(sim);
});
test('wire results survive memory growth, later calls and rejected encodes', async () => {
  const instance = await loadSim(), sim = createWireSim(instance);
  const bytes = new Uint8Array(300).fill(12), kept = sim.encodeWordDelta(bytes, bytes);
  const reference = encodeWordDelta(bytes, bytes);
  instance.exports.memory.grow(2);
  assert.deepEqual(sim.encodeWordDelta(bytes), encodeWordDelta(bytes));
  assert.deepEqual(kept, reference);
  const bad = emptySnapshot(1, 1); bad.world.set('7:0', {id:0,schema:7,generation:1,payload:new Uint8Array(1080)});
  assert.throws(() => sim.encodeEntities(bad,null), /exceeds datagram/);
  const empty = emptySnapshot(2,1);
  assert.deepEqual(sim.encodeEntities(empty,null), encodeEntities(empty,null));
  assert.throws(() => sim.encodeOwnDatagram({epoch:1,tick:1,cosmeticBytes:new Uint8Array(1100)}), /exceeds budget/);
});
