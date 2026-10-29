// Imported-content acceptance for every playable Instant Action map/era.
// Each process owns one world so runtime globals and assets cannot leak
// between cases. --list also supplies the collaborative browser's matrix.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { battlesOf } from '../src/mission.js';
import { MAX_TEAM_SIZE } from '../src/battle-limits.js';

const content = process.env.BFE_CONTENT_DIR || 'public';
if (!process.argv.includes('--case')) {
  const cases = readdirSync(`${content}/maps`).sort().flatMap(map => {
    const world = JSON.parse(readFileSync(`${content}/maps/${map}/map.json`, 'utf8'));
    return battlesOf(world).filter(b => b.playable && !b.historical).map(b =>
      `?map=${map}&mission=${b.name}&side=${b.sides.find(s => ['rep', 'cis', 'all', 'imp'].includes(s))}&ai=classic&seed=1`);
  });
  if (process.argv.includes('--list')) console.log(JSON.stringify(cases));
  else {
    const failures = [];
    for (const query of cases) {
      const result = spawnSync(process.execPath, [fileURLToPath(import.meta.url), '--case', query], { encoding: 'utf8', timeout: 120000 });
      if (result.status !== 0) failures.push({ query, error: result.stderr || result.error?.message });
      console.log(result.stdout.trim() || JSON.stringify({ query, pass: false, error: result.stderr }));
    }
    console.log(JSON.stringify({ cases: cases.length, failures }));
    assert.equal(failures.length, 0);
  }
} else {
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createLocalServer } = await import('../src/runtime/local-server.js');
  const { EntityReceiver } = await import('../src/runtime/network-entity-stream.js');
  const query = process.argv.at(-1), game = await headlessGame(query);
  await game.factory.prepareSoldiers(); // The renderer's startup prerequisites.
  const expected = game.ai.spawning.teams.slice(1, 3).map(t => Math.min(MAX_TEAM_SIZE, t.unitCount));
  const receiver = new EntityReceiver({ maxEntities: 768 });
  let packets = 0, server;
  server = await createLocalServer(game, { send(channel, raw) {
    if (!(raw instanceof ArrayBuffer)) return;
    const packet = receiver.apply(new Uint8Array(raw)); packets++;
    if (packet) server.host.ack('local', packet.ack);
  } });
  assert.deepEqual(server.world.roster.counts().map(t => t.total), expected);
  server.welcome();
  try {
    for (let tick = 0; tick < 1800; tick++) server.step();
    const living = [1, 2].map(team => game.units.all().filter(u => u.soldier && u.alive && u.team === team).length);
    for (let i = 0; i < 2; i++) assert.ok(living[i] > 0 && living[i] <= expected[i]);
    console.log(JSON.stringify({ pass: true, query, expected, living, packets, tick: game.worldClock.tick }));
  } finally { server.dispose(); }
}
