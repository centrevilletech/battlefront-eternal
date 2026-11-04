import { bridgeFor } from '../sim/runtime.js';
import { putWsSeparation, WsSeparationResultMirror, syncWsSeparationResult } from '../sim/gen/world.js';
import { clockPorts } from './world-clock-sim.js';
import { eventPorts } from './world-events-sim.js';
import { localPorts } from './world-local-sim.js';
import { stepPorts } from './world-step-sim.js';
import { networkPorts } from './world-network-sim.js';

const worlds = new WeakMap();

export function createWorldSim(instance) {
  const bridge = bridgeFor(instance), x = bridge.x;
  if (worlds.has(bridge)) return worlds.get(bridge);
  const input = { words: x.world_separation_words(0), put: putWsSeparation };
  const output = { Mirror: WsSeparationResultMirror, sync: syncWsSeparationResult };
  function separate(all, { player = null, playerOut = false, global = false, camera }) {
    const members = all.filter(unit => unit.soldier);
    bridge.callArgs(input, { members: members.map(unit => ({ id: unit.id, position: unit.position, tickCount: unit.tickCount,
      walker: Boolean(unit.walker), posture: unit.hitPosture === 'prone' ? 'prone' : unit.hitPosture === 'crouch' ? 'crouch' : 'stand',
      seat: Boolean(unit.seat), held: Boolean(unit.held), removed: Boolean(unit.removed), alive: Boolean(unit.alive), dead: Boolean(unit.dead) })),
    player: player?.id ?? null, playerOut, global, camera });
    const out = bridge.callResult(output, x.world_separate());
    for (let i = 0; i < members.length; i++) {
      const value = out.members[i], unit = members[i];
      unit.position.copy(value.position); unit.tickCount = value.tickCount;
    }
  }
  const world = { bridge, separate, ...clockPorts(bridge), ...eventPorts(bridge), ...localPorts(bridge), ...stepPorts(bridge), ...networkPorts(bridge) };
  worlds.set(bridge, world);
  return world;
}
