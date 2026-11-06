import test from 'node:test';
import assert from 'node:assert/strict';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

test('native battle tail removes a lingering server corpse after battle', async () => {
  const world = createWorldSim(await loadSim()), calls = [];
  const state = {input:{}, server:true, dead:true, deathTime:4, corpseLinger:4};
  const host = {state:() => state, publish(){}, call(op) { calls.push(op); if (op === 'remove') state.removed = true; }};
  world.battleTail(host, 1/60);
  assert.deepEqual(calls, ['battle','clearHuman','remove','present']);
  assert.equal(state.removed, true);
});
