import {encodeEntityPacket,decodeEntityPacket,encodeEntityRecord,prepareEntityRecord,sameEntityPayload,encodePreparedEntityPacket,ENTITY_PACKET_BYTES,entityPacketHeaderBytes} from './network-entity-codec.js';
const key=r=>`${r.kind==='seat'?'seat':r.kind==='correction'?'correction':r.kind==='summary'?'summary':r.kind==='combat'?'combat':r.kind==='item'?'item':r.kind==='remote'?'remote':r.kind==='mount'?'mount':'entity'}:${r.id}`;
const versionKey=r=>`${key(r)}:${r.kind}:${r.generation}:${r.tick}:${r.schema??''}:${r.active??''}`;
const newer=(a,b)=>{
 if(!b)return true;
 if((a.kind==='item'||a.kind==='remote')&&a.kind===b.kind)return a.generation>b.generation||a.generation===b.generation&&b.active&&(a.tick>b.tick||a.tick===b.tick&&!a.active);
 if(a.kind==='combat'&&b.kind==='combat')return a.generation>b.generation||a.generation===b.generation&&![2,7].includes(b.schema)&&(a.tick>b.tick||a.tick===b.tick&&[2,7].includes(a.schema));
 return a.generation>b.generation||a.generation===b.generation&&!(b.kind==='tombstone'&&a.kind!=='tombstone')&&(a.tick>b.tick||a.tick===b.tick&&a.kind==='tombstone'&&b.kind!=='tombstone');
};
const copy=x=>structuredClone(x);
const heartbeatKind=kind=>kind==='summary'||kind==='item'||kind==='remote'||kind==='mount'||kind==='turret';
// One instance per recipient. ACKs name exact packet membership, not a global
// world tick. Absolute live records remain usable even if every ACK is lost.
// Fixed actor roster prototype: retained lifecycle high-water marks are bounded
// by maxEntities until epoch reset. Combat IDs use separate byte/count/TTL bounds.
export class EntityScheduler {
 constructor({epoch=1,maxEntities=256,maxPackets=128,maxBytes=ENTITY_PACKET_BYTES,summaryHeartbeat=120,maxEvents=1024,maxEventBytes=262144}={}){if(!Number.isSafeInteger(summaryHeartbeat)||summaryHeartbeat<1)throw Error('invalid summary heartbeat');this.summaryHeartbeat=summaryHeartbeat;this.epoch=epoch;this.maxEntities=maxEntities;this.maxPackets=maxPackets;this.maxBytes=maxBytes;this.serial=0;this.ranked=null;this.rankedTick=-1;this.records=new Map();this.versions=new Map();this.itemSlots=new Set();this.remoteSlots=new Set();this.sent=new Map();this.offered=new Map();this.seatReferences=new Map();this.events=new Map();this.eventBytes=0;this.maxEvents=maxEvents;this.maxEventBytes=maxEventBytes;this.eventMetrics={expired:0,cosmeticEvicted:0,overflow:0};}
 retireSeat(id){const entry=this.records.get(id);if(!entry)return;this.ranked=null;const r=entry.record;for(const actor of new Set([r.id,r.vehicleId])){const refs=this.seatReferences.get(actor);refs?.delete(id);if(refs?.size===0)this.seatReferences.delete(actor);}this.records.delete(id);}
 pruneEvents(tick){for(const[id,event]of this.events)if(event.expiresTick<tick){this.removeEvent(id);this.eventMetrics.expired++;}}
 removeEvent(id){const event=this.events.get(id);if(!event)return;this.ranked=null;this.eventBytes-=event.bytes;this.events.delete(id);this.records.delete(id);this.versions.delete(id);}
 updateEvent({record,expiresTick,retention},priority=1,interval=Infinity){
  const prepared=prepareEntityRecord(record);record=prepared.record;
  if(!Number.isFinite(priority)||priority<=0||record.kind!=='combat'||!Number.isSafeInteger(expiresTick)||expiresTick<record.tick||expiresTick-record.tick>7320||!['projectile','cosmetic'].includes(retention))throw Error('invalid entity event retention');
  const id=key(record),old=this.versions.get(id);if(!newer(record,old))return false;
  this.pruneEvents(record.tick);const previous=this.events.get(id),bytes=prepared.byteLength;
  const full=()=>this.events.size+(previous?0:1)>this.maxEvents||this.eventBytes-(previous?.bytes??0)+bytes>this.maxEventBytes;
  if(full())for(const[candidate,event]of this.events){if(candidate!==id&&event.retention==='cosmetic'){this.removeEvent(candidate);this.eventMetrics.cosmeticEvicted++;if(!full())break;}}
  if(full()){this.eventMetrics.overflow++;return false;}
  this.eventBytes+=bytes-(previous?.bytes??0);this.events.set(id,{expiresTick,retention,bytes});
  return this.update(prepared,priority,true,interval);
 }
 update(record,priority=1,event=false,interval=Infinity){
  if(interval!==Infinity&&(!Number.isSafeInteger(interval)||interval<1||interval>600))throw Error('invalid entity update interval');
  const prepared=prepareEntityRecord(record);record=prepared.record;if(!Number.isFinite(priority)||priority<=0)throw Error('invalid entity priority');
  if(record.kind==='combat'&&!event)throw Error('combat record requires bounded event retention');
  this.ranked=null;
  const id=key(record),old=this.versions.get(id),entry=this.records.get(id);if(!newer(record,old)){if(entry){entry.priority=priority;entry.interval=interval;}return false;}
  if(!event&&!old&&record.kind!=='item'&&record.kind!=='remote'&&this.versions.size-this.events.size-this.itemSlots.size-this.remoteSlots.size>=this.maxEntities)throw Error('entity scheduler capacity exceeded');
  // A current capture tick alone is not a stationary payload change. Keep its acknowledged
  // version and low-rate heartbeat instead of spending a full record every tick.
  if(heartbeatKind(record.kind)&&entry&&sameEntityPayload(prepared,entry.prepared)){this.versions.set(id,record);entry.priority=priority;entry.interval=interval;return false;}
  if(record.kind==='item')this.itemSlots.add(id);if(record.kind==='remote')this.remoteSlots.add(id);this.versions.set(id,record);const last=entry?.last??-Infinity;
  if(record.kind==='seat'){if(entry)this.retireSeat(id);for(const actor of new Set([record.id,record.vehicleId])){let refs=this.seatReferences.get(actor);if(!refs)this.seatReferences.set(actor,refs=new Set());refs.add(id);}}
  if(entry){entry.record=record;entry.prepared=prepared;entry.priority=priority;entry.interval=interval;entry.acked=false;if(!this.records.has(id))this.records.set(id,entry);}
  else this.records.set(id,{record,prepared,priority,last,interval,first:record.tick,acked:false});
  if(record.kind!=='seat'&&record.kind!=='correction'&&record.kind!=='summary'&&record.kind!=='combat'&&record.kind!=='item'&&record.kind!=='remote')for(const seatKey of this.seatReferences.get(record.id)??[]){
   const r=this.records.get(seatKey).record;const gone=record.kind==='tombstone';
   if(r.id===record.id&&(r.generation<record.generation||gone&&r.generation===record.generation)||r.vehicleId===record.id&&(r.vehicleGeneration<record.generation||gone&&r.vehicleGeneration===record.generation))this.retireSeat(seatKey);
  }
  return true;
 }
 packet({tick,own,ackInput=0,ackActions=0,ackFire=0,excludePacketIds=[],maxBytes=this.maxBytes}){
  this.pruneEvents(tick);
  if(!Number.isSafeInteger(maxBytes)||maxBytes<entityPacketHeaderBytes||maxBytes>this.maxBytes)throw Error('invalid entity packet budget');
  // Remote-only packets must not pay for a full replay envelope. Own state
  // cadence and remote coverage share a byte budget, not a packet dependency.
  if(own&&own.kind!=='correction')throw Error('invalid own correction');
  const ownPrepared=own?prepareEntityRecord(own):null,chosen=ownPrepared?[ownPrepared]:[],ownBytes=ownPrepared?ownPrepared.byteLength+2:0;let bytes=entityPacketHeaderBytes+ownBytes;
  if(bytes>maxBytes)throw Error('own correction cannot fit budget');
  // Outstanding offers own their membership until admission or explicit drop.
  // Apply backpressure instead of evicting a still-refundable offer.
  if(this.offered.size>=this.maxPackets)return null;
  this.pending??=new Set();
  if(!Array.isArray(excludePacketIds)||excludePacketIds.length>this.maxPackets||excludePacketIds.some(id=>!Number.isSafeInteger(id)||id<1))throw Error('invalid entity packet exclusions');
  const excluded=new Set();for(const id of excludePacketIds){const offer=this.offered.get(id);if(offer?.tick===tick)for(const member of offer.membership)excluded.add(member.key);}
  // Reuse only ordering within a tick; membership and budgets are per packet.
  // ACKs, admission, updates and removal invalidate ordering, including items.
  if(!this.ranked||this.rankedTick!==tick){
   const ranked=[];
   for(const entry of this.records.values())if(!entry.acked||!heartbeatKind(entry.record.kind)||tick-entry.last>=this.summaryHeartbeat){
    // Changed presentation summaries still respect their refresh interval;
    // eagerly backfilling them spends the next pose publication's budget.
    if(heartbeatKind(entry.record.kind)&&Number.isFinite(entry.interval)&&tick-entry.last<entry.interval)continue;
    entry.rank=(entry.last===-Infinity?1e9:tick-entry.last)*entry.priority;
    entry.deadline=(Number.isFinite(entry.last)?entry.last:entry.first)+entry.interval;
    // Advance a new own-combat event by a bounded window. Actual admission
    // spends this boost even if its ACK is lost; older world state still wins.
    if(entry.record.kind==='combat'&&entry.interval===1&&entry.last===-Infinity)entry.deadline-=24;
    // Give due close/fast poses a bounded head start. Unconditional near-first
    // partitions starve distant actors and world state under sustained load.
    // Keep actual due-ness separate so this never spends the head start on a
    // pose which has already met its delivery interval.
    const pose=['soldier','vehicle','droideka'].includes(entry.record.kind);
    entry.serviceDeadline=entry.deadline-(pose&&entry.interval<=6?12:0);
    ranked.push(entry);
   }
   // Meet nearby combat deadlines first. Old distant records eventually
   // become overdue too, retaining service under sustained event pressure.
   ranked.sort((a,b)=>{
    const dueA=a.deadline<=tick,dueB=b.deadline<=tick;
    return dueA!==dueB?(dueA?-1:1):dueA&&a.serviceDeadline!==b.serviceDeadline?a.serviceDeadline-b.serviceDeadline:b.rank-a.rank;
   });this.ranked=ranked;this.rankedTick=tick;
  }
  const ranked=this.ranked;
  for(const entry of ranked){if(excluded.has(key(entry.record))||this.pending.has(versionKey(entry.record)))continue;const size=entry.prepared.byteLength+2;if(bytes+size>maxBytes)continue;chosen.push(entry.prepared);bytes+=size;}
  if(!chosen.length)return null;
  const packetId=++this.serial,buffer=encodePreparedEntityPacket({epoch:this.epoch,packetId,tick,ackInput,ackActions,ackFire,records:chosen},maxBytes);
  const membership=chosen.map(({record:r})=>({key:key(r),generation:r.generation,tick:r.tick,kind:r.kind,schema:r.schema,active:r.active}));
  // Own corrections are intentionally refreshed independently of pose/event
  // suppression and may share a tick, so only scheduled records are pending.
  const pending=chosen.slice(ownPrepared?1:0).map(({record})=>versionKey(record));
  for(const version of pending)this.pending.add(version);
  this.offered.set(packetId,{tick,membership,pending,chargedBytes:0});
  return {packetId,buffer};
 }
 dropped(packetId){
  const offer=this.offered.get(packetId);if(!offer)return null;
  this.offered.delete(packetId);for(const version of offer.pending)this.pending.delete(version);
  return offer;
 }
 admitted(packetId){
  const offer=this.dropped(packetId);if(!offer)return false;this.ranked=null;
  for(const version of offer.membership){const entry=this.records.get(version.key);if(entry&&entry.record.generation===version.generation)entry.last=Math.max(entry.last,offer.tick);}
  this.sent.set(packetId,offer.membership);
  while(this.sent.size>this.maxPackets)this.sent.delete(this.sent.keys().next().value);
  return true;
 }
 ack({epoch,packetId}){
  if(epoch!==this.epoch)return false;const membership=this.sent.get(packetId);if(!membership)return false;this.ranked=null;
  for(const version of membership){const entry=this.records.get(version.key),current=entry?.record;if(current?.kind===version.kind&&current.generation===version.generation&&current.tick===version.tick&&current.schema===version.schema&&current.active===version.active){if(current.kind==='combat')this.records.delete(version.key);else if(current.kind==='seat')this.retireSeat(version.key);else if((current.kind==='item'||current.kind==='remote')&&!current.active)this.records.delete(version.key);else if(current.kind==='tombstone')this.records.delete(version.key);else if(heartbeatKind(current.kind))entry.acked=true;}}
  this.sent.delete(packetId);return true;
 }
}
// Independently apply each entity's latest version. Partial packets never
// advance absent entities, and losing one message cannot poison later messages.
export class EntityReceiver {
 constructor({maxEntities=256,maxPackets=128}={}){this.maxEntities=maxEntities;this.maxPackets=maxPackets;this.reset(0);}
 reset(epoch){this.epoch=epoch;this.records=new Map();this.itemCount=0;this.remoteCount=0;this.seats=new Map();this.packets=new Set();}
 get(kind,id){
  const value=this.records.get(key({kind,id}));
  if(kind==='seat'&&value?.vehicleId){const owner=this.seats.get(`${value.vehicleId}:${value.vehicleGeneration}:${value.seatIndex}`);if(owner?.id!==id||owner.generation!==value.generation||owner.leaseVersion!==value.leaseVersion)return undefined;}
  return value&&copy(value);
 }
 seatOwner(vehicleId,generation,index){return copy(this.seats.get(`${vehicleId}:${generation}:${index}`)||null);}
 apply(bytes){
  const packet=decodeEntityPacket(bytes);if(packet.epoch<this.epoch)return null;
  const changedEpoch=packet.epoch>this.epoch,records=changedEpoch?new Map():new Map(this.records),seats=changedEpoch?new Map():new Map(this.seats),applied=[];
  let itemCount=changedEpoch?0:this.itemCount,remoteCount=changedEpoch?0:this.remoteCount;
  if(!changedEpoch&&this.packets.has(packet.packetId))return null;
  for(const record of packet.records){
   // Combat has its own bounded lifetime/high-water receiver in presentation.
   // Never insert projectile IDs into permanent actor or seat namespaces.
   if(record.kind==='combat'){applied.push(record);continue;}
   const id=key(record);if(!newer(record,records.get(id)))continue;
   if(record.kind==='seat'){
    const unit=records.get(`entity:${record.id}`),hull=records.get(`entity:${record.vehicleId}`);
    if(unit&&(unit.generation>record.generation||unit.kind==='tombstone'&&unit.generation>=record.generation)||record.vehicleId&&hull&&(hull.generation>record.vehicleGeneration||hull.kind==='tombstone'&&hull.generation>=record.vehicleGeneration))continue;
    const old=records.get(id);if(old&&record.generation===old.generation&&record.leaseVersion<=old.leaseVersion)continue;
    // Ownership is one atomic unit-to-hull seat record, never independent
    // driver/gunner booleans. Reject a stale competing claim for the same slot.
    const slot=`${record.vehicleId}:${record.vehicleGeneration}:${record.seatIndex}`,occupant=seats.get(slot);
    if(record.vehicleId&&occupant&&occupant.id!==record.id&&occupant.tick>=record.tick)continue;
    if(old?.vehicleId){const previous=`${old.vehicleId}:${old.vehicleGeneration}:${old.seatIndex}`;if(seats.get(previous)?.id===record.id)seats.delete(previous);}
    if(record.vehicleId)seats.set(slot,record);
   }
   if(record.kind!=='seat'&&record.kind!=='correction'&&record.kind!=='summary'&&record.kind!=='item'&&record.kind!=='remote')for(const [slot,owner]of seats){
    const retired=record.kind==='tombstone';
    if(owner.id===record.id&&(owner.generation<record.generation||retired&&owner.generation===record.generation)||owner.vehicleId===record.id&&(owner.vehicleGeneration<record.generation||retired&&owner.vehicleGeneration===record.generation))seats.delete(slot);
   }
   if(record.kind==='item'&&!records.has(id))itemCount++;if(record.kind==='remote'&&!records.has(id))remoteCount++;records.set(id,record);applied.push(record);
  }
  if(records.size-itemCount-remoteCount>this.maxEntities)throw Error('entity receiver capacity exceeded');
  const oldOwners=new Map([...this.seats.values()].map(r=>[r.id,r])),newOwners=new Map([...seats.values()].map(r=>[r.id,r])),seatChanges=[];
  for(const id of new Set([...oldOwners.keys(),...newOwners.keys()])){const old=oldOwners.get(id),now=newOwners.get(id);if(changedEpoch||old!==now)seatChanges.push({id,generation:now?.generation??old.generation,seat:now?copy(now):null});}
  if(changedEpoch)this.reset(packet.epoch);
  this.records=records;this.itemCount=itemCount;this.remoteCount=remoteCount;this.seats=seats;this.packets.add(packet.packetId);
  while(this.packets.size>this.maxPackets)this.packets.delete(this.packets.values().next().value);
  return {ack:{epoch:packet.epoch,packetId:packet.packetId},tick:packet.tick,ackInput:packet.ackInput,ackActions:packet.ackActions,ackFire:packet.ackFire,records:copy(applied),seatChanges};
 }
}
