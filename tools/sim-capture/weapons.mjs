// Ordinary vehicle and turret weapon methods. Do not import weapons.js:
// its seeded generator must first pass through the capture loader.
import { exportWeapon } from '../../src/sim/gen/combat.js';

function owners(game) {
  const found = new Map();
  for (const hull of game.vehicles.vehicles) for (let seat = 0; seat < hull.seats.length; seat++) {
    for (let index = 0; index < hull.seats[seat].weapons.length; index++) found.set(hull.seats[seat].weapons[index].state, { kind: 'vehicle', hull: hull.name, seat, index });
  }
  for (let index = 0; index < (game.turretSeats?.turrets.length ?? 0); index++) found.set(game.turretSeats.turrets[index].weapon, { kind: 'turret', index });
  return found;
}
const books = new WeakMap();
function book(game) {
  let value = books.get(game);
  if (!value) { value = owners(game); books.set(game, value); }
  return value;
}
function state(game, args, _cap, weapon) {
  const owner = book(game).get(weapon);
  if (!owner) return { skipped: true };
  const value = exportWeapon(weapon, { seen: new WeakMap(), def: (_table, def) => def });
  return { owner, weapon: value, definition: weapon.levels.indexOf(weapon.def), seeded: Boolean(weapon.salvoRandom),
    target: args[1] && typeof args[1] === 'object' && 'id' in args[1]
      ? { id: args[1].id, generation: args[1].generation ?? 0, alive: Boolean(args[1].alive) } : null };
}
const METHODS = ['update', 'deselect', 'clearLock', 'updateLock', 'startReload', 'baseLevel', 'spread'];
export const weapons = { frames: METHODS.map(method => ({ method: `Weapon.${method}`,
  target: game => Object.getPrototypeOf(game.playerUnit.combat.arsenal.weapons[0]), state,
})) };
