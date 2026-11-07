import { contentPath } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

const content = contentPath();

// The battle's units, their combat and their bots' controllers declare every
// field in their constructors, so each keeps one shape however its life went
// (spawned, seated, in water, driving, a network player's): code walking
// them all stays optimized instead of deoptimizing at every new layout.
test('units, combats and controllers keep one layout through a battle', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async (t) => {
  process.env.BFE_CONTENT_DIR ||= content;
  let now = 0;
  t.mock.method(performance, 'now', () => now);
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createLocalServer } = await import('../src/runtime/local-server.js');
  const game = await headlessGame('?map=nab2&side=cis&era=cw&ai=classic&seed=3');
  const server = await createLocalServer(game);
  for (let i = 0; i < 1200; i++) {
    now += 1000 / 60;
    const tick = game.worldClock.tick + 1;
    server.world.input(server.id, { epoch: 1, sequence: tick, tick, generation: server.actor.unit.generation, down: i % 240 < 160 ? ['KeyW'] : ['KeyA', 'Mouse0'], actions: [], look: [[4, 1]] });
    server.world.step();
    await new Promise((resolve) => setImmediate(resolve));
  }
  const layouts = (objects) => new Set(objects.map((o) => Object.keys(o).join()));
  const units = game.factory.built;
  assert.ok(units.some((u) => u.alive) && units.some((u) => u.networkHuman) && units.some((u) => u.walker), 'soldiers, droidekas and the player were in the battle');
  assert.equal(layouts(units).size, 1, 'units');
  assert.equal(layouts(units.map((u) => u.combat)).size, 1, 'combats');
  const controllers=game.ai.owner.controllerRefs.frames;
  assert.ok(controllers.some(controller=>controller.simGen>0),'native bot identities were published');
  assert.equal(layouts(controllers).size, 1, 'controller identity facades');
});
