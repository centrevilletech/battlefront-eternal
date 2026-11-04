// Synthetic oracle for the native record/ring test. No game content is read.
// Run with the production server's Node version; the native test consumes
// stdout as JSON lines and compares every published byte.
import { Heap } from '../src/sim/runtime.js';
import { putWireCapture } from '../src/sim/gen/wire.js';
import { SnapshotSlotTable, snapshotFromRecords } from '../src/runtime/snapshot-records.js';
import { SnapshotRing } from '../src/runtime/snapshot-ring.js';
import { DEFAULT_SNAPSHOT_BOUNDS, quantizePosition, quantizeVelocity, quantizeYaw,
  quantizePitch, quantizeQuaternion, encodeEntities } from '../src/runtime/snapshot-codec.js';

const emit = value => process.stdout.write(JSON.stringify(value) + '\n');
const words = values => {
  const b = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) b.writeDoubleLE(values[i], i * 8);
  return b.toString('hex');
};
let state = 0x8a72b91d;
const random = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 4294967296; };
const integer = n => Math.floor(random() * n);
const vector = scale => Array.from({ length: 3 }, () => (random() - 0.5) * scale);
const edge = [0, -0, NaN, Infinity, -Infinity, Number.MIN_VALUE, -Number.MIN_VALUE,
  31.75, -31.75, 31.750000000001, -31.750000000001, 0.125, -0.125, Math.PI, -Math.PI,
  1e25, -1e25, 1e-200, -1e-200];
for (let i = 0; i < 600; i++) {
  const p = i < edge.length ? [edge[i], edge[(i + 3) % edge.length], edge[(i + 7) % edge.length]] : vector(10000);
  const v = i < edge.length ? p : vector(120), q = [...p, i % 5 ? random() : -0];
  const velocity = quantizeVelocity(v, !!(i & 1)), quaternion = quantizeQuaternion(q);
  emit({ kind: 'quantize', input: words([...p, ...v, ...q]), wide: !!(i & 1),
    expected: words([...quantizePosition(p), velocity.wide ? 1 : 0, ...velocity.v,
      quantizeYaw(p[0]), quantizePitch(p[1]), quaternion.index, ...quaternion.v]) });
}

const ring = new SnapshotRing(7);
for (let i = 0; i < 600; i++) {
  const tick = integer(70) - 15, epoch = integer(4), value = i;
  const evicted = [];
  ring.onEvict = (v, held) => evicted.push({ value: v, ticks: [...held.ticks], newest: held.newestTick });
  const clear = i % 113 === 0;
  if (clear) ring.clear(); else ring.set(tick, epoch, value);
  emit({ kind: 'ring', clear, tick, epoch, value, ticks: [...ring.ticks], newest: ring.newestTick, oldest: ring.oldestTick,
    evicted, values: ring.ticks.map(t => [t, ring.entries.get(t).epoch, ring.get(t)]) });
}

const strings = [];
const bridge = { strings: { id(s) { let i = strings.indexOf(s); if (i < 0) { i = strings.length; strings.push(s); } return i; } } };
const slots = new SnapshotSlotTable(), snapshots = [];
let previous = null;
for (let tick = 1; tick <= 240; tick++) {
  const records = [];
  for (let i = 0; i < 40; i++) {
    const id = i + 1, kind = i % 7 === 0 ? 'droideka' : 'soldier';
    if ((tick + i) % 19 === 0) { records.push({ kind: 'tombstone', id }); continue; }
    const period = Math.floor(tick / 13), generation = Math.floor(tick / 83) + 1;
    const position = [i * 5 + (period % 2 ? 50 : 0), -2 + i / 3, i - (tick % 4) * 0.02];
    const record = { kind, id, generation, classId: i + 10, team: 1 + (i & 1),
      position, velocity: [i % 2 ? 31.75 : 31.7501, tick % 4 ? -0 : 0, i / 100],
      yaw: i * Math.PI / 40, pitch: (tick % 13) / 120, health: 500 - tick,
      posture: i % 9, state: i % 17, weapon: 70 + i, fireCounter: tick * 3 + i,
      charge: tick % 8, mode: tick % 5, reverse: !!(tick % 2), dead: tick > 225,
      shieldOn: !!(tick % 3), time: tick / 60, roll: tick % 2 ? -0 : 0, lean: i / 17, legs: 1, shield: 50 };
    if (i % 3 === 0) { delete record.classId; delete record.team; delete record.charge; }
    records.push(record);
    if (i < 6 && tick % 17 < 12) records.push({ kind: 'seat', id, vehicleId: 100 + (i % 3), seatIndex: i });
  }
  for (let i = 0; i < 3; i++) {
    records.push({ kind: 'vehicle', id: 100 + i, generation: 1, classId: i + 100, team: 1,
      position: [i * 100, tick / 100, 0], velocity: [10, -3, -0], quaternion: [0.1, 0.2, 0.3, -0.9],
      health: 2000 - tick, state: tick % 4, power: tick, fireCounter: tick });
    records.push({ kind: 'mount', id: 100 + i, slot: tick % 5 ? i : null,
      tilt: Math.PI / 8, roll: -Math.PI / 7, walkPhase: tick / 3, walkBlend: tick % 12 / 10,
      seats: [{ yaw: -0, pitch: 0.2 }, { yaw: tick / 10, pitch: -0.3 }] });
  }
  records.push({ kind: 'turret', id: 200, generation: 1, building: 32, team: tick % 3,
    yaw: tick / 59, pitch: -tick / 59, gunYaw: Math.PI * 3, gunPitch: -0,
    health: 700, state: ['unbuilt', 'built', 'destroyed'][Math.floor(tick / 17) % 3] });
  records.push({ kind: 'item', id: 1, generation: Math.floor(tick / 53), active: tick % 3 !== 0,
    position: [tick / 99, 2, -3], velocity: [0.02, -0.02, 31.75], yaw: -tick,
    definitionId: 7, up: tick % 4 ? [0, 1, 0] : [-1.1, 0.7, 1.2] });
  records.push({ kind: 'remote', id: 1, generation: 1, active: tick % 5 !== 0,
    position: [1, 2, 3], velocity: [-0, 0, 0], health: 8, definitionId: 9, yaw: -Math.PI, pitch: 0.5 });
  const world = new Map([
    ['2:7', { schema: 2, id: 7, generation: 1, payload: Uint8Array.from({ length: 150 }, (_, i) => i % 5 ? 0 : (tick + i) & 255) }],
    ['1:8', { schema: 1, id: 8, generation: 1, payload: new Uint8Array([1, 4, 9, tick & 255]) }],
  ]);
  if (tick % 5 === 0) world.delete('1:8');
  const snapshot = snapshotFromRecords({ tick, epoch: 2, records, world, slots, previous });
  const baseline = tick % 7 === 0 ? null : snapshots[Math.max(0, snapshots.length - 17)] ?? null;
  const heap = new Heap(), row = new Float64Array(12);
  putWireCapture(bridge, { tick, epoch: 2, records, world: [...world.values()], bounds: DEFAULT_SNAPSHOT_BOUNDS }, row, 0, heap);
  const interval = tick % 3 + 1;
  emit({ kind: 'capture', strings: [...strings], row: words(row), heap: words(heap.words), baseline: baseline?.tick ?? 0, interval,
    parts: encodeEntities(snapshot, baseline, { interval }).map(p => Buffer.from(p).toString('hex')),
    order: [...snapshot.slots.keys()] });
  snapshots.push(snapshot); previous = snapshot;
}
