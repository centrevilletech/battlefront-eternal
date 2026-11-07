import test from 'node:test';
import assert from 'node:assert/strict';
import {AiWorld} from '../src/sim/wasm/ai-world.js';
import {loadPageWeaponPolicies,bindWeaponPolicyContent} from '../src/sim/wasm/weapon-policy-content.js';
import {loadSim} from '../src/sim/module.js';
import {AiWorldSetupMirror,AiWorldContentMirror} from '../src/sim/gen/ai-phase.js';
import {weaponAI} from '../src/weapon-policy.js';
import {Weapon} from '../src/weapons.js';
import {f3World} from './fixtures/f3-world.js';
import {worldFrame} from './helpers/world-frame.js';
import {weaponDefinition} from './fixtures/client-weapons.js';
import {fleetFixture} from './fixtures/f3-fleet.js';

async function fixture(mask=4,mounted=false) {
  const launcher=weaponDefinition('policy-launcher',0,{base:'launcher',LockOnRange:100,LockOnAngle:45,LockOffAngle:50,LockTime:1,
    TargetEnemy:1,TargetFriendly:0,MaxPressedTime:1,ordnance:{class:'policy-missile',base:'missile'},charges:[{at:.5}]});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[launcher]},target:{class:'target',base:'soldier',MaxHealth:100,weapons:[]}},{prepareFleet:mounted?fleetFixture({positions:[0],weapon:launcher}):null});
  const unit=world.spawn('soldier',0,0,true,null),target=world.spawn('target',0,12,true,null);
  for(const body of [unit,target]) {world.phase.begin(world.phase.bindings[body.id],'team');world.phase.change.value=1;world.phase.commit();}
  // A mounted network rider fires in the crew phase; the registered local
  // viewpoint stays elsewhere, as it does for a remote player in a battle.
  const viewpoint=mounted?world.spawn('target',80,80,true,null):unit;
  const step=worldFrame(world,viewpoint),weapon=unit.combat.arsenal.weapons[0];
  const hull=world.factory.context.vehicles?.vehicles[0];
  if(hull)assert.equal(world.factory.context.vehicles.board(unit,hull,0),true);
  const retained=weaponAI(weapon.levels[0]),charged=weaponAI(weapon.levels[1]);
  assert.equal(retained.relationMask,1);
  const setup=new AiWorldSetupMirror();
  Object.assign(setup.settings.capacity,{registry:64,characters:4,posts:0,hulls:mounted?1:0,buildings:0,controls:4,grenades:8,scopes:4,cameras:1,supportKeys:8,repairJobs:8});
  Object.assign(setup.navigation.pools,{paths:8,pathNodes:64,requests:8});
  const content=new AiWorldContentMirror();
  content.weapons=[launcher];
  content.weaponData=[{json:new TextEncoder().encode(JSON.stringify({weapons:{'policy-launcher':{
    ai:{relationMask:mask,categoryMask:1,maxRange:91},levels:[{at:.5,ai:{relationMask:2,categoryMask:4,maxRange:72}}],
  }}}))}];
  // AI-off seats use the same cold content entry before any AI activation.
  let owner=null;
  if(mounted)bindWeaponPolicyContent(world.phase.bridge,content.weapons,content.weaponData.map(row=>row.json));
  else {owner=new AiWorld(world.phase,setup);owner.content(content);}
  return {world,owner,unit,target,weapon,retained,charged,step,launcher,hull};
}

test('native cooked policies refresh retained player and HUD views including charge levels',async()=>{
  const f=await fixture();
  assert.equal(weaponAI(f.weapon.levels[0]),f.retained);assert.equal(f.retained.relationMask,4);
  assert.equal(f.retained.categoryMask,1);assert.equal(f.retained.maxRange,91);
  assert.equal(weaponAI(f.weapon.levels[1]),f.charged);assert.equal(f.charged.relationMask,2);assert.equal(f.charged.categoryMask,4);
  const later=new Weapon(f.launcher);
  assert.equal(weaponAI(later.levels[1]),f.charged,'future cold bodies share their charge policy without a class-name scan');
  f.step();
  assert.equal(f.weapon.lockTargetId,f.target.id,'native player lock services use the cooked friendly mask too');
});

test('matching weapon names in separate native worlds cannot replace one another\'s policy',async()=>{
  const first=await fixture(4),second=await fixture(1);
  assert.equal(weaponAI(first.weapon.levels[0]).relationMask,4);
  assert.equal(weaponAI(second.weapon.levels[0]).relationMask,1);
  assert.notEqual(first.retained,second.retained);
});


test('native AI-off mounted targeting receives content masks without replacing weapon state',async()=>{
  const f=await fixture(4,true),state=f.hull.seats[0].weapons[0].state;
  const identity=state;
  for(let i=0;i<6;i++)f.step();
  assert.equal(f.hull.seats[0].weapons[0].state,identity);
  assert.equal(state.lockTargetId,f.target.id);
});

// A presentation-only page has no AiWorld or authoritative unit phase.
// It must still load policies for future bodies, seats and stationary guns.
test('page resource loading publishes native policies without an AI world',async()=>{
  const instance=await loadSim(),unit=weaponDefinition('page-player',0,{charges:[{at:.5}]}),
    hull=weaponDefinition('page-seat'),turret=weaponDefinition('page-turret'),child=weaponDefinition('page-droid');
  unit.droid={weapons:[child]};
  const retained=weaponAI(unit),records={weapons:{}};
  for(const [i,def] of [unit,hull,turret,child].entries())records.weapons[def.class]={ai:{relationMask:4,categoryMask:1<<i},
    levels:[{at:.5,ai:{relationMask:2,categoryMask:16}}]};
  const fetchBefore=globalThis.fetch;
  globalThis.fetch=async()=>({ok:true,arrayBuffer:async()=>new TextEncoder().encode(JSON.stringify(records)).buffer});
  try {
    await loadPageWeaponPolicies(instance,{sides:{rep:{units:{soldier:{weapons:[unit]}}}},
      vehicles:{tank:{seats:[{weapons:[{def:hull}]}]}},props:[{weapon:turret}]});
  }finally{globalThis.fetch=fetchBefore;}
  assert.equal(weaponAI(unit),retained);assert.equal(retained.relationMask,4);
  for(const [i,def] of [unit,hull,turret,child].entries())assert.equal(weaponAI(def).categoryMask,1<<i);
  assert.equal(weaponAI(new Weapon(unit).levels[1]).relationMask,2);
});

// AI-off battles still use the native content owner for human targeting.
test('native human targeting loads cooked masks when no AI world exists',async()=>{
  const launcher=weaponDefinition('human-policy',0,{base:'launcher',LockOnRange:100,LockOnAngle:45,LockOffAngle:50,
    LockTime:1,TargetEnemy:1,TargetFriendly:0,ordnance:{class:'human-missile',base:'missile'}});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[launcher]}});
  const viewer=world.spawn('soldier',0,0,true,null),target=world.spawn('soldier',0,12,true,null);
  for(const body of [viewer,target]){world.phase.begin(world.phase.bindings[body.id],'team');world.phase.change.value=1;world.phase.commit();}
  const data=new TextEncoder().encode(JSON.stringify({weapons:{'human-policy':{ai:{relationMask:4,categoryMask:1}}}}));
  bindWeaponPolicyContent(world.phase.bridge,[launcher],[data]);
  worldFrame(world,viewer)();
  assert.equal(viewer.combat.arsenal.weapons[0].lockTargetId,target.id);
  assert.ok(!world.phase.aiOwner);
});
