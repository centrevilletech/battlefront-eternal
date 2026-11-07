import { contentPath } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { headlessGame } from '../tools/headless-world.mjs';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createSnapshotHost, publicationBuffersFor } from '../src/runtime/snapshot-host.js';
import { SnapshotClient } from '../src/runtime/snapshot-client.js';
import { encodeSnapshotAckKeepalive } from '../src/runtime/network-entity-input.js';
import { encodeEntities, readSnapshotHeader } from '../src/runtime/snapshot-codec.js';
import { decodeOwnDatagram } from '../src/runtime/snapshot-own.js';
import { packCosmeticEvents } from '../src/runtime/snapshot-cosmetics.js';
import { combatRecord } from '../src/runtime/network-entity-combat.js';

// Measure the real native host, including its cadence and chosen ACK baseline.
// Re-encoding decoded entity state only separates its bytes from world fields;
// it does not walk authority facades or construct an authoritative checkpoint.
const content = contentPath();
const pct = (values, p) => values.length ? [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * p))] : null;
test('snapshot sizes on real 32 + 32 captures', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 32 });
  await world.addPlayer('size', { reliableFire: true, separateActions: true });
  const actor = world.players.get('size'); actor.autoRespawn = true;
  let at = 0;
  const host = createSnapshotHost({ game, world, now: () => at });
  host.addClient('size'); host.setStarted('size', true); host.setReady('size');
  const client = new SnapshotClient();
  const partsPrev = [], bytesPrev = [], entitiesBytes = [], ownDelta = [], ownAbs = [], recoveryParts = [];
  function publication(frame, measure) {
    at += 1000 / 60;
    host.input('size', encodeSnapshotAckKeepalive(client.snapshotAck.tick ? client.snapshotAck : { epoch: 1, tick: 0 }));
    world.input('size', { epoch: 1, sequence: frame + 1, tick: game.worldClock.tick + 1, generation: actor.unit.generation,
      down: frame % 2 ? ['KeyW', 'Mouse0'] : ['Mouse0'], actions: [], look: [] });
    world.step();
    const out = host.publish({ at }); assert.deepEqual(out.faults, []);
    const prior = client.current;
    for (const packet of out.ownPackets) {
      const bytes = new Uint8Array(packet.buffer), header = readSnapshotHeader(bytes);
      const base = client.ownRing.get(header.baselineTick, header.epoch);
      const decoded = decodeOwnDatagram(bytes, base?.payload, base?.summaries);
      if (measure && decoded.correction) {
        ownAbs.push(decoded.correction.payload.length);
        ownDelta.push(new DataView(bytes.buffer, bytes.byteOffset).getUint16(21, true) - 10);
      }
    }
    for (const bytes of publicationBuffersFor(out, 'size')) client.receive(new Uint8Array(bytes));
    if (measure && out.shared.length) {
      const current = client.current;
      const parts = encodeEntities({ ...current, world: new Map() }, { ...prior, world: new Map() });
      entitiesBytes.push(parts.reduce((n, part) => n + part.length, 0));
      assert.equal(parts.length, 1, 'no-loss infantry entities must fit one datagram');
      partsPrev.push(out.shared[0].parts.length); bytesPrev.push(out.shared[0].bytes);
      recoveryParts.push(encodeEntities({ ...current, world: new Map() }, null).length);
    }
    return out;
  }
  try {
    for (let frame = 0; frame < 900; frame++) publication(frame, frame >= 600);
    assert.ok(partsPrev.length >= 100, `measured ${partsPrev.length} snapshots`);
    assert.equal(pct(partsPrev, .5), 1, 'median no-loss snapshot is one entities datagram');
    assert.ok(pct(ownDelta, .95) < 200, `infantry own delta p95 ${pct(ownDelta, .95)} bytes < 200`);
    assert.ok(Math.max(...recoveryParts) <= 3, 'empty-baseline entity recovery fits three parts');

    // The client codec's independent size guard still covers 100 launches
    // and impacts. Production's retained packer is checked by native goldens.
    const events = [];
    for (let i = 0; i < 100; i++) {
      for (const schema of [1, 2]) events.push(combatRecord({ id: 2 * i + schema, generation: 1, tick: 1000 + i, schema, expiryTick: 1400,
        definitionId: 12, ownerId: 3, ownerGeneration: 1, fireId: i, launchTick: 1000 + i, executionTick: 1000 + i,
        position: [i - 50, 2, i], direction: [0, .1, 1], offset: [0, 0, 0], speed: 60, life: 2, merge: .4, surface: 5 }));
    }
    const cosmetics = packCosmeticEvents(events, 1 << 20); assert.equal(cosmetics.dropped.length, 0);

    let pilotAbs = null, pilotDelta = null;
    for (const vehicle of game.vehicles?.vehicles ?? []) {
      if (!vehicle.alive) continue;
      if (vehicle.seats?.[0]?.lease?.unit) game.vehicles.unboard(vehicle.seats[0].lease.unit, { force: true });
      game.vehicles.board(actor.unit, vehicle, 0);
      if (actor.unit.seat?.index === 0) break;
    }
    assert.equal(actor.unit.seat?.index, 0, 'real pilot size case');
    const before = ownDelta.length;
    for (let frame = 900; frame < 930; frame++) publication(frame, false);
    // Decode actual pilot packets without using a JS correction producer.
    for (let frame = 930; frame < 960; frame++) {
      const out = publication(frame, false);
      for (const packet of out.ownPackets) {
        const bytes = new Uint8Array(packet.buffer); if (!(bytes[20] & 1)) continue;
        ownDelta.push(new DataView(bytes.buffer).getUint16(21, true) - 10);
      }
      pilotAbs = client.ownRing.get(client.snapshotAck.tick, 1)?.payload?.length ?? pilotAbs;
    }
    pilotDelta = pct(ownDelta.slice(before), .95); assert.ok(pilotDelta < 400, `pilot own delta p95 ${pilotDelta} bytes < 400`);
    console.log(JSON.stringify({ slots: client.current.slots.size, worldFields: client.current.world.size,
      entitiesBytes: { p50: pct(entitiesBytes, .5), p95: pct(entitiesBytes, .95), max: Math.max(...entitiesBytes) },
      totalBytesVsPrevious: { p50: pct(bytesPrev, .5), p95: pct(bytesPrev, .95) }, partsVsPrevious: { p50: pct(partsPrev, .5), max: Math.max(...partsPrev) },
      ownAbsolute: { p50: pct(ownAbs, .5) }, ownDelta: { p50: pct(ownDelta.slice(0, before), .5), p95: pct(ownDelta.slice(0, before), .95) },
      recoveryParts: { p50: pct(recoveryParts, .5), max: Math.max(...recoveryParts) }, cosmetics100ShotsBytes: cosmetics.bytes.length, pilotOwnAbsolute: pilotAbs, pilotOwnDeltaP95: pilotDelta }));
  } finally { host.dispose(); }
});
