import { contentPath } from './helpers/content.js';
// Actual bot intent drives the native hull out of the imported hangar.
// Synthetic pilot and terrain cases run in the native flight suite.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

// The imported Hoth hangar (hot1): its four snowspeeder pads sit under the
// hangar's roof. Each bot pilot waits on his pad for his gunner, lifts off
// as his agent sends him out along the craft's heading, and flies out of
// the door: every takeoff reaches open sky (nothing 100 m above) within a
// few seconds, and none dies under the roof.
const CONTENT = contentPath();
test('imported: Hoth\'s bot snowspeeders fly out of the hangar without hitting its roof', { skip: !fs.existsSync(path.join(CONTENT, 'maps/hot1/map.json')) && 'no hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame, idleCommand } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=hot1&ai=classic&tier=medium&seed=1');
  await game.ready; game.begin?.();
  const T = game.THREE, UP = new T.Vector3(0, 1, 0), hit = { point: new T.Vector3(), normal: new T.Vector3() };
  const roofed = (v) => Boolean(game.collision.cast(v.position, UP, 100, hit, v.body));
  const craft = game.vehicles.vehicles.filter((v) => v.name === 'all_fly_snowspeeder');
  assert.ok(craft.length >= 2 && craft.every((v) => roofed(v)), 'the snowspeeders wait under the hangar roof');
  const runs = new Map(), done = [], lost = [];
  let scrapes = 0;
  for (const v of craft) {
    const damage = v.damage.bind(v);
    v.damage = (amount, attacker, ...rest) => { if (attacker === undefined && amount > 0) scrapes++; return damage(amount, attacker, ...rest); };
  }
  for (let t = 0; t < 45 * 60 && done.length < craft.length; t++) {
    game.step(idleCommand(game));
    for (const v of craft) {
      const run = runs.get(v);
      if (!run && v.flightState === 'TakingOff' && roofed(v)) runs.set(v, { from: t });
      else if (run && !run.end) {
        if (!v.alive) { run.end = t; lost.push(v.name); done.push(run); }
        else if (v.flightState === 'Flying' && !roofed(v)) { run.end = t; done.push(run); }
      }
    }
  }
  assert.ok(done.length >= 2, `${done.length} bot takeoffs from the hangar`);
  assert.deepEqual(lost, [], 'none lost under the roof');
  for (const run of done) assert.ok((run.end - run.from) / 60 < 8, `out in ${((run.end - run.from) / 60).toFixed(1)} s`);
  assert.ok(scrapes < 30 * done.length, `${scrapes} scrapes on the way out`);
});
