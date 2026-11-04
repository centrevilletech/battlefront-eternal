import assert from 'node:assert/strict';
import { combatRecord, decodeCombat } from '../../src/runtime/network-entity-combat.js';
import { projectileRecord } from '../../src/runtime/network-combat-projectile.js';
import { explosionRecord } from '../../src/runtime/network-combat-explosion.js';
import { presentationRecord } from '../../src/runtime/entity-presentation-events.js';
import { cosmeticEvent, CosmeticOutbox } from '../../src/runtime/snapshot-cosmetic-outbox.js';
import { encodeCosmeticEvent, cosmeticPriority, packEncodedCosmeticEvents } from '../../src/runtime/snapshot-cosmetics.js';
import { DEFAULT_SNAPSHOT_BOUNDS } from '../../src/runtime/snapshot-codec.js';

const xyz = (x = 0, y = 0, z = 0) => ({ x, y, z });
function launch(id, tick = 10, options = {}) {
  return combatRecord({ id, generation: 1, tick, schema: 1, expiryTick: tick + 300, definitionId: 3, ownerId: 9, ownerGeneration: 2, fireId: 8,
    launchTick: tick - .25, executionTick: tick, position: [10, 0, 0], direction: [1, 0, 0], offset: [0, 0, 0], speed: 100, life: 1, merge: 8, ...options });
}
function terminal(id, tick = 12, options = {}) {
  return combatRecord({ id, generation: 1, tick, schema: 2, expiryTick: tick + 200, definitionId: 3, ownerId: 9, ownerGeneration: 2,
    launchTick: tick - 2, executionTick: tick - 1, position: [10, 0, 0], direction: [0, 1, 0], surface: 1, ...options });
}
const sample = (id, tick = 10, options = {}) => projectileRecord({ id, generation: 1, tick, definitionId: 3, type: 'round', ownerId: 9, ownerGeneration: 2, launchTick: Math.max(0, tick - 2), position: [10, 0, 0], velocity: [40, 0, 0], life: 2, ...options });
const presentation = (id, tick = 10, options = {}) => presentationRecord({ id, generation: 1, tick, event: { kind: 'shot', ownerId: 9, ownerGeneration: 2, weaponId: 1, fireId: 0, position: [10, 0, 0], ...options } });

export function cosmeticCodecCases(native, { count = 600 } = {}) {
  let state = 983742;
  const random = () => (state = (Math.imul(state, 1664525) + 1013904223) >>> 0) / 4294967296;
  const cases = [];
  for (let i = 0; i < count; i++) {
    const direction = [random() * 2 - 1, random() * 2 - 1, random() * 2 - 1];
    const len = Math.hypot(...direction); for (let j = 0; j < 3; j++) direction[j] /= len;
    const options = { position: [random() * 2200 - 1100, random() * 900 - 450, random() * 2200 - 1100], direction,
      ownerId: i % 3, surface: i % 6, channel: i % 2, launchTick: 10 - (i % 64) / 64 };
    cases.push(i % 2 ? launch(i, 10, { ...options, offset: [random() * 280 - 140, 1 / 512, -1 / 512], speed: random() * 1e5, life: random() * 120 }) : terminal(i, 12, options));
  }
  for (const position of [[-1024, -400, -1024], [1024, 300, 1024], [0, 515, 0], [1024 + 1e-10, 0, 0], [0, -400 - 1e-10, 0]]) {
    for (const offset of [[32767 / 256, -32767 / 256, 0], [128, 0, 0], [-1 / 512, 1 / 512, -0]]) cases.push(launch(5000, 10, { position, offset }));
  }
  for (const direction of [[0, 1, 0], [-0, -1, 0], [-0, 0, -1], [0, 1 + 1e-12, 0]]) cases.push(launch(5001, 10, { direction }));
  for (let schema = 3; schema <= 20; schema++) cases.push({ kind: 'combat', id: schema, generation: 0xffffffff, tick: 10, schema, payload: Uint8Array.from({ length: schema * 3 }, (_, n) => (n * 37) & 255) });
  for (const record of cases) {
    for (const bounds of [undefined, { ...DEFAULT_SNAPSHOT_BOUNDS, xMin: -40, xExtent: 80, yMin: -20, yExtent: 30 }]) {
      const expected = encodeCosmeticEvent(record, bounds);
      assert.deepEqual(native.encodeCosmeticEvent(record, bounds), expected);
      if (record.schema <= 2) assert.deepEqual(native.encodeCosmeticEvent(record, bounds, decodeCombat(record)), expected);
    }
  }
  for (let i = 0; i < count; i++) {
    const view = { schema: i % 3, position: i % 13 ? [random() * 1000 - 500, random() * 200 - 100, random() * 1000 - 500] : null,
      direction: i % 7 ? [-1, 0, 0] : null, ownerId: i % 5 ? i % 3 : undefined, speed: random() * 200, life: random() * 10 };
    const position = i % 17 ? xyz(random() * 10, 0, random() * 10) : null, owner = i % 5 ? i % 3 : undefined, range = i % 19 ? 300 : Infinity;
    assert.equal(native.cosmeticPriority(view, position, owner, range), cosmeticPriority(view, position, owner, range));
  }
  for (const owner of [undefined, null, 0, -0, NaN]) for (const theirs of [undefined, null, 0, -0, NaN]) {
    const view = { schema: 5, position: [900, 0, 0], ownerId: theirs };
    assert.equal(native.cosmeticPriority(view, xyz(), owner), cosmeticPriority(view, xyz(), owner));
  }
  const encoded = cases.slice(0, 80).map(record => ({ record, bytes: encodeCosmeticEvent(record) }));
  for (const budget of [-1, 0, 1, 16, 76, 150, 1100, 5000, Infinity, NaN]) {
    const actual = native.packEncodedCosmeticEvents(encoded, budget), expected = packEncodedCosmeticEvents(encoded, budget);
    assert.deepEqual(actual, expected);
    for (let i = 0; i < actual.packed.length; i++) assert.equal(actual.packed[i], expected.packed[i]);
  }
  return cases.length;
}

function snapshot(box, identity) {
  return { dropped: box.dropped, owedDropped: box.owedDropped, resent: box.resent, prunedTick: box.prunedTick,
    entries: [...box.entries].map(([key, e]) => [key, { identity: identity(e.event), d2: e.d2, rank: e.rank, sentTick: e.sentTick }]),
    inflight: box.inflight.map(s => ({ tick: s.tick, events: s.events.map(identity) })),
    projectiles: [...box.projectiles],
  };
}
export function cosmeticOutboxCases(native, { rounds = 700 } = {}) {
  const make = record => {
    const view = decodeCombat(record), bytes = encodeCosmeticEvent(record);
    assert.deepEqual(native.encodeCosmeticEvent(record, undefined, view), bytes);
    const result = native.cosmeticEvent(record, view, bytes);
    assert.deepEqual(result, cosmeticEvent(record, view, bytes));
    assert.equal(result.record, record); assert.equal(result.view, view); assert.equal(result.bytes, bytes);
    return result;
  };
  const js = new CosmeticOutbox(), rust = native.createOutbox();
  let operations = 0;
  const check = () => {
    assert.deepEqual(rust.inspect(), snapshot(js, native.identity));
    assert.deepEqual([rust.dropped, rust.owedDropped, rust.resent, rust.prunedTick, rust.size], [js.dropped, js.owedDropped, js.resent, js.prunedTick, js.entries.size]);
  };
  const offer = (events, position = xyz(), owner = 7) => { js.offer(events, position, owner); rust.offer(events, position, owner); check(); operations++; };
  const pack = (tick, first = 1000, second = 1000, confirmable = true, spare = Infinity) => {
    const expected = js.pack(tick, first, second, confirmable, spare);
    assert.deepEqual(rust.pack(tick, first, second, confirmable, spare), expected); check(); operations++; return expected;
  };
  const ack = tick => { js.acknowledge(tick); rust.acknowledge(tick); check(); operations++; };
  const resend = () => { js.resend(); rust.resend(); check(); operations++; };
  try {
    const same = make(launch(1));
    offer([same]); pack(10); pack(12); ack(12); pack(14); ack(14); pack(16);
    offer([make(sample(3, 20))]); pack(20); offer([make(sample(3, 22))]); pack(22); ack(20);
    offer([make(sample(3, 24)), make(sample(3, 26))]); pack(26, 1000, 0, true, 0); pack(28);
    offer([make(sample(3, 29)), make(terminal(3, 30))]); pack(30); resend(); pack(32);
    // Reoffering the very same event object preserves identity against its
    // earlier in-flight send; a replacement with equal values does not.
    offer([same]); pack(34); offer([same]); pack(34); ack(34);
    offer([make(launch(4, 40, { position: [290, 0, 0], direction: [0,0,1] })), make(sample(5, 40))]); pack(40);
    offer([make(terminal(4, 42, { position: [900,0,0] })), make(projectileRecord({ id: 5, generation: 1, tick: 42, terminal: true })), make(projectileRecord({ id: 6, generation: 1, tick: 42, terminal: true }))]); pack(42);
    offer([make(launch(7, 44, { position: [450,0,0], direction: [-1,0,0], life: 4 })), make(launch(8,44,{position:[450,0,0],life:4}))]); pack(44);
    offer([make(presentation(50,50)), make(launch(51,50)), make(terminal(52,50))]); pack(50,76,76,false,100); pack(52,76,76,true,0); pack(54); ack(54);
    // More than the queue limit, followed by expiry without a send.
    const overflow = Array.from({ length: 1090 }, (_, i) => {
      const record = { id: 10000 + i, generation: 1, tick: 100, schema: i % 3 ? 8 : 5, payload: new Uint8Array() };
      const view = { position: [i % 10, 0, 0], ownerId: 9, schema: record.schema };
      const event = native.cosmeticEvent(record, view, Uint8Array.of(i & 255));
      assert.deepEqual(event, cosmeticEvent(record, view, event.bytes)); return event;
    });
    offer(overflow); pack(125,0,0); resend();
    let seed = 771;
    const random = n => { seed = (Math.imul(seed,1664525) + 1013904223) >>> 0; return seed % n; };
    for (let i = 0; i < rounds; i++) {
      const tick = 140 + i * 2, events = [];
      for (let n = random(10); n > 0; n--) {
        const id = 100 + random(100), at = [random(1500) - 750, random(40), random(500) - 250], owner = random(3) ? 9 : 7;
        switch (random(6)) {
          case 0: events.push(make(launch(id,tick,{position:at,ownerId:owner,direction:random(2)?[-1,0,0]:[1,0,0],life:random(8),speed:random(100)}))); break;
          case 1: events.push(make(terminal(id,tick,{position:at,ownerId:owner}))); break;
          case 2: events.push(make(sample(id,tick,{position:at,ownerId:owner,life:random(100)/30}))); break;
          case 3: events.push(make(projectileRecord({id,generation:1,tick,terminal:true}))); break;
          case 4: events.push(make(explosionRecord({id,generation:1,tick,definitionId:1,position:at,wet:!!random(2)}))); break;
          default: events.push(make(presentation(id,tick,{position:at,ownerId:owner}))); break;
        }
      }
      offer(events, i % 31 ? xyz(random(100)-50,0,0) : null, i % 29 ? 7 : undefined);
      if (i % 3) pack(tick,random(500),random(500),!!random(4),i % 7 ? random(500) : Infinity);
      if (i % 5 === 0) ack(tick - random(8));
      if (i % 19 === 0) resend();
      if (i % 23 === 0) pack(tick + 60,random(400),random(400),true,random(300));
    }
    pack(100000); ack(100000); check();
  } finally { rust.dispose(); }
  return operations;
}
