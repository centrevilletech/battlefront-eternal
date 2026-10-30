import { Vector3 } from 'three';
import { MINE_SINK } from '../items.js';
import { encodeEntityRecord } from './network-entity-codec.js';
import { DEFAULT_SNAPSHOT_BOUNDS } from './snapshot-codec.js';

// Slots survive removal; only their generation changes when reused. The total
// high-water history is bounded by the mission's actual item pools.
export class EntityItemCapture {
 constructor(items,registry,{capacity=(items.pools?.mine??50)+(items.pools?.powerupitem??25)}={}){
  if(!Number.isSafeInteger(capacity)||capacity<1||capacity>256)throw Error('unsupported item pool capacity');
  this.items=items;this.registry=registry;this.slots=Array.from({length:capacity},()=>({generation:0,object:null,record:null,bytes:null}));this.objects=new Map();
 }
 capture(tick){
  const live=new Set(this.items.items.filter(item=>!item.gone));
  for(const[item,index]of this.objects)if(!live.has(item)){const slot=this.slots[index];slot.object=null;slot.record={kind:'item',id:index,generation:slot.generation,tick,active:false};slot.bytes=null;this.objects.delete(item);}
  for(const item of live){let index=this.objects.get(item);if(index===undefined){index=this.slots.findIndex(slot=>!slot.object);if(index<0)throw Error('item capture capacity exceeded');const slot=this.slots[index];if(slot.generation===0xffffffff)throw Error('item slot generation exhausted');slot.generation++;slot.object=item;this.objects.set(item,index);}
   const slot=this.slots[index],raw=[item.position.x,item.position.y,item.position.z,item.velocity.x,item.velocity.y,item.velocity.z,item.yaw,item.up.x,item.up.y,item.up.z];
   if(slot.record?.active&&slot.record.generation===slot.generation&&slot.raw&&raw.every((n,i)=>n===slot.raw[i]))continue;
   slot.raw=raw;const record={kind:'item',id:index,generation:slot.generation,tick,active:true,definitionId:this.registry.definitionId(item.def),position:item.position.toArray(),velocity:item.velocity.toArray(),yaw:item.yaw,up:item.up.toArray()},bytes=encodeEntityRecord(record);
   if(slot.bytes&&bytes.length===slot.bytes.length&&bytes.every((value,i)=>i>=9&&i<=12||value===slot.bytes[i]))continue;
   slot.record=record;slot.bytes=bytes;
  }
  return this.slots.flatMap(slot=>slot.record?[slot.record]:[]);
 }
}

// A page's items online, flown here by the world's own rules (Items.fly:
// gravity, the ray against this page's collision, bounce and settle) one
// 60 Hz tick at a time as the playout clock crosses ticks, and drawn between
// the last two at the clock's fraction, so a thrown item falls smoothly
// however its records bunch. When the clock reaches a record, a copy further
// off than the records' rounding takes the world's state behind a short
// fading offset; a closer one is nudged halfway, so the rounding averages
// out. The wire has no landed bit: a record at rest (no velocity) lands the
// copy where the world says, so a resting item never drifts or falls through
// ground this page lacks. An item appears when the clock reaches its first
// record and goes when it reaches its removal. This is presentation only:
// no update(), watch(), offer(), damage or grant runs here; the world alone
// decides where an item ends up and who takes it.
const STEP=1/60,SMOOTH=.08,TOLERANCE=.05,SPEED_TOLERANCE=1,NUDGE=.5,MAX_OFFSET=2,JUMP=12,MAX_STEPS=60,MAX_RECORDS=64,MAX_LIVES=4;
const ORIGIN=new Vector3();
// How far a record can round a position (half a code of the snapshot's u16
// per axis), and a little over: a copy at rest closer than this to a record
// at rest lies where the world's does.
const ROUNDING=['xExtent','yExtent','zExtent'].map(key=>DEFAULT_SNAPSHOT_BOUNDS[key]/65535/2+.002);
const rounded=v=>Math.abs(v.x)<=ROUNDING[0]&&Math.abs(v.y)<=ROUNDING[1]&&Math.abs(v.z)<=ROUNDING[2];
// The world's state from a record: at rest is landed; a mine going straight
// down at its sinking speed (within the record's rounding) is sinking.
function follow(item,record){
 item.position.fromArray(record.position);item.previous.copy(item.position);item.velocity.fromArray(record.velocity);item.up.fromArray(record.up).normalize();item.yaw=record.yaw;
 const v=item.velocity;item.landed=v.x===0&&v.y===0&&v.z===0;item.sinking=item.mine&&!item.landed&&v.x===0&&v.z===0&&Math.abs(v.y+MINE_SINK)<.15;
}
export class EntityItemPresentation {
 constructor(items,registry){this.items=items;this.registry=registry;this.slots=new Map();this.epoch=null;this.error=new Vector3();this.shift=new Vector3();this.probe=null;}
 hide(life){const view=life.view;if(!view)return;if(view.mesh)this.items.scene.remove(view.mesh);this.items.views?.delete(view);life.view=null;}
 remove(id){const entry=this.slots.get(id);if(!entry)return;for(const life of entry.lives)this.hide(life);entry.lives.length=0;entry.view=null;}
 reset(epoch){for(const id of this.slots.keys())this.remove(id);this.slots.clear();this.epoch=epoch;}
 // Records wait here for the clock. A slot's lives queue (a slot can be
 // reused inside the playout delay); a removal or the next life's first
 // record ends one.
 receive(record,epoch){
  if(this.epoch!==null&&epoch<this.epoch)return false;if(epoch!==this.epoch)this.reset(epoch);
  const old=this.slots.get(record.id);if(old&&(record.generation<old.record.generation||record.generation===old.record.generation&&(!old.record.active||record.tick<old.record.tick||record.tick===old.record.tick&&record.active)))return false;
  const entry=old??{record,view:null,lives:[]};entry.record=record;this.slots.set(record.id,entry);
  let life=entry.lives.find(value=>value.generation===record.generation);
  if(!record.active){if(life)life.end=Math.min(life.end,record.tick);return true;}
  if(!life){
   const def=this.registry.definitions.get(record.definitionId);if(!def||!['mine','powerupitem'].includes(def.base))throw Error('missing item definition');
   for(const other of entry.lives)other.end=Math.min(other.end,record.tick);
   life={generation:record.generation,def,start:record.tick,end:Infinity,records:[],checked:-Infinity,view:null,sim:null,tick:0,from:new Vector3(),fromUp:new Vector3(),offset:new Vector3(),shown:null};
   entry.lives.push(life);if(entry.lives.length>MAX_LIVES)this.hide(entry.lives.shift());
  }
  life.records.push(record);if(life.records.length>MAX_RECORDS)life.records.shift();
  return true;
 }
 present(tick){
  for(const entry of this.slots.values()){
   const lives=entry.lives;while(lives.length&&tick>=lives[0].end)this.hide(lives.shift());
   const life=lives[0];if(life&&(tick>=life.start||life.view))this.show(life,tick);
   entry.view=lives.find(value=>value.generation===entry.record.generation)?.view??null;
  }
 }
 show(life,tick){
  const base=Math.floor(tick),target=base+1;
  if(life.shown!==null&&tick>life.shown)life.offset.multiplyScalar(Math.exp((life.shown-tick)/(60*SMOOTH)));
  life.shown=tick;
  // First shown, or the clock jumped (a stall, a resume): from the world's
  // newest state at or before it. Before its first record (a clock run
  // back) it holds where it was drawn.
  if(!life.sim||target<life.tick||target-life.tick>JUMP){const record=life.records.findLast(value=>value.tick<=base);if(!record)return;this.seed(life,record);}
  for(const record of life.records){
   if(record.tick<=life.checked)continue;if(record.tick>target)break;
   this.step(life,record.tick);this.reconcile(life,record);life.checked=record.tick;
  }
  this.step(life,target);this.draw(life,tick-base);
  // Records behind the clock: only the newest stays, to start again from.
  while(life.records.length>1&&life.records[1].tick<=life.checked)life.records.shift();
 }
 seed(life,record){
  const items=this.items;
  if(!life.view){life.view=items.itemState(life.def,items.presentationMesh(life.def),ORIGIN,ORIGIN,record.yaw);items.views?.add(life.view);}
  life.sim??=items.itemState(life.def,null,ORIGIN,ORIGIN,record.yaw,life.view.radius);
  follow(life.sim,record);life.tick=life.checked=record.tick;life.from.copy(life.sim.position);life.fromUp.copy(life.sim.up);life.offset.set(0,0,0);
 }
 // Fly the copy on to tick t, keeping where it was a tick before for drawing.
 step(life,t){
  const sim=life.sim;let n=Math.min(t-life.tick,MAX_STEPS);if(n<=0)return;
  if(sim.landed)n=1;
  while(n--){life.from.copy(sim.position);life.fromUp.copy(sim.up);if(!sim.landed)this.items.fly(sim,STEP);}
  life.tick=t;
 }
 // A record the clock has reached, flown on to the copy's tick if it came late.
 reconcile(life,record){
  const sim=life.sim,probe=this.probe??=this.items.itemState(life.def,null,ORIGIN,ORIGIN,0);
  probe.def=life.def;probe.mine=sim.mine;probe.radius=sim.radius;follow(probe,record);
  for(let t=record.tick;t<life.tick&&!probe.landed;t++)this.items.fly(probe,STEP);
  const error=this.error.subVectors(probe.position,sim.position),off=error.length();
  if(probe.landed){if(!sim.landed||!rounded(error)||sim.up.dot(probe.up)<.995)this.adopt(life,probe);return;}
  if(off>TOLERANCE||!sim.landed&&sim.velocity.distanceTo(probe.velocity)>SPEED_TOLERANCE){this.adopt(life,probe);return;}
  if(sim.landed)return;
  sim.position.addScaledVector(error,NUDGE);life.from.addScaledVector(error,NUDGE);life.offset.addScaledVector(error,-NUDGE);
 }
 // The world's state, the jump hidden by an offset that fades over SMOOTH.
 adopt(life,state){
  const sim=life.sim,shift=this.shift.subVectors(sim.position,state.position);
  sim.position.copy(state.position);sim.previous.copy(state.position);sim.velocity.copy(state.velocity);sim.up.copy(state.up);sim.landed=state.landed;sim.sinking=state.sinking;
  life.from.sub(shift);life.offset.add(shift);if(life.offset.length()>MAX_OFFSET)life.offset.set(0,0,0);
 }
 draw(life,f){
  const view=life.view,sim=life.sim;
  view.position.lerpVectors(life.from,sim.position,f).add(life.offset);
  view.up.lerpVectors(life.fromUp,sim.up,f);if(view.up.lengthSq()<1e-8)view.up.copy(sim.up);view.up.normalize();
  view.yaw=sim.yaw;this.items.place(view);this.items.presentLight(view);
 }
}
