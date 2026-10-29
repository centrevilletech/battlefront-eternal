import { SnapshotClient } from '../src/runtime/snapshot-client.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  emptySnapshot, encodeEntities, decodeEntities, applyEntities, cloneSnapshot, worldFieldKey, readSnapshotHeader,
} from '../src/runtime/snapshot-codec.js';
import { SnapshotRing } from '../src/runtime/snapshot-ring.js';
import { encodeOwnDelta, decodeOwnDelta, encodeOwnDatagram, decodeOwnDatagram } from '../src/runtime/snapshot-own.js';
import { encodeCosmeticEvent, decodeCosmeticEvent, packCosmeticEvents, unpackCosmeticEvents, isGameplayCombat } from '../src/runtime/snapshot-cosmetics.js';
import { combatRecord, decodeCombat } from '../src/runtime/network-entity-combat.js';
import { entryFromRecord, recordFromEntry, mountRecordFromEntry, snapshotFromRecords, SnapshotSlotTable, WorldFieldStagger } from '../src/runtime/snapshot-records.js';

function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const ri = (r, lo, hi) => lo + Math.floor(r() * (hi - lo + 1));
function randomEntry(r, cls, entityId, generation = 1) {
  const e = { cls, entityId, generation, classId: ri(r, 1, 400), team: ri(r, 1, 2), disc: false,
    pos: [ri(r, 0, 65535), ri(r, 0, 65535), ri(r, 0, 65535)],
    vel: r() < 0.5 ? { wide: false, v: [ri(r, -127, 127), ri(r, -127, 127), ri(r, -127, 127)] } : { wide: true, v: [ri(r, -32767, 32767), ri(r, -32767, 32767), ri(r, -32767, 32767)] } };
  if (cls === 'soldier' || cls === 'droideka') Object.assign(e, { yaw: ri(r, 0, 65535), pitch: ri(r, -32767, 32767), health: ri(r, 0, 65535),
    posture: ri(r, 0, 2), charge: ri(r, 0, 63), state: ri(r, 0, 8), weapon: ri(r, 0, 400), seatSlot: -1, seatIndex: 0, fireCounter: ri(r, 0, 255) });
  if (cls === 'droideka') { delete e.charge; Object.assign(e, { mode: ri(r, 0, 13), reverse: r() < 0.5, dead: r() < 0.5, shieldOn: r() < 0.5, anim: [r() * 4, r() - 0.5, r() - 0.5, r(), r() * 200] }); }
  if (cls === 'vehicle') Object.assign(e, { quat: { index: ri(r, 0, 3), v: [ri(r, 0, 32767), ri(r, 0, 32767), ri(r, 0, 32767)] }, health: ri(r, 0, 65535), state: ri(r, 0, 6), power: ri(r, 0, 65535), fireCounter: ri(r, 0, 255),
    mount: { slot: r() < 0.5 ? null : ri(r, 0, 20), tilt: ri(r, -32767, 32767), roll: ri(r, -32767, 32767), walkPhase: ri(r, 0, 65535), walkBlend: ri(r, 0, 255), seats: [[ri(r, -32767, 32767), ri(r, -32767, 32767)]] } });
  if (cls === 'turret') { delete e.pos; delete e.vel; Object.assign(e, { yaw: ri(r, -32767, 32767), pitch: ri(r, -32767, 32767), gunYaw: ri(r, -32767, 32767), gunPitch: ri(r, -32767, 32767), health: ri(r, 0, 65535), state: ri(r, 0, 2) }); }
  if (cls === 'item') Object.assign(e, { yaw: ri(r, 0, 65535), active: true, definitionId: ri(r, 1, 400), up: [ri(r, -32767, 32767), ri(r, 0, 32767), ri(r, -32767, 32767)] });
  if (cls === 'remote') Object.assign(e, { yaw: ri(r, 0, 65535), pitch: ri(r, -32767, 32767), active: true, definitionId: ri(r, 1, 400), health: ri(r, 0, 65535) });
  return e;
}
const stripDisc = snapshot => new Map([...snapshot.slots].map(([k, v]) => { const e = { ...structuredClone(v), disc: false }; if (e.anim) e.anim = e.anim.map(Math.fround); return [k, e]; }));
function roundTrip(current, baseline) {
  const parts = encodeEntities(current, baseline);
  const work = cloneSnapshot(baseline ?? emptySnapshot(0, current.epoch));
  for (const part of parts) applyEntities(work, decodeEntities(part, baseline));
  return { parts, work };
}

test('every group of every class round-trips as new and as a full update', () => {
  const r = rng(7);
  for (const cls of ['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote']) {
    const a = emptySnapshot(100, 1); a.slots.set(3, randomEntry(r, cls, 42));
    const first = roundTrip(a, null);
    assert.deepEqual(stripDisc(first.work), stripDisc(a), `${cls} new`);
    const b = emptySnapshot(102, 1); const changed = randomEntry(r, cls, 42); changed.classId = a.slots.get(3).classId; b.slots.set(3, changed);
    const second = roundTrip(b, a);
    assert.deepEqual(stripDisc(second.work), stripDisc(b), `${cls} update`);
  }
});

test('property: random snapshots, baselines, slot reuse and class changes round-trip', () => {
  const r = rng(1234), classes = ['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote'];
  for (let n = 0; n < 60; n++) {
    const base = emptySnapshot(1000 + n * 4, 1), current = emptySnapshot(1002 + n * 4, 1);
    for (let slot = 0; slot < 40; slot++) if (r() < 0.6) base.slots.set(slot, randomEntry(r, classes[ri(r, 0, 5)], 100 + slot, ri(r, 1, 3)));
    for (let slot = 0; slot < 40; slot++) {
      const roll = r(), old = base.slots.get(slot);
      if (!old) { if (roll < 0.3) current.slots.set(slot, randomEntry(r, classes[ri(r, 0, 5)], 500 + slot)); continue; }
      if (roll < 0.15) continue; // removed
      if (roll < 0.3) { current.slots.set(slot, randomEntry(r, classes[ri(r, 0, 5)], 900 + slot, old.generation + 1)); continue; } // reuse / class change
      const next = structuredClone(old);
      if (next.pos) next.pos = next.pos.map(c => Math.max(0, Math.min(65535, c + ri(r, -40, 40))));
      if ('health' in next) next.health = Math.max(0, next.health - ri(r, 0, 10));
      if ('fireCounter' in next) next.fireCounter = (next.fireCounter + ri(r, 0, 3)) & 0xff;
      current.slots.set(slot, next);
    }
    const fieldCount = ri(r, 0, 4);
    for (let i = 0; i < fieldCount; i++) {
      const field = { id: 1000 + i, schema: 7, generation: 1, payload: Uint8Array.from({ length: ri(r, 1, 40) }, () => ri(r, 0, 255)) };
      base.world.set(worldFieldKey(field), field);
      if (r() < 0.7) { const next = structuredClone(field); next.payload = next.payload.slice(); if (r() < 0.5) next.payload[0] ^= 0xff; current.world.set(worldFieldKey(field), next); }
    }
    const { work } = roundTrip(current, base);
    assert.deepEqual(stripDisc(work), stripDisc(current), `iteration ${n}`);
    assert.deepEqual(work.world, current.world, `world iteration ${n}`);
  }
});

test('parts are independent and a crowded snapshot splits without overflow', () => {
  const r = rng(99), current = emptySnapshot(50, 1);
  for (let slot = 0; slot < 120; slot++) current.slots.set(slot, randomEntry(r, 'soldier', 1000 + slot));
  const parts = encodeEntities(current, null);
  assert.ok(parts.length > 1, 'crowded snapshot needs several parts');
  for (const part of parts) assert.ok(part.length <= 1100);
  const alone = decodeEntities(parts[1], null);
  assert.ok(alone.set.size > 0, 'a later part decodes without part 0');
  assert.equal(alone.header.partIndex, 1);
  const againstBase = encodeEntities(current, emptySnapshot(48, 1));
  assert.throws(() => decodeEntities(againstBase[0], emptySnapshot(999, 1)), /baseline not held/);
});

test('rings answer only the exact tick and epoch stored', () => {
  const ring = new SnapshotRing(128);
  ring.set(5, 1, 'a'); ring.set(133, 1, 'b');
  assert.equal(ring.get(5, 1), 'a', 'ticks a capacity apart are both held');
  assert.equal(ring.get(6, 1), undefined, 'a tick never stored is a miss');
  assert.equal(ring.get(133, 1), 'b');
  assert.equal(ring.get(133, 2), undefined, 'wrong epoch is a miss');
  ring.clear(); assert.equal(ring.get(133, 1), undefined);
});

test('rings hold the newest capacity snapshots, not capacity ticks', () => {
  // Online snapshots are two ticks apart: 128 slots must cover 128 of them
  // (about 4.3 s at 30 Hz), not 64.
  const ring = new SnapshotRing(128);
  for (let tick = 2; tick <= 400; tick += 2) ring.set(tick, 1, tick);
  assert.equal(ring.size, 128);
  assert.equal(ring.oldestTick, 400 - 127 * 2);
  for (let tick = 400 - 127 * 2; tick <= 400; tick += 2) assert.equal(ring.get(tick, 1), tick);
  assert.equal(ring.get(400 - 128 * 2, 1), undefined, 'the 129th newest was evicted');
  // Uneven spacing (a skipped publication) costs nothing.
  ring.set(405, 1, 405); assert.equal(ring.get(405, 1), 405); assert.equal(ring.size, 128);
  // A late snapshot fills its place; one older than everything held in a
  // full ring is not kept, so the newest ticks always stay.
  ring.set(147, 1, 'late'); assert.equal(ring.get(147, 1), undefined);
  ring.set(301, 1, 'late'); assert.equal(ring.get(301, 1), 'late'); assert.equal(ring.size, 128);
  assert.deepEqual(ring.ticks, [...ring.ticks].sort((x, y) => x - y));
  // Re-storing a tick replaces it without evicting anything.
  const oldest = ring.oldestTick; ring.set(405, 1, 'again');
  assert.equal(ring.get(405, 1), 'again'); assert.equal(ring.oldestTick, oldest);
  assert.equal(ring.newestTick, 405);
});

test('own correction delta is byte-exact and needs its baseline', () => {
  const r = rng(5);
  for (let n = 0; n < 40; n++) {
    const baseline = Uint8Array.from({ length: ri(r, 40, 700) }, () => ri(r, 0, 255));
    const current = baseline.slice();
    for (let i = 0; i < current.length; i++) if (r() < 0.2) current[i] = ri(r, 0, 255);
    assert.deepEqual(decodeOwnDelta(encodeOwnDelta(current, baseline), baseline), current);
    assert.deepEqual(decodeOwnDelta(encodeOwnDelta(current, null), null), current);
  }
  const baseline = new Uint8Array(64).fill(1), current = baseline.slice(); current[10] = 9;
  assert.throws(() => decodeOwnDelta(encodeOwnDelta(current, baseline), null), /baseline not held/);
});

test('own datagram carries correction, summaries and cosmetics under 1100 bytes', () => {
  const payload = Uint8Array.from({ length: 300 }, (_, i) => i % 251);
  const summaries = new Map([['3:65536', { id: 65536, schema: 3, generation: 1, payload: new Uint8Array([1, 2, 3]) }]]);
  const bytes = encodeOwnDatagram({ epoch: 1, tick: 200, baselineTick: 198, ackInput: 77,
    correction: { id: 9, generation: 2, schema: 11, payload }, summaries, cosmeticBytes: new Uint8Array([5, 6, 7, 8]) });
  assert.ok(bytes.length <= 1100);
  const decoded = decodeOwnDatagram(bytes);
  assert.equal(decoded.header.ackInput, 77);
  assert.deepEqual(decoded.correction.payload, payload);
  assert.equal(decoded.correction.id, 9);
  assert.deepEqual(decoded.summaries[0].payload, new Uint8Array([1, 2, 3]));
  assert.deepEqual(decoded.cosmeticBytes, new Uint8Array([5, 6, 7, 8]));
});

test('cosmetic launches and impacts round-trip compactly; gameplay feedback is not cosmetic', () => {
  const launch = combatRecord({ id: 7, generation: 1, tick: 100, schema: 1, expiryTick: 400, definitionId: 12, ownerId: 3, ownerGeneration: 1, fireId: 5,
    launchTick: 99.5, executionTick: 100, position: [10, 2, -20], direction: [0, 0.2, 0.98], offset: [0.1, 0.2, 0], speed: 60, life: 2, merge: 0.4, surface: 5 });
  const packed = encodeCosmeticEvent(launch);
  const back = decodeCombat(decodeCosmeticEvent(packed)), original = decodeCombat(launch);
  assert.ok(packed.length < launch.payload.length, `compact launch ${packed.length} < payload ${launch.payload.length}`);
  for (let i = 0; i < 3; i++) assert.ok(Math.abs(back.position[i] - original.position[i]) < 0.05, 'position within quantization');
  assert.equal(back.fireId, 5); assert.equal(back.speed, 60);
  const { bytes, dropped } = packCosmeticEvents([launch, launch, launch], packed.length + 4);
  assert.equal(unpackCosmeticEvents(bytes).length, 1);
  assert.equal(dropped.length, 2, 'over-budget events drop from the tail');
  assert.equal(isGameplayCombat({ kind: 'combat', schema: 3 }), true);
  assert.equal(isGameplayCombat(launch), false);
});

test('a launch outside the snapshot bounds keeps its exact start; one inside stays compact', () => {
  // The recon droid's orbital strike: 600 m out at 70 degrees, so 515 m up.
  const strike = combatRecord({ id: 8, generation: 1, tick: 536, schema: 1, expiryTick: 897, definitionId: 40, ownerId: 3, ownerGeneration: 1, fireId: 0,
    launchTick: 536, executionTick: 536, position: [-147.26, 515.05, 260.82], direction: [0, -0.9397, -0.342], speed: 300, life: 2.0045, merge: 8, surface: 5 });
  const impact = combatRecord({ id: 8, generation: 1, tick: 657, schema: 2, expiryTick: 897, definitionId: 40, ownerId: 3, ownerGeneration: 1,
    launchTick: 536, executionTick: 536, position: [-1100, -50.04, 55.14], direction: [0, 1, 0], surface: 0 });
  for (const record of [strike, impact]) {
    const { bytes } = packCosmeticEvents([record], 1000);
    const [back] = unpackCosmeticEvents(bytes);
    assert.deepEqual(decodeCombat(back).position, decodeCombat(record).position, `schema ${record.schema} keeps its exact position`);
  }
  const near = combatRecord({ id: 9, generation: 1, tick: 536, schema: 1, expiryTick: 897, definitionId: 40, ownerId: 3, ownerGeneration: 1, fireId: 0,
    launchTick: 536, executionTick: 536, position: [-147.26, 299, 260.82], direction: [0, -0.9397, -0.342], speed: 300, life: 1, merge: 8, surface: 5 });
  assert.ok(encodeCosmeticEvent(near).length < encodeCosmeticEvent(strike).length, 'an in-bounds launch keeps the compact form');
});

test('capture records convert to entries and back within quantization', () => {
  const soldier = { kind: 'soldier', id: 4, generation: 2, tick: 60, classId: 7, team: 1, position: [1, 2, 3], velocity: [4, 0, 0], yaw: 0.5, pitch: -0.2, health: 80, posture: 1, state: 0, weapon: 9, fireCounter: 300, charge: 12 };
  const back = recordFromEntry(entryFromRecord(soldier), 60);
  assert.ok(Math.abs(back.position[0] - 1) < 0.04 && Math.abs(back.yaw - 0.5) < 0.001);
  assert.equal(back.fireCounter, 44, 'fire counter travels as u8');
  const vehicle = { kind: 'vehicle', id: 8, generation: 1, tick: 60, classId: 20, team: 2, position: [5, 1, 5], velocity: [0, 0, 9], quaternion: [0, 0.7, 0, 0.7141428], health: 900, state: 5, power: 100 };
  const mount = { kind: 'mount', id: 8, generation: 1, tick: 60, slot: 2, tilt: 0.1, roll: -0.1, walkPhase: 1.5, walkBlend: 0.5, seats: [{ yaw: 0.3, pitch: 0.1 }] };
  const vEntry = entryFromRecord(vehicle, { mount });
  assert.ok(Math.abs(mountRecordFromEntry(vEntry, 60).seats[0].yaw - 0.3) < 0.001);
  const slots = new SnapshotSlotTable();
  const snap = snapshotFromRecords({ tick: 60, records: [vehicle, mount, soldier, { kind: 'seat', id: 4, generation: 2, tick: 60, vehicleId: 8, vehicleGeneration: 1, seatIndex: 0, leaseVersion: 1 }], slots });
  const rider = [...snap.slots.values()].find(e => e.cls === 'soldier');
  assert.equal(rider.seatSlot, slots.slotForUnit(8), 'the seat names the vehicle slot');
  snapshotFromRecords({ tick: 62, records: [{ kind: 'tombstone', id: 4, generation: 2, tick: 62 }], slots, previous: snap });
  assert.equal(slots.slotForUnit(4), -1, 'a tombstone frees the slot');
});

test('seat changes keep the discontinuity bit even for short moves', () => {
  const soldier = (tick, position) => ({ kind: 'soldier', id: 4, generation: 2, tick, classId: 7, team: 1, position, velocity: [0, 0, 0], yaw: 0.5, pitch: 0, health: 80, posture: 0, state: 0, weapon: 9, fireCounter: 0, charge: 0 });
  const vehicle = tick => ({ kind: 'vehicle', id: 8, generation: 1, tick, classId: 20, team: 1, position: [1, 2, 3], velocity: [0, 0, 0], quaternion: [0, 0, 0, 1], health: 900, state: 5, power: 100 });
  const seat = (tick, vehicleId) => ({ kind: 'seat', id: 4, generation: 2, tick, vehicleId, vehicleGeneration: 1, seatIndex: 0, leaseVersion: tick });
  const slots = new SnapshotSlotTable();
  const first = snapshotFromRecords({ tick: 60, records: [vehicle(60), soldier(60, [0, 2, 3])], slots });
  // Boarding moves the rider 1 m into the seat: inside the teleport
  // allowance, so only the seat change can flag the snap. Without the bit
  // the client interpolates the rider through the hull.
  const boarded = snapshotFromRecords({ tick: 62, records: [vehicle(62), soldier(62, [1, 2, 3]), seat(62, 8)], slots, previous: first });
  const rider = [...boarded.slots.values()].find(e => e.cls === 'soldier');
  assert.equal(rider.seatSlot, slots.slotForUnit(8));
  assert.equal(rider.disc, true, 'boarding snaps instead of sliding');
  const settled = snapshotFromRecords({ tick: 64, records: [vehicle(64), soldier(64, [1, 2, 3]), seat(64, 8)], slots, previous: boarded });
  assert.equal([...settled.slots.values()].find(e => e.cls === 'soldier').disc, false, 'staying seated does not snap again');
  const left = snapshotFromRecords({ tick: 66, records: [vehicle(66), soldier(66, [0, 2, 3]), seat(66, 0)], slots, previous: settled });
  assert.equal([...left.slots.values()].find(e => e.cls === 'soldier').disc, true, 'leaving the seat snaps too');
});

test('world fields refresh on a staggered cadence; building state changes refresh at once', () => {
  const stagger = new WorldFieldStagger({ 7: 30, 6: 18 });
  const building = pulse => ({ kind: 'summary', id: 200001, generation: 1, tick: 0, schema: 7, payload: new TextEncoder().encode(JSON.stringify([1, 'built', 100, 1, null, 0, pulse, 0, 0, false, false])) });
  let values = [];
  for (let tick = 0; tick <= 58; tick += 2) values = stagger.update(tick, [building(tick / 60)], new Set([7]));
  const phase = 2 * Math.ceil((((200001 * 2654435761) >>> 0) % 30) / 2);
  const shown = JSON.parse(new TextDecoder().decode(values[0].payload));
  assert.equal(shown[6], phase / 60, 'a timer-only change waits for its cadence');
  const destroyed = { ...building(0), payload: new TextEncoder().encode(JSON.stringify([1, 'destroyed', 0, 0, null, 0, 0, 0, 0, false, false])) };
  values = stagger.update(58, [destroyed], new Set([7]));
  assert.equal(JSON.parse(new TextDecoder().decode(values[0].payload))[1], 'destroyed', 'a destruction refreshes immediately');
  values = stagger.update(60, [], new Set([7]));
  assert.equal(values.length, 0, 'a field absent from a complete capture leaves the snapshot');
});

test('large snapshots exceed the old 15-part limit and retain every entry', () => {
  const current = emptySnapshot(100, 1), r = rng(81);
  for (let slot = 0; slot < 512; slot++) current.slots.set(slot, randomEntry(r, 'droideka', slot + 1000));
  const { parts, work } = roundTrip(current, null);
  assert.ok(parts.length > 15);
  assert.ok(parts.every(part => part.length <= 1100));
  assert.deepEqual(stripDisc(work), stripDisc(current));
  for (let i = 0; i < parts.length; i++) {
    const h = readSnapshotHeader(parts[i]);
    assert.equal(h.partIndex, i); assert.equal(h.partCount, parts.length); assert.equal(h.headerBytes, 24);
  }
});

test('extended snapshots acknowledge only all unique parts, then accept deltas', () => {
  const current = emptySnapshot(100, 1), r = rng(82);
  for (let slot = 0; slot < 512; slot++) current.slots.set(slot, randomEntry(r, 'droideka', slot + 1000));
  const parts = encodeEntities(current, null), client = new SnapshotClient();
  client.receive(encodeOwnDatagram({ epoch: 1, tick: 100 }));
  for (const part of parts.slice(1).reverse()) { client.receive(part); client.receive(part); }
  assert.equal(client.snapshotAck.tick, 0, 'one lost part prevents acknowledgement');
  assert.equal(client.receive(parts[0]).complete, true);
  assert.equal(client.snapshotAck.tick, 100);
  assert.deepEqual(stripDisc(client.ring.get(100, 1)), stripDisc(current));
  const next = cloneSnapshot(current); next.tick = 102; next.slots.get(0).health = 0;
  for (const part of encodeEntities(next, current)) client.receive(part);
  client.receive(encodeOwnDatagram({ epoch: 1, tick: 102, baselineTick: 100 }));
  assert.equal(client.snapshotAck.tick, 102);
  assert.deepEqual(stripDisc(client.ring.get(102, 1)), stripDisc(next));
});

test('part counters are bounded, validated and consistent within a snapshot', () => {
  const current = emptySnapshot(100, 1), r = rng(83);
  for (let slot = 0; slot < 512; slot++) current.slots.set(slot, randomEntry(r, 'droideka', slot + 1000));
  const parts = encodeEntities(current, null);
  const altered = (index, count) => {
    const bytes = parts[0].slice(), v = new DataView(bytes.buffer);
    v.setUint16(20, index, true); v.setUint16(22, count, true); return bytes;
  };
  for (const [index, count] of [[0, 0], [0, 15], [0, 257], [30, 30]]) assert.throws(() => readSnapshotHeader(altered(index, count)), /snapshot parts/);
  assert.throws(() => readSnapshotHeader(parts[0].slice(0, 22)), /truncated/);
  const client = new SnapshotClient(); client.receive(parts[1]);
  assert.throws(() => client.receive(altered(0, parts.length + 1)), /part count changed/);
  assert.equal(client.snapshotAck.tick, 0);
  const normal = encodeEntities(emptySnapshot(1, 1), null)[0]; normal[15] = 0x11;
  assert.throws(() => readSnapshotHeader(normal), /invalid snapshot parts/);
});

test('oversized indivisible records fail before emitting an oversized UDP datagram', () => {
  const current = emptySnapshot(1, 1), field = { id: 1, schema: 7, generation: 1, payload: new Uint8Array(1100) };
  current.world.set(worldFieldKey(field), field);
  assert.throws(() => encodeEntities(current, null), /record exceeds datagram/);
});

test('extended part-count limit accepts 256 and rejects 257 without wrapping', () => {
  const current = emptySnapshot(1, 1);
  for (let id = 0; id < 257; id++) {
    const field = { id, schema: 7, generation: 1, payload: new Uint8Array(900) };
    current.world.set(worldFieldKey(field), field);
    if (id === 255) {
      const { parts, work } = roundTrip(current, null);
      assert.equal(parts.length, 256);
      assert.equal(readSnapshotHeader(parts[255]).partIndex, 255);
      assert.deepEqual(work.world, current.world);
    }
  }
  assert.throws(() => encodeEntities(current, null), /too many parts/);
});
