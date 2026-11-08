import * as THREE from 'three';
import {sceneTransform} from './rust-render/scene-changes.js';
import {SIM_WASM} from './sim-wasm.js';
import {simImports} from './sim/module.js';
import {emitSound} from './audio/events.js';
import {legacyRandom} from './ai/random.js';
import {FLIGHT_CLASSES} from './flight/config.js';
import {OrdnanceHostField_NAMES,OrdnanceHostExtra_NAMES,OrdnanceHostAuthored_NAMES,OrdnanceHostWord_NAMES,
  OrdnanceHostWorld_NAMES,OrdnanceHostItems_NAMES,OrdnanceHostOwner_VALUE as O,
  OrdnanceHostInvokeLayout as I,OrdnanceHostValueLayout as V,OrdnanceHostArgumentLayout as A,
  OrdnanceHostShotLayout as P,OrdnanceHostPublication_NAMES,OrdnanceHostUnitLayout as U,
  ORDNANCE_HOST_EXTRA_BASE as EXTRA,ORDNANCE_HOST_AUTHORED_BASE as AUTHORED} from './sim/gen/ordnance-host.js';
import {ClientShotRecordMirror,syncClientShotRecord,ClientShotRecordLayout as R} from './sim/gen/client-shots.js';
const hosts=new WeakMap();let isolated;
const releases=new FinalizationRegistry(r=>r.x.client_shots_drop(r.handle));
// Resource identities and ABI conversion only. Every operation and decision is
// made by sim::ordnance::control or sim::client::shot_host.
export function resourceHost(collision){
  let instance=collision?.rays?.instance;
  if(!instance){if(!isolated){const holder={};const bytes=Uint8Array.fromBase64?Uint8Array.fromBase64(SIM_WASM):Uint8Array.from(atob(SIM_WASM),c=>c.charCodeAt(0));isolated=new WebAssembly.Instance(new WebAssembly.Module(bytes),simImports(holder));isolated.holder=holder;}instance=isolated;}
  let host=hosts.get(instance.exports);if(!host){host=new Resources(instance.exports);instance.holder.ordnanceResources=host.io;hosts.set(instance.exports,host);}return host;
}
export function bindResources(Class,owner,names){
  for(let op=0;op<names.length;op++)Class.prototype[names[op]]=function(a,b,c,d,e,f,g,h,i,j){return this.resources.run(owner,op,this,a,b,c,d,e,f,g,h,i,j);};
}
class Resources {
  constructor(x){
    this.x=x;this.at=x.ordnance_host_input()/8;this.keys=OrdnanceHostField_NAMES;this.extra=OrdnanceHostExtra_NAMES;this.authored=OrdnanceHostAuthored_NAMES;
    this.refs=[undefined,null,false,true,'',...OrdnanceHostWord_NAMES];this.primitives=new Map();for(let i=0;i<this.refs.length;i++)this.primitives.set(this.refs[i],i);
    this.objects=new WeakMap();this.generations=new Uint16Array(65536);this.firstSlot=this.refs.length;this.cursor=this.firstSlot;
    this.scopes=Array.from({length:16},()=>new Array(10));this.depth=0;this.args=Array.from({length:13},(_,n)=>new Array(n));
    this.points=Array.from({length:32},()=>new THREE.Vector3());this.hits=Array.from({length:8},()=>({distance:0,point:new THREE.Vector3(),normal:new THREE.Vector3(),body:null,unit:null,part:null,terrain:false}));
    this.emitSound=emitSound;this.legacySpin=legacyRandom('grenade-spin');this.legacyWaver=legacyRandom('missile-waver');this.legacyDrop=legacyRandom('item-drop');this.flightClasses=FLIGHT_CLASSES;
    this.io={resource_kind:o=>{const v=this.object(o);return v===undefined?0:v===null?1:typeof v==='boolean'?2:typeof v==='number'?3:typeof v==='string'?5:4;},reference:(o,k)=>this.ref(this.object(o)?.[this.key(k)]),number:(o,k)=>Number(this.object(o)?.[this.key(k)]),boolean:(o,k)=>Number(Boolean(this.object(o)?.[this.key(k)])),
      index:(o,i)=>this.ref(this.object(o)?.[i]),numeric_index:(o,i)=>Number(this.object(o)?.[i]),dynamic:(o,k)=>this.ref(this.object(o)?.[this.object(k)]),
      read_vector:(o,p)=>{const v=this.object(o),f=this.memory(),at=p/8;f[at]=v?.x;f[at+1]=v?.y;f[at+2]=v?.z;},
      read_matrix:(o,p)=>{const m=this.object(o)?.elements,f=this.memory(),at=p/8;for(let i=0;i<16;i++)f[at+i]=m?.[i];},
      write_vector:(o,p)=>{const v=this.object(o),f=this.memory(),at=p/8;if(v){v.set(f[at],f[at+1],f[at+2]);sceneTransform(v);}},
      property:(o,k,t,v)=>{const target=this.object(o);if(target)target[this.key(k)]=this.value(t,v);},
      invoke:(o,k,p,n,numeric)=>{const target=this.object(o),args=this.args[n],f=this.memory(),at=p/8;for(let i=0;i<n;i++)args[i]=this.value(f[at+i*A.words+A.kind],f[at+i*A.words+A.value]);const result=target?.[this.key(k)]?.apply(target,args);return numeric?Number(result):this.ref(result);},
      construct:(kind,p)=>{const f=this.memory(),at=p/8;return this.ref(kind===1?new THREE.Vector3(f[at],f[at+1],f[at+2]):kind===2?[]:{});},
      text:(o,p,n)=>{const s=this.object(o),out=new Uint16Array(x.memory.buffer,p,n);for(let i=0;i<n&&i<s?.length;i++)out[i]=s.charCodeAt(i);return s?.length??0;},
      intern:(p,n)=>{const u=new Uint16Array(x.memory.buffer,p,n);let s='';for(let i=0;i<n;i++)s+=String.fromCharCode(u[i]);return this.ref(s);},
      publish:(root,p,n)=>this.publish(this.object(root),p/8,n),clock:()=>0,
      stable:id=>this.ref(this.object(id)),scratch_vector:(i,p)=>{const v=this.points[i],f=this.memory(),at=p/8;v.set(f[at],f[at+1],f[at+2]);return this.ref(v);},scratch_hit:i=>this.ref(this.hits[i]),
      refresh:id=>this.object(id)?.updateWorldMatrix?.(true,false),unit:(id,p)=>this.unit(id,p/8)};
  }
  memory(){if(this.buffer!==this.x.memory.buffer){this.buffer=this.x.memory.buffer;this.words=new Float64Array(this.buffer);}return this.words;}
  key(k){return k>=AUTHORED?this.authored[k-AUTHORED]:k>=EXTRA?this.extra[k-EXTRA]:this.keys[k];}
  object(id){if(id>>>31)return this.scopes[(id>>>8)&15][id&255];const slot=id&65535;if(this.generations[slot]!==id>>>16)return undefined;const value=this.refs[slot];return value instanceof WeakRef?value.deref():value;}
  ref(value){
    if(value===undefined)return 0;const object=value!==null&&(typeof value==='object'||typeof value==='function'),map=object?this.objects:this.primitives;
    const known=map.get(value);if(known!==undefined)return known;
    for(let n=0;n<65536;n++){const slot=this.cursor++&65535,old=this.refs[slot];if(slot<this.firstSlot)continue;if(old!==undefined&&(!(old instanceof WeakRef)||old.deref()!==undefined))continue;if(this.generations[slot]===32767)continue;
      if(old!==undefined)this.generations[slot]++;const id=(this.generations[slot]*65536+slot)>>>0;this.refs[slot]=object?new WeakRef(value):value;map.set(value,id);return id;}
    return 0;
  }
  value(kind,value){return kind===0?undefined:kind===1?null:kind===2?Boolean(value):kind===3?value:this.object(value);}
  argument(value,at,index){const f=this.memory();if(value===undefined){f[at+V.kind]=0;f[at+V.value]=0;}else if(value===null){f[at+V.kind]=1;f[at+V.value]=0;}else if(typeof value==='boolean'){f[at+V.kind]=2;f[at+V.value]=Number(value);}else if(typeof value==='number'){f[at+V.kind]=3;f[at+V.value]=value;}else{this.scopes[this.depth][index]=value;f[at+V.kind]=4;f[at+V.value]=(0x80000000|(this.depth<<8)|index)>>>0;}}
  run(owner,op,root,a,b,c,d,e,f,g,h,i,j){
    if(this.depth===16)return undefined;const words=this.memory(),at=this.at;words[at+I.owner]=owner;words[at+I.op]=op;words[at+I.root]=this.ref(root);
    this.argument(a,at+I.a0,0);this.argument(b,at+I.a1,1);this.argument(c,at+I.a2,2);this.argument(d,at+I.a3,3);this.argument(e,at+I.a4,4);this.argument(f,at+I.a5,5);this.argument(g,at+I.a6,6);this.argument(h,at+I.a7,7);this.argument(i,at+I.a8,8);this.argument(j,at+I.a9,9);
    const scope=this.scopes[this.depth++];try{const out=this.x.ordnance_host_run()/8,view=this.memory();return this.value(view[out+V.kind],view[out+V.value]);}finally{this.depth--;scope.fill(undefined);}
  }
  unit(id,at){const u=this.object(id),s=u?.seat,b=u?.sphere,f=this.memory();f[at+U.alive]=Number(Boolean(u?.alive));f[at+U.dead]=Number(Boolean(u?.dead));f[at+U.removed]=Number(Boolean(u?.removed));f[at+U.shapes]=this.ref(u?.hitShapes);f[at+U.seat]=this.ref(s);f[at+U.exposed]=Number(Boolean(s?.exposed));f[at+U.vehicle]=this.ref(s?.vehicle);f[at+U.vehicleBody]=this.ref(s?.vehicle?.body);f[at+U.turretBody]=this.ref(s?.turret?.bodies?.built);f[at+U.center]=b?.center?.x;f[at+U.center+1]=b?.center?.y;f[at+U.center+2]=b?.center?.z;f[at+U.radius]=b?.radius;}
  release(root){releases.unregister(root);this.x.client_shots_drop(root.nativeHandle);}
  attach(root){root.nativeHandle=this.x.client_shots_create();releases.register(root,{x:this.x,handle:root.nativeHandle},root);}
  publish(root,at,count){const f=this.memory();for(let n=0;n<count;n++,at+=P.words){const action=OrdnanceHostPublication_NAMES[f[at+P.action]],id=f[at+P.record+R.id],key=f[at+P.key];
    if(action==='clear'){root.shots.clear();root.views.clear();root.claims.clear();continue;}if(action==='remove'){root.shots.delete(id);continue;}if(action==='drop'){root.views.delete(id);continue;}if(action==='unclaim'){root.claims.delete(key);continue;}if(action==='claim'){root.claims.set(key,id?root.views.get(id):null);continue;}
    let s=root.views.get(id);if(action==='create'){s=new ClientShotRecordMirror();s.metadata={};s.owner=this.object(f[at+P.owner]);s.ignoreBody=this.object(f[at+P.ignore]);s.bolt=f[at+P.record+R.straight]?{def:this.object(f[at+P.def]),position:s.position,direction:s.direction}:null;root.views.set(id,s);root.shots.set(id,s);}
    syncClientShotRecord(null,f,at+P.record,0,s);s.confirmedTick=Number.isNaN(s.confirmed)?null:s.confirmed;s.metadata.playerId=s.playerId;s.metadata.generation=s.generation;s.metadata.channel=s.channel;s.metadata.fireId=s.fireId;
    if(s.bolt){const b=s.bolt;sceneTransform(b.position);b.speed=s.speed;b.life=s.life;b.flown=s.flown;b.drawn=s.drawn;b.merge=s.merge;b.offset=s.hasOffset?s.offset:null;b.ended=s.ended;b.fade=s.fade;}
  }}
}
export class OrdnanceWorld {constructor(ordnance){this.ordnance=ordnance;this.resources=resourceHost(ordnance.collision);this.at={fireId:0,unit:0,generation:0};this.near=[];this.mounted={vehicle:null,body:null,boundRadius:0};}}
bindResources(OrdnanceWorld,O.world,OrdnanceHostWorld_NAMES);
export class ItemsWorld {constructor(items){this.items=items;this.resources=resourceHost(items.collision);this.ownerIds=new WeakMap();this.nextOwnerId=1;}}
bindResources(ItemsWorld,O.items,OrdnanceHostItems_NAMES);
