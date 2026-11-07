// Real content/GC checks accompany the synthetic sampler lifecycle test.
// These need --expose-gc and an installed data set, as the farm harness does.
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {content} from '../tools/headless-world.mjs';
import {createGame} from '../src/game.js';
import {loadSim} from '../src/sim/module.js';
import {collect} from '../tools/host-qualification/common.mjs';
const available=existsSync(`${content}/maps/nab2/map.json`);
const query='?map=nab2&side=rep&ai=classic&seed=1';

test('explicit battle teardown releases the actual game, instance and memory', {skip:!available}, async()=>{
  const refs=[];
  await (async()=>{
    const game=await createGame({headless:true,query,simulationFactory:async()=>{
      const instance=await loadSim();refs.push(new WeakRef(instance),new WeakRef(instance.exports.memory));return instance;
    }});
    refs.push(new WeakRef(game));
    assert.equal(game.dispose(),true);assert.equal(game.dispose(),false);
  })();
  assert.ok(await collect(refs),'closed game remained reachable');
});

test('failed construction after sampler installation releases its instance', {skip:!available}, async()=>{
  const refs=[];let reached=false;
  await assert.rejects(createGame({headless:true,query,simulationFactory:async()=>{
    const instance=await loadSim();refs.push(new WeakRef(instance),new WeakRef(instance.exports.memory));
    // Fail at the cold factory reservation, after the real sampler attached.
    // Every preceding kernel and schema operation remains the production one.
    return {holder:instance.holder,exports:{...instance.exports,f3_factory_reserve(){reached=true;throw Error('construction fixture');}}};
  }}),/construction fixture/);
  assert.equal(reached,true);assert.ok(await collect(refs),'failed construction retained its sampler');
});
