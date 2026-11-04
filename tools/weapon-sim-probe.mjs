// Compare ordinary holders' exact weapon words and assert that native
// holders no longer enter JavaScript's Weapon.update implementation.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
import { exportWeapon } from '../src/sim/gen/combat.js';

let game, fd = null, records = 0, calls = 0, nativeCalls = 0, entries;
const writer = new Writer();
export function battle(context) {
  game = context.game; const flags = context.options;
  entries = [];
  for (const hull of game.vehicles.vehicles) for (const seat of hull.seats) for (const w of seat.weapons) entries.push({ weapon: w.state, native: flags.hulls === 'rust' });
  for (const turret of game.turretSeats?.turrets ?? []) if (turret.weapon) entries.push({ weapon: turret.weapon, native: flags.battle === 'rust' });
  const owned = new Map(entries.map(e => [e.weapon, e]));
  const proto = Object.getPrototypeOf(game.playerUnit.combat.arsenal.weapons[0]), update = proto.update;
  proto.update = function (...args) { const entry = owned.get(this); if (entry) { calls++; assert.equal(entry.native, false, 'a native holder called JS Weapon.update'); } return update.apply(this, args); };
  for (const { weapon, native } of entries) if (native) {
    assert.ok(Object.hasOwn(weapon, 'update'), 'the native ordinary weapon is attached');
    const update = weapon.update;
    weapon.update = function (...args) { nativeCalls++; return update.apply(this, args); };
  }
  const i = process.argv.indexOf('--weapons-trace');
  if (i >= 0) { fd = fs.openSync(process.argv[i + 1], 'w'); fs.writeSync(fd, MAGIC); }
}
export function ticked(tick) {
  if (fd === null) return;
  const values = entries.map(({ weapon }) => ({ state: exportWeapon(weapon, { seen: new WeakMap(), def: (_table, d) => d.class }),
    definition: weapon.levels.indexOf(weapon.def), seeded: Boolean(weapon.salvoRandom) }));
  writer.record(RECORD.frame, tick, 'ordinary-weapons', values); fs.writeSync(fd, writer.take()); records++;
}
export function ended() {
  if (fd !== null) { fs.closeSync(fd); fd = null; }
  assert.ok(calls + nativeCalls > 0, 'ordinary holders updated their weapons');
  console.error(JSON.stringify({ weaponProbe: { weapons: entries.length, records, javascript: calls, native: nativeCalls } }));
}
