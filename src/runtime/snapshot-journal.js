// Immutable wire snapshots for an optional unordered state channel. Baselines
// belong to the protocol, never to live (and temporarily predicted) game objects.
import { BinaryReader, BinaryWriter } from './packet.js';
import { packetParts } from './shared-packet.js';

export class MissingSnapshotBaseline extends Error {}
export class SnapshotMemoryLimit extends MissingSnapshotBaseline {}
// Conservative materialized JS accounting, including duplicated shared values.
// Immutable subtrees are measured once; the WeakMap does not keep them alive.
const measured = new WeakMap(), retainedSize = new WeakMap();
function memoryBytes(value) {
  if(typeof value==='string')return 32+value.length*2;
  if(!value || typeof value!=='object')return 16;
  if(measured.has(value))return measured.get(value);
  let bytes=64;
  if(ArrayBuffer.isView(value))bytes+=value.byteLength;
  else if(value instanceof Map)for(const [key,item]of value)bytes+=48+memoryBytes(key)+memoryBytes(item);
  else if(Array.isArray(value))for(const item of value)bytes+=8+memoryBytes(item);
  else for(const [key,item]of Object.entries(value))bytes+=32+memoryBytes(key)+memoryBytes(item);
  measured.set(value,bytes);return bytes;
}
function measureSnapshot(snapshot,payloadBytes) { const bytes=memoryBytes(snapshot)+payloadBytes;retainedSize.set(snapshot,bytes);return bytes; }
const equal = (a,b) => a === b || !!a && !!b && a.tag === b.tag && a.data.length === b.data.length && a.data.every((v,i)=>Object.is(v,b.data[i]));
const sameJSON = (a,b) => a === b || JSON.stringify(a) === JSON.stringify(b);
const lists = 6;

function fields(bin, tables, previous) {
  const schema = bin.u16[bin.p16++], names = tables.schemas[schema];
  if (!names) throw Error('Unknown snapshot schema');
  const words = Math.ceil(names.length/16), masks = bin.u16.slice(bin.p16,bin.p16+words);
  bin.p16 += words;
  const sameSchema=previous?.schema===schema;
  const values=sameSchema?previous.values.slice():new Array(names.length);
  if(previous && !sameSchema) {
    const oldNames=tables.schemas[previous.schema];
    for(let i=0;i<names.length;i++)values[i]=previous.values[oldNames.indexOf(names[i])];
  }
  for(let i=0;i<names.length;i++) if(masks[i>>4] & (1<<(i&15))) {
    let tag=bin.u8[bin.p8++]; let data=[];
    if(tag===12 || tag===13) {
      const old=values[i], count=tag===12?3:1, original=tag===12?8:4;
      if(!old || old.tag!==original && !(original===4 && old.tag===5))throw new MissingSnapshotBaseline('Relative field is missing its immutable baseline');
      data=Array.from({length:count},(_,i)=>old.data[i]+bin.f64[bin.p64++]);tag=original;
    }
    else if(tag===4)data=[bin.f64[bin.p64++]];
    else if(tag===5)data=[bin.i32[bin.p32++]];
    else if(tag===6)data=[bin.u16[bin.p16++]];
    else if(tag>=7 && tag<=11) {
      const count=tag===7?2:tag===9?4:tag===11?bin.u16[bin.p16++]:3;
      data=Array.from(bin.f64.slice(bin.p64,bin.p64+count));bin.p64+=count;
      if(tag===10)data.push(bin.u16[bin.p16++]);
    } else if(tag<0 || tag>3)throw Error('Unknown snapshot field tag');
    values[i]={tag,data};
  }
  return {schema,values};
}

function materialize(packet, previous, ownTables=false) {
  const bin=packet.bin, tables=packet.reader;
  const records=new Map(previous?.records), count=bin.u16[bin.p16++];
  for(let i=0;i<count;i++) {
    const key=bin.u16[bin.p16++];
    records.set(key,fields(bin,tables,records.get(key)));
  }
  const nextLists=[];
  const refs=new Map((packet.listRefs||[]).map(([index,items])=>[index,new Map(items)]));
  for(let l=0;l<lists;l++) {
    const count=bin.u16[bin.p16++];
    if(count===0xffff){nextLists.push(null);continue;}
    const items=new Map();
    for(let i=0;i<count;i++) {
      const id=bin.i32[bin.p32++], flags=bin.u8[bin.p8++], old=previous?.lists[l]?.get(id);
      const record=flags&1?fields(bin,tables,old):old;
      if(!record)throw Error('Snapshot list member has no baseline');
      const reference=flags&2?refs.get(l)?.get(id):old?.refs;
      items.set(id,{...record,refs:reference,flightTick:l===0 && packet.linearProjectiles ? (packet.linearFlightTicks?.[id] ?? (flags&1 ? packet.tick : old?.flightTick)) : undefined});
    }
    nextLists.push(items);
  }
  if(bin.p8!==bin.u8.length || bin.p16!==bin.u16.length || bin.p32!==bin.i32.length || bin.p64!==bin.f64.length)throw Error('Snapshot streams did not end together');
  const built=new Map(previous?.built), seats=new Map(previous?.seats);
  for(const [id,name]of packet.built||[])built.set(id,name);
  for(const [key,value]of packet.seats||[])seats.set(key,value);
  const created=[...(previous?.created||[]),...(packet.created||[])];
  const {bin:unused,reader,defs,built:ignoreBuilt,seats:ignoreSeats,created:ignoreCreated,listRefs,calls,journal,keyframe,bytes,...meta}=packet;
  if(meta.characters===undefined && previous?.meta.characters!==undefined)meta.characters=previous.meta.characters;
  const strings=ownTables?tables.strings:tables.strings.slice(),schemas=ownTables?tables.schemas:tables.schemas.slice();
  const stringIds=previous?.strings.length===strings.length?previous.stringIds:new Map(strings.map((s,i)=>[s,i]));
  const schemaIds=previous?.schemas.length===schemas.length?previous.schemaIds:schemas.map(names=>names.map(name=>stringIds.get(name)));
  return {tick:packet.tick,records,lists:nextLists,built,seats,created,meta,strings,schemas,stringIds,schemaIds};
}

function writeFields(w,record,previous,tables,relative=false) {
  if(record===previous || record.schema===previous?.schema && record.values===previous.values)return false;
  const names=tables.schemas[record.schema],sameSchema=record.schema===previous?.schema;
  const oldNames=previous && !sameSchema ? tables.schemas[previous.schema] : null;
  let masks=-1;
  for(let i=0;i<names.length;i++) {
    const value=record.values[i],old=previous?.values[sameSchema?i:oldNames.indexOf(names[i])];
    if(!value || equal(value,old))continue;
    // Reserve the schema/mask only when the first change is found. This keeps
    // unchanged records empty without allocating a changed-field tuple list.
    if(masks<0){w.put16(record.schema);masks=w.reserve16(Math.ceil(names.length/16));}
    w.u16[masks+(i>>4)]|=1<<(i&15);
    const {tag,data}=value;
    let relativeTag=0;
    if(relative) {
      if(tag===8 && (names[i]==='position'||names[i]==='velocity') && old?.tag===8)relativeTag=12;
      else if(tag===4 && (names[i]==='aimYaw'||names[i]==='aimPitch'||names[i]==='yaw'||names[i]==='pitch') && (old?.tag===4||old?.tag===5))relativeTag=13;
    }
    // Only exact bounded binary fractions use relative coding. Addition then
    // recovers the original wire values exactly, with no new quantization.
    if(relativeTag) {
      const scale=relativeTag===12?256:4096;
      for(let n=0;n<data.length;n++) {
        const next=data[n]*scale,prior=old.data[n]*scale;
        if(!Number.isSafeInteger(next)||!Number.isSafeInteger(prior)||Math.abs(next)>=2**40||Math.abs(prior)>=2**40){relativeTag=0;break;}
      }
    }
    if(relativeTag){w.put8(relativeTag);for(let n=0;n<data.length;n++)w.putF(data[n]-old.data[n]);continue;}
    w.put8(tag);
    if(tag===4)w.putF(data[0]);else if(tag===5)w.put32(data[0]);else if(tag===6)w.put16(data[0]);
    else if(tag>=7 && tag<=11) {
      const count=tag===10?data.length-1:data.length;
      if(tag===11)w.put16(count);
      for(let n=0;n<count;n++)w.putF(data[n]);
      if(tag===10)w.put16(data.at(-1));
    }
  }
  return masks>=0;
}

function encode(snapshot,base,journal,extra={},relative=false,w=new BinaryWriter()) {
  w.begin();
  w.stringList=snapshot.strings;w.schemaList=snapshot.schemaIds;
  w.full=!base;
  w.newStrings=snapshot.strings.slice(base?.strings.length||0);
  w.newSchemas=w.schemaList.map((v,i)=>[i,v]).slice(base?.schemas.length||0);
  const countAt=w.reserve16(1);let count=0;
  for(const[key,record]of snapshot.records) {
    const start=w.n16;w.put16(key);
    if(writeFields(w,record,base?.records.get(key),snapshot,relative))count++;else w.n16=start;
  }
  w.u16[countAt]=count;
  const listRefs=[];
  snapshot.lists.forEach((items,l)=>{
    if(!items){w.put16(0xffff);return;}
    w.put16(items.size);const references=[];
    for(const[id,record]of items) {
      const old=base?.lists[l]?.get(id);w.put32(id);const flagAt=w.n8;w.put8(0);
      // A mode switch changes how unchanged bolt samples are interpreted.
      // Force their sample fields so the renderer installs/clears flight time.
      const modeChanged=l===0 && !!snapshot.meta.linearProjectiles!==!!base?.meta.linearProjectiles;
      let flags=writeFields(w,record,modeChanged?undefined:old,snapshot,relative)?1:0;
      if(!old || !sameJSON(record.refs,old.refs)){flags|=2;references.push([id,record.refs||{}]);}
      w.u8[flagAt]=flags;
    }
    if(references.length)listRefs.push([l,references]);
  });
  const built=[...snapshot.built].filter(([id,name])=>base?.built.get(id)!==name);
  const seats=[...snapshot.seats].filter(([key,value])=>!base || !sameJSON(value,base.seats.get(key)));
  const meta={...snapshot.meta};
  if(base && sameJSON(meta.characters,base.meta.characters))delete meta.characters;
  return w.finish({...meta,...extra,baseTick:base?.tick??null,keyframe:!base,built,seats,
    created:snapshot.created.slice(base?.created.length||0),listRefs,calls:[],journal,
    ...(snapshot.meta.linearProjectiles ? {linearFlightTicks:Object.fromEntries([...(snapshot.lists[0]||[])].map(([id,record])=>[id,record.flightTick]))} : {})});
}

export class SnapshotJournal {
  constructor({epoch=1,maxSnapshots=48,maxTicks=120,maxPins=128,maxBytes=64*1024*1024,relativeVectors=false}={}) {
    this.epoch=epoch;this.maxSnapshots=maxSnapshots;this.maxTicks=maxTicks;this.maxPins=maxPins;this.pins=new Map();this.relativeVectors=relativeVectors;
    if(!Number.isFinite(maxBytes)||maxBytes<1)throw Error('Invalid snapshot memory budget');
    this.maxBytes=maxBytes;this.retainedBytes=0;
    this.reader=new BinaryReader();this.writer=new BinaryWriter();this.snapshots=new Map();this.current=null;this.eventSequence=0;
  }
  push(buffer) {
    const packet=this.reader.read(buffer);
    const initial=!this.current;
    const sameTick=!!this.current && packet.tick===this.current.tick;
    if(this.current && packet.tick<this.current.tick)throw Error('Snapshot source must advance');
    if(this.current && packet.tick===this.current.tick) {
      if(!packet.keyframe)throw Error('Same-tick world mutation requires a keyframe');
      // Admission/roster changes may happen between simulation steps. A tick
      // number must never name two different immutable acknowledgement bases.
      this.epoch++;this.snapshots.clear();this.pins.clear();this.current=null;
    }
    this.current=materialize(packet,packet.keyframe?null:this.current);
    if(measureSnapshot(this.current,buffer.byteLength)>this.maxBytes){
      this.current=null;this.snapshots.clear();this.pins.clear();this.retainedBytes=0;
      throw new SnapshotMemoryLimit('One materialized snapshot exceeds the journal memory budget');
    }
    this.snapshots.set(packet.tick,this.current);
    this.prune();
    const calls=initial||sameTick?[]:packet.calls||[],ids=new Set(calls.flatMap(referencedUnits)),generations=[];
    for(const id of ids) {
      const record=this.current.records.get(this.current.stringIds.get(`u${id}`));
      const generation=record?.values[this.current.schemas[record.schema].indexOf('generation')]?.data[0];
      if(Number.isSafeInteger(generation))generations.push([id,generation]);
    }
    return {epoch:this.epoch,tick:packet.tick,id:++this.eventSequence,calls,generations};
  }
  // Two bounded owner slots per peer: its applied baseline and its one
  // in-flight checkpoint. Reliable delivery can exceed the rolling history;
  // expiry must not cause a repeated full-bootstrap congestion feedback loop.
  pin(owner,tick) {
    if(!this.snapshots.has(tick))return false;
    if(!this.pins.has(owner) && this.pins.size>=this.maxPins)throw Error('Snapshot pin capacity exceeded');
    this.pins.set(owner,tick);this.prune();return this.snapshots.has(tick);
  }
  unpin(owner) {const removed=this.pins.delete(owner);this.prune();return removed;}
  prune() {
    const pinned=new Set(this.pins.values());
    let rolling=this.snapshots.size-pinned.size;
    for(const[tick]of this.snapshots) {
      if(pinned.has(tick))continue;
      if(rolling>this.maxSnapshots || this.current.tick-tick>this.maxTicks){this.snapshots.delete(tick);rolling--;}
    }
    this.retainedBytes=[...this.snapshots.values()].reduce((n,s)=>n+retainedSize.get(s),0);
    // Prefer ordinary history, then revoke old pinned bases rather than allow
    // delayed peers to grow memory without bound. Their next delta recovers.
    for(const allowPinned of [false,true])for(const[tick,snapshot]of this.snapshots){
      if(this.retainedBytes<=this.maxBytes)break;
      if(snapshot===this.current || !allowPinned&&pinned.has(tick))continue;
      this.snapshots.delete(tick);this.retainedBytes-=retainedSize.get(snapshot);
      for(const[owner,held]of this.pins)if(held===tick)this.pins.delete(owner);
    }
  }
  encode({tick=this.current?.tick,baseTick=null,checkpoint=false,relativeVectors=this.relativeVectors}={}) {
    const snapshot=this.snapshots.get(tick),base=baseTick==null?null:this.snapshots.get(baseTick);
    if(!snapshot || baseTick!=null && !base)throw new MissingSnapshotBaseline('Snapshot baseline expired');
    if(base && base.tick>=snapshot.tick)throw Error('Snapshot baseline must precede target');
    return encode(snapshot,base,{epoch:this.epoch,baseTick,checkpoint}, {},relativeVectors,this.writer);
  }
}

export class SnapshotReceiver {
  constructor({maxCheckpoints=8,maxBytes=64*1024*1024}={}) {if(!Number.isFinite(maxBytes)||maxBytes<1||!Number.isInteger(maxCheckpoints)||maxCheckpoints<1)throw Error('Invalid snapshot cache budget');this.maxCheckpoints=maxCheckpoints;this.maxBytes=maxBytes;this.writer=new BinaryWriter();this.reset();}
  reset(){this.epoch=null;this.latest=-1;this.checkpoints=new Map();this.delivered=null;this.activeBaseTick=null;this.retainedBytes=0;}
  prune(){
    const count=()=>{const states=new Set(this.checkpoints.values());if(this.delivered)states.add(this.delivered);return [...states].reduce((n,s)=>n+retainedSize.get(s),0);};
    this.retainedBytes=count();
    for(const allowActive of [false,true])for(const[tick,snapshot]of this.checkpoints){
      if(this.checkpoints.size<=this.maxCheckpoints&&this.retainedBytes<=this.maxBytes)break;
      if(snapshot===this.delivered || !allowActive&&tick===this.activeBaseTick)continue;
      this.checkpoints.delete(tick);this.retainedBytes=count();
    }
  }
  receive(buffer) {
    const {meta}=packetParts(buffer),header=meta.journal;
    if(!header || !Number.isSafeInteger(header.epoch)||!Number.isSafeInteger(meta.tick))throw Error('Invalid journal header');
    if(this.epoch!==null && header.epoch<this.epoch)return null;
    const stale=header.epoch===this.epoch && meta.tick<=this.latest;
    if(stale && !header.checkpoint)return null;
    if(stale && this.checkpoints.has(meta.tick))return {buffer:null,epoch:this.epoch,tick:meta.tick,checkpoint:true};
    const newEpoch=header.epoch!==this.epoch;
    const base=header.baseTick==null?null:!newEpoch&&this.checkpoints.get(header.baseTick);
    if(header.baseTick!=null && !base) {
      // A delayed obsolete checkpoint may reference a baseline already retired
      // after several successful advances. It must not reset current state.
      if(stale)return null;
      throw new MissingSnapshotBaseline('Unknown snapshot checkpoint');
    }
    const reader=new BinaryReader();
    if(base){reader.strings=base.strings.slice();reader.schemas=base.schemas.slice();}
    const packet=reader.read(buffer),snapshot=materialize(packet,base,true);
    if(measureSnapshot(snapshot,buffer.byteLength)>this.maxBytes){this.reset();throw new SnapshotMemoryLimit('One materialized snapshot exceeds the receiver memory budget');}
    if(newEpoch)this.reset();
    this.epoch=header.epoch;
    if(!stale){this.latest=meta.tick;this.activeBaseTick=header.baseTick;}
    if(header.checkpoint || !base) {
      this.checkpoints.set(snapshot.tick,snapshot);
    }
    const acknowledgement={epoch:this.epoch,tick:meta.tick,checkpoint:!!header.checkpoint||!base};
    if(stale){this.prune();return this.checkpoints.has(snapshot.tick)?{buffer:null,...acknowledgement}:null;}
    const result=encode(snapshot,this.delivered,header,{snapshotAck:acknowledgement},false,this.writer);
    this.delivered=snapshot;
    this.prune();
    return {buffer:result,...acknowledgement};
  }
}

function referencedUnits([target,method,args,fire]) {
  const ids=new Set();
  if(target.startsWith('anim:'))ids.add(Number(target.slice(5)));
  if(fire?.playerId!==undefined)ids.add(fire.playerId);
  function visit(value) {
    if(!value || typeof value!=='object')return;
    if(Number.isSafeInteger(value.$u))ids.add(value.$u);
    if(Number.isSafeInteger(value.$c))ids.add(value.$c);
    if(Array.isArray(value.$w))ids.add(value.$w[0]);
    for(const child of Object.values(value))visit(child);
  }
  visit(args);return [...ids];
}

// Reliable events may arrive after a newer state has respawned their actor.
// Preserve announcements; do not animate/play body-bound effects on a new life.
export function snapshotEventCalls(batch,generationOf) {
  const generations=new Map(batch.generations||[]);
  return batch.calls.filter(call=>{
    if(['hud','hs','audio'].includes(call[0]))return true;
    const fire=call[3];
    for(const id of referencedUnits(call)) {
      const expected=fire?.playerId===id?fire.generation:generations.get(id);
      if(expected!==undefined && generationOf(id)!==expected)return false;
    }
    return true;
  });
}
