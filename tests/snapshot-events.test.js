import test from 'node:test';import assert from 'node:assert/strict';
import {SnapshotEvents} from '../src/runtime/snapshot-events.js';
const batch=(id,tick=10)=>({epoch:1,id,tick,generations:[[7,1]],calls:[['anim:7','fire',[]],['hud','message',['durable']]]});
test('reliable events wait for applied state and run once despite duplicate delivery',()=>{
 const events=new SnapshotEvents(),played=[];events.receive(batch(1));events.receive(batch(1));
 events.drain({epoch:1,tick:9},()=>1,calls=>played.push(...calls));assert.equal(played.length,0);
 events.drain({epoch:1,tick:10},()=>1,calls=>played.push(...calls));assert.equal(played.length,2);
 events.drain({epoch:1,tick:11},()=>1,calls=>played.push(...calls));assert.equal(played.length,2);assert.equal(events.bytes,0);
});
test('state overtaking an event preserves announcements but not a dead life animation',()=>{
 const events=new SnapshotEvents(),played=[];events.receive(batch(1));events.drain({epoch:1,tick:30},()=>2,calls=>played.push(...calls));
 assert.deepEqual(played,[['hud','message',['durable']]]);
 events.receive({...batch(2),epoch:2});events.receive(batch(3));assert.equal(events.epoch,2);assert.equal(events.queue.length,1);
 events.reset();assert.equal(events.queue.length,0);
});

test('long rendering stalls discard obsolete cosmetics but preserve announcements and cleanup',()=>{
 const events=new SnapshotEvents(),played=[];
 for(let id=1;id<=400;id++)events.receive({...batch(id,id*3),calls:[['anim:7','fire',[]],...(id===1?[['hud','message',['keep']],['fxh','stop',[8]]]:[])]});
 assert.ok(events.queue.length<50);assert.ok(events.droppedCosmetics>300);assert.ok(events.bytes<100000);
 events.drain({epoch:1,tick:1200},()=>1,calls=>played.push(...calls));
 assert.ok(played.some(call=>call[0]==='hud'));assert.ok(played.some(call=>call[0]==='fxh'&&call[1]==='stop'));assert.equal(events.bytes,0);
});

test('stalled reliable animation and effect lifecycles retain their ordered final state',()=>{
 const events=new SnapshotEvents(),state={bank:'old',action:null,seat:null,effect:null,resets:0};
 const calls=[['anim:7','reset',[]],['anim:7','setBank',['rifle']],['anim:7','gunner',[2]],
  ['anim:7','weaponAction',['reload']],['anim:7','endWeaponAction',[]],['anim:7','weaponAction',['charge']],
  ['fx','play',[8,'trail']],['fxh','move',[8,[1,2,3]]],['fxh','stop',[8]],['anim:7','fire',[]]];
 events.receive({...batch(1,1),calls});events.receive({...batch(2,150),calls:[]});
 events.drain({epoch:1,tick:150},()=>1,played=>{
  for(const[target,method,args]of played){
   assert.notEqual(method,'fire','obsolete cosmetic must expire');
   if(target==='anim:7'){
    if(method==='reset'){state.resets++;state.action=null;}
    if(method==='setBank')state.bank=args[0];
    if(method==='gunner')state.seat=args[0];
    if(method==='weaponAction')state.action=args[0];
    if(method==='endWeaponAction')state.action=null;
   }else if(target==='fx')state.effect=args[1];
   else if(method==='stop')state.effect=null;
  }
 });
 assert.deepEqual(state,{bank:'rifle',action:'charge',seat:2,effect:null,resets:1});assert.equal(events.bytes,0);
});
