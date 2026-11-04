// Exercise the optional native prop adapter without changing the game's
// dispatch. When other phases use the module, join their same bridge.
import { attachPropsSim, PropsSim } from '../src/props-sim.js';
import { loadSim } from '../src/sim/module.js';
const instance = await loadSim();
export function battle({ game }) {
  const bridge = game.simBridge ?? game.unitsSim?.bridge;
  game.propsSim = bridge ? new PropsSim(bridge, game.live) : attachPropsSim(instance, game.live);
}
