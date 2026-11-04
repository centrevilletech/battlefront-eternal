// Join the battle's bridge where another phase already made it. Imports
// of the game classes happen after any capture hooks are ready. The
// world-hash battle hook is synchronous, so prepare classes up front.
import { loadSim } from '../src/sim/module.js';
if (process.argv.some(a => a === '--capture-out' || a === '--capture-scan')) await import('./sim-capture/plugin.mjs');
const { PropsSim, attachPropsSim } = await import('../src/props-sim.js');
const { attachTurretSeatsWithClasses, turretSeatClasses } = await import('../src/turret-seats-sim.js');
const classes = await turretSeatClasses();
const instance = await loadSim();
export function battle({ game }) {
  const bridge = game.propsSim?.bridge ?? game.unitsSim?.bridge ?? game.simBridge;
  const props = game.propsSim ?? (bridge ? new PropsSim(bridge, game.live) : attachPropsSim(instance, game.live));
  game.turretSeatsSim = attachTurretSeatsWithClasses(props.bridge, game.turretSeats, classes);
}
