import test from 'node:test';
import assert from 'node:assert/strict';
import {EntityScheduler} from '../src/runtime/network-entity-stream.js';
import {decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const pose=(tick=1,generation=1)=>({kind:'soldier',id:1,generation,tick,classId:1,team:1,position:[0,0,tick],velocity:[0,0,1],yaw:0,pitch:0,health:100,posture:0,state:0,weapon:1,fireCounter:0});
const records=p=>decodeEntityPacket(p.buffer).records;

test('pending versions wait for admission/drop across ticks while newer poses remain eligible',()=>{
 const s=new EntityScheduler();s.update(pose());const first=s.packet({tick:1});
 for(let tick=2;tick<=6;tick++)assert.equal(s.packet({tick}),null,'do not serialize identical queued pose again');
 assert.equal(s.records.get('entity:1').last,-Infinity);
 s.update(pose(7));const newer=s.packet({tick:7});assert.equal(records(newer)[0].tick,7);
 assert.equal(s.pending.size,2);assert.ok(s.dropped(first.packetId));assert.equal(s.pending.size,1);
 assert.equal(s.packet({tick:8}),null,'dropping old version cannot release newer pending version');
 assert.equal(s.admitted(newer.packetId),true);assert.equal(s.pending.size,0);
 assert.equal(s.records.get('entity:1').last,7);assert.ok(s.packet({tick:9}),'unACKed admitted version remains loss-repairable');
 assert.equal(s.dropped(first.packetId),null);assert.equal(s.admitted(first.packetId),false);
});

test('pending launch can be replaced by same-tick terminal and each dropped event remains retryable',()=>{
 const s=new EntityScheduler();const event=schema=>({record:{kind:'combat',id:9,generation:1,tick:1,schema,payload:new Uint8Array(2)},expiresTick:120,retention:'projectile'});
 s.updateEvent(event(1));const launch=s.packet({tick:1});assert.equal(s.packet({tick:2}),null);
 s.updateEvent(event(2));const terminal=s.packet({tick:2});assert.equal(records(terminal)[0].schema,2);
 s.dropped(launch.packetId);assert.equal(s.packet({tick:3}),null);
 s.dropped(terminal.packetId);const retry=s.packet({tick:4});assert.equal(records(retry)[0].schema,2);
 assert.equal(s.records.get('combat:9').last,-Infinity);s.admitted(retry.packetId);
 const lostAck=s.packet({tick:5});assert.equal(records(lostAck)[0].schema,2);
 s.ack({epoch:1,packetId:retry.packetId});s.dropped(lostAck.packetId);assert.equal(s.packet({tick:6}),null);
});

test('pending ledger is bounded without forgetting refundable offers and old lives cannot satisfy freshness',()=>{
 const s=new EntityScheduler({maxPackets:2});s.update(pose());const a=s.packet({tick:1});
 s.update(pose(2));const b=s.packet({tick:2});s.update(pose(3,2));
 assert.equal(s.packet({tick:3}),null);assert.equal(s.offered.size,2);assert.equal(s.pending.size,2);
 assert.ok(s.admitted(a.packetId));assert.equal(s.records.get('entity:1').last,-Infinity,'old-life admission cannot refresh new life');
 const c=s.packet({tick:4});assert.equal(records(c)[0].generation,2);assert.equal(s.offered.size,2);
 s.dropped(b.packetId);s.dropped(c.packetId);assert.equal(s.offered.size,0);assert.equal(s.pending.size,0);assert.ok(s.packet({tick:5}));
});
