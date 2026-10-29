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
export class EntityRemotePresentation {
 constructor(remotes,registry){this.remotes=remotes;this.registry=registry;this.slots=new Map();this.epoch=null;this.scratch=new Vector3();}
 retire(droid){
  const remotes=this.remotes;if(remotes.piloted===droid)remotes.exit();
  if(droid.mesh)remotes.scene.remove(droid.mesh);
  remotes.droids=remotes.droids.filter(value=>value!==droid);remotes.strikes=remotes.strikes.filter(value=>value!==droid);
 }
 reset(epoch){for(const droid of [...this.remotes.droids])this.retire(droid);this.slots.clear();this.epoch=epoch;}
 receive(record,epoch){
  if(this.epoch!==null&&epoch<this.epoch)return false;if(this.epoch===null)this.epoch=epoch;else if(epoch!==this.epoch)this.reset(epoch);
  const old=this.slots.get(record.id);if(old&&(record.generation<old.record.generation||record.generation===old.record.generation&&(!old.record.active||record.tick<old.record.tick||record.tick===old.record.tick&&record.active)))return false;
  const remotes=this.remotes;
  if(remotes.droids.some(droid=>droid.networkSlot===record.id&&droid.networkGeneration>record.generation))return false;
  const def=record.active?this.registry.definitions.get(record.definitionId):null;if(record.active&&(!def||def.base!=='droid'))throw Error('missing remote definition');
  const existing=remotes.droids.find(droid=>droid.networkSlot===record.id&&droid.networkGeneration===record.generation);if(record.active&&existing&&existing.def!==def)throw Error('remote identity definition changed');
  for(const droid of [...remotes.droids])if(droid.networkSlot===record.id&&(!record.active||droid.networkGeneration!==record.generation))this.retire(droid);
  if(!record.active){this.slots.set(record.id,{record,view:null});return true;}
  let view=remotes.droids.find(droid=>droid.networkSlot===record.id&&droid.networkGeneration===record.generation);
  if(view&&view.def!==def)throw Error('remote identity definition changed');
  if(!view){view=remotes.createDroid(def,null,new Vector3().fromArray(record.position),new Vector3().fromArray(record.velocity),record.yaw,record.team);view.networkSlot=record.id;view.networkGeneration=record.generation;remotes.droids.push(view);}
  this.slots.set(record.id,{record,previous:old?.record.active&&old.record.generation===record.generation?old.record:record,view});return true;
 }
 present(tick){
  for(const {record,previous,view}of this.slots.values())if(view&&this.remotes.piloted!==view){
   const f=record.tick>previous.tick?Math.max(0,Math.min(1,(tick-previous.tick)/(record.tick-previous.tick))):1;
   view.position.fromArray(previous.position).lerp(this.scratch.fromArray(record.position),f);view.velocity.fromArray(record.velocity);view.yaw=previous.yaw+Math.atan2(Math.sin(record.yaw-previous.yaw),Math.cos(record.yaw-previous.yaw))*f;view.pitch=previous.pitch+(record.pitch-previous.pitch)*f;view.health=record.health;view.team=record.team;
   this.remotes.place(view);
  }
 }
}
