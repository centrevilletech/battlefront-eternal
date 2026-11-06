import test from 'node:test';
import assert from 'node:assert/strict';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';
import { createNetworkLifecycleBridge } from '../src/runtime/world-network-lifecycle-sim.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { f3World } from './fixtures/f3-world.js';

// Lifecycle fixtures use canonical bodies. Their expected state is independent
// of the deleted JavaScript unit flags and needs no extracted game content.
test('network lifecycle ports are retained per native battle', async () => {
  const first = await loadSim(), bridge = bridgeFor(first);
  const ports = createNetworkLifecycleBridge(bridge), handler = bridge.handlers.networkLifecycle;
  assert.equal(createNetworkLifecycleBridge(bridge), ports);
  assert.equal(bridge.handlers.networkLifecycle, handler);
  const world = createWorldSim(first);
  for (const name of ['networkSpawn', 'networkUnspawn', 'networkRemove']) assert.equal(world[name], ports[name]);
  const other = await loadSim();
  assert.notEqual(createNetworkLifecycleBridge(bridgeFor(other)), ports);
});

test('native network unspawn clears a live canonical body and its connection status', async () => {
  const world = await f3World({trooper:{class:'trooper',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit = world.spawn('trooper',2,4,true), position = unit.position, life = unit.generation;
  const player = {unit,team:1,spawn:{phase:'alive',spawnedOnce:true,post:'home'}};
  const game = {ai:{spawning:{teams:{1:{living:1}}}}};
  const ports = createNetworkLifecycleBridge(world.phase.bridge);
  ports.networkUnspawn(game,player);
  assert.deepEqual([unit.alive,unit.dead,unit.combat.dead,unit.soldier.dead,unit.targetable], [false,true,true,true,false]);
  assert.equal(unit.position,position);assert.equal(unit.generation,life);
  assert.equal(game.ai.spawning.teams[1].living,0);
  assert.deepEqual(player.spawn,{phase:'unspawned',spawnedOnce:false,post:null,deadTime:unit.deadTime});
  assert.equal(unit.actor.root.visible,false);
});

test('native network removal stamps the connection and releases its current control once', async () => {
  const instance = await loadSim(), ports = createNetworkLifecycleBridge(bridgeFor(instance));
  const calls = [], unit = {seat:{vehicle:{}}}, player = {id:'peer',unit,disconnectedAt:null};
  const game = {worldClock:{tick:20},vehicles:{unboard(body,options){
    assert.equal(body,unit);assert.deepEqual(options,{force:true,placement:false});calls.push('unboard');body.seat=null;
  }}};
  const players = new Map([['peer',player]]);
  ports.networkRemove(game,players,'peer',p=>{assert.equal(p,player);calls.push('stop');},{exit(id){assert.equal(id,'peer');calls.push('exit');}});
  assert.equal(player.disconnectedAt,21);assert.equal(unit.seat,null);
  assert.deepEqual(calls,['stop','exit','unboard']);
  ports.networkRemove(game,players,'absent',()=>assert.fail('missing connection'));
});
