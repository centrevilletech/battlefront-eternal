import {test} from 'node:test';
import assert from 'node:assert/strict';
import {entityImportance,entityUpdateInterval} from '../src/runtime/network-entity-importance.js';
import {ENTITY_SOLDIER_STATES} from '../src/runtime/network-entity-codec.js';
const view={position:[0,0,0],velocity:[0,0,0],forward:[0,0,1]};
const soldier=(position,velocity=[0,0,0])=>({kind:'soldier',health:100,position,velocity});
test('continuous distance and authoritative view distinguish otherwise equal actors',()=>{
 assert(entityImportance(soldier([0,0,30]),view)>entityImportance(soldier([0,0,300]),view));
 assert(entityImportance(soldier([0,0,100]),view)>entityImportance(soldier([0,0,-100]),view));
 const a=entityImportance(soldier([0,0,39.99]),view),b=entityImportance(soldier([0,0,40.01]),view);assert(Math.abs(a-b)<.01);
});
test('closing aircraft retain importance at flying engagement distances',()=>{
 const aircraft={kind:'vehicle',health:100,position:[0,0,500],velocity:[0,0,-125]};
 const approaching=entityImportance(aircraft,{...view,flying:true});
 assert(approaching>entityImportance({...aircraft,velocity:[0,0,125]},{...view,flying:true}));
 assert(approaching>entityImportance(soldier([0,0,500]),view));
 assert.equal(entityImportance({...aircraft,health:0},view),1);
 assert(approaching<=48);
});
test('lifecycle and viewpoint-free spectators retain bounded priorities',()=>{
 assert.equal(entityImportance({kind:'tombstone'}),16);assert.equal(entityImportance({kind:'seat'}),16);
 assert.equal(entityImportance({kind:'vehicle'}),6);assert.equal(entityImportance({kind:'soldier'}),1);
});
test('freshness targets cover close actors, crosshair targets, closing vehicles and distant starvation',()=>{
 assert.equal(entityUpdateInterval(soldier([30,0,0]),view),3);
 assert.equal(entityUpdateInterval(soldier([0,0,180]),view),3);
 assert.equal(entityUpdateInterval(soldier([180,0,0]),view),12);
 assert.equal(entityUpdateInterval({kind:'vehicle',position:[300,0,0],velocity:[-160,0,0]},view),3);
 assert.equal(entityUpdateInterval({kind:'vehicle',position:[500,0,0],velocity:[30,0,0]},view),6);
 assert.equal(entityUpdateInterval(soldier([500,0,0]),view),30);
 assert.equal(entityUpdateInterval({...soldier([0,0,5]),health:0},view),30);
 assert.equal(entityUpdateInterval({kind:'combat'},view),2);
});

test('airborne corpses retain moving-body service through the apex, then settle to corpse service',()=>{
 const flying={...soldier([20,5,0],[8,6,0]),health:0,state:ENTITY_SOLDIER_STATES.indexOf('thrownDead')};
 for(const velocity of [[8,6,0],[0,0,0],[8,-6,0]]) {
  const record={...flying,velocity};
  assert.equal(entityUpdateInterval(record,view),3);
  assert.equal(entityImportance(record,view),entityImportance({...record,health:100},view));
 }
 const landed={...flying,state:ENTITY_SOLDIER_STATES.indexOf('dead')};
 assert.equal(entityUpdateInterval(landed,view),30);
 assert.equal(entityImportance(landed,view),1);
 assert.equal(entityUpdateInterval({...flying,position:[500,0,0]},view),30,'distance still bounds service');
});
