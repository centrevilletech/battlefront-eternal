import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

// Phase 1 size gate (docs/snapshot-delta-design.md): real 32 + 32 captures
// from a headless battle, encoded as snapshot deltas. The design fails the
// test when the no-loss infantry snapshot needs more than one entities
// datagram. World fields ride on the staggered refresh cadence the host
// uses; their bytes are reported separately so the breakdown stays visible.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
const pct = (values, p) => values.length ? [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * p))] : null;

test('snapshot sizes on real 32 + 32 captures', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createEntityRegistry } = await import('../src/runtime/network-entity-registry.js');
  const { INPUT_CODES, DEFAULT_BINDING_PROFILES } = await import('../src/runtime/network-entity-input.js');
  const { entityPostNames } = await import('../src/runtime/entity-spawn-posts.js');
  const { EntityWorldCapture, capturePlayerCorrection } = await import('../src/runtime/network-entity-capture.js');
  const { captureUnitState } = await import('../src/runtime/entity-unit-state.js');
  const { captureWorldPresentation } = await import('../src/runtime/entity-world-state.js');
  const { captureWorldSummaries } = await import('../src/runtime/network-entity-summary.js');
  const { EntityShotCounters } = await import('../src/runtime/network-entity-shots.js');
  const { snapshotFromRecords, SnapshotSlotTable, WorldFieldStagger } = await import('../src/runtime/snapshot-records.js');
  const { encodeEntities } = await import('../src/runtime/snapshot-codec.js');
  const { encodeOwnDelta, encodeOwnDatagram, ownDatagramBudget } = await import('../src/runtime/snapshot-own.js');
  const { packCosmeticEvents } = await import('../src/runtime/snapshot-cosmetics.js');
  const { combatRecord } = await import('../src/runtime/network-entity-combat.js');

  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 32 });
  await world.addPlayer('size', { reliableFire: true, separateActions: true });
  const actor = world.players.get('size'); actor.autoRespawn = true;
  const registry = createEntityRegistry(game.data, { contentEpoch: 1, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES, posts: entityPostNames(game) });
  const shots = new EntityShotCounters(game.simEvents);
  const capture = new EntityWorldCapture(game, registry, { fireCounter: u => shots.counter(u) });
  const slots = new SnapshotSlotTable(), stagger = new WorldFieldStagger(), COMPLETE = new Set([1, 2, 4, 6, 7, 8]);
  const snapshots = [], ownPayloads = new Map();
  const partsPrev = [], bytesPrev = [], entitiesBytes = [], worldBytes = [], ownDelta = [], ownAbs = [], recoveryParts = [];
  let previous = null;
  for (let frame = 0; frame < 900; frame++) {
    world.input('size', { epoch: 1, sequence: frame + 1, tick: frame + 1, generation: actor.unit.generation, down: frame % 2 ? ['KeyW', 'Mouse0'] : ['Mouse0'], actions: [], look: [] });
    world.step();
    const tick = game.worldClock.tick;
    if (frame < 600 || tick % 2 !== 0) continue;
    for (const [slot, c] of game.battle.characters.entries()) c.rosterSlot = slot;
    const records = capture.capture(tick);
    const unitStates = [...game.units.all()].filter(u => u.soldier && u.combat && !u.removed).map(u => captureUnitState(u, registry, tick));
    const worldRecords = stagger.update(tick, [...captureWorldSummaries(game, world, registry, { tick, epoch: 1 }), ...captureWorldPresentation(game, tick, 1), ...unitStates], COMPLETE);
    const snap = snapshotFromRecords({ tick, records, worldRecords, slots, previous });
    const prevSnap = snapshots.at(-1);
    if (prevSnap) {
      const parts = encodeEntities(snap, prevSnap);
      partsPrev.push(parts.length); bytesPrev.push(parts.reduce((n, p) => n + p.length, 0));
      const entitiesOnly = encodeEntities({ ...snap, world: new Map() }, { ...prevSnap, world: new Map() });
      entitiesBytes.push(entitiesOnly.reduce((n, p) => n + p.length, 0));
      assert.equal(entitiesOnly.length, 1, 'no-loss infantry entities must fit one datagram');
      worldBytes.push(bytesPrev.at(-1) - entitiesBytes.at(-1));
      recoveryParts.push(encodeEntities({ ...snap, world: new Map() }, null).length);
      const own = capturePlayerCorrection(game, world, 'size', { registry, bindingProfileId: 0, epoch: 1 });
      ownAbs.push(own.payload.length);
      const prevOwn = ownPayloads.get(prevSnap.tick);
      if (prevOwn) ownDelta.push(encodeOwnDelta(own.payload, prevOwn).length);
      ownPayloads.set(tick, own.payload);
    }
    snapshots.push(snap); previous = snap;
    if (snapshots.length > 40) snapshots.shift();
  }
  assert.ok(partsPrev.length >= 100, `measured ${partsPrev.length} snapshots`);
  assert.equal(pct(partsPrev, 0.5), 1, 'median no-loss snapshot is one entities datagram');
  assert.ok(pct(ownDelta, 0.95) < 200, `infantry own delta p95 ${pct(ownDelta, 0.95)} bytes < 200`);
  assert.ok(Math.max(...recoveryParts) <= 3, `empty-baseline entities recovery is ${Math.max(...recoveryParts)} parts`);

  // 100 shots per second of cosmetics: 100 launches + 100 impacts.
  const events = [];
  for (let i = 0; i < 100; i++) {
    events.push(combatRecord({ id: 2 * i + 1, generation: 1, tick: 1000 + i, schema: 1, expiryTick: 1400, definitionId: 12, ownerId: 3, ownerGeneration: 1, fireId: i, launchTick: 1000 + i, executionTick: 1000 + i, position: [i - 50, 2, i], direction: [0, 0.1, 1], offset: [0, 0, 0], speed: 60, life: 2, merge: 0.4, surface: 5 }));
    events.push(combatRecord({ id: 2 * i + 2, generation: 1, tick: 1001 + i, schema: 2, expiryTick: 1400, definitionId: 12, ownerId: 3, ownerGeneration: 1, fireId: i, launchTick: 1000 + i, executionTick: 1000 + i, position: [i - 40, 1, i + 5], direction: [0, 1, 0], surface: 1 }));
  }
  const { bytes: cosmeticBytes, dropped } = packCosmeticEvents(events, 1 << 20);
  assert.equal(dropped.length, 0);

  // A pilot's own datagram: board the first live hull and measure again.
  let pilotDelta = null, pilotAbs = null;
  for (const vehicle of game.vehicles?.vehicles ?? []) {
    if (!vehicle.alive) continue;
    const seat0 = vehicle.seats?.[0];
    if (seat0?.lease?.unit) game.vehicles.unboard(seat0.lease.unit, { force: true });
    actor.unit.soldier.position.copy(vehicle.position);
    game.vehicles.board(actor.unit, vehicle);
    if (actor.unit.seat?.vehicle === vehicle && actor.unit.seat.index === 0) break;
  }
  if (actor.unit.seat?.index === 0) {
    let prevPilot = null; const deltas = [];
    for (let i = 0; i < 30; i++) {
      world.input('size', { epoch: 1, sequence: 5000 + i, tick: game.worldClock.tick + 1, generation: actor.unit.generation, down: ['KeyW'], actions: [], look: [] });
      world.step();
      if (game.worldClock.tick % 2) continue;
      const own = capturePlayerCorrection(game, world, 'size', { registry, bindingProfileId: 0, epoch: 1 });
      pilotAbs = own.payload.length;
      if (prevPilot) deltas.push(encodeOwnDelta(own.payload, prevPilot).length);
      prevPilot = own.payload;
    }
    pilotDelta = pct(deltas, 0.95);
    assert.ok(pilotDelta < 400, `pilot own delta p95 ${pilotDelta} bytes < 400`);
  }

  // The measured breakdown, printed into the test log for the phase report.
  console.log(JSON.stringify({
    slots: previous.slots.size, worldFields: previous.world.size,
    entitiesBytes: { p50: pct(entitiesBytes, 0.5), p95: pct(entitiesBytes, 0.95), max: Math.max(...entitiesBytes) },
    worldBytes: { p50: pct(worldBytes, 0.5), p95: pct(worldBytes, 0.95), max: Math.max(...worldBytes) },
    totalBytesVsPrevious: { p50: pct(bytesPrev, 0.5), p95: pct(bytesPrev, 0.95) },
    partsVsPrevious: { p50: pct(partsPrev, 0.5), max: Math.max(...partsPrev) },
    ownAbsolute: { p50: pct(ownAbs, 0.5) }, ownDelta: { p50: pct(ownDelta, 0.5), p95: pct(ownDelta, 0.95) },
    recoveryParts: { p50: pct(recoveryParts, 0.5), max: Math.max(...recoveryParts) },
    cosmetics100ShotsBytes: cosmeticBytes.length, pilotOwnAbsolute: pilotAbs, pilotOwnDeltaP95: pilotDelta,
  }));
});
