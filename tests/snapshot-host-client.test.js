import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

// Phase 2 loopback: the snapshot host publishes a real battle, the snapshot
// client applies it and acknowledges complete snapshots through real input
// datagrams, and a dropped-datagram stretch still converges because the
// next snapshot is encoded against the older acknowledged baseline.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;

test('snapshot host and client converge through acks and datagram loss', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost, publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { encodeEntityInput } = await import('../src/runtime/network-entity-input.js');
  const { decodeOwnPlayer } = await import('../src/runtime/network-entity-player.js');

  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 32 });
  await world.addPlayer('loop', { reliableFire: true, separateActions: true });
  const actor = world.players.get('loop'); actor.autoRespawn = true;
  const host = createSnapshotHost({ game, world });
  host.addClient('loop'); host.setStarted('loop', true); host.setReady('loop');
  const client = new SnapshotClient();
  const registry = host.registry;
  let sequence = 0, corrections = 0, soldierRecords = 0, lastAckTick = 0, lostStretchSeen = false;
  const ownPayloads = new Map();
  for (let frame = 0; frame < 720; frame++) {
    const tick = game.worldClock.tick + 1;
    // The client's input for this tick, carrying its snapshot ack.
    const bytes = encodeEntityInput({ epoch: 1, generation: actor.unit.generation,
      commands: [{ epoch: 1, generation: actor.unit.generation, sequence: ++sequence, tick, down: frame % 2 ? ['KeyW'] : [], lookTotal: [0, 0] }],
      edges: [], snapshotAck: client.snapshotAck.tick ? client.snapshotAck : null }, registry);
    host.input('loop', bytes);
    assert.deepEqual(host.advanceEdges(), []);
    world.step();
    assert.deepEqual(host.consumedEdges(game.worldClock.tick), []);
    const out = host.publish({});
    assert.deepEqual(out.faults, []);
    // Lose every datagram for a 30-tick stretch in the middle.
    const dropping = frame >= 400 && frame < 430;
    const buffers = publicationBuffersFor(out, 'loop');
    if (dropping && buffers.length) lostStretchSeen = true;
    if (dropping) continue;
    for (const buffer of buffers) {
      const applied = client.receive(new Uint8Array(buffer));
      if (!applied) continue;
      for (const record of applied.records) {
        if (record.kind === 'soldier') soldierRecords++;
        if (record.kind === 'correction') { corrections++; decodeOwnPlayer(record); ownPayloads.set(record.tick, record.payload); }
      }
    }
    assert.ok(client.snapshotAck.tick >= lastAckTick, 'ack never moves backward');
    lastAckTick = client.snapshotAck.tick;
  }
  assert.ok(lostStretchSeen, 'the loss stretch actually dropped datagrams');
  assert.ok(corrections > 100, `own corrections flow (${corrections})`);
  assert.ok(soldierRecords > 1000, `remote soldiers flow (${soldierRecords})`);
  assert.ok(client.snapshotAck.tick > 650, `client acknowledged up to ${client.snapshotAck.tick}`);
  assert.equal(client.missingBaselines, 0, 'the server never names a baseline the client does not hold');
  host.input('loop', encodeEntityInput({ epoch: 1, generation: actor.unit.generation, commands: [], edges: [], snapshotAck: client.snapshotAck }, registry));
  const hostState = host.inspect('loop');
  assert.equal(hostState.baselineTick, client.snapshotAck.tick, 'host baseline follows the client ack');
  // The client's ring holds the acknowledged snapshot with the field's soldiers in it.
  const held = client.ring.get(client.snapshotAck.tick, 1);
  assert.ok(held && [...held.slots.values()].filter(e => e.cls === 'soldier' || e.cls === 'droideka').length >= 40, 'ring snapshot carries the armies');
});

test('publication shares one copy of the entity parts across clients on the same baseline', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost, publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');

  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 32 });
  const host = createSnapshotHost({ game, world });
  for (const id of ['alpha', 'beta']) {
    await world.addPlayer(id, { reliableFire: true, separateActions: true });
    world.players.get(id).autoRespawn = true;
    host.addClient(id); host.setStarted(id, true); host.setReady(id);
  }
  let checked = 0;
  for (let frame = 0; frame < 240 && checked < 20; frame++) {
    world.step();
    const out = host.publish({});
    assert.deepEqual(out.faults, []);
    if (!out.shared.length) continue;
    checked++;
    // Neither client acks, so both sit on the empty baseline: exactly one
    // shared group, listing both, whose parts are the single encoded copy.
    assert.equal(out.shared.length, 1, 'one baseline group for both clients');
    const [group] = out.shared;
    assert.deepEqual([...group.clients].sort(), ['alpha', 'beta']);
    assert.ok(group.parts.length >= 1 && group.parts.every(p => p instanceof ArrayBuffer));
    const a = publicationBuffersFor(out, 'alpha'), b = publicationBuffersFor(out, 'beta');
    assert.equal(a.length, group.parts.length + 1, 'parts then one own datagram');
    assert.equal(b.length, a.length);
    for (let i = 0; i < group.parts.length; i++) {
      assert.equal(a[i], group.parts[i], 'expansion hands out the shared buffer itself, not a copy');
      assert.equal(b[i], group.parts[i], 'both clients expand to the same shared buffer');
    }
    assert.notEqual(a.at(-1), b.at(-1), 'own datagrams stay per client');
    assert.equal(host.inspect('alpha').datagrams, host.inspect('beta').datagrams, 'metrics count shared parts per client as before');
  }
  assert.ok(checked >= 20, `checked ${checked} publications`);
});

test('reliable gameplay records before the first snapshot acknowledge nothing', async () => {
  // The events lane can beat the first state datagram; epoch 0 is not a
  // baseline and a session rejects it as an acknowledgement.
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const client = new SnapshotClient();
  const early = client.gameplayRecords([], 120);
  assert.equal(early.snapshotAck, null);
  assert.equal(early.epoch, 0);
});
