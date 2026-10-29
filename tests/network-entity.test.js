import test from 'node:test';
import assert from 'node:assert/strict';
import {encodeEntityPacket,decodeEntityPacket,encodeEntityRecord} from '../src/runtime/network-entity-codec.js';
import {EntityScheduler,EntityReceiver} from '../src/runtime/network-entity-stream.js';
import {captureSoldierCorrection,restoreSoldierCorrection,soldierCorrectionBytes} from '../src/runtime/network-entity-correction.js';
import {Soldier} from '../src/soldier.js';
const soldier=(id,tick=1,generation=1)=>({kind:'soldier',id,generation,tick,classId:2,team:1,position:[1.234,-3.456,9.876],velocity:[1.23,2.34,-3.45],yaw:.345,pitch:-.234,health:100,posture:0,state:0,weapon:1,fireCounter:3});
const correction=(tick=1,payload=new Uint8Array(8))=>({kind:'correction',id:1,generation:1,tick,schema:1,payload});
const packet=(packetId,records,epoch=1)=>encodeEntityPacket({epoch,packetId,tick:Math.max(...records.map(r=>r.tick)),records});

test('absolute soldier and vehicle records have fixed cost and validated quantization bounds',()=>{
 const s=soldier(1),v={kind:'vehicle',id:2,generation:1,tick:1,classId:3,team:2,position:s.position,velocity:s.velocity,quaternion:[0,0,0,1],health:1000,state:2,power:500};
 const d=decodeEntityPacket(packet(1,[s,v]));
 assert.equal(encodeEntityRecord(s).length,48);assert.equal(encodeEntityRecord(v).length,47);
 for(let i=0;i<3;i++){assert.ok(Math.abs(d.records[0].position[i]-s.position[i])<=1/512);assert.ok(Math.abs(d.records[0].velocity[i]-s.velocity[i])<=1/64);}
 assert.ok(Math.abs(d.records[0].yaw-s.yaw)<=Math.PI/65534);
 for(const bad of [{position:[NaN,0,0]},{position:[1e8,0,0]},{velocity:[1024,0,0]},{health:65536}])assert.throws(()=>encodeEntityRecord({...s,...bad}));
 assert.throws(()=>encodeEntityRecord({...v,quaternion:[0,0,0,2]}));
 const wrapped=decodeEntityPacket(packet(2,[{...s,yaw:10000*Math.PI*2+.345}])).records[0];assert.ok(Math.abs(wrapped.yaw-.345)<Math.PI/65534);
 const badQuat=packet(1,[v]);badQuat.fill(0,32+34,32+42);assert.throws(()=>decodeEntityPacket(badQuat),/quaternion/);
 const bytes=packet(1,[s]);assert.throws(()=>decodeEntityPacket(bytes.subarray(0,bytes.length-1)));
});

test('own soldier correction preserves every explicit replay value exactly and validates before mutation',()=>{
 const source=new Soldier({},{}),target=new Soldier({},{});source.position.set(1.23456789,2,3);source.settled.set(-4,5,6);source.stateTime=.123456789;source.air='fall';source.contactCount=2;
 const expected=captureSoldierCorrection(source);
 const bytes=captureSoldierCorrection(source);assert.equal(bytes.length,soldierCorrectionBytes);
 restoreSoldierCorrection(target,bytes);const actual=captureSoldierCorrection(target);assert.deepEqual(actual,expected);
 const encoded=packet(1,[correction(1,bytes)]);assert.ok(encoded.length<=1100);console.log(`Exact Soldier correction: ${bytes.length} payload bytes, ${encoded.length} complete message bytes`);
 const invalid=bytes.slice();new DataView(invalid.buffer).setFloat64(0,NaN,true);assert.throws(()=>restoreSoldierCorrection(target,invalid));
 const unchanged=captureSoldierCorrection(target);assert.deepEqual(unchanged,actual);
 const decoded=decodeEntityPacket(encoded);decoded.records[0].payload.fill(0);assert.notDeepEqual(bytes,decoded.records[0].payload,'correction payload has independent ownership');
});

test('loss and reordered partial packets update only included entities, with explicit generation tombstones',()=>{
 const r=new EntityReceiver();r.apply(packet(1,[soldier(1),soldier(2)]));
 // Packet2 is lost. Packet3 remains completely usable without it.
 r.apply(packet(3,[soldier(1,3)]));r.apply(packet(2,[soldier(2,2)]));
 assert.equal(r.get('soldier',1).tick,3);assert.equal(r.get('soldier',2).tick,2);
 assert.equal(r.apply(packet(3,[soldier(1,3)])),null);
 r.apply(packet(4,[{kind:'tombstone',id:1,generation:1,tick:4}]));
 r.apply(packet(5,[soldier(1,30)]));assert.equal(r.get('soldier',1).kind,'tombstone');
 r.apply(packet(6,[soldier(1,5,2)]));assert.equal(r.get('soldier',1).generation,2);
 r.apply(packet(7,[{kind:'tombstone',id:1,generation:1,tick:99}]));assert.equal(r.get('soldier',1).generation,2);
 r.apply(packet(1,[soldier(8,1)],2));assert.equal(r.get('soldier',1),undefined);assert.equal(r.apply(packet(8,[soldier(1,100)],1)),null);
});

test('priority publication never waits for ACK and membership ACK cannot remove an unsent tombstone',()=>{
 const scheduler=new EntityScheduler({maxBytes:107,maxPackets:2});scheduler.update(soldier(2),100);scheduler.update({kind:'tombstone',id:3,generation:1,tick:1},1);
 const first=scheduler.packet({tick:1,own:correction()});assert.deepEqual(decodeEntityPacket(first.buffer).records.map(r=>r.id),[1,2]);
 scheduler.admitted(first.packetId);scheduler.ack({epoch:1,packetId:first.packetId});assert.equal(scheduler.records.get('entity:3').record.kind,'tombstone');
 const second=scheduler.packet({tick:2,own:correction(2)});assert.ok(decodeEntityPacket(second.buffer).records.some(r=>r.id===3));
 scheduler.admitted(second.packetId);scheduler.update({kind:'tombstone',id:3,generation:2,tick:3});scheduler.ack({epoch:1,packetId:second.packetId});assert.ok(scheduler.records.has('entity:3'),'old ACK cannot clear newer death generation');
 for(let tick=3;tick<20;tick++)scheduler.packet({tick,own:correction(tick)});
 assert.equal(scheduler.offered.size,2);assert.equal(scheduler.ack({epoch:1,packetId:first.packetId}),false);
 assert.throws(()=>scheduler.packet({tick:21,own:correction(21,new Uint8Array(100))}),/own correction cannot fit/);
});

test('seat ownership changes atomically and stale competing claims cannot create two pilots',()=>{
 const r=new EntityReceiver(),seat=(id,tick,vehicleId=10,index=0)=>({kind:'seat',id,generation:1,tick,vehicleId,vehicleGeneration:1,seatIndex:index,leaseVersion:tick});
 r.apply(packet(1,[seat(1,1)]));assert.equal(r.seatOwner(10,1,0).id,1);
 r.apply(packet(3,[seat(1,3,11)]));assert.equal(r.seatOwner(10,1,0),null);assert.equal(r.seatOwner(11,1,0).id,1);
 r.apply(packet(2,[seat(1,2)]));assert.equal(r.seatOwner(10,1,0),null);
 const swap=r.apply(packet(4,[seat(2,4,11)]));assert.equal(r.seatOwner(11,1,0).id,2);assert.equal(r.get('seat',1),undefined);assert.deepEqual(swap.seatChanges.find(c=>c.id===1),{id:1,generation:1,seat:null});
 r.apply(packet(5,[seat(1,3,11)]));assert.equal(r.seatOwner(11,1,0).id,2);
 r.apply(packet(6,[soldier(2,5,2)]));assert.equal(r.seatOwner(11,1,0),null,'new life clears old lease even if death packet was lost');
 r.apply(packet(7,[{kind:'tombstone',id:11,generation:1,tick:6}]));assert.equal(r.seatOwner(11,1,0),null);
 r.apply(packet(8,[seat(2,7,11)]));assert.equal(r.seatOwner(11,1,0),null,'destroyed hull cannot acquire a stale-generation pilot');
});

import {CollisionWorld} from '../src/collision.js';
test('exact correction resumes actual Soldier collision, jump and stance physics identically',()=>{
 const world=new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
 const a=new Soldier(world,{}),b=new Soldier(world,{});a.spawn(0,0,0,0);b.spawn(0,0,0,0);
 for(let tick=1;tick<=180;tick++){
  const input={forward:1,right:tick>90?.25:0,turn:.1,jump:tick===50,crouch:tick===130,prone:false};
  a.tick(input,1/60);
  if(tick===65){const record=decodeEntityPacket(packet(1,[correction(tick,captureSoldierCorrection(a))])).records[0];restoreSoldierCorrection(b,record.payload,record.schema);}
  else if(tick>65)b.tick(input,1/60);
  if(tick>=65){const aa=captureSoldierCorrection(a),bb=captureSoldierCorrection(b);assert.deepEqual(bb,aa,`physics tick${tick}`);}
 }
});

test('bounded receiver rejects capacity overflow atomically and scheduler retires only acknowledged tombstones',()=>{
 const r=new EntityReceiver({maxEntities:1});r.apply(packet(1,[soldier(1)]));
 assert.throws(()=>r.apply(packet(2,[soldier(1,2),soldier(2,2)])),/capacity/);assert.equal(r.get('soldier',1).tick,1);
 const s=new EntityScheduler();s.update({kind:'tombstone',id:3,generation:1,tick:1});
 const p=s.packet({tick:1,own:correction()});assert.equal(s.ack({epoch:2,packetId:p.packetId}),false);assert.ok(s.records.has('entity:3'));
 assert.equal(s.ack({epoch:1,packetId:p.packetId}),false);assert.equal(s.admitted(p.packetId),true);assert.equal(s.ack({epoch:1,packetId:p.packetId}),true);assert.equal(s.records.has('entity:3'),false);
 assert.equal(s.update(soldier(3,1)),false,'retired tombstone high-water mark prevents resurrection');
});

test('remote-only packets cover roster without repeating a large own envelope',()=>{
 const s=new EntityScheduler(),receiver=new EntityReceiver();
 for(let id=2;id<=65;id++)s.update(soldier(id));
 const own=s.packet({tick:1,own:correction(1,new Uint8Array(930))});
 s.admitted(own.packetId);receiver.apply(own.buffer);
 assert.ok(decodeEntityPacket(own.buffer).records.length<=4);
 for(let tick=2;tick<=4;tick++){
  const p=s.packet({tick});assert.ok(p.buffer.length<=1100);
  const decoded=decodeEntityPacket(p.buffer);assert.equal(decoded.records.some(r=>r.kind==='correction'),false);
  s.admitted(p.packetId);receiver.apply(p.buffer);
 }
 for(let id=2;id<=65;id++)assert.equal(receiver.get('soldier',id)?.tick,1,`actor ${id} covered`);
 assert.equal(receiver.get('correction',1).tick,1,'remote receipt cannot fabricate a newer own correction');
});

test('same-publication offers exclude duplicate records without authorizing admission',()=>{
 const scheduler=new EntityScheduler();for(let id=2;id<=65;id++)scheduler.update(soldier(id));
 const own=scheduler.packet({tick:1,own:correction(1,new Uint8Array(930))});
 const first=new Set(decodeEntityPacket(own.buffer).records.filter(r=>r.kind!=='correction').map(r=>r.id));
 const remote=scheduler.packet({tick:1,excludePacketIds:[own.packetId]});
 assert(decodeEntityPacket(remote.buffer).records.every(r=>!first.has(r.id)));
 assert.equal(scheduler.ack({epoch:1,packetId:own.packetId}),false);assert.equal(scheduler.ack({epoch:1,packetId:remote.packetId}),false);
 assert([...scheduler.records.values()].every(entry=>entry.last===-Infinity));
 const pending=scheduler.packet({tick:2,excludePacketIds:[own.packetId]});assert(decodeEntityPacket(pending.buffer).records.every(r=>!first.has(r.id)),'queued versions are excluded across ticks');
 scheduler.dropped(own.packetId);
 const retry=scheduler.packet({tick:3});assert(decodeEntityPacket(retry.buffer).records.some(r=>first.has(r.id)),'explicit drop immediately releases membership for repair');
});
