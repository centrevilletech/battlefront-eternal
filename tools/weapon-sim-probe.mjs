// Record ordinary holders' exact native weapon words. Resource prototypes
// contain no JavaScript controller to wrap or invoke during a world step.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
import { exportWeapon } from '../src/sim/gen/combat.js';

let game, fd = null, records = 0;
const writer = new Writer();
function entries() {
  const entries = [];
  for (const hull of game.vehicles.vehicles) for (const seat of hull.seats) for (const w of seat.weapons) entries.push(w.state);
  for (const turret of game.turretSeats?.turrets ?? []) if (turret.weapon) entries.push(turret.weapon);
  return entries;
}
export function battle(context) {
  game = context.game;
  assert.equal(typeof Object.getPrototypeOf(game.playerUnit.combat.arsenal.weapons[0]).update, 'undefined', 'weapon resources contain no controller');
  const i = process.argv.indexOf('--weapons-trace');
  if (i >= 0) { fd = fs.openSync(process.argv[i + 1], 'w'); fs.writeSync(fd, MAGIC); }
}
export function ticked(tick) {
  if (fd === null) return;
  const values = entries().map(weapon => ({ state: exportWeapon(weapon, { seen: new WeakMap(), def: (_table, d) => d.class }),
    definition: weapon.levels.indexOf(weapon.def), seeded: Boolean(weapon.salvoRandom) }));
  writer.record(RECORD.frame, tick, 'ordinary-weapons', values); fs.writeSync(fd, writer.take()); records++;
}
export function ended() {
  if (fd !== null) { fs.closeSync(fd); fd = null; }
  console.error(JSON.stringify({ weaponProbe: { weapons: entries().length, records } }));
}
