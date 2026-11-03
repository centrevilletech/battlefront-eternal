// The hulls' seat batch on the simulation's module (src/seats-sim.js,
// ?seats=rust), in the headless world the worker and the server run. The
// batch writes the battle's units into the store and gives them back
// after, and the store keeps only the units a write lists: every unit the
// store holds a row of must come out of a batch as it went in. With the
// combats on the module too (?combat=rust) each unit's combat record lives
// in its row, so a batch that let a non-rider's row go would lose his
// fight.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { KIND_UNIT } from '../src/sim/gen/index.js';
import { sameCombat } from '../src/sim/gen/combat.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');

test('imported: a seat batch on the module keeps a non-rider\'s unit row and combat record as they were', { skip: !fs.existsSync(path.join(CONTENT, 'maps/hot1/props.json')) && 'no hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=hot1&side=all&ai=off&seats=rust&combat=rust');
  await game.ready;
  const vehicles = game.vehicles, sim = vehicles.seatsSim;
  assert.ok(sim, 'the seat batch is on the module');
  const bridge = sim.bridge;
  let className = null;
  for (const s of Object.values(game.data.sides)) for (const [n, d] of Object.entries(s.units || {})) {
    if (!className && d.base === 'soldier' && !d.hero && !Number(d.NoEnterVehicles ?? 0)) className = n;
  }
  await game.factory.prepare(className);
  const hull = vehicles.vehicles.find((v) => v.alive && !v.flying && v.seats.length > 1);
  const rider = game.factory.acquire(className), other = game.factory.acquire(className);
  for (const [unit, dx] of [[rider, 3], [other, -3]]) {
    unit.team = unit.combat.team = 1;
    unit.spawn(hull.position.x + dx, hull.position.y + 1, hull.position.z + 4, 0);
  }
  // A bot aboard: the next batch runs on the module.
  assert.ok(vehicles.board(rider, hull), 'a bot boards');
  assert.ok(rider.seat && !rider.seat.human, 'he holds a bot\'s lease');
  const row = (unit) => bridge.read(KIND_UNIT).find((r) => r.slot === unit.id)?.object;
  const before = row(other);
  assert.ok(before, 'the non-rider has a row in the store (his combat is the module\'s)');
  const ticks = vehicles.ticks;
  vehicles.latchCrews();
  assert.equal(vehicles.ticks, ticks + 1, 'the batch ran');
  const after = row(other);
  assert.ok(after, 'his row is still there after the batch');
  assert.equal(sameCombat(before.combat, after.combat), null, 'his combat record is as it was');
  assert.equal(rider.seat?.vehicle, hull, 'the rider still holds his seat');
  assert.ok(row(rider), 'the rider has his row');
});
