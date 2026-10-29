import test from 'node:test';
import assert from 'node:assert/strict';
import * as fast from '../src/runtime/snapshot-codec.js';
import * as reference from './fixtures/snapshot-codec-reference.js';

// The scratch-buffer encoder must produce exactly the bytes the original
// encoder produced, for every baseline age a host can encode against. A
// synthetic battle evolves for a few hundred snapshots: entities move,
// change groups, board and leave vehicles, die and are replaced, appear and
// vanish; world fields refresh some words, change length, appear and vanish.
// Unchanged entries keep their object, as snapshotFromRecords does.
function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const ri = (r, lo, hi) => lo + Math.floor(r() * (hi - lo + 1));
const CLASSES = ['soldier', 'soldier', 'soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote'];
function entry(r, cls, entityId, generation) {
  const e = { cls, entityId, generation, classId: ri(r, 1, 400), team: ri(r, 1, 2), disc: false,
    pos: [ri(r, 0, 65535), ri(r, 0, 65535), ri(r, 0, 65535)], vel: { wide: false, v: [ri(r, -127, 127), ri(r, -127, 127), ri(r, -127, 127)] } };
  if (cls === 'soldier' || cls === 'droideka') Object.assign(e, { yaw: ri(r, 0, 65535), pitch: ri(r, -32767, 32767), health: ri(r, 0, 65535),
    posture: ri(r, 0, 2), charge: ri(r, 0, 63), state: ri(r, 0, 8), weapon: ri(r, 0, 400), seatSlot: -1, seatIndex: 0, fireCounter: ri(r, 0, 255) });
  if (cls === 'droideka') { delete e.charge; Object.assign(e, { mode: ri(r, 0, 13), reverse: r() < 0.5, dead: r() < 0.5, shieldOn: r() < 0.5, anim: [r() * 4, r() - 0.5, r() - 0.5, r(), r() * 200].map(Math.fround) }); }
  if (cls === 'vehicle') Object.assign(e, { quat: { index: ri(r, 0, 3), v: [ri(r, 0, 32767), ri(r, 0, 32767), ri(r, 0, 32767)] }, health: ri(r, 0, 65535), state: ri(r, 0, 6), power: ri(r, 0, 65535), fireCounter: ri(r, 0, 255),
    mount: { slot: r() < 0.5 ? null : ri(r, 0, 20), tilt: ri(r, -32767, 32767), roll: ri(r, -32767, 32767), walkPhase: ri(r, 0, 65535), walkBlend: ri(r, 0, 255), seats: [[ri(r, -32767, 32767), ri(r, -32767, 32767)], [0, 0]] } });
  if (cls === 'turret') { delete e.pos; delete e.vel; Object.assign(e, { yaw: ri(r, -32767, 32767), pitch: ri(r, -32767, 32767), gunYaw: ri(r, -32767, 32767), gunPitch: ri(r, -32767, 32767), health: ri(r, 0, 65535), state: ri(r, 0, 2) }); }
  if (cls === 'item') Object.assign(e, { yaw: ri(r, 0, 65535), active: true, definitionId: ri(r, 1, 400), up: [ri(r, -32767, 32767), ri(r, 0, 32767), ri(r, -32767, 32767)] });
  if (cls === 'remote') Object.assign(e, { yaw: ri(r, 0, 65535), pitch: ri(r, -32767, 32767), active: true, definitionId: ri(r, 1, 400), health: ri(r, 0, 65535) });
  return e;
}
function evolve(r, before) {
  const e = { ...before, disc: false };
  if (e.pos) {
    const step = r() < 0.05 ? 3000 : 60;
    e.pos = e.pos.map(c => Math.max(0, Math.min(65535, c + ri(r, -step, step))));
    if (r() < 0.05) e.disc = true;
    e.vel = r() < 0.2 ? { wide: true, v: [ri(r, -32767, 32767), ri(r, -32767, 32767), ri(r, -32767, 32767)] } : { wide: false, v: e.vel.v.map(c => Math.max(-127, Math.min(127, c + ri(r, -4, 4)))) };
  }
  if ('yaw' in e) e.yaw = e.cls === 'turret' ? ri(r, -32767, 32767) : (e.yaw + ri(r, -300, 300)) & 0xffff;
  if ('health' in e && r() < 0.1) e.health = ri(r, 0, 65535);
  if ('fireCounter' in e && r() < 0.2) e.fireCounter = (e.fireCounter + 1) & 0xff;
  if (e.anim) e.anim = e.anim.map(v => Math.fround(v + (r() - 0.5) * 0.1));
  if (e.quat) e.quat = { index: e.quat.index, v: e.quat.v.map(c => Math.max(0, Math.min(32767, c + ri(r, -50, 50)))) };
  if (e.mount && r() < 0.3) e.mount = { ...e.mount, tilt: ri(r, -32767, 32767), seats: [[ri(r, -32767, 32767), ri(r, -32767, 32767)], [0, 0]] };
  if (r() < 0.01) e.team = 3 - e.team;
  return e;
}
const payload = (r, n) => Uint8Array.from({ length: n }, () => ri(r, 0, 255));
function battle(seed, count) {
  const r = rng(seed), snapshots = [];
  let previous = null, nextId = 1;
  for (let i = 0; i < count; i++) {
    const tick = 100 + i * 2, slots = new Map(), world = new Map();
    if (!previous) {
      for (let slot = 0; slot < 140; slot++) slots.set(slot * (slot % 7 === 6 ? 3 : 1), entry(r, CLASSES[slot % CLASSES.length], nextId++, 1));
      for (let id = 0; id < 30; id++) { const field = { id, schema: 1 + (id % 8), generation: 1, payload: payload(r, ri(r, 4, 300)) }; world.set(fast.worldFieldKey(field), field); }
    } else {
      for (const [slot, before] of previous.slots) {
        const roll = r();
        if (roll < 0.004) continue; // vanished
        if (roll < 0.008) { slots.set(slot, entry(r, before.cls, before.entityId, before.generation + 1)); continue; } // replaced
        slots.set(slot, roll < 0.5 ? before : evolve(r, before));
      }
      if (r() < 0.3) for (let slot = 0; slot < fast.SNAPSHOT_SLOT_COUNT; slot++) if (!slots.has(slot)) { slots.set(slot, entry(r, CLASSES[ri(r, 0, CLASSES.length - 1)], nextId++, 1)); break; }
      // Riders board and leave vehicles (packing keeps them in one part).
      const vehicles = [...slots].filter(([, e]) => e.cls === 'vehicle').map(([slot]) => slot);
      for (const [slot, e] of slots) if ((e.cls === 'soldier' || e.cls === 'droideka') && r() < 0.02) slots.set(slot, { ...e, seatSlot: e.seatSlot >= 0 || !vehicles.length ? -1 : vehicles[ri(r, 0, vehicles.length - 1)], seatIndex: ri(r, 0, 1), disc: true });
      for (const [key, before] of previous.world) {
        const roll = r();
        if (roll < 0.01) continue;
        if (roll < 0.7) { world.set(key, before); continue; }
        const next = before.payload.slice();
        if (roll < 0.95) for (let n = ri(r, 1, 4); n > 0; n--) next[ri(r, 0, next.length - 1)] ^= ri(r, 1, 255);
        world.set(key, { ...before, generation: roll < 0.96 ? before.generation + 1 : before.generation, payload: roll < 0.97 ? payload(r, ri(r, 4, 300)) : next });
      }
      if (r() < 0.1) { const field = { id: 100 + i, schema: ri(r, 1, 8), generation: 1, payload: payload(r, ri(r, 4, 300)) }; world.set(fast.worldFieldKey(field), field); }
    }
    const snapshot = { tick, epoch: 1, slots, world };
    snapshots.push(snapshot); previous = snapshot;
  }
  return snapshots;
}
const same = (a, b, message) => {
  assert.equal(a.length, b.length, message);
  for (let i = 0; i < a.length; i++) assert.ok(Buffer.from(a[i]).equals(Buffer.from(b[i])), `${message} part ${i}`);
};

test('the scratch-buffer encoder is byte-identical to the reference across baseline ages', () => {
  for (const seed of [1, 2]) {
    const snapshots = battle(seed, 140);
    for (let i = 64; i < snapshots.length; i++) for (const age of [0, 1, 2, 4, 8, 16, 32, 63]) {
      const baseline = age ? snapshots[i - age] : null;
      for (const interval of [1, 2]) same(fast.encodeEntities(snapshots[i], baseline, { interval }), reference.encodeEntities(snapshots[i], baseline, { interval }), `seed ${seed} snapshot ${i} age ${age}`);
    }
  }
});

test('world sections and word deltas are byte-identical to the reference', () => {
  const snapshots = battle(9, 120), r = rng(4);
  for (let i = 1; i < snapshots.length; i++) for (const age of [0, 1, 5, 30]) {
    const baseline = age && i >= age ? snapshots[i - age].world : null;
    same([fast.encodeWorldSection(snapshots[i].world, baseline)], [reference.encodeWorldSection(snapshots[i].world, baseline)], `world ${i} age ${age}`);
  }
  for (let n = 0; n < 400; n++) {
    const baseline = payload(r, ri(r, 1, 700)), current = r() < 0.1 ? payload(r, ri(r, 1, 700)) : baseline.slice();
    for (let i = 0; i < current.length; i++) if (r() < [0, 0.01, 0.2, 0.9][n % 4]) current[i] = ri(r, 0, 255);
    for (const base of [baseline, null]) same([fast.encodeWordDelta(current, base)], [reference.encodeWordDelta(current, base)], `word delta ${n}`);
  }
});

test('encoded parts own exact-size buffers that later encodes never touch', () => {
  const snapshots = battle(5, 70);
  const first = fast.encodeEntities(snapshots[69], snapshots[60]), copy = first.map(p => p.slice());
  for (const part of first) assert.equal(part.byteLength, part.buffer.byteLength);
  fast.encodeEntities(snapshots[68], null); fast.encodeWorldSection(snapshots[67].world, null); fast.encodeWordDelta(new Uint8Array(64), null);
  same(first, copy, 'parts survive later encodes');
});
