import test from 'node:test';
import assert from 'node:assert/strict';
import { emptySnapshot, encodeEntities, decodeEntities, applyEntities, cloneSnapshot } from '../src/runtime/snapshot-codec.js';
import { snapshotFromRecords, SnapshotSlotTable } from '../src/runtime/snapshot-records.js';
import { cosmeticPriority } from '../src/runtime/snapshot-cosmetics.js';

// Allocation-light snapshot capture: quantized entries whose values are
// unchanged since the previous snapshot must be reused by reference (so the
// encoder's identity fast path can skip them), without changing one wire
// byte. A discontinuity must never be reused away.
const soldier = (over = {}) => ({ kind: 'soldier', id: 4, generation: 2, tick: 60, classId: 7, team: 1, position: [1, 2, 3], velocity: [0, 0, 0], yaw: 0.5, pitch: -0.2, health: 80, posture: 1, state: 0, weapon: 9, fireCounter: 3, charge: 12, ...over });
const turret = (over = {}) => ({ kind: 'turret', id: 9, generation: 1, tick: 60, building: 5, state: 'built', team: 2, health: 400, yaw: 0.3, pitch: 0.1, gunYaw: 0.3, gunPitch: 0.1, ...over });

function sequence(frames) {
  const slots = new SnapshotSlotTable();
  let previous = null;
  return frames.map(([tick, records, worldRecords = []]) => {
    const snap = snapshotFromRecords({ tick, records, worldRecords, slots, previous });
    previous = snap;
    return snap;
  });
}
const only = (snap, cls) => [...snap.slots.values()].find(e => e.cls === cls);

test('unchanged entries are reused by reference; changed entries reuse unchanged parts', () => {
  const [a, b, c] = sequence([
    [60, [soldier(), turret()]],
    [62, [soldier({ tick: 62 }), turret({ tick: 62 })]],
    [64, [soldier({ tick: 64, health: 50 }), turret({ tick: 64, yaw: 0.9 })]],
  ]);
  assert.equal(only(b, 'soldier'), only(a, 'soldier'), 'unchanged soldier entry object is reused');
  assert.equal(only(b, 'turret'), only(a, 'turret'), 'unchanged turret entry object is reused');
  assert.equal('pos' in only(a, 'turret'), false, 'turret entries carry no quantized position');
  const before = only(b, 'soldier'), after = only(c, 'soldier');
  assert.notEqual(after, before, 'a health change makes a new entry');
  assert.equal(after.health, 50);
  assert.equal(after.pos, before.pos, 'unchanged position array is reused');
  assert.equal(after.vel, before.vel, 'unchanged velocity object is reused');
  assert.notEqual(only(c, 'turret'), only(b, 'turret'), 'a turret aim change makes a new entry');
});

test('a discontinuity is never reused away and reuse never changes wire bytes', () => {
  const snaps = sequence([
    [60, [soldier()]],
    [62, [soldier({ tick: 62 })]],
    [64, [soldier({ tick: 64, position: [200, 2, 3] })]],
    [66, [soldier({ tick: 66, position: [200, 2, 3], health: 10 })]],
  ]);
  assert.equal(only(snaps[1], 'soldier'), only(snaps[0], 'soldier'));
  assert.notEqual(only(snaps[2], 'soldier'), only(snaps[1], 'soldier'), 'teleport entry is fresh');
  assert.equal(only(snaps[2], 'soldier').disc, true, 'teleport keeps its discontinuity bit');
  assert.notEqual(only(snaps[3], 'soldier'), only(snaps[2], 'soldier'));
  assert.equal(only(snaps[3], 'soldier').disc, false, 'the disc flag does not stick to later snapshots');
  // The same snapshots with every shared reference cloned away must encode
  // to exactly the same bytes, against previous, older and empty baselines.
  for (const [current, baseline] of [[3, 2], [3, 0], [2, 1], [1, 0], [3, null]]) {
    const cur = snaps[current], base = baseline === null ? null : snaps[baseline];
    const shared = encodeEntities(cur, base).map(p => [...p]);
    const cloned = encodeEntities(structuredClone(cur), base && structuredClone(base)).map(p => [...p]);
    assert.deepEqual(shared, cloned, `wire bytes identical for snapshot ${current} vs ${baseline}`);
  }
});

test('encoder identity fast path still emits a shared discontinuity entry', () => {
  const [a] = sequence([[60, [soldier()]]]);
  const entry = only(a, 'soldier');
  const baseline = cloneSnapshot(a);
  const current = emptySnapshot(62, 1);
  current.slots.set([...a.slots.keys()][0], entry);
  assert.equal(encodeEntities(current, baseline).reduce((n, p) => n + p.length, 0),
    encodeEntities(structuredClone(current), structuredClone(baseline)).reduce((n, p) => n + p.length, 0));
  // Same object on both sides with disc set: the old encoder emitted the pos
  // group to carry the snap; the fast path must not swallow it.
  entry.disc = true;
  const parts = encodeEntities(current, baseline);
  const work = cloneSnapshot(baseline);
  for (const part of parts) applyEntities(work, decodeEntities(part, baseline));
  assert.equal(only(work, 'soldier').disc, true, 'shared disc entry still snaps on the client');
  assert.deepEqual(parts.map(p => [...p]), encodeEntities(structuredClone(current), structuredClone(baseline)).map(p => [...p]));
});

test('own cosmetic events outrank distance when the datagram budget overflows', () => {
  const here = { x: 0, y: 0, z: 0 };
  // A tank shell 250 m downrange is still the shooter's own round: it must
  // pack ahead of closer third-party events or it vanishes mid-flight.
  assert.equal(cosmeticPriority({ position: [250, 0, 0], ownerId: 7 }, here, 7), -1);
  assert.equal(cosmeticPriority({ position: [900, 0, 0], ownerId: 7 }, here, 7), -1, 'own events stay relevant past cosmetic range');
  assert.equal(cosmeticPriority({ position: [10, 0, 0], ownerId: 8 }, here, 7), 100);
  assert.equal(cosmeticPriority({ position: [900, 0, 0], ownerId: 8 }, here, 7), null, 'remote events past range are irrelevant');
  assert.equal(cosmeticPriority({}, here, 7), 0, 'position-less events (terminals) always travel');
  assert.equal(cosmeticPriority({ position: [5, 0, 0] }, null, 7), 0, 'no viewer position keeps the event');
});

test('world field wrappers are reused while their payload is the same object', () => {
  const field = { kind: 'summary', id: 700, generation: 1, tick: 60, schema: 7, payload: new Uint8Array([1, 2, 3]) };
  const [a, b, c] = sequence([
    [60, [], [field]],
    [62, [], [field]],
    [64, [], [{ ...field, tick: 64, payload: new Uint8Array([1, 2, 4]) }]],
  ]);
  assert.equal(b.world.get('7:700'), a.world.get('7:700'), 'same payload object reuses the wrapper');
  assert.notEqual(c.world.get('7:700'), b.world.get('7:700'), 'a new payload makes a new wrapper');
});
