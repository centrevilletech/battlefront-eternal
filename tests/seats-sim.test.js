// The hulls' seat batch on the simulation's module (src/seats-sim.js,
// ?seats=rust), in the headless world the worker and the server run. The
// batch exchanges only its riders. Other units' rows and JavaScript
// objects may have advanced independently and must both stay untouched.
// With the combats on the module too (?combat=rust), losing a non-rider's
// row would also lose his combat record.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { KIND_UNIT, sameUnit } from '../src/sim/gen/index.js';
import { sameCombat } from '../src/sim/gen/combat.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');

test('imported: a seat batch on the module keeps a non-rider\'s unit row and combat record as they were', { skip: !fs.existsSync(path.join(CONTENT, 'maps/hot1/props.json')) && 'no hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=hot1&side=all&ai=off&hulls=rust&seats=rust&combat=rust');
  await game.ready;
  const vehicles = game.vehicles, sim = vehicles.seatsSim;
  assert.ok(sim, 'the seat batch is on the module');
  const bridge = sim.bridge;
  assert.equal(vehicles.sim.bridge, bridge, 'seats and hulls share the same store');
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
  // Keep a full non-rider row, then let JavaScript advance independently.
  // Rewriting every known unit would change this row; syncing every row
  // after a partial write would undo the JavaScript movement instead.
  bridge.writeSome(KIND_UNIT, [other]);
  const row = (unit) => bridge.read(KIND_UNIT).find((r) => r.slot === unit.id)?.object;
  const before = row(other);
  assert.ok(before, 'the non-rider has a row in the store (his combat is the module\'s)');
  other.soldier.position.x += 7;
  other.tickCount += 3;
  const position = other.soldier.position.clone(), tickCount = other.tickCount, combat = other.combat;
  const ticks = vehicles.ticks;
  vehicles.latchCrews();
  assert.equal(vehicles.ticks, ticks + 1, 'the batch ran');
  const after = row(other);
  assert.ok(after, 'his row is still there after the batch');
  assert.equal(sameUnit(before, after), null, 'his entire stored row is unchanged');
  assert.equal(sameCombat(before.combat, after.combat), null, 'his combat record is as it was');
  assert.deepEqual(other.soldier.position, position, 'his JavaScript movement is unchanged');
  assert.equal(other.tickCount, tickCount, 'his JavaScript tick is unchanged');
  assert.equal(other.combat, combat, 'his combat facade keeps its identity');
  assert.equal(rider.seat?.vehicle, hull, 'the rider still holds his seat');
  assert.ok(row(rider), 'the rider has his row');
  vehicles.requestExit(rider, { placement: false, promote: false });
  vehicles.latchCrews();
  assert.equal(rider.seat, null, 'the rider receives the batch\'s exit');
  assert.equal(rider.combat.mounted, false, 'the exit effect reaches his combat facade');
  assert.equal(sameUnit(before, row(other)), null, 'an exit also leaves the non-rider\'s row alone');
  assert.deepEqual(other.soldier.position, position, 'an exit leaves his JavaScript position alone');
});
