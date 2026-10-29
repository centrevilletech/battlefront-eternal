import { Vector3 } from 'three';
import { encodeEntityRecord } from './network-entity-codec.js';

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

// Pure presentation: Items.place and the shared model/light helpers are reused.
// No spawn(), update(), watch(), offer(), damage or grant callback runs here.
export class EntityItemPresentation {
 constructor(items,registry){this.items=items;this.registry=registry;this.slots=new Map();this.epoch=null;this.scratch=new Vector3();}
 remove(id){const entry=this.slots.get(id);if(entry?.view?.mesh)this.items.scene.remove(entry.view.mesh);if(entry?.view)this.items.views?.delete(entry.view);if(entry)entry.view=null;}
 reset(epoch){for(const id of this.slots.keys())this.remove(id);this.slots.clear();this.epoch=epoch;}
 receive(record,epoch){
  if(this.epoch!==null&&epoch<this.epoch)return false;if(epoch!==this.epoch)this.reset(epoch);
  const old=this.slots.get(record.id);if(old&&(record.generation<old.record.generation||record.generation===old.record.generation&&(!old.record.active||record.tick<old.record.tick||record.tick===old.record.tick&&record.active)))return false;
  if(!record.active){this.remove(record.id);this.slots.set(record.id,{record,view:null});return true;}
  const definition=this.registry.definitions.get(record.definitionId);if(!definition||!['mine','powerupitem'].includes(definition.base))throw Error('missing item definition');
  let view=old?.view;if(!view||old.record.generation!==record.generation){this.remove(record.id);view={def:definition,mesh:this.items.presentationMesh(definition),position:new Vector3(),previous:new Vector3(),velocity:new Vector3(),up:new Vector3(0,1,0),yaw:record.yaw};this.items.views?.add(view);}
  this.slots.set(record.id,{record,previous:old?.record.active&&old.record.generation===record.generation?old.record:record,view});return true;
 }
 present(tick){
  for(const {record,previous,view}of this.slots.values())if(view){const f=record.tick>previous.tick?Math.max(0,Math.min(1,(tick-previous.tick)/(record.tick-previous.tick))):1;
   view.position.fromArray(previous.position).lerp(this.scratch.fromArray(record.position),f);view.up.fromArray(previous.up).lerp(this.scratch.fromArray(record.up),f).normalize();view.yaw=previous.yaw+Math.atan2(Math.sin(record.yaw-previous.yaw),Math.cos(record.yaw-previous.yaw))*f;
   this.items.place(view);this.items.presentLight(view);
  }
 }
}
