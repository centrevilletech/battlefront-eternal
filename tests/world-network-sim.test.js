import test from 'node:test';
import assert from 'node:assert/strict';
import {networkPorts} from '../src/runtime/world-network-sim.js';
import {createNetworkStepHost} from '../src/runtime/world-network-host.js';
import {f3World} from './fixtures/f3-world.js';

// Exercise the connection driver with its real canonical bodies. The golden
// schedule records gameplay operations, without retaining a JavaScript twin.
async function connection() {
  const world=await f3World({trooper:{class:'trooper',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit=world.spawn('trooper',0,0,true), trace=[];
  const p={id:'peer',unit,team:1,character:{unit},spawn:{phase:'alive',spawnedOnce:true,post:'home'},
    lastInputAt:0,inputsStale:false,disconnectedAt:null,spectator:false,autoRespawn:false,
    mailbox:{take(){trace.push('input');return {actions:[],human:[]};}},controls:{accept(){trace.push('accept');}}};
  const players=new Map([['peer',p]]);
  const game={worldClock:{tick:10},battle:{gameOver:false},playerUnit:unit,
    step(){trace.push('step');world.phase.phase(0);this.worldClock.tick++;}};
  const host=createNetworkStepHost({game,players,
    hitHistory:{capture(tick){trace.push(`capture ${tick}`);}},
    roster:{release(character){assert.equal(character.unit,unit);trace.push('release');}},
    remoteWorld:{retire(id){assert.equal(id,'peer');trace.push('retire');}},
    stopInput(){trace.push('stop');},spawn(){assert.fail('no automatic spawn requested');},
    traceHuman(){assert.fail('no discrete human choice');},controls(){trace.push('controls');},
    updateViews(){trace.push('views');}});
  return {world,unit,p,players,trace,game,network:networkPorts(world.phase.bridge).network,host};
}

test('native connection driver stops stale input before applying the next command',async()=>{
  const f=await connection();
  assert.equal(f.network(f.host,10,501,3600,4),11);
  assert.deepEqual(f.trace,['capture 10','stop','input','accept','controls','views','step']);
  assert.equal(f.p.inputsStale,true);assert.equal(f.game.worldClock.tick,11);
});

test('native connection driver finishes removed-body delay without consuming controls',async()=>{
  const f=await connection();f.unit.combat.damage(Infinity);f.unit.remove();f.world.phase.deadTime(f.unit,3.99);
  f.network(f.host,10,500,3600,4);
  assert.equal(f.unit.deadTime,4);assert.equal(f.p.spawn.phase,'awaiting');assert.equal(f.p.spawn.deadTime,4);
  assert.deepEqual(f.trace,['capture 10','input','accept','views','step']);
});

test('native connection driver retires an expired disconnected body before advancing the battle',async()=>{
  const f=await connection();f.p.disconnectedAt=1;
  f.network(f.host,3600,500,3600,4);
  assert.equal(f.players.size,0);
  assert.deepEqual(f.trace,['capture 10','release','retire','views','step']);
});
