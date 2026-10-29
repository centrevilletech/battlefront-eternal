import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;

test('a disconnecting player leaves his vehicle or turret seat at once, and his place after 60 simulated seconds', { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
  const world = createMultiplayerWorld(game, { armySize: 32 });

  await world.addPlayer('driver', { reliableFire: false, team: 1 });
  const driver = world.players.get('driver').unit;
  const vehicle = game.vehicles.vehicles.find(v => game.vehicles.eligibleSeat(driver, v) >= 0);
  assert.ok(vehicle && game.vehicles.board(driver, vehicle));
  const index = driver.seat.index;
  assert.equal(world.connectedCount(), 1);
  world.removePlayer('driver');
  assert.equal(driver.seat, null, 'the absent driver is out of the hull');
  assert.ok(vehicle.seatFree(index), 'the seat is free for a bot');
  assert.equal(world.connectedCount(), 0);
  assert.ok(world.players.has('driver'), 'his place is held for a reconnect');

  await world.addPlayer('gunner', { reliableFire: false, team: 1 });
  const gunner = world.players.get('gunner').unit;
  // Naboo's turrets start unbuilt; one is raised for him.
  const turret = game.turretSeats.turrets[0];
  if (!turret.alive) turret.setHealth(turret.maxHealth);
  assert.ok(game.turretSeats.man(gunner, turret), 'he mans the turret');
  world.removePlayer('gunner');
  assert.equal(gunner.seat, null, 'the absent gunner is off the gun');
  assert.equal(turret.lease, null);
  assert.equal(turret.gunner, null);

  // The grace period is 60 s of simulation (3600 ticks at 60 Hz).
  const from = game.worldClock.tick;
  while (game.worldClock.tick - from < 3598) world.step();
  assert.ok(world.players.has('driver'));
  while (game.worldClock.tick - from < 3602) world.step();
  assert.ok(!world.players.has('driver') && !world.players.has('gunner'));
});
