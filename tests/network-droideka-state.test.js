import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Clip } from '../src/anim.js';
import { CollisionWorld } from '../src/collision.js';
import { Droideka } from '../src/droideka.js';
import { captureDroidekaState as capture, decodeDroidekaState as decode, restoreDroidekaState as restore } from '../src/runtime/network-droideka-state.js';

function fixture() {
  const world = new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]), new Uint32Array([0,2,1,0,3,2]), new Uint32Array([0,1]));
  const root = new THREE.Group(), bone = new THREE.Bone(); bone.name = 'dummyroot'; root.add(bone);
  const clips = new Map();
  for (const name of ['idle','turnl','turnr','death01','idle_to_leftfoot','walk_leftfoot_rightfoot','walk_rightfoot_leftfoot','leftfoot_to_idle','rightfoot_to_idle','fold','roll','unfold']) {
    const frames = 13, data = new Float32Array(frames * 7);
    for (let i=0;i<frames;i++) { data[i*7+3]=1; data[i*7+6]=name.includes('foot') ? i/30 : 0; }
    clips.set(name, new Clip(name,{frames,tracks:[0],offset:0,flags:1,travel:[0,0,.4]},data,30));
  }
  const d = new Droideka(world,{AnimationSpeed:2},clips,{root,bones:[bone]});
  d.arm([{aimers:['dummyroot']},{base:'shield',MaxShield:100,AddShield:-5,AddShieldOff:2}]);
  d.spawn(0,0,0,0); return d;
}
function step(d,i) {
  d.look(Math.sin(i)*5,Math.cos(i)*2);
  if (i===8) d.toggleShield();
  if (i%9===0) d.nextGun(new THREE.Vector3());
  d.tick({forward:i<55?1:i<90?-1:0,right:i%17===0?.5:0,use:i===35||i===85},1/60);
}
test('shared Droideka continuation remains exact through walk, reverse, fold, roll, unfold and shield',()=>{
  for (const checkpoint of [0,12,40,65,92]) {
    const authority=fixture(), predicted=fixture();
    for(let i=0;i<checkpoint;i++) step(authority,i);
    restore(predicted,capture(authority));
    for(let i=checkpoint;i<125;i++) {
      step(authority,i);step(predicted,i);
      assert.deepEqual(capture(predicted),capture(authority),`checkpoint ${checkpoint}, tick ${i}`);
    }
  }
});
test('restore is atomic for malformed payloads and mismatched content',()=>{
  const d=fixture();d.nextGun(new THREE.Vector3());const bytes=capture(d);
  for(let n=0;n<bytes.length;n++) { assert.throws(()=>restore(d,bytes.subarray(0,n)));assert.deepEqual(capture(d),bytes); }
  const corrupt=bytes.slice();new DataView(corrupt.buffer).setFloat64(corrupt.length-8,NaN,true);
  assert.throws(()=>restore(d,corrupt));assert.deepEqual(capture(d),bytes);
  const other=fixture();other.shield.max=1;other.shield.value=1;
  const before=capture(other);assert.throws(()=>restore(other,bytes));assert.deepEqual(capture(other),before);
});
test('restoring death state runs no callbacks and clears presentation endpoints and absent barrel',()=>{
  const source=fixture(),target=fixture();source.dead=true;source.tick({},1/60);
  let blasts=0,explosions=0;target.onDeathBlast=()=>blasts++;target.onExploded=()=>explosions++;
  target.nextGun(new THREE.Vector3());target.previous.set(99,99,99);target.settled.set(-99,-99,-99);
  restore(target,capture(source));assert.equal(blasts,0);assert.equal(explosions,0);
  assert.equal(target.lastGun,undefined);assert.deepEqual(target.previous,target.position);assert.deepEqual(target.settled,target.position);
  for(let i=0;i<60;i++)target.tick({},1/60);
  assert.equal(explosions,1);
});
test('decoder accepts subarray views and preserves selected muzzle',()=>{
  const d=fixture();d.nextGun(new THREE.Vector3());const bytes=capture(d),framed=new Uint8Array(bytes.length+9);framed.set(bytes,5);
  assert.deepEqual(decode(framed.subarray(5,5+bytes.length)),decode(bytes));
  assert.deepEqual(decode(bytes).lastGun,{index:0,barrel:0});
});

import { SoldierPresentation } from '../src/soldier-presentation.js';
import { Unit } from '../src/units.js';
import { EntityWorldCapture } from '../src/runtime/network-entity-capture.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';
import { encodeEntityPacket, decodeEntityPacket, encodeEntityRecord } from '../src/runtime/network-entity-codec.js';
import { createSnapshotSender } from './snapshot-send.js';

function remoteFixture() {
  const d=fixture(), combat={health:100,dead:false,arsenal:{current:()=>null,weapons:[]},updateHeld(){}};
  const unit=new Unit({className:'droideka',def:{},soldier:d,combat,presentation:new SoldierPresentation(d),actor:d.actor,animator:d.animator,walker:true});
  Object.assign(unit,{id:2,generation:1,team:2,removed:false,alive:true});
  const registry={classes:new Map([[1,'droideka']]),weapons:new Map()}, map=new Map([[2,unit]]);
  const game={playerUnit:{id:1},units:{all:()=>map.values(),byId:id=>map.get(id),byIdMap:map},vehicles:{vehicles:[]},ordnance:{clearProjectilePresentation(){}},client:{seats:{}},factory:{build(){throw Error('unexpected build');}}};
  return {d,unit,game,registry};
}
test('Droideka capture, real shared presentation, lifecycle and stale packets use mandatory entity transport',()=>{
  const authority=remoteFixture(),client=remoteFixture(),captureWorld=new EntityWorldCapture(authority.game,authority.registry);
  const adapter=new EntityGameState(client.game,client.registry);adapter.baseline();
  const sender=createSnapshotSender(adapter);
  const send=(tick,id)=>{const records=captureWorld.capture(tick);assert.equal(captureWorld.unsupported.length,0);return sender.send(tick,records);};
  authority.d.mode=12;authority.d.roll=7;authority.d.velocity.x=60;authority.d.position.x=10;send(10,1);
  authority.d.roll=9;authority.d.position.x=20;const latest=send(20,2);adapter.present(15);
  const presented=client.unit.presentation.update(0,1);
  assert.ok(Math.abs(presented.position.x-15)<.05);assert.equal(presented.roll,8);assert.equal(presented.mode,12);
  assert.equal(client.d.roll,9,'interpolation leaves authoritative simulation state untouched');
  client.unit.animator.update(1/60,presented);
  const record=captureWorld.capture(20)[0];assert.equal(record.kind,'droideka');assert.equal(encodeEntityRecord(record).length,70);
  authority.unit.removed=true;send(30,3);assert.equal(client.unit.removed,true);
  sender.deliver(latest);assert.equal(client.unit.removed,true,'stale pose cannot revive tombstoned generation');
  authority.unit.removed=false;authority.unit.generation=2;authority.d.mode=0;authority.d.roll=0;send(40,4);
  assert.equal(client.unit.removed,false);assert.equal(client.unit.generation,2);adapter.present(35);
  assert.equal(client.unit.presentation.update(0,1).roll,0,'new life cannot interpolate old shell rotation');
});
test('Droideka wire rejects malformed pose before applying a packet',()=>{
  const f=remoteFixture(),record=new EntityWorldCapture(f.game,f.registry).capture(1)[0];
  for(const changes of [{mode:14},{roll:NaN},{time:Infinity},{shield:-1},{reverse:1}])assert.throws(()=>encodeEntityRecord({...record,...changes}));
  const bytes=encodeEntityPacket({epoch:1,packetId:1,tick:1,records:[record]});
  for(let n=0;n<bytes.length;n++)assert.throws(()=>decodeEntityPacket(bytes.subarray(0,n)));
});

import { captureOwnPlayer, applyOwnPlayer, decodeOwnPlayer } from '../src/runtime/network-entity-player.js';
import { Arsenal } from '../src/weapons.js';
import { PlayerCombat } from '../src/combat.js';
test('exact own Droideka correction restores shared solver and fits one datagram',()=>{
  const a=remoteFixture(),b=remoteFixture();
  const registry={...a.registry,contentEpoch:1,inputCodes:[],bindingProfiles:new Map([[0,{}]])};
  for(const f of [a,b]) {
    const combat=Object.create(PlayerCombat.prototype);
    Object.assign(combat,{soldier:f.d,arsenal:new Arsenal([]),health:100,dead:false,deathTime:0,zoomStep:0,firstPerson:true,kick:{rate:0,hold:0,offset:0}});
    f.unit.combat=combat;
  }
  for(let i=0;i<55;i++)step(a.d,i);
  const record=captureOwnPlayer({unit:a.unit,tick:55,epoch:1,registry,classId:1});
  assert.equal(decodeOwnPlayer(record).bodySchema,10);
  assert.ok(encodeEntityPacket({epoch:1,packetId:1,tick:55,records:[record]}).length<=1100);
  const context=applyOwnPlayer({unit:b.unit,epoch:1,registry,record});assert.equal(context.reset,true);
  assert.deepEqual(capture(b.d),capture(a.d));
  for(let i=55;i<100;i++){step(a.d,i);step(b.d,i);assert.deepEqual(capture(b.d),capture(a.d));}
});
test('unseen remote Droideka is built once and death notification fires once per life',()=>{
  const authority=remoteFixture(),client=remoteFixture(),events=[];let built=0;
  client.game.units.byIdMap.clear();client.game.presentEvent=(event,unit)=>events.push([event,unit.id]);
  client.game.factory.build=className=>{assert.equal(className,'droideka');built++;client.unit.id=99;client.game.units.byIdMap.set(99,client.unit);return client.unit;};
  const adapter=new EntityGameState(client.game,client.registry);adapter.baseline();
  const captureWorld=new EntityWorldCapture(authority.game,authority.registry);
  const sender=createSnapshotSender(adapter);
  const send=tick=>sender.send(tick,captureWorld.capture(tick));
  send(1);assert.equal(built,1);assert.ok(client.game.units.byId(2)===client.unit);
  authority.unit.dead=authority.unit.combat.dead=authority.d.dead=true;authority.d.mode=3;
  send(2);send(3);assert.deepEqual(events,[['died',2]]);assert.equal(built,1);
  authority.unit.generation=2;authority.unit.dead=authority.unit.combat.dead=authority.d.dead=false;authority.d.mode=0;send(4);
  authority.unit.dead=authority.unit.combat.dead=authority.d.dead=true;authority.d.mode=3;send(5);
  assert.deepEqual(events,[['died',2],['died',2]]);
});

test('production prediction preserves exact rolling checkpoints and private recoil through folding and shield edges', async()=>{
  const {DroidekaPrediction}=await import('../src/runtime/droideka-prediction.js');
  const {PlayerCombat}=await import('../src/combat.js');
  const {infantryFireBlocked}=await import('../src/combat-input.js');
  const authority=fixture(),predicted=fixture();
  function combat(soldier){return Object.assign(Object.create(PlayerCombat.prototype),{soldier,kick:{rate:.2,hold:.3,offset:.04},gunUnderwater:false,arsenal:{current:()=>({def:{base:'shield'}})}});}
  const live=combat(authority),client=combat(predicted),originalKick={...client.kick};
  let continuation, kick;
  for(let i=0;i<130;i++){
    const saved=capture(predicted);
    if(continuation)restore(predicted,continuation);
    const replay=new DroidekaPrediction(predicted,client,kick);
    const input={forward:i<60?1:-.3,right:0,use:i===35||i===85};
    const press=i===8||i===45||i===100;
    authority.look(3,-1);predicted.look(3,-1);
    authority.tick(input,1/60);
    if(press&&!infantryFireBlocked(authority,false,Math.abs(input.forward)))authority.toggleShield();
    live.updateKick(1/60);replay.step(input,1/60,press);
    assert.deepEqual(capture(predicted),capture(authority),`tick ${i}`);
    continuation=capture(predicted);kick=replay.kick;
    restore(predicted,saved);assert.deepEqual(capture(predicted),saved,'authority rollback includes nested shield state');
  }
  assert.deepEqual(client.kick,originalKick);
});

test('production Droideka replay suppresses death callbacks and honors shield gating',async()=>{
  const {DroidekaPrediction}=await import('../src/runtime/droideka-prediction.js');
  const d=fixture();let callbacks=0;const callback=()=>callbacks++;
  d.onExploded=d.onDeathBlast=callback;
  const combat={kick:{},gunUnderwater:true,arsenal:{current:()=>({def:{base:'shield'}})},updateKick(){}};
  const replay=new DroidekaPrediction(d,combat);
  replay.step({},1/60,true);assert.equal(d.shield.on,false);
  d.dead=true;for(let n=0;n<90;n++)replay.step({},1/60);
  assert.equal(callbacks,0);assert.equal(d.onExploded,callback);assert.equal(d.onDeathBlast,callback);
});

test('a droideka terminal record latches its dissolve through repeated records and resets on a new life', () => {
  const source = remoteFixture(), client = remoteFixture();
  const adapter = new EntityGameState(client.game, client.registry); adapter.baseline();
  const sender = createSnapshotSender(adapter);
  const capture = new EntityWorldCapture(source.game, source.registry);
  client.unit.undraw = function () { this.actor.root.visible = false; };
  source.d.dead = true; source.d.mode = 4; source.unit.alive = false;
  sender.send(10, capture.capture(10));
  assert.equal(client.unit.gone.generation, 1);
  assert.ok(client.unit.presentGone(.25) > 0);
  sender.send(20, capture.capture(20));
  assert.equal(client.unit.gone.t, .25, 'repeated terminal cannot restart the fade');
  assert.equal(client.unit.presentGone(.26), -1);
  sender.send(30, capture.capture(30));
  assert.equal(client.unit.presentGone(0), -1);
  assert.equal(client.unit.actor.root.visible, false);
  source.unit.generation = 2; source.unit.alive = true; source.d.dead = false; source.d.mode = 0;
  sender.send(40, capture.capture(40));
  assert.equal(client.unit.presentGone(0), 0);
  assert.equal(client.unit.gone, null);
});
