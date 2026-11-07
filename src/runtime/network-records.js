// Changed facts are readonly native leases. Snapshot and display history never
// acquire JS mirrors; these retained views serve existing resource callbacks.
import {NetRecord as R,NetKind as K,NetHeader as H,NetLimits as L,NetLeaseChanges as C} from './network-layout.js';
import {TURRET_STATES} from './network-entity-codec.js';
const kinds=['soldier','droideka','vehicle','turret','item','remote','mount','tombstone','summary','correction','combat','seat'];
class SeatAngle {
  constructor(record,at){this.record=record;this.at=at;}
  get yaw(){return this.record.read(this.at);}get pitch(){return this.record.read(this.at+1);}
}
class SeatAngles {
  constructor(record){this.record=record;this.rows=Array.from({length:16},(_,i)=>new SeatAngle(record,R.SEAT_ANGLES+i*2));}
  get length(){return this.record.read(R.SEAT_COUNT);}
}
for(let i=0;i<16;i++)Object.defineProperty(SeatAngles.prototype,i,{get(){return i<this.length?this.rows[i]:undefined;}});
class SeatLease {
  constructor(record){this.record=record;}
  get vehicleId(){return this.record.read(R.SEAT_VEHICLE_ID);}get vehicleGeneration(){return this.record.read(R.SEAT_VEHICLE_GENERATION);}get seatIndex(){return this.record.read(R.SEAT_INDEX);}
}
const metadata={};
for(const [name,at]of Object.entries({id:R.ID,generation:R.GENERATION,tick:R.TICK,schema:R.SCHEMA}))metadata[name]={enumerable:true,get(){return this.read(at);}};
metadata.kind={enumerable:true,get(){return kinds[this.read(R.KIND)];}};
class RecordView {
  constructor(owner,index){
    Object.defineProperties(this,{owner:{value:owner},index:{value:index},at:{value:index*R.STRIDE},...metadata});
    this.buffer=null;this.positionView=null;this.velocityView=null;this.quaternionView=null;this.upView=null;
    this.angles=new SeatAngles(this);this.lease=new SeatLease(this);this.views();
  }
  read(at){const n=this.owner.network;if(n.disposed)return 0;n.refresh();return n.records[this.at+at];}
  // A cold resource load can grow presentation memory. Addresses remain
  // stable; only that event recreates the four typed vector spans.
  views(){const n=this.owner.network;if(n.disposed)return;n.refresh();const b=n.buffer;if(this.buffer===b)return;this.buffer=b;
    const at=n.records.byteOffset+this.at*8;this.positionView=new Float64Array(b,at+R.POSITION*8,3);this.velocityView=new Float64Array(b,at+R.VELOCITY*8,3);this.quaternionView=new Float64Array(b,at+R.QUATERNION*8,4);this.upView=new Float64Array(b,at+R.UP*8,3);}
  get position(){this.views();return this.positionView;}get velocity(){this.views();return this.velocityView;}get quaternion(){this.views();return this.quaternionView;}get up(){this.views();return this.upView;}
  get state(){const state=this.read(R.STATE);return this.read(R.KIND)===K.TURRET?TURRET_STATES[state]:state;}
  get charge(){return this.read(R.KIND)===K.DROIDEKA?undefined:this.read(R.CHARGE);}
  get slot(){const slot=this.read(R.SLOT);return slot<0?null:slot;}
  get seats(){return this.angles;}
  get seat(){return this.read(R.SEAT_PRESENT)?this.lease:null;}
  // Opaque bytes are borrowed only while their synchronous decoder runs.
  // Deferred records retain the native row, then borrow its bytes on retry.
  get payload(){return this.owner.payload(this);}
  get paired(){const index=this.read(R.PAIR),rows=this.owner.rows;if(index<0||index>=rows.length)return null;const row=rows[index];
    return row.id===this.id&&row.generation===this.generation&&((this.kind==='vehicle'&&row.kind==='mount')||(this.kind==='mount'&&row.kind==='vehicle'))?row:null;}
  retain(){this.owner.change(this,1);}release(){this.owner.change(this,-1);}
}
const scalars={classId:R.CLASS_ID,team:R.TEAM,health:R.HEALTH,yaw:R.YAW,pitch:R.PITCH,power:R.POWER,posture:R.POSTURE,weapon:R.WEAPON,fireCounter:R.FIRE_COUNTER,mode:R.MODE,time:R.TIME,lean:R.LEAN,legs:R.LEGS,shield:R.SHIELD,tilt:R.TILT,walkPhase:R.WALK_PHASE,walkBlend:R.WALK_BLEND,building:R.BUILDING,definitionId:R.DEFINITION_ID,gunYaw:R.GUN_YAW,gunPitch:R.GUN_PITCH};
for(const [name,at]of Object.entries(scalars))Object.defineProperty(RecordView.prototype,name,{get(){return this.read(at);}});
for(const [name,at]of Object.entries({active:R.ACTIVE,reverse:R.REVERSE,dead:R.DEAD,shieldOn:R.SHIELD_ON}))Object.defineProperty(RecordView.prototype,name,{get(){return !!this.read(at);}});
// A mount's roll uses its authored lean lane rather than the body animation
// roll. Keep that distinction at the transport view, without pose arithmetic.
Object.defineProperty(RecordView.prototype,'roll',{get(){return this.read(this.read(R.KIND)===K.MOUNT?R.MOUNT_ROLL:R.ROLL);}});
export class RecordViews {
  constructor(network){this.network=network;this.scratch=new Uint8Array(L.PAYLOAD);this.spans=Array.from({length:L.PAYLOAD+1},(_,n)=>new Uint8Array(this.scratch.buffer,0,n));this.last=-1;this.serial=NaN;this.rows=Array.from({length:L.LEASES},(_,i)=>new RecordView(this,i));}
  payload(record){const n=this.network;if(n.disposed)return this.spans[0];n.refresh();const serial=n.header[H.LEASE_SERIAL],length=n.records[record.at+R.PAYLOAD_LENGTH];
    if(this.last!==record.index||this.serial!==serial){const start=n.records[record.at+R.PAYLOAD_OFFSET];for(let i=0;i<length;i++)this.scratch[i]=n.payloads[start+i];this.last=record.index;this.serial=serial;}
    return this.spans[length];
  }
  // Requests cross with the next packet. The Rust owner applies the whole
  // batch before releasing the preceding packet's borrow or recycling rows.
  change(record,delta){const n=this.network;if(n.disposed)return;n.refresh();const x=n.leaseChanges,count=x[C.COUNT];if(count<L.LEASE_CHANGES){x[count+C.STRIDE]=delta*(record.index+1);x[C.COUNT]=count+1;}else{x[C.COUNT]=L.LEASE_CHANGES+1;}}
}
export const retainRecord=record=>record?.retain?.();
export const releaseRecord=record=>record?.release?.();
// Maps hold resource prerequisites, not a copy of network truth. Their lease
// requests share the receive exchange; no individual retain calls enter WASM.
export class RecordMap extends Map {
  set(key,value){const old=super.get(key);if(old!==value){retainRecord(value);releaseRecord(old);}return super.set(key,value);}
  delete(key){const old=super.get(key);if(old===undefined)return false;releaseRecord(old);return super.delete(key);}
  clear(){this.forEach(releaseRecord);super.clear();}
}

// A packet's publication indices stay native until the next receive. Deferred
// corrections retain the same rows and copy only their small transport header.
export class RecordList {
  constructor(owner,seats,retained=false){this.owner=owner;this.seats=seats;this.ids=retained?new Uint32Array(seats?L.SLOTS*2:L.RECORDS):null;this.count=0;}
  get length(){if(this.ids)return this.count;const n=this.owner.network;n.refresh();return n.header[this.seats?H.SEATS:H.RECORDS];}
  get(index){const n=this.owner.network;n.refresh();const ids=this.ids||(this.seats?n.seats:n.publications);return index<this.length?this.owner.rows[ids[index]]:undefined;}
  capture(source){this.release();const count=Math.min(source.length,this.ids.length);for(let i=0;i<count;i++){const row=source.get(i);this.ids[i]=row.index;row.retain();}this.count=count;}
  release(){for(let i=0;i<this.count;i++)this.owner.rows[this.ids[i]].release();this.count=0;}
  [Symbol.iterator](){return Array.prototype.values.call(this);}
}
for(let i=0;i<L.RECORDS;i++)Object.defineProperty(RecordList.prototype,i,{get(){return this.get(i);}});
export class PacketLease {
  constructor(views){this.tick=NaN;this.epoch=NaN;this.epochChanged=false;this.interval=NaN;this.ackInput=NaN;this.complete=false;this.ack={epoch:NaN,tick:NaN};this.snapshotAck=this.ack;this.records=new RecordList(views,false,true);this.seatChanges=new RecordList(views,true,true);}
  capture(packet){this.records.capture(packet.records);this.seatChanges.capture(packet.seatChanges);this.tick=packet.tick;this.epoch=packet.epoch;this.epochChanged=packet.epochChanged;this.interval=packet.interval;this.ackInput=packet.ackInput;this.complete=packet.complete;this.ack.epoch=packet.ack.epoch;this.ack.tick=packet.ack.tick;}
  release(){this.records.release();this.seatChanges.release();}
}
