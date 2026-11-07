import { bridgeFor } from '../sim/runtime.js';
import { clockPorts } from './world-clock-sim.js';
import { eventPorts } from './world-events-sim.js';
import { localPorts } from './world-local-sim.js';
import { networkPorts } from './world-network-sim.js';
import { networkControlPorts } from './world-network-controls-sim.js';
import { createNetworkLifecycleBridge } from './world-network-lifecycle-sim.js';

const worlds = new WeakMap();

export function createWorldSim(instance) {
  const bridge = bridgeFor(instance), x = bridge.x;
  if (worlds.has(bridge)) return worlds.get(bridge);
  const world = { bridge, ...clockPorts(bridge), ...eventPorts(bridge), ...localPorts(bridge), ...networkPorts(bridge), ...networkControlPorts(bridge), ...createNetworkLifecycleBridge(bridge) };
  worlds.set(bridge, world);
  return world;
}
