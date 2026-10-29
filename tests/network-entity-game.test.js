import test from 'node:test';
import assert from 'node:assert/strict';
import { Object3D } from 'three';
import { Soldier } from '../src/soldier.js';
import { SoldierAnimator } from '../src/animator.js';
import { SoldierPresentation } from '../src/soldier-presentation.js';
import { PlayerCombat } from '../src/combat.js';
import { Arsenal } from '../src/weapons.js';
import { Unit } from '../src/units.js';
import { CollisionWorld } from '../src/collision.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';
import { captureOwnPlayer } from '../src/runtime/network-entity-player.js';
import {captureSpawnSummary} from '../src/runtime/network-entity-summary.js';
import { createSnapshotSender } from './snapshot-send.js';

const registry={contentEpoch:1,classes:new Map([[1,'trooper']]),weapons:new Map([[1,'rifle']]),inputCodes:[],bindingProfiles:new Map([[0,{}]])};
function body(id) {
  const world=new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
  const soldier=new Soldier(world,{});soldier.spawn(0,0,0,0);
  const combat=Object.create(PlayerCombat.prototype);
  Object.assign(combat,{soldier,arsenal:new Arsenal([{class:'rifle',channel:0,RoundsPerClip:30,ammo:3,base:'cannon'}]),health:100,dead:false,deathTime:0,zoomStep:0,firstPerson:true,kick:{rate:0,hold:0,offset:0},updateHeld(){}});
  const unit=new Unit({className:'trooper',def:{},soldier,combat,presentation:new SoldierPresentation(soldier),actor:{root:new Object3D(),byName:{}},animator:{reset(){},update(){}}});
  Object.assign(unit,{id,generation:1,team:1,removed:false,alive:true});return unit;
}
function fixture() {
  const own=body(1),remote=body(2),map=new Map([[1,own],[2,remote]]),applied=[],corrections=[];
  const game={playerTeam:1,applyAuthoritativeHumanTeam(team){this.playerTeam=team;this.playerUnit.team=team;},ordnance:{cleared:0,clearProjectilePresentation(){this.cleared++;}},playerUnit:own,units:{byId:id=>map.get(id),byIdMap:map},vehicles:{vehicles:[]},client:{seats:{}},factory:{build(){throw Error('unexpected create');}}};
  const adapter=new EntityGameState(game,registry,{onApplied:p=>applied.push(p.tick),onCorrection:c=>corrections.push(c)});
  const sender=createSnapshotSender(adapter);
  return {own,remote,game,adapter,applied,corrections,sender};
}
const remoteRecord=(tick,x)=>({kind:'soldier',id:2,generation:1,tick,classId:1,team:2,position:[x,0,0],velocity:[0,0,0],yaw:.2,pitch:.1,health:80,posture:0,state:0,weapon:1,fireCounter:0});
function packet(f,authority,tick,ackInput,records=[]) {
  const own=captureOwnPlayer({unit:authority,tick,epoch:1,registry,classId:1});
  f.sender.send(tick,[own,...records],{ackInput});
}
function emptyPacket(f,tick,ackInput) { f.sender.send(tick,[],{ackInput}); }

test('semantic own correction applies immediately while real remote bodies use their own timeline',()=>{
  const f=fixture(),authority=body(1);authority.soldier.position.x=10;
  assert.throws(()=>packet(f,authority,100,1),/baseline/);
  f.adapter.baseline();assert.equal(f.game.ordnance.cleared,1);packet(f,authority,100,1,[{...remoteRecord(100,20),velocity:[36,0,0]}]);
  assert.equal(f.own.soldier.position.x,10);assert.equal(f.corrections[0].tick,100);assert.equal(f.own.alive,true);
  authority.soldier.position.x=30;
  packet(f,authority,110,2,[{...remoteRecord(110,26),velocity:[36,0,0]}]);
  f.adapter.present(105);
  assert.equal(f.own.soldier.position.x,30,'own authority is never delayed to the remote render clock');
  assert.equal(f.own.presentation.networkPose,null);
  assert.ok(Math.abs(f.remote.presentation.update(0,0).position.x-23)<0.05);
  assert.ok(Math.abs(f.remote.soldier.position.x-26)<0.05,'remote collision state remains latest authority');
  assert.deepEqual(f.corrections.map(c=>c.tick),[100,110]);
  f.sender.resend();assert.deepEqual(f.corrections.map(c=>c.tick),[100,110],'duplicate datagrams cannot reapply corrections');
});

test('new life restores exact own state and remote tombstone explicitly removes the body',()=>{
  const f=fixture(),authority=body(1);f.adapter.baseline();packet(f,authority,100,1,[remoteRecord(100,20)]);
  authority.generation=2;authority.soldier.spawn(40,0,20,1);authority.combat.dead=true;authority.removed=true;
  packet(f,authority,120,2,[{kind:'tombstone',id:2,generation:1,tick:120}]);
  assert.equal(f.own.generation,2);assert.equal(f.own.alive,false);assert.equal(f.own.dead,true);assert.equal(f.own.actor.root.visible,false);
  assert.equal(f.corrections.at(-1).reset,true);assert.equal(f.remote.removed,true);assert.equal(f.remote.actor.root.visible,false);
});

test('missing semantic prerequisites never produce application acknowledgements',()=>{
  const f=fixture(),authority=body(1);f.adapter.baseline();
  assert.throws(()=>packet(f,authority,100,1,[{...remoteRecord(100,20),classId:999}]),/definition/);
  assert.deepEqual(f.applied,[]);
});

test('remote death animation waits for its sampled pose and side effects occur once per life',async()=>{
 const {ENTITY_SOLDIER_STATES}=await import('../src/runtime/network-entity-capture.js');
 const f=fixture(),authority=body(1);let deaths=0,notifications=0;
 f.remote.animator.die=()=>deaths++;f.game.presentEvent=kind=>{if(kind==='died')notifications++;};
 const draw=tick=>{f.adapter.present(tick);f.remote.present(1/60,1);};
 f.adapter.baseline();packet(f,authority,100,1,[remoteRecord(100,20)]);
 const dead={...remoteRecord(101,20),state:ENTITY_SOLDIER_STATES.indexOf('dead'),health:0};
 packet(f,authority,101,2,[dead]);
 assert.equal(f.remote.dead,true);assert.equal(deaths,0);assert.equal(notifications,1);
 draw(100);assert.equal(deaths,0,'the visible living sample must not play a death clip');
 draw(101);assert.equal(deaths,1);
 packet(f,authority,250,3,[{...dead,tick:250}]);draw(250);draw(250);
 assert.equal(deaths,1);assert.equal(notifications,1,'repeated dead state cannot replay death side effects');
 packet(f,authority,251,4,[{...remoteRecord(251,20),generation:2}]);
 assert.equal(f.remote.dead,false);assert.equal(f.remote.presentation.deathPending,false);
 packet(f,authority,252,5,[{...dead,tick:252,generation:2}]);draw(251);
 assert.equal(deaths,1);draw(252);assert.equal(deaths,2);assert.equal(notifications,2);
});

test('own death animation plays when the entities datagram lands before the correction',async()=>{
 const {ENTITY_SOLDIER_STATES}=await import('../src/runtime/network-entity-capture.js');
 const f=fixture(),authority=body(1);let deaths=0,notifications=0;
 f.own.animator.die=()=>deaths++;f.game.presentEvent=kind=>{if(kind==='died')notifications++;};
 // The sender delivers the shared entities parts (carrying the player's own
 // soldier record) before the per-client correction datagram, as production
 // does: the uncorrected branch must start the death itself, or correct()
 // later sees combat.dead already true and never does (2026-10-14).
 const ownRecord=(tick,state,health)=>({kind:'soldier',id:1,generation:1,tick,classId:1,team:1,position:[0,0,0],velocity:[0,0,0],yaw:0,pitch:0,health,posture:0,state,weapon:1,fireCounter:0});
 f.adapter.baseline();packet(f,authority,100,1,[ownRecord(100,0,100)]);
 assert.equal(deaths,0);
 authority.combat.dead=true;authority.combat.health=0;authority.dead=true;authority.alive=false;
 packet(f,authority,101,2,[ownRecord(101,ENTITY_SOLDIER_STATES.indexOf('dead'),0)]);
 assert.equal(f.own.dead,true);assert.equal(deaths,1,'the death clip starts on the page body');assert.equal(notifications,1);
 packet(f,authority,102,3,[ownRecord(102,ENTITY_SOLDIER_STATES.indexOf('dead'),0)]);
 assert.equal(deaths,1,'and only once');assert.equal(notifications,1);
});

test('delayed launch and landing use the actual animator thrown and dead-rest transitions',async()=>{
 const {ENTITY_SOLDIER_STATES}=await import('../src/runtime/network-entity-capture.js');
 const f=fixture(),authority=body(1);
 // Exercise the real action state machine without a GPU or extracted skeleton.
 const animator=Object.create(SoldierAnimator.prototype),clip={duration:1};
 Object.assign(animator,{clips:new Map([['human_rifle_die',clip]]),transitions:{},
  thrownClips:{front:{flying:clip,land:clip,rest:clip},back:{flying:clip,land:clip,rest:clip}},
  update(dt,state){this.actionTime+=dt;this.updateAction(dt,state,Boolean(state.air));}});
 animator.reset(0);f.remote.animator=animator;
 let deaths=0;animator.die=function(){deaths++;SoldierAnimator.prototype.die.call(this);};
 const draw=tick=>{f.adapter.present(tick);f.remote.present(1/60,1);};
 f.adapter.baseline();packet(f,authority,100,1,[remoteRecord(100,20)]);
 const flying={...remoteRecord(101,20),state:ENTITY_SOLDIER_STATES.indexOf('thrownDead'),health:0,position:[20,2,0],velocity:[-9,8,0]};
 packet(f,authority,101,2,[flying]);
 assert.equal(f.remote.dead,true);assert.equal(f.remote.alive,false);assert.equal(f.remote.soldier.air,'thrown');
 draw(100);assert.equal(animator.action,'loco');assert.equal(deaths,0);
 draw(101);assert.equal(animator.action,'thrown');assert.equal(animator.died,true);assert.equal(deaths,1);
 const landed={...remoteRecord(160,12),state:ENTITY_SOLDIER_STATES.indexOf('dead'),health:0};
 packet(f,authority,160,3,[landed]);
 assert.equal(f.remote.soldier.air,null,'collision already knows the landing');
 draw(110);assert.equal(animator.action,'thrown','visible historical body is still flying');
 assert.equal(f.remote.presentation.state.air,'thrown');
 draw(160);assert.equal(animator.action,'thrownRest');assert.equal(deaths,1,'landing dead is the same death');
 // An already-dead body can be thrown again without another death notification.
 packet(f,authority,170,4,[{...flying,tick:170,position:[12,1,0]}]);
 draw(165);assert.equal(animator.action,'thrownRest');
 draw(170);assert.equal(animator.action,'thrown');assert.equal(deaths,1);
 packet(f,authority,180,5,[{...landed,tick:180}]);draw(180);
 assert.equal(animator.action,'thrownRest');assert.equal(deaths,1);
});

test('own class switches preserve ID classes, generations and a bounded detached proxy', async()=>{
 const {Units}=await import('../src/registry.js');
 const units=new Units(), own=body(0);units.add(own);
 const classes=new Map([[1,'trooper'],[2,'heavy']]), reg={...registry,classes};
 let builds=0, released=0;
 const factory={build(name){builds++;const unit=body(0);unit.className=name;units.add(unit);return unit;}};
 const heavy=factory.build('heavy');heavy.generation=7;
 const oldId=own.id, heavyId=heavy.id;
 let stopped=0;const character={unit:heavy};heavy.character=character;heavy.jetFlame={stop(){stopped++;}};
 const vehicle={seats:[{lease:{}}]};heavy.seat={vehicle,index:0};
 const game={playerUnit:own,units,factory,ordnance:{},bodies:new Map([['trooper',{}],['heavy',{}]]),
  adopt(value){own.className=value===this.bodies.get('heavy')?'heavy':'trooper';},
  client:{seats:{setLease(a,v,b,set){set(null);heavy.seat=null;released++;},setDriver(){}}}};
 const adapter=new EntityGameState(game,reg);
 adapter.presentation.bindSoldier(heavyId,7,heavy.presentation);
 adapter.timeline.addSnapshot(10,new Map([[heavyId,{...remoteRecord(10,0),id:heavyId,generation:7}]]));
 const correction=(id,name,generation)=>{
  const authority=body(id);authority.className=name;authority.generation=generation;
  return captureOwnPlayer({unit:authority,tick:10,epoch:1,registry:reg,classId:name==='heavy'?2:1});
 };
 adapter.prepareOwn(correction(heavyId,'heavy',8));
 assert.equal(own.id,heavyId);assert.equal(own.className,'heavy');
 assert.equal(units.byId(oldId).className,'trooper');assert.equal(units.byId(oldId).generation,1);
 assert.equal(released,1);assert.equal(vehicle.seats[0].lease,null);
 assert.equal(stopped,1);assert.equal(heavy.jetFlame,null);assert.equal(heavy.character,null);assert.equal(character.unit,null);
 assert.equal(heavy.actor.root.visible,false);assert.equal(units.list.includes(heavy),false);
 assert.equal(adapter.presentation.bindings.has(heavyId),false);assert.equal(adapter.timeline.sample(heavyId,10),null);
 adapter.soldier({...remoteRecord(11,5),id:oldId});
 assert.equal(units.byId(oldId).className,'trooper');
 for(let i=0;i<40;i++) {
  const toTrooper=i%2===0;
  adapter.prepareOwn(correction(toTrooper?oldId:heavyId,toTrooper?'trooper':'heavy',9+i));
  const remoteId=toTrooper?heavyId:oldId;
  assert.equal(units.byId(remoteId).className,toTrooper?'heavy':'trooper');
  assert.equal(new Set(units.list.map(u=>u.id)).size,units.list.length);
  assert.equal(units.list.length,2);assert.equal(units.byIdMap.size,2);
  assert.equal(adapter.ownProxy.actor.root.visible,false);
 }
 assert.equal(builds,2,'repeated swaps reuse the detached proxy instead of allocating actors');
 assert.throws(()=>adapter.soldier({...remoteRecord(100,0),id:oldId,classId:2}),/Unsupported entity soldier body/);
 adapter.prepareOwn(correction(500,'trooper',100));
 assert.equal(units.byId(500),own);assert.ok(units.nextId>500);
 for(let i=0;i<3;i++)assert.ok(factory.build('heavy').id>500);
 assert.equal(units.byId(500),own,'future factory allocations cannot overwrite a newly observed own ID');
});


test('new baseline releases the detached class proxy without retaining a scene root or factory body',()=>{
 const f=fixture(),proxy=body(99),root=new Object3D();root.add(proxy.actor.root);
 f.game.factory.built=[proxy];f.adapter.ownProxy=proxy;
 f.adapter.baseline();
 assert.equal(f.adapter.ownProxy,null);assert.equal(proxy.actor.root.parent,null);assert.equal(f.game.factory.built.length,0);
 assert.equal(f.game.units.byId(99),undefined);assert.equal(proxy.removed,true);
});

test('own correction waits for an unordered lease release with its original input ACK',()=>{
 const f=fixture(),authority=body(1);f.adapter.baseline();
 const packets=[];f.adapter.onCorrection=(context,p)=>{f.corrections.push(context);packets.push(p);};
 f.own.seat={vehicle:{},index:0};authority.soldier.position.x=42;
 packet(f,authority,100,1);
 assert.equal(f.corrections.length,0);assert.notEqual(f.own.soldier.position.x,42);
 f.own.seat=null;
 emptyPacket(f,101,999);
 assert.equal(f.own.soldier.position.x,42);assert.equal(f.corrections[0].tick,100);
 assert.equal(f.adapter.pendingCorrection,null);assert.equal(packets[0].ackInput,1);
});

test('latest deferred own correction replaces older state and expires without applying',()=>{
 const f=fixture(),authority=body(1);f.adapter.baseline();f.own.seat={vehicle:{},index:0};
 authority.soldier.position.x=20;packet(f,authority,100,1);
 authority.soldier.position.x=30;packet(f,authority,110,2);
 assert.equal(f.adapter.pendingCorrection.record.tick,110);
 emptyPacket(f,231,3);
 assert.equal(f.adapter.pendingCorrection,null);assert.equal(f.corrections.length,0);
 f.own.seat=null;emptyPacket(f,232,4);
 assert.notEqual(f.own.soldier.position.x,30);
});


test('own9 passenger and turret corrections wait for exact lease without borrowing a later ACK',async()=>{
 const {mountFixture}=await import('./mount-fixture.js');
 for(const mode of [2,3]){
  const f=fixture(),authority=body(1),packets=[];f.adapter.onCorrection=(c,p)=>packets.push(p);f.adapter.baseline();
  let source,target;
  if(mode===2){source=mountFixture().v;target=mountFixture().v;source.aiTarget={id:70,generation:2};target.aiTarget={id:70,generation:2};authority.seat={vehicle:source,index:1};f.game.vehicles.vehicles=[target];source.seats[1].aimYaw=.123456789;}
  else {source={aiTarget:{id:71,generation:3},alive:true,yaw:.123456789,pitch:.1,yawVel:.2,pitchVel:.3,gunYaw:.4,gunPitch:.5};target={...source,yaw:0};authority.seat={turret:source};f.game.turretSeats={turrets:[target]};}
  authority.soldier.position.x=42;
  const record=captureOwnPlayer({unit:authority,tick:100,epoch:1,registry,classId:1});
  f.sender.send(100,[record],{ackInput:17});
  assert.equal(packets.length,0);assert.notEqual(f.own.soldier.position.x,42);
  f.own.seat=mode===2?{vehicle:target,index:1}:{turret:target};
  emptyPacket(f,101,999);
  assert.equal(f.own.soldier.position.x,42);assert.equal(packets.length,1);assert.equal(packets[0].ackInput,17);assert.equal(f.adapter.pendingCorrection,null);
  assert.equal(mode===2?target.seats[1].aimYaw:target.yaw,.123456789);
 }
});


test('own team is authoritative when alive correction and spawn summaries arrive in either order',()=>{
 for(const summaryFirst of [false,true]){
  const f=fixture(),authority=body(1);authority.team=2;authority.generation=2;let teamChanges=0;f.game.applyAuthoritativeHumanTeam=function(team){teamChanges++;this.playerTeam=team;this.playerUnit.team=team;};
  f.game.humanSpawn={};registry.posts=new Map();
  const summary=captureSpawnSummary({unit:authority,team:2,spawn:{phase:'alive',className:'trooper',post:null,spawnedOnce:true,ended:false,deadTime:0,pending:null,auto:null}},{tick:101,epoch:1,registry,eligiblePosts:[],eligibleClasses:['trooper']});
  f.adapter.baseline();
  const summaryPacket=tick=>f.sender.send(tick,[summary]);
  if(summaryFirst){summaryPacket(101);assert.equal(f.game.playerTeam,1);assert.equal(f.game.humanSpawn.phase,undefined);}
  packet(f,authority,102,2);assert.equal(f.own.alive,true);assert.equal(f.own.team,2);assert.equal(f.game.playerTeam,2);
  if(!summaryFirst)summaryPacket(103);
  assert.equal(f.adapter.eligibility.team,2);assert.equal(f.game.humanSpawn.phase,'alive');
  packet(f,authority,104,5);assert.equal(teamChanges,1,'same-team checkpoints do not retarget UI repeatedly');
  const stale={...summary,tick:99};f.sender.send(105,[stale]);assert.equal(f.game.playerTeam,2);
  const oldLife=captureSpawnSummary({unit:{id:1,generation:1},team:1,spawn:{phase:'awaiting',className:'trooper',post:null,spawnedOnce:true,ended:false,deadTime:5,pending:null,auto:null}},{tick:104,epoch:1,registry,eligiblePosts:[],eligibleClasses:['trooper']});
  f.sender.send(106,[oldLife]);assert.equal(f.game.humanSpawn.phase,'alive');assert.equal(f.game.playerTeam,2);
 }
});


test('unavailable local human team fails before own body restore',()=>{
 const f=fixture(),authority=body(1);authority.team=2;authority.soldier.position.x=88;
 f.game.validateAuthoritativeHumanTeam=()=>{throw Error('unsupported mission team');};f.adapter.baseline();
 assert.throws(()=>packet(f,authority,1,1),/unsupported mission team/);
 assert.equal(f.own.soldier.position.x,0);assert.equal(f.own.team,1);
});

test("another player's charge arrives with his soldier record and shows on his weapon",()=>{
 const f=fixture(),authority=body(1);f.adapter.baseline();
 // A charging weapon of his (MaxPressedTime 2 s), as the bowcaster.
 f.remote.combat.arsenal=new Arsenal([{class:'rifle',channel:0,RoundsPerClip:35,ammo:6,base:'cannon',MaxPressedTime:2}]);
 const weapon=f.remote.combat.arsenal.weapons[0];
 packet(f,authority,100,1,[{...remoteRecord(100,20),charge:32}]);
 assert.equal(weapon.charging,true);assert.ok(Math.abs(weapon.held-2*32/63)<1e-9);
 packet(f,authority,101,2,[{...remoteRecord(101,20),charge:0}]);
 assert.equal(weapon.charging,false);assert.equal(weapon.held,0);
});
