// Install the optional page adapter before the capture wraps its methods.
if (process.argv.includes('--capture-out') || process.argv.includes('--capture-scan')) await import('./sim-capture/plugin.mjs');
const { loadSim } = await import('../src/sim/module.js');
const { attachDebrisSim, DebrisSim } = await import('../src/debris-sim.js');
const instance = await loadSim();
export function battle({ game }) {
  const bridge = game.propsSim?.bridge ?? game.simBridge ?? game.unitsSim?.bridge;
  game.debrisSim = bridge ? new DebrisSim(bridge, game.debris) : attachDebrisSim(instance, game.debris);
}
