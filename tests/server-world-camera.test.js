import { contentPath } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

const content = contentPath();

// A server world's own player is dormant (createMultiplayerWorld), and its
// step leaves his chase camera where it was. That is only sound while
// nothing in the step reads that camera: every read of its pose is caught
// here, beyond the camera's own matrix update.
test('a server world\'s step never reads its dormant player\'s camera', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async (t) => {
  process.env.BFE_CONTENT_DIR ||= content;
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createLocalServer } = await import('../src/runtime/local-server.js');
  const game = await headlessGame('?map=nab2&side=rep&era=cw&ai=classic&seed=1');
  const server = await createLocalServer(game);
  const camera = game.simulationCamera, reads = new Map();
  let stepping = false;
  const note = (what) => {
    if (!stepping) return;
    const caller = new Error().stack.split('\n').slice(3).find((line) => !/three\.module\.js|three\.cjs/.test(line))?.trim();
    if (/updateMatrixWorld/.test(new Error().stack.split('\n').slice(3, 6).join())) return;
    reads.set(`${what} ${caller}`, (reads.get(`${what} ${caller}`) || 0) + 1);
  };
  for (const [vector, keys] of [[camera.position, ['x', 'y', 'z']], [camera.quaternion, ['_x', '_y', '_z', '_w']]]) {
    for (const key of keys) {
      let value = vector[key];
      Object.defineProperty(vector, key, { get() { note(key); return value; }, set(v) { value = v; } });
    }
  }
  camera.matrixWorld.elements = new Proxy(camera.matrixWorld.elements, { get(target, key) { if (typeof key === 'string' && /^\d+$/.test(key)) note('matrixWorld'); return target[key]; } });
  const before = camera.position.toArray();
  let seated = 0;
  for (let i = 0; i < 1200; i++) {
    now += 1000 / 60;
    const tick = game.worldClock.tick + 1;
    server.world.input(server.id, { epoch: 1, sequence: tick, tick, generation: server.actor.unit.generation, down: i % 240 < 160 ? ['KeyW'] : ['KeyA', 'Mouse0'], actions: [], look: [[4, 1]] });
    stepping = true;
    try { server.world.step(); } finally { stepping = false; }
    seated = Math.max(seated, game.vehicles.vehicles.filter((v) => v.occupied).length);
    await new Promise((resolve) => setImmediate(resolve));
  }
  assert.deepEqual([...reads.keys()], [], 'nothing in the step reads the dormant camera');
  assert.deepEqual(camera.position.toArray(), before, 'the dormant camera stays put');
  assert.ok(seated > 0 && game.units.all().filter((u) => u.alive && u.soldier).length > 20, 'the battle was under way');
});
