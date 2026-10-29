import test from 'node:test';import assert from 'node:assert/strict';
import {prepareEntityRecord,encodeEntityPacket,encodePreparedEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
import {EntityScheduler,EntityReceiver} from '../src/runtime/network-entity-stream.js';
const soldier=(id,tick=1)=>({kind:'soldier',id,generation:1,tick,classId:1,team:1,position:[id,0,tick/60],velocity:[0,0,1],yaw:0,pitch:0,health:100,posture:0,state:0,weapon:1,fireCounter:0});
const summary=(tick,value=1)=>({kind:'summary',id:0,generation:1,tick,schema:1,payload:new Uint8Array([value])});
const admit=(s,p)=>{assert.ok(p);assert.equal(s.admitted(p.packetId),true);return p;};
const ownLaunch=(id,tick)=>({record:{kind:'combat',id,generation:1,tick,schema:1,payload:new Uint8Array(130)},expiresTick:tick+120,retention:'projectile'});
test('own launch first-admission boost is finite and failed offers cannot spend it',()=>{
 const s=new EntityScheduler(),event=ownLaunch(90,100),budget=30+prepareEntityRecord(event.record).byteLength+2;
 const ids=p=>decodeEntityPacket(p.buffer).records.map(r=>r.id);
 s.update(soldier(1,92),1,false,3); // deadline 95
 s.updateEvent(event,24,1); // first deadline 77 after the bounded boost
 const failed=s.packet({tick:100,maxBytes:budget});assert.deepEqual(ids(failed),[90]);
 assert.equal(s.records.get('combat:90').last,-Infinity);
 s.dropped(failed.packetId);
 const sent=admit(s,s.packet({tick:101,maxBytes:budget}));
 assert.deepEqual(ids(sent),[90],'failed admission retains first-delivery urgency');
 assert.deepEqual(ids(s.packet({tick:102,maxBytes:budget})),[1],'a lost ACK does not renew the admitted launch boost');
 assert.ok(s.records.has('combat:90'),'launch remains retryable until its ACK');
 s.ack({epoch:1,packetId:sent.packetId});
 const old=new EntityScheduler();old.update(soldier(2,77),1,false,3);old.updateEvent(event,24,1);
 assert.deepEqual(ids(old.packet({tick:100,maxBytes:budget})),[2],'state older than the finite boost still takes precedence');
});
test('continuous full-sized own launches preserve actor and summary service under a tight packet budget',()=>{
 const s=new EntityScheduler(),last=new Map(),worst=new Map();
 for(let tick=1;tick<=360;tick++){
  for(let id=1;id<=64;id++)s.update(soldier(id,tick),1,false,30);
  s.update(summary(tick,tick%256),4,false,30);
  s.update({...summary(tick,tick%256),id:65536},4,false,6);
  s.updateEvent(ownLaunch(1000+tick,tick),24,1);
  const p=admit(s,s.packet({tick,maxBytes:300}));
  for(const r of decodeEntityPacket(p.buffer).records){
   if(r.kind==='combat')continue;
   const key=`${r.kind}:${r.id}`;worst.set(key,Math.max(worst.get(key)||0,tick-(last.get(key)||0)));last.set(key,tick);
  }
  s.ack({epoch:1,packetId:p.packetId});
 }
 assert.equal(last.size,66,'all 64 actors and both summary classes receive service');
 for(const [key,tick]of last){assert.ok(360-tick<=60,`${key} current age bounded`);assert.ok(worst.get(key)<=60,`${key} delivery gaps bounded`);}
});
test('own combat feedback goes ahead of overdue world state and acknowledged summaries still refresh',()=>{
 const s=new EntityScheduler({summaryHeartbeat:12}),ids=p=>decodeEntityPacket(p.buffer).records.map(r=>r.id);
 s.update(summary(1),4,false,6);const first=admit(s,s.packet({tick:1,maxBytes:80}));s.ack({epoch:1,packetId:first.packetId});
 s.update(soldier(1,2),48,false,3);
 s.updateEvent({record:{kind:'combat',id:90,generation:1,tick:13,schema:3,payload:new Uint8Array(30)},expiresTick:60,retention:'cosmetic'},24,1);
 const combat=admit(s,s.packet({tick:13,maxBytes:80}));assert.deepEqual(ids(combat),[90]);s.ack({epoch:1,packetId:combat.packetId});
 // First catch the overdue actor, then its refreshed deadline lets the
 // older summary heartbeat through despite sustained moving captures.
 admit(s,s.packet({tick:14,maxBytes:80}));s.update(soldier(1,15),48,false,3);
 const refresh=admit(s,s.packet({tick:15,maxBytes:80}));assert.deepEqual(ids(refresh),[0]);
});
test('freshness deadlines survive failed admission and keep nearby updates ahead of fresh cosmetic floods',()=>{
 const s=new EntityScheduler(),ids=p=>decodeEntityPacket(p.buffer).records.map(r=>r.id);
 s.update(soldier(1,1),1,false,3);s.update(soldier(2,1),1,false,30);
 admit(s,s.packet({tick:1}));
 s.update(soldier(1,4),1,false,3);
 s.updateEvent({record:{kind:'combat',id:90,generation:1,tick:4,schema:3,payload:new Uint8Array(1)},expiresTick:60,retention:'cosmetic'},48,2);
 const failed=s.packet({tick:4,maxBytes:80});assert.deepEqual(ids(failed),[1]);
 assert.equal(s.records.get('entity:1').last,1,'offering does not satisfy freshness');
 s.dropped(failed.packetId);
 const sent=admit(s,s.packet({tick:5,maxBytes:80}));assert.deepEqual(ids(sent),[1]);
 for(let tick=6;tick<=36;tick++){
   s.update(soldier(1,tick),48,false,3);
   s.update(soldier(2,tick),1,false,30);
   admit(s,s.packet({tick,maxBytes:80}));
 }
 assert.ok(s.records.get('entity:2').last>=31,'distant actor becomes overdue and receives service');
 assert.throws(()=>s.update(soldier(3),1,false,0),/interval/);
});
test('prepared records share one immutable encoding and match canonical bytes',()=>{const raw=soldier(1),p=prepareEntityRecord(raw);assert.equal(prepareEntityRecord(p),p);const packet={epoch:1,packetId:1,tick:1,records:[raw]};assert.deepEqual(encodePreparedEntityPacket({...packet,records:[p]}),encodeEntityPacket(packet));raw.position[0]=900;assert.equal(decodeEntityPacket(encodePreparedEntityPacket({...packet,records:[p]})).records[0].position[0],1);const a=new EntityScheduler(),b=new EntityScheduler();a.update(p);b.update(p);assert.equal(a.records.get('entity:1').prepared,b.records.get('entity:1').prepared);assert.throws(()=>encodePreparedEntityPacket({...packet,records:[raw]}),/prepared/);});
test('unchanged summaries become heartbeat only after exact admitted ACK; lost ACK never raises priority',()=>{const s=new EntityScheduler({summaryHeartbeat:120});s.update(summary(1),4);let p=admit(s,s.packet({tick:1}));s.update(summary(30),4);assert.equal(s.records.get('summary:0').record.tick,1);assert.equal(s.ack({epoch:1,packetId:p.packetId}),true);assert.equal(s.packet({tick:119}),null);p=admit(s,s.packet({tick:121}));assert.ok(p);s.update(summary(122,2),4);assert.equal(s.ack({epoch:1,packetId:p.packetId}),true);assert.equal(s.records.get('summary:0').acked,false,'old heartbeat ACK cannot quiet changed payload');p=admit(s,s.packet({tick:123}));s.update(summary(150,2),4);assert.equal(s.update(summary(140,3),4),false,'suppressed captures still retain timestamp high-water mark');assert.equal(s.records.get('summary:0').priority,4);assert.equal(s.ack({epoch:1,packetId:p.packetId}),true);assert.equal(s.packet({tick:151}),null);});
test('seat retirement requires exact admitted ACK or authoritative actor/hull lifecycle',()=>{const s=new EntityScheduler(),seat={kind:'seat',id:1,generation:1,tick:1,vehicleId:9,vehicleGeneration:1,seatIndex:0,leaseVersion:1};s.update(seat);const p=admit(s,s.packet({tick:1}));s.update({...seat,tick:2,vehicleId:0,leaseVersion:2});s.ack({epoch:1,packetId:p.packetId});assert.ok(s.records.has('seat:1'));const next=admit(s,s.packet({tick:2}));s.ack({epoch:1,packetId:next.packetId});assert.equal(s.records.has('seat:1'),false);assert.equal(s.update({...seat,tick:2,vehicleId:0,leaseVersion:2}),false);s.update({...seat,tick:3,leaseVersion:3});s.update({kind:'tombstone',id:9,generation:1,tick:4});assert.equal(s.records.has('seat:1'),false);});
test('variable budgets admit whole records only and no empty packet; own overflow explicit',()=>{const s=new EntityScheduler();s.update(soldier(1));assert.equal(s.packet({tick:1,maxBytes:79}),null);const p=s.packet({tick:1,maxBytes:80});assert.equal(p.buffer.length,80);assert.throws(()=>s.packet({tick:1,maxBytes:29}),/budget/);assert.throws(()=>s.packet({tick:1,maxBytes:80,own:{kind:'correction',id:2,generation:1,tick:1,schema:5,payload:new Uint8Array(40)}}),/own correction/);});
test('64 moving actors retain bounded coverage under deterministic data/ACK loss and summary heartbeats',()=>{const s=new EntityScheduler({maxPackets:32}),r=new EntityReceiver(),last=new Map(),gaps=[],delivered=new Set();let bytes=0,summaryRecords=0;for(let tick=3;tick<=600;tick+=3){for(let id=1;id<=64;id++)s.update(prepareEntityRecord(soldier(id,tick)),1);s.update(summary(tick),4);const p=admit(s,s.packet({tick,maxBytes:1100}));bytes+=p.buffer.length;const decoded=decodeEntityPacket(p.buffer);summaryRecords+=decoded.records.filter(x=>x.kind==='summary').length;if(tick%51===0)continue;const accepted=r.apply(p.buffer);for(const record of accepted.records)if(record.kind==='soldier'){if(last.has(record.id))gaps.push(tick-last.get(record.id));last.set(record.id,tick);delivered.add(record.id);}if(tick%21!==0)s.ack(accepted.ack);assert.ok(s.sent.size<=32&&s.offered.size<=32);}assert.equal(delivered.size,64);assert.ok(Math.max(...gaps)<=24);assert.ok(summaryRecords<=12);assert.ok(bytes/10<=22000);console.log(`Deterministic64 actor fixture: ${bytes/10} B/s, max receive gap ${Math.max(...gaps)} ticks, ${summaryRecords} summaries/10s; not RTC capacity evidence`);});
test('seat reverse references follow transfers, release ACKs and both actor lifetimes',()=>{
 const s=new EntityScheduler(),seat={kind:'seat',id:1,generation:1,tick:1,vehicleId:9,vehicleGeneration:1,seatIndex:0,leaseVersion:1};
 s.update(seat);s.update({...seat,tick:2,vehicleId:10,leaseVersion:2});
 s.update({kind:'tombstone',id:9,generation:1,tick:3});assert.ok(s.records.has('seat:1'),'retired former hull cannot remove transferred lease');
 s.update({...soldier(1,3),generation:1});assert.ok(s.records.has('seat:1'),'ordinary pose preserves current lease');
 s.update({...soldier(1,4),generation:2});assert.equal(s.records.has('seat:1'),false);assert.equal(s.seatReferences.size,0);
 s.update({...seat,tick:5,generation:2,vehicleId:10,leaseVersion:3});s.update({...soldier(10,6),generation:2});assert.equal(s.records.has('seat:1'),false);assert.equal(s.seatReferences.size,0);
 s.update({...seat,tick:7,generation:2,vehicleId:0,leaseVersion:4});const p=admit(s,s.packet({tick:7}));s.ack({epoch:1,packetId:p.packetId});assert.equal(s.seatReferences.size,0);
});
test('prepared payloads and extension objects cannot alias caller mutations',()=>{
 const raw=summary(1,7),prepared=prepareEntityRecord(raw);raw.payload[0]=99;
 const packet=decodeEntityPacket(encodePreparedEntityPacket({epoch:1,packetId:1,tick:1,records:[prepared]}));
 assert.equal(prepared.record.payload[0],7);assert.equal(packet.records[0].payload[0],7);
 const actor=soldier(1);actor.extension={nested:[{value:4}]};const copy=prepareEntityRecord(actor);actor.extension.nested[0].value=9;
 assert.equal(copy.record.extension.nested[0].value,4);assert(Object.isFrozen(copy.record.extension.nested[0]));
});
test('same-tick packet selection observes priority changes, admission, ACK and expiry',()=>{
 const s=new EntityScheduler(),first=soldier(1),second=soldier(2);s.update(first);s.update(second);
 const ids=p=>decodeEntityPacket(p.buffer).records.map(r=>r.id);
 const a=s.packet({tick:1,maxBytes:80});assert.deepEqual(ids(a),[1]);
 const other=s.packet({tick:1,maxBytes:80,excludePacketIds:[a.packetId]});assert.deepEqual(ids(other),[2]);s.dropped(other.packetId);
 s.update(second,2);const b=s.packet({tick:1,maxBytes:80});assert.deepEqual(ids(b),[2]);s.admitted(b.packetId);
 s.dropped(a.packetId);
 assert.deepEqual(ids(s.packet({tick:1,maxBytes:80})),[1]);
 const q=new EntityScheduler();q.update(summary(1));const p=admit(q,q.packet({tick:1}));q.ack({epoch:1,packetId:p.packetId});assert.equal(q.packet({tick:1}),null);
 const event={record:{kind:'combat',id:99,generation:1,tick:1,schema:3,payload:new Uint8Array(1)},expiresTick:1,retention:'cosmetic'};
 q.updateEvent(event);assert.deepEqual(ids(q.packet({tick:1})),[99]);assert.equal(q.packet({tick:2}),null);
});

for(const kind of ['mount','turret'])test(`${kind} unchanged payload waits for exact ACK then heartbeats, while state/life changes send immediately`,()=>{
 const base=kind==='mount'?{kind,id:90,generation:1,tick:1,slot:0,tilt:0,roll:0,walkPhase:0,walkBlend:0,seats:[{yaw:0,pitch:0}]}:{kind,id:90,generation:1,tick:1,building:0,state:'built',team:1,health:100,yaw:0,pitch:0,gunYaw:0,gunPitch:0};
 const s=new EntityScheduler({summaryHeartbeat:120}),r=new EntityReceiver();
 s.update(base);const first=admit(s,s.packet({tick:1}));r.apply(first.buffer);
 s.update({...base,tick:30});const retry=admit(s,s.packet({tick:30}));assert.equal(decodeEntityPacket(retry.buffer).records[0].tick,1,'lost receipt preserves retryable exact version');
 s.ack({epoch:1,packetId:first.packetId});assert.equal(s.packet({tick:31}),null);
 const heartbeat=admit(s,s.packet({tick:150}));assert.equal(r.apply(heartbeat.buffer).records.length,0,'unchanged heartbeat cannot rewind client interpolation');
 const changed={...base,tick:151,...(kind==='mount'?{seats:[{yaw:.5,pitch:0}]}:{gunYaw:.5})};s.update(changed);s.ack({epoch:1,packetId:heartbeat.packetId});const moving=admit(s,s.packet({tick:151}));assert.equal(r.apply(moving.buffer).records[0].tick,151,'old ACK cannot quiet changed aim');
 s.update({...changed,tick:160});assert.equal(s.update({...base,tick:155}),false,'suppressed captures still advance reorder watermark');s.ack({epoch:1,packetId:moving.packetId});assert.equal(s.packet({tick:161}),null);
 s.update({...changed,generation:2,tick:162});const replaced=admit(s,s.packet({tick:162}));assert.equal(r.apply(replaced.buffer).records[0].generation,2,'new life never inherits acknowledged suppression');
 if(kind==='turret'){s.ack({epoch:1,packetId:replaced.packetId});s.update({...changed,generation:2,tick:163,state:'destroyed',health:0});assert.equal(decodeEntityPacket(s.packet({tick:163}).buffer).records[0].state,'destroyed');}
});
