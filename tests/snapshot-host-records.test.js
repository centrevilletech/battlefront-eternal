import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

// Tab's table opens DETAILS on any row, a bot's or another player's
// included, so everyone's records (battle-stats.js) reach every client
// during play: in whole seconds, each chunk refreshed every 5 s. Once the
// scores stop they come exact, for the end table's DETAILS and AWARDS.
// The player's own record keeps coming from his own datagram.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;

test('everyone\'s records reach a client during play, then exact once the scores stop', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost, publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { encodeEntityInput } = await import('../src/runtime/network-entity-input.js');
  const { applyWorldSummary, decodeWorldSummary, RECORD_SUMMARY_ID, OWN_RECORD_ID } = await import('../src/runtime/network-entity-summary.js');
  const { personalStats, recordOf } = await import('../src/battle-stats.js');

  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=3');
  const world = createMultiplayerWorld(game, { armySize: 32 });
  await world.addPlayer('rec', { reliableFire: true, separateActions: true });
  const actor = world.players.get('rec'); actor.autoRespawn = true;
  const host = createSnapshotHost({ game, world });
  host.addClient('rec'); host.setStarted('rec', true); host.setReady('rec');
  const client = new SnapshotClient();
  const registry = host.registry;
  // The page's battle: Characters made from the roster, its own slot named
  // by the spawn summary, as a client's are.
  const page = { battle: { characters: [], local: null, gameOver: false, get human() { return this.local; }, set human(c) { this.local = c; } } };
  const changes = new Map(); // everyone's chunk id -> [tick of each new payload]
  const last = new Map();
  let sequence = 0, own = 0;
  const run = (frames) => {
    for (let frame = 0; frame < frames; frame++) {
      const tick = game.worldClock.tick + 1;
      host.input('rec', encodeEntityInput({ epoch: 1, generation: actor.unit.generation,
        commands: [{ epoch: 1, generation: actor.unit.generation, sequence: ++sequence, tick, down: [], lookTotal: [0, 0] }],
        edges: [], snapshotAck: client.snapshotAck.tick ? client.snapshotAck : null }, registry));
      host.advanceEdges();
      world.step();
      host.consumedEdges(game.worldClock.tick);
      const out = host.publish({});
      assert.deepEqual(out.faults, []);
      for (const buffer of publicationBuffersFor(out, 'rec')) {
        const applied = client.receive(new Uint8Array(buffer));
        for (const record of applied?.records ?? []) {
          if (record.kind !== 'summary' || ![3, 4, 5].includes(record.schema)) continue;
          if (record.schema === 5 && record.id === OWN_RECORD_ID) own++;
          else if (record.schema === 5) {
            assert.ok(record.id >= RECORD_SUMMARY_ID);
            const payload = Buffer.from(record.payload).toString('hex');
            if (last.get(record.id) !== payload) {
              last.set(record.id, payload);
              changes.set(record.id, [...(changes.get(record.id) ?? []), record.tick]);
            }
          }
          applyWorldSummary(page, record, { registry, spawn: {} });
        }
      }
    }
  };
  run(60 * 12);
  const characters = game.battle.characters;
  const mine = characters.indexOf(actor.character);
  const carried = new Set();
  for (const [id, ticks] of changes) {
    const record = { kind: 'summary', id, generation: 1, tick: ticks.at(-1), schema: 5, payload: Buffer.from(last.get(id), 'hex') };
    for (const { slot } of decodeWorldSummary(record, registry).records) carried.add(slot);
    // Each chunk changes at most once a refresh.
    for (let i = 1; i < ticks.length; i++) assert.ok(ticks[i] - ticks[i - 1] >= 298, `chunk ${id} refreshed ${ticks[i] - ticks[i - 1]} ticks apart`);
  }
  const bots = characters.filter((c, i) => !c.human && carried.has(i)).length;
  assert.ok(bots >= 60, `every bot's record arrived during play (${bots})`);
  assert.ok(own > 0, 'his own record comes in his own datagram');
  assert.equal(page.battle.local, page.battle.characters[mine], 'the spawn summary names his slot');
  // A bot's DETAILS on the page are his, in whole seconds, a refresh old at most.
  const bot = characters.findIndex((c, i) => !c.human && c.team === 1 && i !== mine && c.record);
  const lifeBest = (c) => recordOf(c).slots[0].lifeBest;
  assert.ok(lifeBest(page.battle.characters[bot]) > 0, 'his time alive arrived');
  assert.ok(Number.isInteger(lifeBest(page.battle.characters[bot])), 'in whole seconds');
  assert.ok(lifeBest(characters[bot]) - lifeBest(page.battle.characters[bot]) < 6.5, 'no more than a refresh behind');
  // The scores stop: each chunk's next refresh is exact.
  game.battle.over = true; game.battle.gameOver = true;
  page.battle.gameOver = true;
  run(60 * 6);
  assert.equal(Math.floor(lifeBest(page.battle.characters[bot]) * 1000), Math.floor(lifeBest(characters[bot]) * 1000), 'to the millisecond');
  const shown = (list, i) => personalStats(list[i], list).map((r) => r.value).filter((_, k) => [0, 4, 5, 6, 7, 8].includes(k));
  for (const i of [bot, mine]) assert.deepEqual(shown(page.battle.characters, i), shown(characters, i), `Character ${i}'s DETAILS`);
});

test('until the scores stop, everyone\'s records leave the player\'s own slot to his own record', async () => {
  const { applyWorldSummary, captureRecordSummaries, OWN_RECORD_ID, RECORD_SUMMARY_ID } = await import('../src/runtime/network-entity-summary.js');
  const { BattleStats, recordOf } = await import('../src/battle-stats.js');
  const registry = { classes: new Map(), classIds: new Map(), weapons: new Map(), weaponIds: new Map(), posts: new Map() };
  const make = () => Array.from({ length: 6 }, (_, index) => ({ index, team: index % 2 ? 2 : 1, name: `C${index}` }));
  const world = make(), stats = new BattleStats();
  stats.kill(world[2], world[1], -1); stats.kill(world[3], world[0], -1); stats.death(world[1]); stats.death(world[0]);
  const page = make(), game = { battle: { characters: page, local: page[2], gameOver: false } };
  const options = { tick: 60, epoch: 1, quantum: 1000 };
  // His own record, then everyone's with a later count of his kills of C1.
  for (const record of captureRecordSummaries({ characters: world }, registry, { ...options, only: world[2], id: OWN_RECORD_ID })) applyWorldSummary(game, record, { registry });
  assert.equal(recordOf(page[2]).slots[0].victims.get(page[1]), 1);
  recordOf(world[2]).slots[0].victims.set(world[1], 7);
  const everyone = captureRecordSummaries({ characters: world }, registry, options);
  assert.equal(everyone[0].id, RECORD_SUMMARY_ID);
  for (const record of everyone) applyWorldSummary(game, record, { registry });
  assert.equal(recordOf(page[2]).slots[0].victims.get(page[1]), 1, 'his own slot keeps his own record');
  assert.equal(recordOf(page[3]).slots[1].victims.get(page[0]), 1, 'the others take theirs');
  game.battle.gameOver = true;
  for (const record of everyone) applyWorldSummary(game, record, { registry });
  assert.equal(recordOf(page[2]).slots[0].victims.get(page[1]), 7, 'once the scores stop, everyone\'s are the record');
});
