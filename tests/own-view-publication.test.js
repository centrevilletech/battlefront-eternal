import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { contentPath } from './helpers/content.js';
import { headlessGame } from '../tools/headless-world.mjs';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createSnapshotHost, publicationBuffersFor } from '../src/runtime/snapshot-host.js';
import { SnapshotClient } from './helpers/network-capture.js';
import { decodeOwnPlayer, OWN_PLAYER_SCHEMA } from '../src/runtime/network-entity-player.js';

// The correction carries the consumed view, including before a weapon draws.
// A dormant combat mirror cannot replace the mailbox's retained perspective.
test('own corrections preserve first-person switches from consumed input', {
  skip: !existsSync(`${contentPath()}/maps/nab2/map.json`),
}, async () => {
  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 16 });
  await world.addPlayer('eye', { reliableFire: true, separateActions: true });
  const host = createSnapshotHost({ game, world });
  host.addClient('eye'); host.setStarted('eye', true); host.setReady('eye');
  const receiver = new SnapshotClient();
  let sequence = 0;
  try {
    for (const firstPerson of [true, false, true]) {
      for (let i = 0; i < 6; i++) {
        const actor = world.players.get('eye'), tick = game.worldClock.tick + 1;
        world.input('eye', { epoch: 1, sequence: ++sequence, tick, generation: actor.unit.generation,
          down: [], actions: [], look: [], firstPerson, aspect: 16 / 9 });
        world.step();
        const publication = host.publish({});
        assert.deepEqual(publication.faults, []);
        for (const bytes of publicationBuffersFor(publication, 'eye')) receiver.receive(new Uint8Array(bytes));
      }
      const snapshot = receiver.capture().snapshot;
      assert.ok(snapshot?.own.payload, 'native publisher emitted an own correction');
      const correction = decodeOwnPlayer({ kind: 'correction', schema: OWN_PLAYER_SCHEMA,
        payload: Uint8Array.from(snapshot.own.payload) });
      assert.equal(correction.view.firstPerson, firstPerson, 'consumed input view');
      assert.equal(correction.firstPerson, firstPerson, 'published presentation view');
    }
  } finally { receiver.dispose(); host.dispose(); }
});
