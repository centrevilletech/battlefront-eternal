import test from 'node:test';
import assert from 'node:assert/strict';
import { encodeEntities } from '../src/runtime/snapshot-codec.js';
import { snapshotFromRecords, SnapshotSlotTable, WorldFieldStagger, EntrySpare } from '../src/runtime/snapshot-records.js';
import { SnapshotRing } from '../src/runtime/snapshot-ring.js';
import { captureWorldPresentation } from '../src/runtime/entity-world-state.js';
import * as reference from './fixtures/snapshot-capture-reference.js';
import { checkPayloadRecordFits, encodeEntityRecord } from '../src/runtime/network-entity-codec.js';

// Capture builds less than it did (no record for a station or tow the
// stagger keeps, no fresh entry for an unchanged actor), but every snapshot
// must hold exactly what the frozen reference capture
// (fixtures/snapshot-capture-reference.js) put in it: the same entries in
// the same slots, the same world fields in the same order, so the same
// datagrams. Synthetic battles cover the edges; a real one, when game data
// is present, covers everything the world does.
function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const ri = (r, lo, hi) => lo + Math.floor(r() * (hi - lo + 1));
const pick = (r, list) => list[ri(r, 0, list.length - 1)];

// Both snapshots hold the same entries (strictly: -0 is not 0) and world
// fields, in the same world order, and encode alike against `baselines`.
function sameSnapshot(actual, expected, baselines, label) {
  assert.deepEqual([...actual.slots.keys()].sort((a, b) => a - b), [...expected.slots.keys()].sort((a, b) => a - b), `${label}: slots`);
  for (const [slot, entry] of expected.slots) assert.deepStrictEqual(actual.slots.get(slot), entry, `${label}: slot ${slot}`);
  assert.deepStrictEqual([...actual.world.keys()], [...expected.world.keys()], `${label}: world order`);
  for (const [key, field] of expected.world) assert.deepStrictEqual(actual.world.get(key), field, `${label}: world ${key}`);
  for (const [base, expectedBase] of baselines) {
    const a = encodeEntities(actual, base, { interval: 1 }), b = encodeEntities(expected, expectedBase, { interval: 1 });
    assert.deepEqual(a.map(p => Buffer.from(p).toString('hex')), b.map(p => Buffer.from(p).toString('hex')), `${label}: datagrams against ${base?.tick ?? 0}`);
  }
}
const sameTable = (actual, expected, label) => {
  assert.deepEqual([...actual.byKey], [...expected.byKey], `${label}: slot keys`);
  assert.deepEqual([...actual.byUnitId], [...expected.byUnitId], `${label}: unit slots`);
  assert.deepEqual([actual.free, actual.next], [expected.free, expected.next], `${label}: free slots`);
};

// The records EntityWorldCapture makes, in its order (items, remotes,
// soldiers with their seats, vehicles with their mounts, turrets,
// tombstones), for a battle that moves, teleports, boards, dies, respawns
// and leaves. Values sit on and around the quantization edges.
function recordBattle(seed, ticks) {
  const r = rng(seed), frames = [];
  const v3 = (scale) => [(r() - 0.5) * scale, (r() - 0.5) * scale, (r() - 0.5) * scale];
  const soldiers = Array.from({ length: 30 }, (_, i) => ({ id: 1 + i, generation: 1, alive: true, droideka: i >= 26, position: v3(1500), velocity: v3(20), seat: null, classId: ri(r, 1, 60), team: 1 + (i % 2) }));
  const vehicles = Array.from({ length: 5 }, (_, i) => ({ id: 100 + i, generation: 1, alive: true, position: v3(1500), velocity: v3(40), q: [0, 0, 0, 1], classId: 70 + i, team: 1 + (i % 2), seats: i + 1 }));
  const turrets = Array.from({ length: 6 }, (_, i) => ({ id: 200 + i, generation: 1, building: i * 3, state: 'built', team: 1 + (i % 2), yaw: 0, pitch: 0, gunYaw: 0, gunPitch: 0, health: 400 }));
  const items = Array.from({ length: 8 }, (_, i) => ({ id: i, generation: 1, active: r() < 0.5, record: null }));
  const remotes = Array.from({ length: 3 }, (_, i) => ({ id: i, generation: 1, active: true, position: v3(1500), health: 300 }));
  const known = new Map();
  for (let tick = 2; tick < 2 + ticks; tick++) {
    const records = [], live = new Map();
    for (const item of items) {
      if (r() < 0.03) { item.active = !item.active; if (item.active) item.generation++; item.record = null; }
      // An unchanged item keeps its record object, as EntityItemCapture does;
      // a changed one may differ in a single value.
      const old = item.record?.active ? item.record : null;
      if (!item.record || item.active && r() < 0.3) item.record = item.active
        ? { kind: 'item', id: item.id, generation: item.generation, tick, active: true, definitionId: old && r() < 0.7 ? old.definitionId : ri(r, 1, 9), position: old && r() < 0.6 ? old.position : v3(1500),
          velocity: r() < 0.5 ? [0, 0, 0] : v3(6), yaw: old && r() < 0.6 ? old.yaw : r() * 7 - 1, up: old && r() < 0.5 ? old.up : [r() - 0.5, r(), r() - 0.5] }
        : { kind: 'item', id: item.id, generation: item.generation, tick, active: false };
      records.push(item.record);
    }
    for (const remote of remotes) {
      if (r() < 0.02) { remote.active = !remote.active; if (remote.active) remote.generation++; }
      if (r() < 0.3) remote.position = v3(1500);
      if (r() < 0.2) remote.health = ri(r, 0, 300);
      records.push(remote.active ? { kind: 'remote', id: remote.id, generation: remote.generation, tick, active: true, definitionId: 5, team: 1, position: remote.position.slice(), velocity: [0, 0, 0], yaw: 1, pitch: 0, health: remote.health }
        : { kind: 'remote', id: remote.id, generation: remote.generation, tick, active: false });
    }
    for (const s of soldiers) {
      if (s.alive && r() < 0.01) { s.alive = false; continue; }
      if (!s.alive) { if (r() < 0.05) { s.alive = true; s.generation++; s.position = v3(1500); } else continue; }
      if (r() < 0.02) s.position = v3(1500); // a teleport
      else if (r() < 0.8) s.position = s.position.map(x => x + (r() - 0.5) * 0.6);
      // Fast, at rest, on either side of the narrow form's 31.75 m/s, or
      // beyond any range (the wide form, clamped).
      if (r() < 0.3) s.velocity = r() < 0.05 ? [Infinity, -Infinity, 0] : r() < 0.1 ? pick(r, [[31.75, -31.75, 31.75], [0, 31.76, 0], [0, 0, -31.76]]) : r() < 0.2 ? [0, -0, 0] : v3(r() < 0.2 ? 80 : 20);
      if (r() < 0.04) s.seat = s.seat ? null : r() < 0.6 ? { vehicle: pick(r, vehicles), index: ri(r, 0, 2) } : { vehicle: pick(r, turrets), index: 0 };
      const still = r() < 0.4;
      const record = { kind: 'soldier', id: s.id, generation: s.generation, tick, classId: s.classId, team: s.team, position: s.position.slice(), velocity: s.velocity.slice(),
        yaw: still ? s.yaw ?? 0 : (s.yaw = r() * 20 - 10), pitch: still ? s.pitch ?? 0 : (s.pitch = r() * 3 - 1.5), health: still ? s.health ?? 100 : (s.health = ri(r, 0, 300)),
        posture: ri(r, 0, 2), state: still ? 0 : ri(r, 0, 8), weapon: still ? 3 : ri(r, 0, 9), fireCounter: s.fire = (s.fire ?? 0) + (r() < 0.2 ? 1 : 0), charge: still ? 0 : ri(r, 0, 5) };
      if (s.droideka) {
        // Its animation goes as f32 and is kept when equal (so 0 keeps -0).
        const zero = r() < 0.5 ? 0 : -0;
        Object.assign(record, { kind: 'droideka', mode: ri(r, 0, 3), reverse: r() < 0.3, dead: false, shieldOn: r() < 0.5, shield: still ? 1 : Math.fround(r()), time: still ? zero : Math.fround(r() * 3), roll: zero, lean: r() < 0.5 ? 0.25 : -0.25, legs: 1 });
      }
      live.set(s.id, record); records.push(record);
      if (s.seat || s.wasSeated) {
        const mount = s.seat ? s.seat.vehicle : null;
        records.push({ kind: 'seat', id: s.id, generation: s.generation, tick, vehicleId: mount?.id || 0, vehicleGeneration: mount?.generation || 0, seatIndex: s.seat?.index || 0, leaseVersion: tick });
        s.wasSeated = !!s.seat;
      }
    }
    for (const v of vehicles) {
      if (v.alive && r() < 0.005) v.alive = false;
      else if (!v.alive && r() < 0.05) { v.alive = true; v.generation++; }
      if (!v.alive) continue;
      // Parked hulls keep still while their crew aims (the mount alone
      // changes), or turn on the spot (the orientation alone).
      const moving = r() < 0.5, turn = () => { const q = [r() - 0.5, r() - 0.5, r() - 0.5, r() - 0.5], n = Math.hypot(...q); v.q = q.map(x => x / n); };
      if (moving) { v.position = v.position.map(x => x + (r() - 0.5) * 2); turn(); } else if (r() < 0.2) turn();
      if (r() < 0.05) v.health = ri(r, 0, 2000);
      const record = { kind: 'vehicle', id: v.id, generation: v.generation, tick, classId: v.classId, team: v.team, position: v.position.slice(), velocity: moving ? v3(60) : [0, 0, 0], quaternion: v.q.slice(), health: v.health ?? 900, state: 5, power: 100 };
      live.set(v.id, record); records.push(record);
      if (!v.mount || r() < 0.3) v.mount = { slot: r() < 0.5 ? 2 : null, tilt: r() < 0.5 ? 0 : r() - 0.5, roll: 0, walkPhase: r() * 70, walkBlend: r() * 1.2, seats: Array.from({ length: v.seats }, () => ({ yaw: r() < 0.5 ? 0 : r() * 6, pitch: 0 })) };
      records.push({ kind: 'mount', id: v.id, generation: v.generation, tick, ...v.mount });
    }
    for (const t of turrets) {
      if (r() < 0.2) { t.yaw = r() * 6 - 3; t.gunPitch = r() - 0.5; }
      if (r() < 0.02) t.state = pick(r, ['built', 'destroyed', 'unbuilt']);
      if (r() < 0.01) t.team = 3 - t.team;
      records.push({ kind: 'turret', id: t.id, generation: t.generation, tick, building: t.building, state: t.state, team: t.team, health: t.health, yaw: t.yaw, pitch: t.pitch, gunYaw: t.gunYaw, gunPitch: t.gunPitch });
    }
    for (const [id, old] of known) if (!live.has(id)) { const tombstone = old.kind === 'tombstone' ? old : { kind: 'tombstone', id, generation: old.generation, tick }; records.push(tombstone); live.set(id, tombstone); }
    known.clear(); for (const [id, record] of live) known.set(id, record);
    frames.push({ tick, records });
  }
  return frames;
}

test('snapshot entries: the same entries as the reference capture, through deaths, seats, slot reuse and teleports', () => {
  for (const seed of [1, 2, 3]) {
    const tables = [new SnapshotSlotTable(), new reference.SnapshotSlotTable()];
    const ours = [], theirs = [];
    for (const { tick, records } of recordBattle(seed, 400)) {
      const actual = snapshotFromRecords({ tick, records, slots: tables[0], previous: ours.at(-1) ?? null });
      const expected = reference.snapshotFromRecords({ tick, records, slots: tables[1], previous: theirs.at(-1) ?? null });
      const label = `seed ${seed} tick ${tick}`;
      sameSnapshot(actual, expected, [[null, null], ...[1, 3].filter(n => ours.length >= n).map(n => [ours.at(-n), theirs.at(-n)])], label);
      sameTable(tables[0], tables[1], label);
      // An unchanged actor is the previous entry itself (the encoder skips it).
      for (const [slot, entry] of actual.slots) if (expected.slots.get(slot) === theirs.at(-1)?.slots.get(slot)) assert.equal(entry, ours.at(-1).slots.get(slot), `${label}: slot ${slot} reused`);
      ours.push(actual); theirs.push(expected);
    }
  }
});

// A world of stations, tows, a creature and a trap whose values change on
// and off the stagger's cadence, including values a JSON round trip alters
// (a NaN team, an undefined state, -0), beside common and unit fields.
function worldBattle(seed) {
  const r = rng(seed), vector = () => { const v = [r(), r(), r()]; return { toArray: () => v.slice() }; };
  const buildings = Array.from({ length: 14 }, (_, i) => ({ state: 'built', health: 500, team: 1 + (i % 3), mode: i % 4 ? null : 'idle', active: 0, pulse: 0, facing: 0, yaw: i, servedHealth: false, servedAmmo: false }));
  const turrets = [buildings[3], buildings[9]];
  const vehicles = Array.from({ length: 4 }, (_, i) => ({ alive: true, aiTarget: { id: 40 + i, generation: 1 }, towTarget: -1, towLocal: vector(), towLeg: '', towAngle: 0, towWinding: 0 }));
  const creature = { grabs: [1], slot: 0, values: [0, 0, 0, 0, -1, 0, 0], state() { return this.values.slice(); } };
  const trap = { values: [0, 0, 0], stateRecord() { return this.values.slice(); } };
  const game = { live: { buildings, animated: [creature], traps: [trap] }, turretSeats: { turrets }, vehicles: { vehicles } };
  const odd = [NaN, undefined, null, -0, 'two', 2.5, Infinity];
  return { game, step(tick) {
    for (const b of buildings) {
      if (b === buildings[0] || r() < 0.3) b.pulse = r(); // a timer changing every tick
      if (r() < 0.01) b.state = pick(r, ['built', 'destroyed', 'unbuilt', undefined, null]);
      if (r() < 0.01) b.team = r() < 0.5 ? pick(r, odd) : ri(r, 0, 3);
      if (r() < 0.02) b.health = r() < 0.2 ? Infinity : ri(r, 0, 500);
    }
    for (const v of vehicles) {
      if (r() < 0.01) { v.alive = !v.alive; if (v.alive) v.aiTarget.generation++; }
      if (r() < 0.05) { v.towTarget = ri(r, -1, 3); v.towLocal = vector(); v.towAngle = r(); }
    }
    if (r() < 0.05) creature.values[0] = ri(r, 0, 4);
    if (r() < 0.05) trap.values = [ri(r, 0, 3), tick, ri(r, 0, 1)];
    if (tick % 97 === 0) creature.slot = creature.slot >= 0 ? -1 : 0;
  } };
}
const summary = (id, schema, tick, bytes) => ({ kind: 'summary', id, generation: 1, tick, schema, payload: Uint8Array.from(bytes) });

// The host refills entries, positions and velocities from the snapshots its
// ring lets go (EntrySpare). Every snapshot the ring still holds stays the
// reference's, so a baseline of any age encodes as it did, through deaths,
// seats, slot reuse and teleports; and once the ring is full most new
// entries are refilled ones, not new objects.
test('entries refilled from snapshots the ring lets go leave every held snapshot as the reference\'s', () => {
  for (const seed of [4, 5]) {
    const spare = new EntrySpare(), ring = new SnapshotRing(128, (snapshot, held) => spare.recycle(snapshot, held.get(held.oldestTick)));
    const table = new SnapshotSlotTable(), referenceTable = new reference.SnapshotSlotTable(), expected = new Map();
    let previous = null, theirs = null, made = 0, refilled = 0;
    for (const { tick, records } of recordBattle(seed, 700)) {
      const before = { soldier: spare.byClass.soldier.length, pos: spare.pos.length };
      const actual = snapshotFromRecords({ tick, records, slots: table, previous, spare });
      const wanted = reference.snapshotFromRecords({ tick, records, slots: referenceTable, previous: theirs });
      if (tick > 300) { refilled += before.soldier - spare.byClass.soldier.length; for (const [slot, entry] of actual.slots) if (entry.cls === 'soldier' && entry !== previous.slots.get(slot)) made++; }
      ring.set(tick, 1, actual); expected.set(tick, wanted); expected.delete(tick - 128);
      const label = `seed ${seed} tick ${tick}`;
      // The newest snapshot against held baselines of several ages, and
      // every held snapshot every 50 ticks.
      const bases = [1, 16, 64, 127].filter(age => ring.has(tick - age)).map(age => [ring.get(tick - age), expected.get(tick - age)]);
      sameSnapshot(actual, wanted, [[null, null], ...bases], label);
      if (tick % 50 === 0) for (const held of ring.ticks) sameSnapshot(ring.get(held), expected.get(held), [], `${label}: held ${held}`);
      previous = actual; theirs = wanted;
    }
    assert.ok(made > 1000 && refilled > 0.9 * made, `seed ${seed}: ${refilled} of ${made} new soldier entries refilled`);
  }
});

test('world fields: stations and tows the stagger keeps are never built, and the world is the reference\'s', () => {
  for (const seed of [11, 12]) {
    const { game, step } = worldBattle(seed);
    const staggers = [new WorldFieldStagger(), new reference.WorldFieldStagger()];
    // One post's record is the same object every time, so it is offered
    // again on its due ticks too.
    const steady = summary(3, 2, 0, [3]);
    let previous = null;
    for (let tick = 1; tick < 700; tick++) {
      step(tick);
      // Common fields when due, as the host offers them; unit fields come and go.
      const complete = new Set([7, 8, 9, 10]), common = [];
      if (staggers[1].anyDue(tick, new Set([1, 2]))) { common.push(summary(0, 1, tick, [tick & 255]), summary(1, 2, tick, [1, tick % 7]), summary(2, 2, tick, [2]), steady); complete.add(1).add(2); }
      const units = [];
      for (let id = 1; id <= 12; id++) {
        if ((id * 31 + tick) % 211 === 0) continue;
        const due = staggers[1].isDue(tick, `6:${100000 + id}`);
        assert.equal(staggers[0].isFieldDue(tick, 6, 100000 + id), due, `tick ${tick}: unit ${id} due`);
        if (due) units.push(summary(100000 + id, 6, tick, [id, tick % 5]));
      }
      const expected = reference.snapshotFromRecords({ tick, records: [], worldRecords: staggers[1].update(tick, [...common, ...units, ...reference.captureWorldPresentation(game, tick, 1)], complete), slots: new reference.SnapshotSlotTable(), previous });
      // A field held is built only when the stagger then takes it.
      const fresh = new Set();
      const presentation = captureWorldPresentation(game, tick, 1, (schema, id, stateTeam) => { const kept = staggers[0].held(tick, schema, id, stateTeam); if (kept === undefined && staggers[0].entry(schema, id)) fresh.add(id); return kept; });
      staggers[0].take(tick, [...common, ...units, ...presentation], complete);
      for (const record of presentation) if (fresh.has(record.id)) assert.equal(staggers[0].entry(record.schema, record.id).record, record, `seed ${seed} tick ${tick}: ${record.schema}:${record.id} built and not taken`);
      const world = staggers[0].world();
      assert.deepStrictEqual([...world.keys()], [...expected.world.keys()], `seed ${seed} tick ${tick}: world order`);
      for (const [key, field] of expected.world) assert.deepStrictEqual(world.get(key), field, `seed ${seed} tick ${tick}: ${key}`);
      assert.deepStrictEqual(staggers[0].values(), staggers[1].values(), `seed ${seed} tick ${tick}: values`);
      assert.equal(staggers[0].anyDue(tick + 1, new Set([1, 2, 4, 5])), staggers[1].anyDue(tick + 1, new Set([1, 2, 4, 5])));
      assert.equal(staggers[0].schemaDue(tick + 1, new Set([4])), staggers[1].schemaDue(tick + 1, new Set([4])));
      previous = expected;
    }
  }
  // Once every station and tow has been taken, a tick builds only the ones
  // due (a station every 48 ticks, a tow every 12); the rest go as held.
  const { game } = worldBattle(5), stagger = new WorldFieldStagger();
  let built = 0, offered = 0;
  for (let tick = 1; tick <= 200; tick++) {
    const held = (schema, id, stateTeam) => { const kept = stagger.held(tick, schema, id, stateTeam); if (tick > 100) { offered++; if (kept === undefined) built++; } return kept; };
    stagger.take(tick, captureWorldPresentation(game, tick, 1, held), new Set([7, 8, 9, 10]));
  }
  assert.equal(offered, 100 * 16);
  assert.ok(built <= 100 * (12 / 48 + 4 / 12) + 1, `${built} of ${offered} built`);
});

// Summaries were encoded once just to check them; they are now checked as
// encoding would check them, with the same errors, and not encoded.
test('a payload record is checked as encoding it checks it, without encoding it', () => {
  const r = rng(13), odd = [0, 1, -1, 1.5, 2 ** 32 - 1, 2 ** 32, NaN, '3', null];
  const attempt = check => { try { check(); return 'fits'; } catch (error) { return error.message; } };
  for (let i = 0; i < 4000; i++) {
    const record = { kind: pick(r, ['summary', 'summary', 'correction', 'combat', 'bogus']), id: r() < 0.9 ? Math.floor(r() * 1e6) : pick(r, odd),
      generation: r() < 0.9 ? 1 : pick(r, odd), tick: r() < 0.9 ? Math.floor(r() * 1e5) : pick(r, odd), schema: r() < 0.9 ? Math.floor(r() * 12) : pick(r, [65535, 65536, -1, 2.5]),
      payload: r() < 0.95 ? new Uint8Array(pick(r, [0, 10, 900, 1051, 1052, 1083, 1084, 2000, 70000])) : pick(r, [[1, 2], null, 'abc']) };
    assert.equal(attempt(() => checkPayloadRecordFits(record)), attempt(() => encodeEntityRecord(record)), JSON.stringify({ ...record, payload: record.payload?.length }));
  }
});

// Canonical publication is checked against recorded complete transcripts
// and native frozen store fixtures; no live JS world producer is retained.
