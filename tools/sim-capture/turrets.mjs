// Turret math and rig commands have no foreign effects. Their own angles,
// local poses, world matrices, output vectors and return values are all
// checked by the native replay.
import { TURRET_COMMANDS, turretInput, turretSettings } from '../../src/turrets-sim.js';
export function turretMathState(method, game, args, turret) {
  return { settings: turretSettings(turret), input: turretInput(method, args, turret, game.live.turrets.indexOf(turret)),
    muzzleDirection: method === 'muzzle' ? args[1] ?? null : null };
}
export const turrets = { frames: TURRET_COMMANDS.map(method => ({
  method: `Turret.${method}`, target: game => game.live.turrets.length ? Object.getPrototypeOf(game.live.turrets[0]) : null,
  state: (game, args, cap, turret) => turretMathState(method, game, args, turret),
})) };
