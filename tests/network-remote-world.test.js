import test from 'node:test';import assert from 'node:assert/strict';
import {Scene,Vector3} from 'three';import {RemoteDroids} from '../src/remote.js';import {NetworkRemoteWorld} from '../src/runtime/network-remote-world.js';
const controls={take:()=>false,held:()=>false};
function fixture(){const shots=[];const remotes=new RemoteDroids({scene:new Scene(),camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,fire:(def,at,dir,owner)=>shots.push(owner),explode:()=>{}});const world=new NetworkRemoteWorld(remotes,{capacity:2,randomFor:key=>()=>key/10});return {world,shots};}
function launch(context){return context.launch({def:{SwitchImmediately:1,droid:{weapons:[],AddHealth:-1}}},new Vector3(0,2,0),new Vector3(),0,1);}
test('world advances each owner once and finishes strikes after body retirement',()=>{
 const {world,shots}=fixture(),a={alive:true},b={alive:true},ca=world.attach(1,a),cb=world.attach(2,b),da=launch(ca),db=launch(cb);world.step(1,1,1/60,{forward:1},controls);const health=da.health;world.finish(1,1/60);assert.equal(da.health,health);assert.equal(db.health,health);assert.equal(world.step(1,1,1/60,{},controls),false);
 const def={SalvoCount:2,SalvoDelay:.5,AimDistance:10,strike:{}};ca.strike(da,def);a.alive=false;ca.kill(da);world.retire(1);world.finish(2,0);assert.equal(ca.active,false);assert.equal(cb.active,true);assert.equal(world.contexts.size,2);world.finish(3,.5);world.finish(4,0);assert.equal(world.contexts.size,1);assert.deepEqual(shots,[a,a]);assert.equal([...world.droids()].length,1);
 world.retire(2);cb.kill(db);world.finish(5,0);assert.equal(world.contexts.size,0);assert.ok(world.attach(1,{alive:true}));
});
test('owner capacity and random requirements are explicit',()=>{const {world}=fixture();world.attach(1,{alive:true});world.attach(2,{alive:true});assert.throws(()=>world.attach(3,{}),/capacity/);assert.throws(()=>world.attach(1,{}),/owner changed/);assert.throws(()=>new NetworkRemoteWorld(world.remotes).attach(1,{}),/random stream/);});

test('actual Unit controller freezes body intent and routes use to shared recon exit',async()=>{
 const {Unit,IntentControls,blankIntent}=await import('../src/units.js');const {world}=fixture();const inputs=[];const unit={alive:true,removed:false,tickCount:0,intent:blankIntent(),input:{},soldier:{tick(input){inputs.push({...input});}},presentation:{beforeStep(){},afterStep(){}},onMoved(){},combat:{update(){},flushMuzzles(){}}};
 const context=world.attach(1,unit),droid=launch(context);unit.combat.remotes=context;unit.controls=new IntentControls(unit);let tick=1;unit.remoteStep=(dt,input,c)=>world.step(1,tick,dt,input,c);unit.intent.forward=1;Unit.prototype.tick.call(unit,1/60);assert.equal(inputs[0].forward,0);assert.ok(droid.velocity.length()>0);
 tick++;unit.intent.use=true;Unit.prototype.tick.call(unit,1/60);assert.equal(context.active,false);assert.equal(unit.intent.use,false);tick++;Unit.prototype.tick.call(unit,1/60);assert.equal(inputs[2].forward,1);
});

test('network Unit launch advances on the same following tick as the single-player order',async()=>{
 const {Unit,IntentControls,blankIntent}=await import('../src/units.js');const {world}=fixture();const solo=world.remotes.createContext('player',{random:()=>.5});
 const unit={alive:true,removed:false,tickCount:0,intent:blankIntent(),input:{},soldier:{tick(){}},presentation:{beforeStep(){},afterStep(){}},onMoved(){},combat:{flushMuzzles(){}}};const context=world.attach(1,unit);unit.combat.remotes=context;unit.controls=new IntentControls(unit);let tick=1,net,sp;
 unit.remoteStep=(dt,input,c)=>world.step(1,tick,dt,input,c);unit.combat.update=()=>{if(!net)net=launch(context);};unit.intent.forward=1;
 solo.update(1/60,null,controls);sp=launch(solo);Unit.prototype.tick.call(unit,1/60);world.finish(1,1/60);assert.deepEqual(net.position.toArray(),sp.position.toArray());assert.equal(net.health,sp.health);
 for(tick=2;tick<30;tick++){solo.update(1/60,{forward:1,right:0,turn:0},controls);Unit.prototype.tick.call(unit,1/60);world.finish(tick,1/60);assert.deepEqual(net.position.toArray(),sp.position.toArray());assert.deepEqual(net.velocity.toArray(),sp.velocity.toArray());assert.equal(net.health,sp.health);}
});

test('headless authority uses shared simulation camera for third-person recon aim',async()=>{
 const {PerspectiveCamera}=await import('three');const {captureRemoteReplay,restoreRemoteReplay}=await import('../src/runtime/network-remote-replay.js');const {world}=fixture();const owner={alive:true};const network=world.attach(1,owner),single=world.remotes.createContext('player',{camera:new PerspectiveCamera(),random:()=>.1});const a=launch(single),b=launch(network);assert.equal(network.camera,null);
 for(let tick=1;tick<40;tick++){single.look(2,-1,.2,false);network.look(2,-1,.2,false);single.updateCamera(1/60,false,true);single.update(1/60,{forward:1,right:.2,turn:.1},controls);world.step(1,tick,1/60,{forward:1,right:.2,turn:.1},controls);assert.deepEqual(a.target.toArray(),b.target.toArray());assert.deepEqual(single.aimCamera.position.toArray(),network.aimCamera.position.toArray());}
 const saved=captureRemoteReplay(network,b),restored=world.remotes.createContext('player',{random:()=>.1}),d=launch(restored);restoreRemoteReplay(restored,d,saved);assert.deepEqual(restored.aimCamera.position.toArray(),network.aimCamera.position.toArray());assert.equal(restored.simulationView.reach,network.simulationView.reach);
});

test('recon slots reject launches gracefully and reuse only retired generations',()=>{
 const {world:fixtureWorld}=fixture();const world=new NetworkRemoteWorld(fixtureWorld.remotes,{capacity:2,slotCapacity:2,randomFor:()=>()=>.5});
 const context=world.attach(1,{alive:true}),a=launch(context),b=launch(context),first=world.identityFor(a),second=world.identityFor(b);
 assert.equal(launch(context),null);assert.equal(context.droids.length,2);assert.equal(first.id,0);assert.equal(second.id,1);
 context.kill(a);const c=launch(context),replacement=world.identityFor(c);assert.equal(replacement.id,0);assert.equal(replacement.generation,2);assert.equal(world.slots.length,2);assert.throws(()=>world.identityFor(a),/dead/);
});


test('shared Weapon admission rejects a full recon pool before ammo, heat, RNG or hidden state',async()=>{
 const {Weapon}=await import('../src/weapons.js');const {world:initial}=fixture();
 const world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:1,randomFor:()=>()=>.5}),context=world.attach(1,{alive:true}),occupied=launch(context);
 let randomCalls=0;const weapon=new Weapon({base:'remote',class:'probe',channel:1,RoundsPerClip:1,ammo:2,InitialSalvoDelay:0,ShotDelay:.5,HeatPerShot:.2,SpreadPerShot:.1,HideOnFire:1,SwitchImmediately:1,droid:{weapons:[]}}, {random:()=>{randomCalls++;return .5;}});
 weapon.admitShot=()=>context.admitLaunch(weapon);const before={clip:weapon.clip,reserve:weapon.reserve,heat:weapon.heat,kick:weapon.kick};const shots=[];
 weapon.startThrow(shots,{stick:0,posture:'stand',zoomed:false});assert.equal(shots.length,0);assert.equal(weapon.hidden,false);assert.equal(weapon.deployed,false);assert.equal(weapon.throwTime,-1);assert.equal(randomCalls,0);
 assert.deepEqual({clip:weapon.clip,reserve:weapon.reserve,heat:weapon.heat,kick:weapon.kick},before);
 context.kill(occupied);weapon.startThrow(shots,{stick:0,posture:'stand',zoomed:false});assert.equal(shots.length,1);assert.ok(weapon.clip<before.clip);
 const droid=context.launch(weapon,new Vector3(0,2,0),new Vector3(),0,1,shots[0].launchAdmission);assert.ok(droid);assert.deepEqual(world.identityFor(droid),{id:0,generation:2});assert.equal(world.slots[0].reservation,null);
});

test('disconnected strike owners retain bounded slots while new owners can attach and retry',()=>{
 const {world:initial}=fixture();const world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:1,randomFor:()=>()=>.5});
 const old=world.attach('old',{alive:false}),droid=launch(old);old.strike(droid,{SalvoCount:2,SalvoDelay:.1,AimDistance:10,strike:{}});old.kill(droid);world.retire('old');
 const current=world.attach('new',{alive:true});assert.equal(world.activeOwners,1);assert.equal(launch(current),null,'pending strike owns its retired identity');
 world.finish(1,.2);world.finish(2,.2);assert.equal(world.contexts.size,1);assert.ok(launch(current));assert.equal(world.slots.length,1);assert.equal(world.slots[0].generation,2);
});

test('failed recon construction releases reservation and exhausted generations refuse launch',()=>{
 const {world:initial}=fixture();const world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:1,randomFor:()=>()=>.5}),context=world.attach(1,{alive:true}),create=context.createDroid;
 context.createDroid=()=>{throw Error('construction failed');};assert.throws(()=>launch(context),/construction failed/);assert.equal(world.slots[0].reservation,null);
 context.createDroid=create;const droid=launch(context);assert.ok(droid);context.kill(droid);world.slots[0].generation=0xffffffff;assert.equal(launch(context),null);
});

test('two remote weapons in one frame cannot overbook a pending slot reservation',async()=>{
 const {Weapon}=await import('../src/weapons.js');const {world:initial}=fixture(),world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:1,randomFor:()=>()=>.5}),context=world.attach(1,{alive:true});
 const definition={base:'remote',class:'probe',channel:1,RoundsPerClip:2,ammo:2,InitialSalvoDelay:0,ShotDelay:.2,ShotsPerSalvo:2,SwitchImmediately:0,droid:{weapons:[]}};
 const a=new Weapon(definition),b=new Weapon(definition);a.admitShot=b.admitShot=()=>context.admitLaunch();const shots=[],aim={stick:0,posture:'stand',zoomed:false};
 a.startThrow(shots,aim);b.startThrow(shots,aim);assert.equal(shots.length,1);assert.equal(b.clip,1);assert.equal(b.hidden,false);assert.equal(b.deployed,false);
 assert.equal(a.clip,1-a.perShot);const droid=context.launch(a,new Vector3(),new Vector3(),0,1,shots[0].launchAdmission);assert.ok(droid);assert.equal(context.droids.length,1);
});


test('empty and dead-only retired owners cannot accumulate during same-tick join churn',()=>{
 const {world:initial}=fixture(),world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:1,randomFor:()=>()=>.5});
 for(let i=0;i<100;i++){const context=world.attach(i,{alive:false});if(i%2){const droid=launch(context);context.kill(droid);}world.retire(i);assert.equal(world.contexts.size,0);assert.equal(world.activeOwners,0);}
 assert.ok(world.slots.length<=1);
});

test('orphan strike churn stays bounded by active owners plus reserved lifecycle slots',()=>{
 const {world:initial}=fixture(),world=new NetworkRemoteWorld(initial.remotes,{capacity:1,slotCapacity:4,randomFor:()=>()=>.5});
 for(let round=0;round<3;round++){
  for(let n=0;n<20;n++){
   const key=round*20+n,context=world.attach(key,{alive:false}),droid=launch(context);
   if(droid){context.strike(droid,{SalvoCount:2,SalvoDelay:.1,AimDistance:10,strike:{}});context.kill(droid);}
   world.retire(key);assert.ok(world.contexts.size<=world.activeOwners+world.slots.length);assert.ok(world.contexts.size<=4);assert.ok(world.slots.length<=4);
  }
  assert.equal(world.contexts.size,4);world.finish(round*2+1,.2);world.finish(round*2+2,.2);assert.equal(world.contexts.size,0);
 }
});
