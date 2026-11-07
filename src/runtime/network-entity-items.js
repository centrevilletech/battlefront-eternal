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
  const records=[];for(const slot of this.slots)if(slot.record)records.push(slot.record);return records;
 }
}
