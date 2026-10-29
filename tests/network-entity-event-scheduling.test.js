import test from 'node:test';
import assert from 'node:assert/strict';
import {EntityScheduler,EntityReceiver} from '../src/runtime/network-entity-stream.js';
import {decodeEntityPacket,encodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const event=(id,tick=1,schema=1,expiresTick=120,retention='projectile')=>({record:{kind:'combat',id,generation:1,tick,schema,payload:new Uint8Array(20)},expiresTick,retention});
const send=(s,tick)=>{const p=s.packet({tick});assert.ok(p);assert.equal(s.admitted(p.packetId),true);return p;};
test('combat ACK quiets exact launch but terminal supersedes it at the same tick',()=>{
 const s=new EntityScheduler();s.updateEvent(event(1));const launch=send(s,1);s.ack({epoch:1,packetId:launch.packetId});assert.equal(s.packet({tick:2}),null);
 assert.equal(s.updateEvent(event(1,1,2)),true);assert.equal(s.updateEvent(event(1,2,1)),false);const terminal=send(s,2);assert.equal(decodeEntityPacket(terminal.buffer).records[0].schema,2);
 s.ack({epoch:1,packetId:launch.packetId});assert.ok(s.records.has('combat:1'));s.ack({epoch:1,packetId:terminal.packetId});assert.equal(s.packet({tick:3}),null);assert.equal(s.events.size,1);
 s.packet({tick:121});assert.equal(s.events.size,0);assert.equal(s.eventBytes,0);assert.equal(s.versions.size,0);
});
test('bounded combat retention sheds cosmetics before denying a new projectile',()=>{
 const s=new EntityScheduler({maxEvents:2,maxEventBytes:1000});s.updateEvent(event(1,1,3,18,'cosmetic'));s.updateEvent(event(2));assert.equal(s.updateEvent(event(3)),true);assert.equal(s.versions.has('combat:1'),false);assert.equal(s.eventMetrics.cosmeticEvicted,1);
 assert.equal(s.updateEvent(event(4)),false);assert.equal(s.eventMetrics.overflow,1);assert.equal(s.events.size,2);assert.equal(s.updateEvent(event(2,1,2)),true);
 const tiny=new EntityScheduler({maxEventBytes:1});assert.equal(tiny.updateEvent(event(1)),false);assert.equal(tiny.eventBytes,0);assert.equal(tiny.events.size,0);
 assert.throws(()=>s.update(event(8).record),/bounded event/);
});
test('combat receiver forwards separate lifecycle records without actor or seat retention',()=>{
 const receiver=new EntityReceiver({maxEntities:1}),seat={kind:'seat',id:1,generation:1,tick:1,vehicleId:9,vehicleGeneration:1,seatIndex:0,leaseVersion:1};
 receiver.apply(encodeEntityPacket({epoch:1,packetId:1,tick:1,records:[seat]}));
 for(let id=2;id<100;id++){const result=receiver.apply(encodeEntityPacket({epoch:1,packetId:id,tick:id,records:[event(id,id).record]}));assert.equal(result.records[0].kind,'combat');}
 assert.equal(receiver.records.size,1);assert.equal(receiver.seatOwner(9,1,0).id,1);
});

test('late launch ACK cannot retire a same-tick terminal',()=>{const s=new EntityScheduler();s.updateEvent(event(1));const old=send(s,1);s.updateEvent(event(1,1,2));s.ack({epoch:1,packetId:old.packetId});assert.equal(s.records.get('combat:1').record.schema,2);});

test('dynamic projectile samples continue after ACK, terminal blocks all later samples',()=>{
 const s=new EntityScheduler();s.updateEvent(event(9,1,6,19));const first=send(s,1);s.ack({epoch:1,packetId:first.packetId});
 assert.equal(s.updateEvent(event(9,4,6,22)),true);const next=send(s,4);
 assert.equal(s.updateEvent(event(9,4,7,22)),true);s.ack({epoch:1,packetId:next.packetId});assert.equal(s.records.get('combat:9').record.schema,7);
 assert.equal(s.updateEvent(event(9,5,6,23)),false);const terminal=send(s,5);assert.equal(decodeEntityPacket(terminal.buffer).records[0].schema,7);
});
