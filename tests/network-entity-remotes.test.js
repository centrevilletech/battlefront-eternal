import test from 'node:test';
import assert from 'node:assert/strict';
import {encodeEntityRecord,encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const live={kind:'remote',id:4095,generation:12,tick:100,active:true,definitionId:3,team:1,position:[10,20,30],velocity:[1,2,3],yaw:.25,pitch:-.15,health:40};
test('fixed recon kind10 carries a bounded self-contained remote pose or terminal',()=>{
 const bytes=encodeEntityRecord(live);assert.equal(bytes[0],10);assert.equal(bytes.length,41);
 const [decoded]=decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:1,tick:100,records:[live]})).records;
 for(const key of ['kind','id','generation','tick','active','definitionId','team','health'])assert.equal(decoded[key],live[key]);
 assert.deepEqual(decoded.position,live.position);assert.deepEqual(decoded.velocity,live.velocity);assert.ok(Math.abs(decoded.pitch-live.pitch)<.0001);
 assert.equal(encodeEntityRecord({...live,active:false}).length,14);
 assert.throws(()=>encodeEntityRecord({...live,id:4096}),/range/);
 assert.throws(()=>encodeEntityRecord({...live,active:1}),/remote state/);
 assert.throws(()=>encodeEntityRecord({...live,yaw:NaN}),/nonfinite/);
});

test('recon slots have independent capacity and terminal exact ACK ordering',async()=>{
 const {EntityScheduler,EntityReceiver}=await import('../src/runtime/network-entity-stream.js');
 const scheduler=new EntityScheduler({maxEntities:1}),receiver=new EntityReceiver({maxEntities:1});
 let serial=0;
 for(let id=0;id<4096;id++) {
  const record={kind:'remote',id,generation:1,tick:1,active:false};scheduler.update(record);
  receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[record]}));
 }
 const actor={kind:'tombstone',id:1,generation:1,tick:1};scheduler.update(actor);receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:1,records:[actor]}));
 assert.equal(receiver.remoteCount,4096);assert.equal(scheduler.remoteSlots.size,4096);assert.equal(receiver.records.size,4097);
 const next={...live,id:1,generation:2,tick:2};scheduler.update(next);
 receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:2,records:[next]}));
 const end={...next,active:false};receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:2,records:[end]}));
 receiver.apply(encodeEntityPacket({epoch:1,packetId:++serial,tick:3,records:[{...next,tick:3}]}));
 assert.equal(receiver.get('remote',1).active,false,'late same-life pose cannot revive a terminal');assert.equal(receiver.remoteCount,4096);
 const small=new EntityScheduler({maxEntities:1});small.update(next);const sent=small.packet({tick:2});small.admitted(sent.packetId);small.update(end);small.ack({epoch:1,packetId:sent.packetId});
 const terminal=small.packet({tick:3});assert.equal(decodeEntityPacket(terminal.buffer).records[0].active,false);small.admitted(terminal.packetId);small.ack({epoch:1,packetId:terminal.packetId});assert.equal(small.records.size,0);assert.equal(small.remoteSlots.size,1);
});

test('shared recon presentation respects own-before-pose, reuse and terminal cleanup without simulation',async()=>{
 const {Scene,Object3D,Vector3}=await import('three');const {RemoteDroids}=await import('../src/remote.js');const {EntityRemotePresentation}=await import('../src/runtime/network-entity-remotes.js');
 let effects=0;const scene=new Scene(),def={base:'droid',GeometryName:'probe',weapons:[]};
 const remotes=new RemoteDroids({scene,camera:null,collision:{cast:()=>null},model:()=>()=>new Object3D(),lights:{flash(){effects++;}},fire(){effects++;},explode(){effects++;}});
 const view=new EntityRemotePresentation(remotes,{definitions:new Map([[3,def]])});
 const own=remotes.createDroid(def,null,new Vector3(90,2,3),new Vector3(),0,1);own.networkSlot=live.id;own.networkGeneration=live.generation;remotes.droids.push(own);remotes.enter(own);
 view.receive(live,1);view.present(100);assert.equal(remotes.piloted,own);assert.equal(own.position.x,90,'coarse pose never overwrites own exact correction');assert.equal(remotes.droids.length,1);
 remotes.exit();view.present(100);assert.equal(own.position.x,10);assert.equal(own.mesh.parent,scene);
 view.receive({...live,generation:13,tick:101},1);assert.equal(own.mesh.parent,null);assert.equal(remotes.droids.length,1);
 const next=remotes.droids[0];remotes.enter(next);remotes.strikes.push(next);
 assert.equal(view.receive({...live,active:false,tick:102},1),false);assert.equal(remotes.piloted,next);
 view.receive({...live,generation:13,active:false,tick:103},1);assert.equal(remotes.piloted,null);assert.equal(remotes.strikes.length,0);assert.equal(remotes.droids.length,0);assert.equal(next.mesh.parent,null);assert.equal(effects,0);
});

test('recon capture reads shared owner controller slots and retains explicit death state',async()=>{
 const {Vector3}=await import('three');const {EntityRemoteCapture}=await import('../src/runtime/network-entity-remotes.js');
 const def={base:'droid'},droid={def,team:1,position:new Vector3(1,2,3),velocity:new Vector3(),yaw:0,pitch:0,health:40,dead:false};
 const world={slots:[{droid,generation:1}],identityFor(){return {id:0,generation:1};},*droids(){yield {droid};}};
 const capture=new EntityRemoteCapture({networkRemotes:world},{definitionId:value=>{assert.equal(value,def);return 3;}});
 assert.equal(capture.capture(1)[0].active,true);droid.dead=true;
 assert.deepEqual(capture.capture(2),[{kind:'remote',id:0,generation:1,tick:2,active:false}]);
 assert.equal(capture.capture(3)[0].tick,2,'terminal identity stays exact until ACK');
});

test('another player\'s recon droid is drawn between the records bracketing the clock, then coasts at most 6 ticks',async()=>{
 const {Scene,Object3D}=await import('three');const {RemoteDroids}=await import('../src/remote.js');const {EntityRemotePresentation}=await import('../src/runtime/network-entity-remotes.js');
 const def={base:'droid',GeometryName:'probe',weapons:[]},remotes=new RemoteDroids({scene:new Scene(),camera:null,collision:{cast:()=>null},model:()=>()=>new Object3D(),lights:{flash(){}},fire(){},explode(){}});
 const view=new EntityRemotePresentation(remotes,{definitions:new Map([[3,def]])}),at=(tick,x,yaw)=>({...live,tick,position:[x,20,30],velocity:[60,0,0],yaw});
 for(const [tick,x,yaw]of [[100,10,3],[102,12,-3],[104,14,-2.9]])view.receive(at(tick,x,yaw),1);
 const droid=remotes.droids[0],x=tick=>{view.present(tick);return droid.position.x;};
 assert.equal(x(99),10,'held at its first record until the clock reaches it');
 assert.ok(Math.abs(x(101)-11)<1e-9&&Math.abs(x(103.5)-13.5)<1e-9,'between the bracketing records, not the newest pair');
 view.present(101);assert.ok(Math.abs(droid.yaw-(3+Math.atan2(Math.sin(-6),Math.cos(-6))/2))<1e-9,'yaw takes the short way round');
 assert.ok(Math.abs(x(106)-16)<1e-9&&Math.abs(x(130)-20)<1e-9,'past the newest it coasts at its velocity for 6 ticks, then holds');
});
