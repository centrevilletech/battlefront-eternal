import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { encodeEntities } from '../src/runtime/snapshot-codec.js';
import { WorldFieldStagger } from '../src/runtime/snapshot-records.js';
import { captureWorldPresentation } from '../src/runtime/entity-world-state.js';
import * as reference from './fixtures/snapshot-capture-reference.js';

// Capture builds less than it did (no record for a station or tow the
// stagger keeps), but every snapshot must hold exactly what the frozen
// reference capture (fixtures/snapshot-capture-reference.js) put in it: the
// same entries in the same slots, the same world fields in the same order,
// so the same datagrams. A synthetic world covers the edges; a real battle,
// when game data is present, covers everything the world does.
function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const ri = (r, lo, hi) => lo + Math.floor(r() * (hi - lo + 1));
const pick = (r, list) => list[ri(r, 0, list.length - 1)];

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

const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
const owned = { skip: !existsSync(`${content}/maps/nab2/map.json`) };
// A real battle: the host publishes as usual while the reference capture
// takes the same world state each tick; every shared part the host sends
// must be what the reference snapshot encodes to against the same baseline.
async function realBattle(query, { ticks, local, clients }) {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost } = await import('../src/runtime/snapshot-host.js');
  const { EntityShotCounters } = await import('../src/runtime/network-entity-shots.js');
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const game = await headlessGame(query);
  await game.factory.prepareSoldiers();
  const world = createMultiplayerWorld(game, local ? { maxPlayers: 1, armySize: null } : { armySize: 16 });
  const clock = { ms: 0 };
  const host = createSnapshotHost({ game, world, local, now: () => clock.ms });
  for (const id of clients) {
    await world.addPlayer(id, { reliableFire: true, separateActions: true });
    world.players.get(id).autoRespawn = true;
    host.addClient(id); host.setStarted(id, true); host.setReady(id);
  }
  const shots = new EntityShotCounters(game.simEvents);
  const capture = reference.referenceCapture({ game, world, registry: host.registry, fireCounter: unit => shots.counter(unit) });
  const receivers = new Map(clients.map(id => [id, new SnapshotClient()])), acks = new Map(clients.map(id => [id, []]));
  const ring = new Map();
  let compared = 0, entities = 0;
  const interval = local ? 1 : 2;
  for (let frame = 0; frame < ticks; frame++) {
    world.step(); clock.ms += 1000 / 60;
    const out = host.publish({}), tick = game.worldClock.tick;
    assert.deepEqual(out.faults, []);
    // Every snapshot the host captured went out, so the reference captures
    // on exactly the host's ticks.
    if (tick % interval) { assert.equal(out.shared.length, 0); continue; }
    assert.ok(out.shared.length && out.shared.every(group => group.tick === tick), `tick ${tick} published`);
    const expected = capture(tick);
    ring.set(tick, expected); ring.delete(tick - 200);
    for (const group of out.shared) {
      const parts = encodeEntities(expected, group.baselineTick ? ring.get(group.baselineTick) : null, { interval });
      assert.deepEqual(group.parts.map(p => Buffer.from(p).toString('hex')), parts.map(p => Buffer.from(p).toString('hex')), `${query} tick ${tick} against ${group.baselineTick}`);
      compared++;
    }
    entities = Math.max(entities, expected.slots.size);
    // Each client acknowledges a couple of publications late, as a page does.
    for (const id of clients) {
      for (const buffer of publicationBuffersFor(out, id)) receivers.get(id).receive(new Uint8Array(buffer));
      const queue = acks.get(id); queue.push(receivers.get(id).snapshotAck.tick);
      while (queue.length > 2) { const ack = queue.shift(); if (ack) host.input(id, encodeSnapshotAckKeepalive({ epoch: 1, tick: ack })); }
    }
  }
  host.dispose(); shots.dispose();
  return { compared, entities };
}

test('a real battle publishes exactly the reference capture\'s snapshots (local host)', { ...owned, timeout: 300000 }, async () => {
  const { compared, entities } = await realBattle('?map=nab2&side=rep&ai=classic&seed=1', { ticks: 1500, local: true, clients: ['local'] });
  assert.ok(compared > 1400, `compared ${compared} publications`);
  assert.ok(entities > 40, `the bots were out (${entities} entities)`);
});

test('a real battle publishes exactly the reference capture\'s snapshots (online host, traps and tows)', { skip: owned.skip || !existsSync(`${content}/maps/end1/map.json`), timeout: 300000 }, async () => {
  const { compared, entities } = await realBattle('?map=end1&side=all&ai=classic&seed=1', { ticks: 1500, local: false, clients: ['a', 'b'] });
  assert.ok(compared > 700, `compared ${compared} publications`);
  assert.ok(entities > 20, `the bots were out (${entities} entities)`);
});
