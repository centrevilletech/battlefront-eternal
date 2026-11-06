import test from 'node:test';
import assert from 'node:assert/strict';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

test('world step runs one owning phase before ordnance, items and battle', async () => {
  const world = createWorldSim(await loadSim()), calls = [];
  const state = {server:true, removed:true, ai:true, live:true, water:true};
  const host = {state:() => state, call(op) { calls.push(op); return {}; }};
  world.step(host, 1/60, 1, null);
  assert.deepEqual(calls, ['start','water','clockAxes','takeSquad','takeSquad','takeSquad','takeSquad',
    'playerCount','eye','zoom','cameraMatrix','ai','nativeUnits','served','live','ordnance',
    'mineTargets','items','battlefield','battle','target','spawnFlush']);
});
