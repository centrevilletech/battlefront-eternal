import test from 'node:test';
import assert from 'node:assert/strict';
import {EntityScheduler} from '../src/runtime/network-entity-stream.js';
import {decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const soldier=(id,tick)=>({kind:'soldier',id,generation:1,tick,classId:1,team:1,position:[id,0,tick/60],velocity:[0,0,1],yaw:0,pitch:0,health:100,posture:0,state:0,weapon:1,fireCounter:0});
const summary=tick=>({kind:'summary',id:90,generation:1,tick,schema:1,payload:Uint8Array.from({length:25},()=>tick%256)});
const take=(s,tick)=>{const p=s.packet({tick,maxBytes:80});assert.ok(p);s.admitted(p.packetId);s.ack({epoch:1,packetId:p.packetId});return decodeEntityPacket(p.buffer).records[0];};

test('a due nearby pose precedes slightly older distant and summary deadlines',()=>{
 const s=new EntityScheduler();s.update(soldier(1,1),1,false,30);s.update(summary(1),4,false,30);s.update(soldier(2,31),10,false,3);
 assert.equal(take(s,34).id,2,'close deadline34 gets a bounded advance ahead of deadline31');
 assert.notEqual(take(s,35).id,2,'the just-serviced pose cannot spend the advance before due');
});

test('near pose urgency remains bounded under a saturated one-record budget',()=>{
 const s=new EntityScheduler();const latest=new Map(),gaps=new Map();
 for(let tick=1;tick<=600;tick++){
  for(let id=1;id<=12;id++)s.update(soldier(id,tick),id<=4?10:1,false,id<=4?3:30);
  s.update(summary(tick),4,false,30);
  const r=take(s,tick),key=`${r.kind}:${r.id}`;
  if(latest.has(key))gaps.set(key,Math.max(gaps.get(key)||0,tick-latest.get(key)));
  latest.set(key,tick);
 }
 assert.equal(latest.size,13,'near pressure cannot starve far actors or world state');
 for(const[key,last]of latest){assert.ok(600-last<60,key);assert.ok(gaps.get(key)<60,key);}
 for(let id=1;id<=4;id++)assert.ok(gaps.get(`soldier:${id}`)<=18,'near tier retains preferential cadence under overload');
});

test('changed presentation summaries respect cadence without delaying their first version',()=>{
 const s=new EntityScheduler();s.update(summary(1),4,false,30);
 const first=s.packet({tick:1});assert.ok(first);s.admitted(first.packetId);
 s.update(summary(4),4,false,30);
 assert.equal(s.packet({tick:4}),null,'changed animation fields cannot eagerly consume the next pose budget');
 s.update(soldier(1,4),10,false,3);
 assert.equal(take(s,4).kind,'soldier');
 const refresh=s.packet({tick:31});assert.ok(refresh);
 assert.ok(decodeEntityPacket(refresh.buffer).records.some(r=>r.kind==='summary'&&r.tick===4),'unacknowledged changed state remains repairable');
 const local=new EntityScheduler();local.update(summary(1));const initial=local.packet({tick:1});local.admitted(initial.packetId);local.update(summary(2));
 assert.equal(decodeEntityPacket(local.packet({tick:2}).buffer).records[0].tick,2,'unthrottled local publication remains unchanged');
});
