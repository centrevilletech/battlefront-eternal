import {retainRecord,releaseRecord} from './network-records.js';
import {Vector3} from 'three';

// Read the same per-owner controllers that the authoritative world advances.
// Slot generations and launch limits belong to NetworkRemoteWorld.
export class EntityRemoteCapture {
 constructor(game,registry){this.game=game;this.registry=registry;this.records=new Map();}
 capture(tick){
  const world=this.game.networkRemotes;if(!world)return [];
  for(const {droid}of world.droids())if(!droid.dead)world.identityFor(droid);
  for(let id=0;id<world.slots.length;id++){
   const {droid,generation}=world.slots[id],old=this.records.get(id);
   if(!droid)continue;
   if(droid.dead){if(old?.generation!==generation||old.active)this.records.set(id,{kind:'remote',id,generation,tick,active:false});continue;}
   this.records.set(id,{kind:'remote',id,generation,tick,active:true,definitionId:this.registry.definitionId(droid.def),team:droid.team,position:droid.position.toArray(),velocity:droid.velocity.toArray(),yaw:droid.yaw,pitch:droid.pitch,health:Math.max(0,Math.min(65535,Math.round(droid.health)))});
  }
  return [...this.records.values()];
 }
}

// Shared model construction and placement only. Never advance these proxies
// through RemoteDroids.update: that would run triggers, weapons and health drain.
// Each life keeps its recent records and is drawn between the two that
// bracket the playout clock (the first until the clock reaches it); past
// the newest it goes on at its velocity for at most 6 ticks, then holds.
const SAMPLES=16,EXTRAPOLATE=6;
const angle=(a,b,t)=>a+Math.atan2(Math.sin(b-a),Math.cos(b-a))*t;
// Each remote resource owns a bounded history of native record leases.
// Rotating that ring changes references without allocating record arrays.
class Samples {
 constructor(){this.rows=new Array(SAMPLES).fill(null);this.head=0;this.length=0;}
 get(index){return this.rows[(this.head+index)%SAMPLES];}
 push(record){retainRecord(record);if(this.length===SAMPLES){releaseRecord(this.rows[this.head]);this.rows[this.head]=record;this.head=(this.head+1)%SAMPLES;}else this.rows[(this.head+this.length++)%SAMPLES]=record;}
 clear(){for(let i=0;i<this.length;i++)releaseRecord(this.get(i));this.rows.fill(null);this.head=0;this.length=0;}
 trim(){while(this.length>1){releaseRecord(this.rows[this.head]);this.rows[this.head]=null;this.head=(this.head+1)%SAMPLES;this.length--;}}
}
const releaseRemote=entry=>{releaseRecord(entry.record);entry.samples?.clear();};
const trimRemote=entry=>entry.samples?.trim();
export class EntityRemotePresentation {
 constructor(remotes,registry){this.remotes=remotes;this.registry=registry;this.slots=new Map();this.epoch=null;this.scratch=new Vector3();}
 retire(droid){
  const remotes=this.remotes;if(remotes.piloted===droid)remotes.exit();
  if(droid.mesh)remotes.scene.remove(droid.mesh);
  remotes.droids=remotes.droids.filter(value=>value!==droid);remotes.strikes=remotes.strikes.filter(value=>value!==droid);
 }
 reset(epoch){for(const droid of [...this.remotes.droids])this.retire(droid);this.slots.forEach(releaseRemote);this.slots.clear();this.epoch=epoch;}
 trimHistory(){this.slots.forEach(trimRemote);}
 receive(record,epoch){
  if(this.epoch!==null&&epoch<this.epoch)return false;if(this.epoch===null)this.epoch=epoch;else if(epoch!==this.epoch)this.reset(epoch);
  const old=this.slots.get(record.id);if(old&&(record.generation<old.record.generation||record.generation===old.record.generation&&(!old.record.active||record.tick<old.record.tick||record.tick===old.record.tick&&record.active)))return false;
  const remotes=this.remotes;
  for(const droid of remotes.droids)if(droid.networkSlot===record.id&&droid.networkGeneration>record.generation)return false;
  const def=record.active?this.registry.definitions.get(record.definitionId):null;if(record.active&&(!def||def.base!=='droid'))throw Error('missing remote definition');
  let existing=null;for(const droid of remotes.droids)if(droid.networkSlot===record.id&&droid.networkGeneration===record.generation){existing=droid;break;}if(record.active&&existing&&existing.def!==def)throw Error('remote identity definition changed');
  for(let i=remotes.droids.length-1;i>=0;i--){const droid=remotes.droids[i];if(droid.networkSlot===record.id&&(!record.active||droid.networkGeneration!==record.generation))this.retire(droid);}
  if(!record.active){retainRecord(record);if(old){releaseRemote(old);old.record=record;old.samples=null;old.view=null;}else this.slots.set(record.id,{record,samples:null,view:null});return true;}
  let view=existing;
  if(view&&view.def!==def)throw Error('remote identity definition changed');
  if(!view){view=remotes.createDroid(def,null,new Vector3().fromArray(record.position),new Vector3().fromArray(record.velocity),record.yaw,record.team);view.networkSlot=record.id;view.networkGeneration=record.generation;remotes.droids.push(view);}
  const same=old?.record.active&&old.record.generation===record.generation;const samples=same?old.samples:new Samples();if(old&&!same)old.samples?.clear();samples.push(record);retainRecord(record);
  if(old){releaseRecord(old.record);old.record=record;old.samples=samples;old.view=view;}else this.slots.set(record.id,{record,samples,view});return true;
 }
 present(tick){
  for(const {samples,view}of this.slots.values())if(view&&this.remotes.piloted!==view){
   let a=samples.get(0),b=a;for(let i=0;i<samples.length;i++){const sample=samples.get(i);if(sample.tick<=tick)a=b=sample;else{b=sample;break;}}
   const f=b.tick>a.tick?Math.max(0,Math.min(1,(tick-a.tick)/(b.tick-a.tick))):0,extra=a===b&&tick>a.tick&&a.health>0?Math.min(EXTRAPOLATE,tick-a.tick)/60:0;
   view.position.fromArray(a.position).lerp(this.scratch.fromArray(b.position),f).addScaledVector(this.scratch.fromArray(a.velocity),extra);
   view.velocity.fromArray(a.velocity).lerp(this.scratch.fromArray(b.velocity),f);view.yaw=angle(a.yaw,b.yaw,f);view.pitch=a.pitch+(b.pitch-a.pitch)*f;view.health=a.health;view.team=a.team;
   this.remotes.place(view);
  }
 }
}
