import { bridgeFor } from '../../runtime.js';
import { clockPorts } from './world-clock-sim.js';
import { eventPorts } from './world-events-sim.js';
import { localPorts } from './world-local-sim.js';

const worlds = new WeakMap();

export function createWorldSim(instance) {
  const bridge = bridgeFor(instance);
  if (worlds.has(bridge)) return worlds.get(bridge);
  const world = { bridge, ...clockPorts(bridge), ...eventPorts(bridge), ...localPorts(bridge) };
  worlds.set(bridge, world);
  return world;
}
