import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createWireCosmetics } from '../src/runtime/wire-cosmetics.js';
import { cosmeticCodecCases, cosmeticOutboxCases } from './fixtures/cosmetic-cases.js';
import { encodeCosmeticEvent } from '../src/runtime/snapshot-cosmetics.js';
import { combatRecord } from '../src/runtime/network-entity-combat.js';

test('native cosmetic compact/raw bytes, V8 direction quantization, priority and packing match JavaScript', async () => {
  const native = createWireCosmetics(await loadSim());
  try { assert.ok(cosmeticCodecCases(native) >= 630); } finally { native.dispose(); }
});
test('native cosmetic outbox preserves queue order, event identity, acknowledgements, expiry and metrics', async () => {
  const native = createWireCosmetics(await loadSim());
  try { assert.ok(cosmeticOutboxCases(native) > 1300); } finally { native.dispose(); }
});
test('cosmetic owners isolate clients, survive memory growth and release all outboxes', async () => {
  const instance = await loadSim(); let nested = false, native;
  native = createWireCosmetics(instance, { trace({ op }) {
    if (op === 0 && !nested) {nested = true; native.cosmeticPriority({position:[10,0,0],ownerId:2}, {x:0,y:0,z:0}, 1);}
  } });
  const a = native.createOutbox(), b = native.createOutbox();
  const record = combatRecord({id:4,generation:1,tick:10,schema:1,expiryTick:130,definitionId:1,position:[1,0,0],direction:[1,0,0],speed:60,life:1,merge:8});
  try {
    const bytes = native.encodeCosmeticEvent(record); assert.deepEqual(bytes,encodeCosmeticEvent(record));
    const event = native.cosmeticEvent(record,{position:[1,0,0],ownerId:0,life:1,executionTick:10},bytes);
    a.offer([event],{x:0,y:0,z:0},1); instance.exports.memory.grow(1);
    assert.ok(a.pack(10,1000,0,true).first); assert.equal(b.pack(10,1000,0,true).first,null);
    assert.deepEqual(bytes,encodeCosmeticEvent(record),'later calls keep returned buffers intact');
    a.dispose(); a.dispose(); assert.throws(()=>a.pack(11,1000,0,true),/disposed/);
    native.dispose(); native.dispose(); assert.throws(()=>b.pack(11,1000,0,true),/disposed/);
    assert.throws(()=>native.createOutbox(),/disposed/);
  } finally {native.dispose();}
});
test('native cosmetic validation retains header and decoded-payload failures', async () => {
  const native = createWireCosmetics(await loadSim());
  const record = combatRecord({id:4,generation:1,tick:10,schema:1,expiryTick:130,definitionId:1,position:[1,0,0],direction:[1,0,0],speed:60,life:1,merge:8});
  try {
    const bad = [{...record,id:-1},{...record,generation:NaN},{...record,schema:65536},{...record,kind:'item'}, {...record,payload:record.payload.slice(1)}, {...record,schema:8,payload:new Uint8Array(65536)}];
    for (const offset of [20,28,52,101]) {const payload=record.payload.slice();new DataView(payload.buffer).setFloat64(offset,NaN,true);bad.push({...record,payload});}
    for (const value of bad) {
      let expected; try {encodeCosmeticEvent(value);} catch(error) {expected=error.message;}
      assert.ok(expected); assert.throws(()=>native.encodeCosmeticEvent(value),error=>error.message===expected);
    }
    assert.deepEqual(native.encodeCosmeticEvent(record),encodeCosmeticEvent(record));
  } finally {native.dispose();}
});
