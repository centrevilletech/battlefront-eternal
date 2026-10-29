import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { chooseSharedBaselines } from '../src/runtime/snapshot-host.js';
import { feedbackRecord, decodeFeedback, feedbackConcerns } from '../src/runtime/network-combat-feedback.js';

// Snapshot publication policy: shared baseline choice, the per-client state
// byte budget, stale recipients, cadence after a skipped tick and damage
// feedback recipients.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
const owned = { skip: !existsSync(`${content}/maps/nab2/map.json`) };

test('shared baselines: at most three, newest first, each one acknowledged by its clients', () => {
  // Fewer distinct acknowledgements than the limit: every client keeps its own newest.
  assert.deepEqual(chooseSharedBaselines(100, [[98, 96], [96, 94], [98]]), [98, 96, 94]);
  // Many clients on staggered acknowledgements: three shared baselines, and
  // the newest chosen one each client acknowledged is never older than it must be.
  const acks = [];
  for (let c = 0; c < 64; c++) { const lag = 2 + 2 * (c % 6), list = []; for (let t = 200 - lag; t > 200 - lag - 32; t -= 2) list.push(t); acks.push(list); }
  const chosen = chooseSharedBaselines(200, acks);
  assert.ok(chosen.length <= 3 && chosen.length > 0);
  assert.deepEqual(chosen, [...chosen].sort((a, b) => b - a));
  for (const list of acks) assert.ok(chosen.some(t => list.includes(t)), 'every client is covered by a baseline it acknowledged');
  // A lone client on an old baseline nobody else holds still gets one when
  // that beats a full snapshot for it.
  const lone = chooseSharedBaselines(300, [[298], [298], [296], [150]]);
  assert.deepEqual(lone, [298, 296, 150]);
  // More disjoint groups than baselines: the leftover goes without (empty
  // baseline) rather than onto a tick it never acknowledged.
  const groups = chooseSharedBaselines(300, [[298], [298], [296], [294], [292]]);
  assert.equal(groups.length, 3);
  assert.ok(groups.includes(298));
  assert.deepEqual(chooseSharedBaselines(10, []), []);
});

test('damage feedback concerns only the attacker and target; deaths concern everyone', () => {
  const event = kind => decodeFeedback(feedbackRecord({ id: 1, generation: 1, tick: 50, event: { kind, target: { unit: { id: 7 }, generation: 2 }, attacker: { unit: { id: 9 }, generation: 3 }, position: [1, 2, 3], team: 1, attackerTeam: 2, source: [0, 0, 0] } }));
  const damage = event('damage'), death = event('death');
  assert.equal(feedbackConcerns(damage, 7), true, 'target');
  assert.equal(feedbackConcerns(damage, 9), true, 'attacker');
  assert.equal(feedbackConcerns(damage, 8), false, 'bystander');
  assert.equal(feedbackConcerns(damage, undefined), false, 'spectator without a unit');
  for (const unit of [7, 9, 8, undefined]) assert.equal(feedbackConcerns(death, unit), true, 'kill messages reach everyone');
});

async function battle(ids, options = {}) {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost } = await import('../src/runtime/snapshot-host.js');
  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 16 });
  const clock = { ms: 0 };
  const host = createSnapshotHost({ game, world, now: () => clock.ms, ...options });
  for (const id of ids) {
    await world.addPlayer(id, { reliableFire: true, separateActions: true });
    world.players.get(id).autoRespawn = true;
    host.addClient(id); host.setStarted(id, true); host.setReady(id);
  }
  // One simulation tick (16.7 ms of the host's clock) and its publication.
  const step = ({ publish = true } = {}) => {
    world.step(); clock.ms += 1000 / 60;
    if (!publish) return null;
    const out = host.publish({});
    assert.deepEqual(out.faults, []);
    return out;
  };
  return { game, world, host, clock, step };
}

test('snapshot clients decode every publication; no client is sent a baseline it did not acknowledge', owned, async () => {
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const ids = ['a', 'b', 'c', 'd', 'e', 'f'];
  const { host, step } = await battle(ids);
  const receivers = new Map(ids.map(id => [id, new SnapshotClient()]));
  const acked = new Map(ids.map(id => [id, new Set()]));
  // Each client loses a different share of its datagrams and acknowledges
  // with a different lag, so their acknowledged baselines diverge.
  const pending = new Map(ids.map(id => [id, []]));
  let published = 0;
  const lateEmpty = new Map(ids.map(id => [id, 0]));
  for (let frame = 0; frame < 600; frame++) {
    const out = step();
    for (const [i, id] of ids.entries()) {
      // Acknowledgements arrive i + 1 publications late.
      const queue = pending.get(id);
      queue.push(receivers.get(id).snapshotAck.tick);
      while (queue.length > i + 1) {
        const tick = queue.shift();
        if (tick) { host.input(id, encodeSnapshotAckKeepalive({ epoch: 1, tick })); acked.get(id).add(tick); }
      }
    }
    if (!out.shared.length) continue;
    published++;
    assert.ok(out.shared.filter(g => g.baselineTick !== 0).length <= 3, 'at most three shared baselines (plus the empty one)');
    for (const group of out.shared) for (const id of group.clients) {
      assert.ok(group.baselineTick === 0 || acked.get(id).has(group.baselineTick), `${id} sent baseline ${group.baselineTick} it never acknowledged`);
      if (frame > 120 && group.baselineTick === 0) lateEmpty.set(id, lateEmpty.get(id) + 1);
    }
    for (const [i, id] of ids.entries()) {
      if ((frame + i) % (i + 3) === 0) continue; // this client's loss
      for (const buffer of publicationBuffersFor(out, id)) receivers.get(id).receive(new Uint8Array(buffer));
    }
  }
  assert.ok(published > 250, `published ${published}`);
  for (const [id, receiver] of receivers) {
    assert.equal(receiver.missingBaselines, 0, `${id} always held the baseline named`);
    assert.ok(receiver.snapshotAck.tick > 500, `${id} kept acknowledging (${receiver.snapshotAck.tick})`);
    assert.equal(lateEmpty.get(id), 0, `${id} needed no full snapshot once acknowledging`);
  }
  host.dispose();
});

test('a baseline is at most 128 ticks old, the most a client released before the 128-snapshot ring holds', owned, async () => {
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { CLIENT_BASELINE_TICKS } = await import('../src/runtime/snapshot-host.js');
  const { host, step } = await battle(['slow'], { stateBytesPerSecond: 500000 });
  let first = null, out;
  for (let frame = 0; frame < 60; frame++) {
    out = step();
    if (out.shared.length && first === null) first = out.shared[0].tick;
  }
  host.input('slow', encodeSnapshotAckKeepalive({ epoch: 1, tick: first }));
  assert.equal(host.inspect('slow').baselineTick, first, 'acknowledgement accepted');
  do out = step(); while (!out.shared.length);
  assert.ok(out.shared[0].tick - first < CLIENT_BASELINE_TICKS);
  assert.equal(out.shared[0].baselineTick, first, 'encoded against the acknowledged snapshot');
  // A tick that was never published to this client is never accepted.
  host.input('slow', encodeSnapshotAckKeepalive({ epoch: 1, tick: out.shared[0].tick + 1 }));
  assert.equal(host.inspect('slow').baselineTick, first);
  // Once it is CLIENT_BASELINE_TICKS old the client may no longer hold it:
  // the host falls back to a full snapshot rather than an undecodable delta.
  for (;;) {
    out = step();
    if (out.shared.length && out.shared[0].tick - first >= CLIENT_BASELINE_TICKS) break;
  }
  assert.equal(out.shared[0].baselineTick, 0);
  host.dispose();
});

test('a client over its state byte budget skips snapshots but keeps its own corrections', owned, async () => {
  const { host, step } = await battle(['tight', 'roomy'], { stateBytesPerSecond: 20000 });
  // Neither acknowledges, so every snapshot is a full one (several KB).
  const sent = [], ownTimes = [], ownOnly = [];
  let fullSnapshots = 0;
  for (let frame = 0; frame < 300; frame++) {
    const out = step();
    const at = frame * 1000 / 60;
    let bytes = 0;
    for (const group of out.shared) if (group.clients.includes('tight')) { bytes += group.bytes; fullSnapshots++; }
    for (const packet of out.ownPackets) if (packet.client === 'tight') { bytes += packet.buffer.byteLength; ownTimes.push(at); }
    if (bytes && out.shared.some(g => g.clients.includes('tight'))) sent.push([at, bytes]);
    else if (bytes) ownOnly.push(bytes);
  }
  const state = host.inspect('tight');
  assert.ok(state.budgetSkips > 50, `snapshots skipped (${state.budgetSkips})`);
  assert.ok(fullSnapshots >= 3, `snapshots still go out (${fullSnapshots})`);
  // Within any one-second window the snapshot bytes stay under budget plus
  // the one publication that crossed it; own-only datagrams are small.
  assert.ok(Math.max(...ownOnly) < 1100 * 2);
  const largest = Math.max(...sent.map(([, b]) => b));
  for (const [start] of sent) {
    const total = sent.filter(([t]) => t >= start && t < start + 1000).reduce((n, [, b]) => n + b, 0);
    assert.ok(total <= 20000 + largest, `window at ${start.toFixed(0)} ms carried ${total} bytes`);
  }
  // Own datagrams never stop for longer than the fallback period plus a tick.
  for (let i = 1; i < ownTimes.length; i++) assert.ok(ownTimes[i] - ownTimes[i - 1] <= 100 + 1000 / 60 * 2 + 1e-6, `own gap ${ownTimes[i] - ownTimes[i - 1]} ms`);
  assert.ok(state.ownOnly > 0, 'own-only datagrams sent while over budget');
  host.dispose();
});

test('a silent client stops receiving snapshots, is reported stale, and resumes when heard', owned, async () => {
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { host, step } = await battle(['live', 'ghost']);
  const recipients = out => new Set([...out.shared.flatMap(g => g.clients), ...out.ownPackets.map(p => p.client)]);
  let lastGhost = -1, frames = 0;
  for (let frame = 0; frame < 420; frame++) {
    if (frame % 10 === 0) host.input('live', encodeSnapshotAckKeepalive({ epoch: 1, tick: 0 }));
    const out = step(); frames++;
    if (recipients(out).has('ghost')) lastGhost = frame;
  }
  // 300 ticks is 5 s on the host's clock.
  assert.ok(lastGhost >= 290 && lastGhost <= 301, `ghost published to until about 5 s (${lastGhost})`);
  assert.deepEqual(host.staleClients(5), ['ghost']);
  assert.deepEqual(host.staleClients(7.5), []);
  assert.ok(host.inspect('ghost').staleSkips > 50);
  host.input('ghost', encodeSnapshotAckKeepalive({ epoch: 1, tick: 0 }));
  assert.deepEqual(host.staleClients(5), []);
  let resumed = false;
  for (let frame = 0; frame < 4 && !resumed; frame++) resumed = recipients(step()).has('ghost');
  assert.ok(resumed, 'publication resumes once the client is heard again');
  host.setStarted('ghost', false);
  for (let frame = 0; frame < 400; frame++) step({ publish: false });
  assert.deepEqual(host.staleClients(5), ['live'], 'a suspended client is never listed');
  host.dispose();
});

test('snapshot cadence survives skipped publications and is unchanged when on time', owned, async () => {
  const { game, host, step } = await battle(['p'], { stateBytesPerSecond: 500000 });
  const ticks = [];
  for (let frame = 0; frame < 40; frame++) { const out = step(); if (out.shared.length) ticks.push(out.shared[0].tick); }
  assert.ok(ticks.length >= 19 && ticks.every(t => t % 2 === 0), 'even ticks, every second tick');
  for (let i = 1; i < ticks.length; i++) assert.equal(ticks[i] - ticks[i - 1], 2);
  // A catch-up batch publishes once after several steps, landing on an odd
  // tick: that publication still carries a snapshot.
  const before = ticks.at(-1);
  do step({ publish: false }); while ((game.worldClock.tick + 1) % 2 === 0 || game.worldClock.tick + 1 - before < 3);
  let out = step();
  const odd = out.shared[0]?.tick;
  assert.ok(odd !== undefined && odd % 2 === 1 && odd - before >= 2, `snapshot after a skipped tick (${before} then ${odd})`);
  out = step(); assert.equal(out.shared.length, 0, 'not due one tick later');
  out = step(); assert.equal(out.shared[0]?.tick, odd + 2, 'then every interval again');
  host.dispose();
});
