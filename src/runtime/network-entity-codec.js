import { NETWORK_CONTRACT } from './network-contract.js';
// Required online entity-state-v1 record codec; one fixed protocol contract.
// Each message is independently decodable; absent records never mean deletion.
export const ENTITY_PROTOCOL = NETWORK_CONTRACT.profile;
export const ENTITY_PACKET_BYTES = 1100;
export const ENTITY_POSTURES=Object.freeze(['stand','crouch','prone']);
// A killed body can still be airborne; death alone does not imply rest.
export const ENTITY_SOLDIER_STATES=Object.freeze(['ground','jump','fall','thrown','jet','diveRight','diveLeft','dead','thrownDead']);
const MAGIC=0x31454e42,HEADER=30;
const kinds={soldier:1,vehicle:2,tombstone:3,seat:4,correction:5,summary:6,mount:7,combat:8,item:9,remote:10,turret:11,droideka:12};
// mount: a hull's crew pose (lean, walk cycle, slot, per-seat aim) beside its
// quantized vehicle record. turret: a stationary armed building's drum, gun and
// state, with the live-object index the client resolves it by. Both are
// presentation and seat-ownership inputs; neither carries replay state.
export const MOUNT_MAX_SEATS=16, TURRET_STATES=Object.freeze(['unbuilt','built','destroyed']);
const ANGLE=32767/Math.PI,wrapAngle=v=>((v+Math.PI)%(2*Math.PI)+2*Math.PI)%(2*Math.PI)-Math.PI;
const names=Object.fromEntries(Object.entries(kinds).map(([k,v])=>[v,k]));
const integer=(v,min,max)=>{if(!Number.isSafeInteger(v)||v<min||v>max)throw Error('entity integer out of range');return v;};
class Writer {
 constructor(){this.bytes=new Uint8Array(ENTITY_PACKET_BYTES);this.view=new DataView(this.bytes.buffer);this.offset=0;}
 reserve(n){if(this.offset+n>this.bytes.length)throw Error('entity record cannot fit one message');}
 raw(bytes){this.reserve(bytes.length);this.bytes.set(bytes,this.offset);this.offset+=bytes.length;}
 finish(){return this.bytes.slice(0,this.offset);}
 put(type,value){const n={Uint8:1,Uint16:2,Uint32:4,Int16:2,Int32:4,Float32:4,Float64:8}[type];this.reserve(n);this.view['set'+type](this.offset,value,true);this.offset+=n;}
 u8(v){this.put('Uint8',integer(v,0,255));}u16(v){this.put('Uint16',integer(v,0,65535));}u32(v){this.put('Uint32',integer(v,0,0xffffffff));}
 quant(v,scale,bits){if(!Number.isFinite(v))throw Error('nonfinite entity field');const q=Math.round(v*scale),max=2**(bits-1)-1;integer(q,-max,max);this.put('Int'+bits,q);}
 vec(v,scale,bits,n=3){if(!Array.isArray(v)||v.length!==n)throw Error('invalid entity vector');for(const x of v)this.quant(x,scale,bits);}
}
class Reader {
 constructor(bytes){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.offset=0;}
 get(type){const n={Uint8:1,Uint16:2,Uint32:4,Int16:2,Int32:4,Float32:4}[type];if(this.offset+n>this.bytes.length)throw Error('truncated entity packet');const v=this.view['get'+type](this.offset,true);this.offset+=n;return v;}
 u8(){return this.get('Uint8');}u16(){return this.get('Uint16');}u32(){return this.get('Uint32');}
 vec(scale,bits,n=3){return Array.from({length:n},()=>this.get('Int'+bits)/scale);}
}
// The size half of encoding a correction record (kind, id, generation,
// tick, schema, length = 17 bytes before the payload), for callers that
// only need the fit guarantee encodeEntityRecord throws on, not the bytes.
export function checkCorrectionRecordFits(payload){
 if(!(payload instanceof Uint8Array))throw Error('invalid correction payload');
 if(17+payload.length>ENTITY_PACKET_BYTES-HEADER-2)throw Error('entity record cannot fit one message');
}
export function encodeEntityRecord(record){
 const w=new Writer(),kind=kinds[record.kind];if(!kind)throw Error('unknown entity kind');
 w.u8(kind);w.u32(record.id);w.u32(record.generation);w.u32(record.tick);
 if(record.kind==='soldier'||record.kind==='vehicle'||record.kind==='droideka'){
  w.u16(record.classId);w.u8(record.team);w.vec(record.position,256,32);w.vec(record.velocity,32,16);
  if(record.kind==='soldier'||record.kind==='droideka'){
   if(!Number.isFinite(record.yaw))throw Error('nonfinite entity yaw');
   // The posture byte's upper six bits carry a soldier's charge: 0 while not
   // charging, else the held share of his primary's MaxPressedTime in 1..63
   // (src/weapons.js chargeStep).
   const yaw=((record.yaw+Math.PI)%(2*Math.PI)+2*Math.PI)%(2*Math.PI)-Math.PI;w.quant(yaw,32767/Math.PI,16);w.quant(record.pitch,32767/Math.PI,16);w.u16(record.health);w.u8(integer(record.posture,0,3)|integer(record.kind==='soldier'?record.charge??0:0,0,63)<<2);w.u8(record.state);w.u16(record.weapon);w.u32(record.fireCounter);
   if(record.kind==='droideka'){
    w.u8(integer(record.mode,0,13));if(record.shield<0)throw Error('invalid droideka shield');
    for(const key of ['reverse','dead','shieldOn'])if(typeof record[key]!=='boolean')throw Error('invalid droideka flag');
    w.u8(Number(record.reverse)|Number(record.dead)<<1|Number(record.shieldOn)<<2);
    for(const key of ['time','roll','lean','legs','shield']){if(!Number.isFinite(record[key])||Math.abs(record[key])>3.402823466e38)throw Error('nonfinite droideka pose');w.put('Float32',record[key]);}
   }
  }else{
   if(Math.abs(Math.hypot(...record.quaternion)-1)>.001)throw Error('vehicle quaternion must be normalized');
   w.vec(record.quaternion,32767,16,4);w.u16(record.health);w.u8(record.state);w.u16(record.power);
  }
 }else if(record.kind==='item'){
  integer(record.id,0,255);if(typeof record.active!=='boolean')throw Error('invalid item state');w.u8(record.active?1:0);
  if(record.active){if(!Array.isArray(record.up)||record.up.length!==3||!record.up.every(Number.isFinite)||Math.abs(Math.hypot(...record.up)-1)>.001)throw Error('invalid item surface normal');w.u16(record.definitionId);w.vec(record.position,256,32);w.vec(record.velocity,32,16);w.quant(Math.atan2(Math.sin(record.yaw),Math.cos(record.yaw)),32767/Math.PI,16);w.vec(record.up,32767,16);}
 }else if(record.kind==='remote'){
  integer(record.id,0,4095);if(typeof record.active!=='boolean')throw Error('invalid remote state');w.u8(record.active?1:0);
  if(record.active){w.u16(record.definitionId);w.u8(record.team);w.vec(record.position,256,32);w.vec(record.velocity,32,16);w.quant(Math.atan2(Math.sin(record.yaw),Math.cos(record.yaw)),32767/Math.PI,16);w.quant(record.pitch,32767/Math.PI,16);w.u16(record.health);}
 }else if(record.kind==='seat'){
  w.u32(record.vehicleId);w.u32(record.vehicleGeneration);w.u8(record.seatIndex);w.u32(record.leaseVersion);
 }else if(record.kind==='mount'){
  if(!Array.isArray(record.seats)||record.seats.length>MOUNT_MAX_SEATS)throw Error('invalid mount seats');
  w.u8(record.slot??255);w.quant(wrapAngle(record.tilt),ANGLE,16);w.quant(wrapAngle(record.roll),ANGLE,16);
  w.u16(Math.round(Math.max(0,Math.min(65.535,record.walkPhase||0))*1000));w.u8(Math.round(Math.max(0,Math.min(1,record.walkBlend||0))*255));
  w.u8(record.seats.length);for(const seat of record.seats){if(!Number.isFinite(seat.yaw)||!Number.isFinite(seat.pitch))throw Error('nonfinite seat aim');w.quant(wrapAngle(seat.yaw),ANGLE,16);w.quant(wrapAngle(seat.pitch),ANGLE,16);}
 }else if(record.kind==='turret'){
  const state=TURRET_STATES.indexOf(record.state);if(state<0)throw Error('unknown turret state');
  w.u16(record.building);w.u8(state);w.u8(record.team);w.u16(record.health);
  for(const key of ['yaw','pitch','gunYaw','gunPitch']){if(!Number.isFinite(record[key]))throw Error('nonfinite turret aim');w.quant(wrapAngle(record[key]),ANGLE,16);}
 }else if(record.kind==='correction'||record.kind==='summary'||record.kind==='combat'){
  // Exact replay and world-summary payloads have explicit bounded schemas.
  // Remote quantized poses must never substitute for replay corrections.
  w.u16(record.schema);if(!(record.payload instanceof Uint8Array))throw Error('invalid correction payload');
  w.u16(record.payload.length);w.raw(record.payload);
 }
 if(w.offset>ENTITY_PACKET_BYTES-HEADER-2)throw Error('entity record cannot fit one message');
 return w.finish();
}
function decodeRecord(bytes){
 const r=new Reader(bytes),kind=names[r.u8()];if(!kind)throw Error('unknown entity kind');
 const x={kind,id:r.u32(),generation:r.u32(),tick:r.u32()};
 if(kind==='soldier'||kind==='vehicle'||kind==='droideka'){
  Object.assign(x,{classId:r.u16(),team:r.u8(),position:r.vec(256,32),velocity:r.vec(32,16)});
  if(kind==='soldier'||kind==='droideka'){Object.assign(x,{yaw:r.get('Int16')*Math.PI/32767,pitch:r.get('Int16')*Math.PI/32767,health:r.u16(),posture:r.u8(),state:r.u8(),weapon:r.u16(),fireCounter:r.u32()});if(kind==='soldier')x.charge=x.posture>>2;else if(x.posture>3)throw Error('invalid droideka posture');x.posture&=3;}
  else {Object.assign(x,{quaternion:r.vec(32767,16,4),health:r.u16(),state:r.u8(),power:r.u16()});if(Math.abs(Math.hypot(...x.quaternion)-1)>.001)throw Error('invalid decoded quaternion');}

  if(kind==='droideka'){x.mode=r.u8();const flags=r.u8();if(x.mode>13||flags>7)throw Error('invalid droideka pose');x.reverse=!!(flags&1);x.dead=!!(flags&2);x.shieldOn=!!(flags&4);for(const key of ['time','roll','lean','legs','shield']){x[key]=r.get('Float32');if(!Number.isFinite(x[key]))throw Error('nonfinite droideka pose');}if(x.shield<0)throw Error('invalid droideka shield');}
 }else if(kind==='item'){
  if(x.id>255)throw Error('item slot outside bound');const active=r.u8();if(active>1)throw Error('invalid item state');x.active=!!active;
  if(x.active){Object.assign(x,{definitionId:r.u16(),position:r.vec(256,32),velocity:r.vec(32,16),yaw:r.get('Int16')*Math.PI/32767,up:r.vec(32767,16)});if(Math.abs(Math.hypot(...x.up)-1)>.001)throw Error('invalid item surface normal');}
 }else if(kind==='remote'){
  if(x.id>4095)throw Error('remote slot outside bound');const active=r.u8();if(active>1)throw Error('invalid remote state');x.active=!!active;
  if(x.active)Object.assign(x,{definitionId:r.u16(),team:r.u8(),position:r.vec(256,32),velocity:r.vec(32,16),yaw:r.get('Int16')*Math.PI/32767,pitch:r.get('Int16')*Math.PI/32767,health:r.u16()});
 }else if(kind==='seat')Object.assign(x,{vehicleId:r.u32(),vehicleGeneration:r.u32(),seatIndex:r.u8(),leaseVersion:r.u32()});
 else if(kind==='mount'){
  const slot=r.u8();Object.assign(x,{slot:slot===255?null:slot,tilt:r.get('Int16')/ANGLE,roll:r.get('Int16')/ANGLE,walkPhase:r.u16()/1000,walkBlend:r.u8()/255});
  const count=r.u8();if(count>MOUNT_MAX_SEATS)throw Error('invalid mount seats');x.seats=Array.from({length:count},()=>({yaw:r.get('Int16')/ANGLE,pitch:r.get('Int16')/ANGLE}));
 }else if(kind==='turret'){
  x.building=r.u16();const state=r.u8();if(state>=TURRET_STATES.length)throw Error('unknown turret state');x.state=TURRET_STATES[state];x.team=r.u8();x.health=r.u16();
  for(const key of ['yaw','pitch','gunYaw','gunPitch'])x[key]=r.get('Int16')/ANGLE;
 }
 else if(kind==='correction'||kind==='summary'||kind==='combat'){x.schema=r.u16();const n=r.u16();if(r.offset+n>bytes.length)throw Error('truncated correction');x.payload=bytes.slice(r.offset,r.offset+n);r.offset+=n;}
 if(r.offset!==bytes.length)throw Error('trailing entity record data');return x;
}
const preparedBytes=new WeakMap();
const freezeRecord=value=>{if(value&&typeof value==='object'&&!ArrayBuffer.isView(value)){for(const child of Object.values(value))freezeRecord(child);Object.freeze(value);}return value;};
// Prepared handles own their encoded bytes. Sharing one handle across recipient
// schedulers cannot mutate a caller's record or re-encode it per recipient.
function cloneRecord(record){
 // Wire records contain scalar fields, numeric vectors and byte payloads.
 // Keep unusual extension objects fully isolated too, using the general path.
 const snapshot={};
 for(const [key,value]of Object.entries(record)){
  if(value instanceof Uint8Array)snapshot[key]=value.slice();
  else if(Array.isArray(value)&&value.every(x=>x===null||typeof x!=='object'))snapshot[key]=value.slice();
  else if(value!==null&&typeof value==='object')return structuredClone(record);
  else snapshot[key]=value;
 }
 return snapshot;
}
export function prepareEntityRecord(record){if(preparedBytes.has(record))return record;const snapshot=freezeRecord(cloneRecord(record)),bytes=encodeEntityRecord(snapshot),handle=Object.freeze({record:snapshot,byteLength:bytes.length});preparedBytes.set(handle,bytes);return handle;}
export function sameEntityPayload(a,b){const x=preparedBytes.get(a),y=preparedBytes.get(b);if(!x||!y||x.length!==y.length)return false;for(let i=0;i<x.length;i++)if((i<9||i>12)&&x[i]!==y[i])return false;return true;}
export function encodePreparedEntityPacket({epoch,packetId,tick,ackInput=0,ackActions=0,ackFire=0,records},maxBytes=ENTITY_PACKET_BYTES){
 integer(maxBytes,HEADER,ENTITY_PACKET_BYTES);const w=new Writer();w.u32(MAGIC);w.u32(epoch);w.u32(packetId);w.u32(tick);w.u32(ackInput);w.u32(ackActions);w.u32(ackFire);w.u16(records.length);
 for(const record of records){const bytes=preparedBytes.get(record);if(!bytes)throw Error('invalid prepared entity record');if(record.record.tick>tick)throw Error('record ahead of packet');w.u16(bytes.length);w.raw(bytes);}
 if(w.offset>maxBytes)throw Error('entity packet exceeds budget');return w.finish();
}
export function encodeEntityPacket(packet,maxBytes=ENTITY_PACKET_BYTES){return encodePreparedEntityPacket({...packet,records:packet.records.map(prepareEntityRecord)},maxBytes);}
export function decodeEntityPacket(bytes){
 if(!(bytes instanceof Uint8Array)||bytes.length>ENTITY_PACKET_BYTES)throw Error('invalid entity packet');
 const r=new Reader(bytes);if(r.u32()!==MAGIC)throw Error('entity protocol mismatch');
 const p={epoch:r.u32(),packetId:r.u32(),tick:r.u32(),ackInput:r.u32(),ackActions:r.u32(),ackFire:r.u32(),records:[]},count=r.u16();
 for(let i=0;i<count;i++){const n=r.u16();if(r.offset+n>bytes.length)throw Error('truncated entity record');const x=decodeRecord(bytes.subarray(r.offset,r.offset+n));if(x.tick>p.tick)throw Error('record ahead of packet');p.records.push(x);r.offset+=n;}
 if(r.offset!==bytes.length)throw Error('trailing entity packet data');return p;
}
export const entityPacketHeaderBytes=HEADER;
