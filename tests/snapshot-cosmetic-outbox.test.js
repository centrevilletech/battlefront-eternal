import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { CosmeticOutbox, cosmeticEvent, RESEND_TICKS } from '../src/runtime/snapshot-cosmetic-outbox.js';
import { combatRecord, decodeCombat } from '../src/runtime/network-entity-combat.js';
import { projectileRecord } from '../src/runtime/network-combat-projectile.js';
import { explosionRecord } from '../src/runtime/network-combat-explosion.js';
import { presentationRecord } from '../src/runtime/entity-presentation-events.js';
import { encodeCosmeticEvent, unpackCosmeticEvents } from '../src/runtime/snapshot-cosmetics.js';

// What a client is owed of the cosmetic combat events: launches, first
// samples, terminals and explosions until a snapshot that carried them is
// acknowledged; refinements once, while there is space.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
const here = { x: 0, y: 0, z: 0 };
const event = record => cosmeticEvent(record, decodeCombat(record), encodeCosmeticEvent(record));
const launch = (id, { tick = 10, x = 10, owner = 9, life = 1, direction = [1, 0, 0] } = {}) => event(combatRecord({ id, generation: 1, tick, schema: 1, expiryTick: tick + 200, definitionId: 1, ownerId: owner, ownerGeneration: 1, position: [x, 0, 0], direction, speed: 100, life, merge: 8 }));
const terminal = (id, { tick = 12, x = 10 } = {}) => event(combatRecord({ id, generation: 1, tick, schema: 2, expiryTick: tick + 200, definitionId: 1, ownerId: 9, ownerGeneration: 1, launchTick: 10, executionTick: 10, position: [x, 0, 0], direction: [0, 1, 0], surface: 1 }));
const sample = (id, tick, x = 10) => event(projectileRecord({ id, generation: 1, tick, definitionId: 1, type: 'round', ownerId: 9, ownerGeneration: 1, launchTick: 10, position: [x, 0, 0], velocity: [40, 0, 0], life: 2 }));
const shot = (id, tick = 10, x = 10) => event(presentationRecord({ id, generation: 1, tick, event: { kind: 'shot', ownerId: 9, ownerGeneration: 1, weaponId: 1, fireId: 0, position: [x, 0, 0] } }));
const ids = bytes => bytes ? unpackCosmeticEvents(bytes).map(r => `${r.schema}:${r.id}`) : [];
const pack = (box, tick, { first = 1000, second = 1000, confirmable = true, spare = Infinity } = {}) => { const out = box.pack(tick, first, second, confirmable, spare); return [...ids(out.first), ...ids(out.second)]; };

test('a launch is owed until a snapshot that carried it is acknowledged, and sent again when a later one is', () => {
  const box = new CosmeticOutbox();
  box.offer([launch(1)], here, 7);
  assert.deepEqual(pack(box, 10), ['1:1']);
  assert.deepEqual(pack(box, 12), [], 'in flight: not repeated on every snapshot');
  box.acknowledge(12); // the client completed a later snapshot, not this one
  assert.equal(box.resent, 1);
  assert.deepEqual(pack(box, 14), ['1:1'], 'presumed lost, sent again');
  box.acknowledge(14);
  assert.deepEqual(pack(box, 16), []);
  assert.equal(box.entries.size, 0);
  assert.equal(box.dropped, 0);
});

test('an unacknowledged send is presumed lost after RESEND_TICKS; nothing is owed past its window', () => {
  const box = new CosmeticOutbox();
  box.offer([launch(1, { life: 1 })], here, 7);
  assert.deepEqual(pack(box, 10), ['1:1']);
  assert.deepEqual(pack(box, 10 + RESEND_TICKS - 2), []);
  assert.deepEqual(pack(box, 10 + RESEND_TICKS), ['1:1']);
  // A bolt flying a second can start late by LAUNCH_PRESENTATION_TICKS: past
  // that it is no longer worth sending.
  assert.deepEqual(pack(box, 10 + 60 + 25), []);
  assert.equal(box.entries.size, 0);
  assert.equal(box.dropped, 0, 'it was in flight when it expired: not counted as never sent');
});

test('a full datagram makes events wait for the next snapshot, owed ones first and nearest first', () => {
  const box = new CosmeticOutbox();
  const size = launch(1).bytes.length + 2;
  box.offer([shot(100), launch(1, { x: 200 }), launch(2, { x: 20 }), terminal(3, { x: 5 }), launch(4, { x: 50 })], here, 7);
  // Room for two events: the launches nearest first, terminals after them.
  assert.deepEqual(pack(box, 10, { first: 2 * size, second: 0 }), ['1:2', '1:4']);
  assert.deepEqual(pack(box, 12, { first: 2 * size, second: 0 }), ['1:1', '2:3']);
  assert.deepEqual(pack(box, 14, { first: 400, second: 0 }), ['8:100'], 'presentation once there is room');
  assert.equal(box.dropped, 0);
  // The client's own event goes before everything.
  box.offer([launch(5, { x: 10, owner: 7 }), launch(6, { x: 1 })], here, 7);
  assert.deepEqual(pack(box, 16, { first: size, second: 0 }), ['1:5']);
});

test('the second datagram takes what the first cannot; refinements stay within the spare budget', () => {
  const box = new CosmeticOutbox();
  const size = launch(1).bytes.length + 2;
  box.offer([launch(1), launch(2), shot(100), shot(101)], here, 7);
  const out = box.pack(10, size, 2 * size + 400, true, 0);
  assert.deepEqual(ids(out.first), ['1:1']);
  assert.deepEqual(ids(out.second), ['1:2'], 'owed events ignore the spare budget; presentation waits for it');
  assert.deepEqual(pack(box, 12, { spare: shot(100).bytes.length + 2 }), ['8:100']);
});

test('an own-only datagram (not acknowledged) carries refinements but leaves owed events for a full snapshot', () => {
  const box = new CosmeticOutbox();
  box.offer([launch(1), shot(100)], here, 7);
  assert.deepEqual(pack(box, 10, { confirmable: false }), ['8:100']);
  assert.deepEqual(pack(box, 12), ['1:1']);
});

test('events out of range are not owed; a terminal is, wherever it lands, once the client was sent its projectile', () => {
  const box = new CosmeticOutbox();
  box.offer([launch(1, { x: 900 }), terminal(1, { x: 900 })], here, 7);
  assert.deepEqual(pack(box, 10), []);
  box.offer([launch(2, { x: 290, direction: [0, 0, 1] }), sample(3, 10, 200)], here, 7);
  assert.deepEqual(pack(box, 12), ['1:2', '6:3']);
  box.offer([terminal(2, { x: 900 }), event(projectileRecord({ id: 3, generation: 1, tick: 14, terminal: true })), event(projectileRecord({ id: 4, generation: 1, tick: 14, terminal: true }))], here, 7);
  assert.deepEqual(pack(box, 14), ['2:2', '7:3'], 'the removal of a projectile it never had is not sent');
});

test('a bolt fired from beyond the range at the client is relevant: its path comes near', () => {
  const box = new CosmeticOutbox();
  box.offer([launch(1, { x: 450, direction: [-1, 0, 0], life: 4 }), launch(2, { x: 450, direction: [1, 0, 0], life: 4 })], here, 7);
  assert.deepEqual(pack(box, 10), ['1:1']);
});

test('a projectile\'s first sample is owed; once one arrives, later samples are sent once and the newest replaces the rest', () => {
  const box = new CosmeticOutbox();
  box.offer([sample(3, 10)], here, 7);
  assert.deepEqual(pack(box, 10), ['6:3']);
  box.offer([sample(3, 12)], here, 7);
  assert.deepEqual(pack(box, 12), ['6:3']);
  box.acknowledge(10);
  box.offer([sample(3, 14)], here, 7);
  box.offer([sample(3, 16)], here, 7);
  assert.equal(box.entries.size, 1, 'the newest sample replaces the rest, in flight or not');
  assert.equal(box.pack(16, 1000, 0, true, 0).first, null, 'a known projectile\'s samples are refinements: no spare, no send');
  assert.deepEqual(pack(box, 18), ['6:3']);
  assert.equal(box.pack(18, 1000, 0, true).first, null, 'sent once');
  // A terminal retires the sample still waiting.
  box.offer([sample(3, 20)], here, 7);
  box.offer([terminal(3, { tick: 22 })], here, 7);
  assert.deepEqual(pack(box, 22), ['2:3']);
});

test('explosions are owed; resync presumes everything in flight lost', () => {
  const box = new CosmeticOutbox();
  box.offer([event(explosionRecord({ id: 5, generation: 1, tick: 10, definitionId: 1, position: [3, 0, 0], wet: false }))], here, 7);
  assert.deepEqual(pack(box, 10), ['5:5']);
  box.resend();
  assert.deepEqual(pack(box, 12), ['5:5']);
  assert.deepEqual(pack(box, 40), [], 'shown only 18 ticks: not owed after');
});

test('a snapshot whose own datagram announces a second completes only with it', async () => {
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { emptySnapshot, encodeEntities } = await import('../src/runtime/snapshot-codec.js');
  const { encodeOwnDatagram } = await import('../src/runtime/snapshot-own.js');
  const parts = encodeEntities(emptySnapshot(10, 1), null), cosmetics = launch(1).bytes;
  const packed = new Uint8Array(2 + cosmetics.length); new DataView(packed.buffer).setUint16(0, cosmetics.length, true); packed.set(cosmetics, 2);
  for (const order of [['parts', 'first', 'second'], ['second', 'parts', 'first'], ['parts', 'first']]) {
    const client = new SnapshotClient();
    const datagrams = { parts, first: [encodeOwnDatagram({ epoch: 1, tick: 10, ownParts: 2 })], second: [encodeOwnDatagram({ epoch: 1, tick: 10, cosmeticBytes: packed, second: true })] };
    let records = 0;
    for (const name of order) for (const bytes of datagrams[name]) records += client.receive(bytes)?.records.filter(r => r.kind === 'combat').length ?? 0;
    assert.equal(client.snapshotAck.tick, order.includes('second') ? 10 : 0, order.join(' '));
    assert.equal(records, order.includes('second') ? 1 : 0);
  }
  // An own datagram that announces no second completes alone, as before.
  const client = new SnapshotClient();
  for (const bytes of [...parts, encodeOwnDatagram({ epoch: 1, tick: 10 })]) client.receive(bytes);
  assert.equal(client.snapshotAck.tick, 10);
});

// Bolts fired by lag-compensated players (their launches rewound behind
// every observer's playout clock, many ending inside that rewind) reach
// observers over a lossy link and are drawn, from the gun to the impact.
test('a battle\'s shots reach every client in range and are drawn, over loss and skipped snapshots', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost, publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { PlayoutClock } = await import('../src/runtime/playout.js');
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { CombatCapture, CombatPresentation } = await import('../src/runtime/network-entity-combat.js');
  const { linearBoltFrame } = await import('../src/runtime/linear-bolt.js');
  const launches = new Map(), emit = CombatCapture.prototype.emit;
  CombatCapture.prototype.emit = function (record) { emit.call(this, record); if (record.schema === 1 || record.schema === 2) { const view = decodeCombat(record), bolt = launches.get(record.id); if (record.schema === 1) launches.set(record.id, { view, flight: view.life * 60, near: new Set() }); else if (bolt && view.surface !== 5) bolt.flight = Math.min(bolt.flight, Math.hypot(...view.position.map((v, i) => v - bolt.view.position[i])) / bolt.view.speed * 60); } };
  try {
    const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
    const world = createMultiplayerWorld(game, { armySize: 8 });
    const clock = { ms: 0 };
    const host = createSnapshotHost({ game, world, now: () => clock.ms });
    let seed = 11; const random = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296);
    const clients = [];
    for (let i = 0; i < 8; i++) {
      const id = `p${i}`;
      await world.addPlayer(id, { separateActions: true, team: i % 2 ? 2 : 1 });
      world.players.get(id).autoRespawn = true;
      host.addClient(id); host.setStarted(id, true); host.setReady(id);
      const c = { id, shooter: i < 4, receiver: new SnapshotClient(), playout: new PlayoutClock({ minDelay: 4, maxDelay: 10 }), down: [], up: [], drawn: new Set(), visible: new Map(), latest: 0, sequence: 0 };
      const effect = { stop() {}, move() {} };
      const ordnance = { clearProjectilePresentation() {}, effects: { play: () => effect }, projectileMesh: () => null, scene: { remove() {} }, presentExplosion() {}, lights: { flash() {}, add() {} },
        drawBolt(source, alpha, tick) { if (linearBoltFrame(source, tick)) c.drawn.add(source); }, drawBeam() {}, drawProjectileView() {} };
      c.presentation = new CombatPresentation({ ordnance, definition: id => host.registry.combat.definitions.get(id), predictedShots: null });
      clients.push(c);
    }
    const latency = 4, start = game.worldClock.tick, measureFrom = start + 120, end = start + 120 + 480;
    for (let tick = start + 1; tick <= end + 30; tick++) {
      for (const c of clients) {
        while (c.down.length && c.down[0].at <= tick) {
          const packet = c.receiver.receive(c.down.shift().bytes);
          if (!packet) continue;
          for (const record of packet.records) if (record.kind === 'combat') c.presentation.receive(record, { epoch: packet.ack.epoch, tick: packet.tick });
          if (packet.tick > c.latest) { c.latest = packet.tick; c.playout.arrive(packet.tick, tick / 60, packet.interval); }
        }
        while (c.up.length && c.up[0].at <= tick) host.input(c.id, c.up.shift().bytes);
        c.drawn.clear(); c.presentation.present(c.playout.advance(1 / 60));
        for (const [id, state] of c.presentation.records) if (state.bolt && c.drawn.has(state.bolt)) c.visible.set(id, (c.visible.get(id) ?? 0) + 1);
      }
      if (tick > end) continue;
      for (const c of clients) if (c.shooter) {
        const unit = world.players.get(c.id).unit;
        // Fire two thirds of the time with a 10-tick rewound view, sweeping the aim.
        if (unit) world.input(c.id, { epoch: 1, generation: unit.generation, sequence: ++c.sequence, tick: tick + 2, down: tick % 90 < 60 ? ['Mouse0'] : [], look: [[tick % 600 < 300 ? 3 : -3, tick % 240 < 120 ? 1 : -1]], actions: [], mouseScale: 1 / 2048, viewTick: Math.max(0, tick - 10) });
      }
      host.advanceEdges(); world.step(); host.consumedEdges(game.worldClock.tick); clock.ms += 1000 / 60;
      const out = host.publish({});
      assert.deepEqual(out.faults, []);
      for (const bolt of launches.values()) if (!bolt.placed) {
        bolt.placed = true;
        if (tick > measureFrom) for (const c of clients) {
          const actor = world.players.get(c.id), at = actor.unit?.position, p = bolt.view.position;
          if (bolt.view.ownerId !== actor.unit?.id && at && Math.hypot(p[0] - at.x, p[1] - at.y, p[2] - at.z) < 250) bolt.near.add(c);
        }
      }
      for (const c of clients) {
        // 3% of datagrams lost, 5% of snapshots skipped whole.
        const buffers = publicationBuffersFor(out, c.id);
        if (buffers.length && random() >= 0.05) for (const bytes of buffers) if (random() >= 0.03) c.down.push({ at: tick + latency, bytes: new Uint8Array(bytes) });
        if (tick % 2 === 0 && c.receiver.snapshotAck.tick && random() >= 0.03) c.up.push({ at: tick + latency, bytes: encodeSnapshotAckKeepalive(c.receiver.snapshotAck) });
      }
    }
    let pairs = 0, drawn = 0, short = 0, shortDrawn = 0, shown = 0;
    for (const [id, bolt] of launches) for (const c of bolt.near) {
      const frames = c.visible.get(id) ?? 0;
      pairs++; if (frames) drawn++; shown += Math.min(1, frames / Math.max(1, bolt.flight));
      if (bolt.flight < 8) { short++; if (frames) shortDrawn++; }
    }
    assert.ok(pairs > 300, `shots seen by others (${pairs})`);
    assert.ok(drawn / pairs > 0.97, `drawn ${drawn} of ${pairs} shots in range`);
    assert.ok(shown / pairs > 0.9, `${Math.round(100 * shown / pairs)}% of their flight shown`);
    assert.ok(short > 20 && shortDrawn / short > 0.9, `bolts ending within the playout delay are drawn (${shortDrawn} of ${short})`);
  } finally { CombatCapture.prototype.emit = emit; }
});
