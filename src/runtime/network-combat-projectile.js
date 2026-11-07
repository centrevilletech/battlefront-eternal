import { Vector3, Quaternion } from 'three';
import { networkShotFor } from './network-shot-aim.js';
import { NetFlag as NET_FLAG } from '../presentation/ordnance-layout.js';
const TYPES=['grenade','round','beam'];
const u32=n=>{if(!Number.isSafeInteger(n)||n<0||n>0xffffffff)throw Error('invalid projectile integer');return n;};
export function projectileRecord({id,generation,tick,definitionId,type,ownerId=0,ownerGeneration=0,fireId=0,channel=0,launchTick=tick,position,velocity=[0,0,0],quaternion=[0,0,0,1],end=position,life=0,terminal=false,native=null}){
 if(native)return native.projectile({id,generation,tick,definitionId,type,ownerId,ownerGeneration,fireId,channel,launchTick,position,velocity,quaternion,end,life,terminal});
 const payload=new Uint8Array(terminal?4:82),v=new DataView(payload.buffer);let at=0;
 const u=n=>{v.setUint32(at,u32(n),true);at+=4;},f=n=>{if(!Number.isFinite(n)||Math.abs(n)>1e8)throw Error('invalid projectile sample');v.setFloat32(at,n,true);at+=4;};
 u(tick+18);
 if(!terminal){u(definitionId);v.setUint8(at++,TYPES.indexOf(type));for(const n of[ownerId,ownerGeneration,fireId,launchTick])u(n);for(const list of[position,velocity,quaternion,end])for(const n of list)f(n);f(life);if(channel!==0&&channel!==1)throw Error('invalid projectile channel');v.setUint8(at++,channel);}
 return{kind:'combat',id:u32(id),generation:u32(generation),tick:u32(tick),schema:terminal?7:6,payload};
}
export function decodeProjectile(record){
 if(record.kind!=='combat'||![6,7].includes(record.schema)||record.payload?.length!==(record.schema===7?4:82))throw Error('invalid projectile payload');
 const v=new DataView(record.payload.buffer,record.payload.byteOffset,record.payload.byteLength);let at=0;const u=()=>{const n=v.getUint32(at,true);at+=4;return n;},f=()=>{const n=v.getFloat32(at,true);at+=4;if(!Number.isFinite(n)||Math.abs(n)>1e8)throw Error('invalid projectile sample');return n;},vec=n=>Array.from({length:n},f);
 const out={...record,expiryTick:u()};if(out.expiryTick!==record.tick+18)throw Error('invalid projectile expiry');if(record.schema===7)return out;
 out.definitionId=u();out.type=TYPES[v.getUint8(at++)];out.ownerId=u();out.ownerGeneration=u();out.fireId=u();out.launchTick=u();out.position=vec(3);out.velocity=vec(3);out.quaternion=vec(4);out.end=vec(3);out.life=f();
 out.channel=v.getUint8(at++);
 if(out.channel>1||out.launchTick>record.tick||!out.type||out.life<0||Math.abs(Math.hypot(...out.quaternion)-1)>.01)throw Error('invalid projectile type/orientation');return out;
}

// Samples only; the authoritative Ordnance owns flight, bounces and collisions.
export class ProjectileCapture {
 constructor(capture,ordnance){this.capture=capture;this.ordnance=ordnance;this.active=new Map();}
 launch(projectile,type,sample=null){
  if(this.active.has(projectile))return;
  const c=this.capture;if(this.active.size>=c.maxRecords)throw Error('dynamic projectile capture capacity exceeded');
  // The stable projectile remains the key; a native notice supplies values
  // sampled before later movement, owner reuse or an immediate terminal.
  const owner=projectile.owner;
  const channel=sample?sample.channel:projectile.networkFireChannel??0;
  const shot=sample?null:networkShotFor(owner,channel);
  this.active.set(projectile,{
   id:c.nextId++,generation:c.generation,type,launchTick:c.tick(),
   definitionId:c.definitionId(sample?sample.def:projectile.def),
   ownerId:sample?sample.ownerId:typeof owner==='object'?owner?.id??0:0,
   ownerGeneration:sample?sample.ownerGeneration:typeof owner==='object'?owner?.generation??0:0,
   fireId:sample?sample.fireId:shot?.id??0,channel,
  });
 }
 sample(){
  const c=this.capture,tick=c.tick(),live=new Set();
  for(const[type,list]of[['grenade',this.ordnance.grenades],['round',this.ordnance.rounds],['beam',this.ordnance.beams]])for(const p of list){
   if(p.gone||type==='beam'&&p.life<=0)continue;live.add(p);this.launch(p,type);const entry=this.active.get(p);if(entry.terminal)continue;
   c.emit(projectileRecord({...entry,tick,position:p.position.toArray(),velocity:p.velocity?.toArray()??[0,0,0],quaternion:(p.quaternion??p.mesh?.quaternion)?.toArray()??[0,0,0,1],end:p.end?.toArray()??p.position.toArray(),life:Math.max(0,p.life??p.fuse??0),native:c.native}));
  }
  for(const[p,entry]of this.active)if(!live.has(p)){if(!entry.terminal)c.emit(projectileRecord({...entry,tick,terminal:true,native:c.native}));this.active.delete(p);}
 }
}

// Replicated rounds as the page shows them: grenades, rockets, shells,
// missiles and fat rays. A projectile's samples, its flight past the last
// one (rockets and shells fly on along their path, grenades and missiles
// coast), the correction that hides a new sample, and its draw are the
// ordnance phase's (rust/bfe-presentation/src/ordnance.rs, which ports the
// previous present/smooth here). The page keeps what only it can: the mesh,
// the trail effect, and the slot the kernel holds each projectile in.
export class ProjectilePresentation {
 constructor(ordnance,definition){this.ordnance=ordnance;this.definition=definition;this.objects=new Map();ordnance.draw.netHost=this;}
 // The kernel's own end for a projectile (its clock passed its last sample
 // or its terminal): the page lets go of what it held.
 expired(p){this.objects.delete(p.id);this.release(p);}
 release(p){if(p.mesh)this.ordnance.scene.remove(p.mesh);p.trail?.stop();this.ordnance.draw.netRelease(p.slot);}
 remove(id){const p=this.objects.get(id);if(!p)return;this.ordnance.draw.netRemove(p.slot);this.objects.delete(id);this.release(p);}
 reset(){for(const id of [...this.objects.keys()])this.remove(id);}
 // A terminal (an impact, schema 2, or a removal, 7) ends the projectile
 // when this page's playout clock reaches its tick, an impact's last stretch
 // flown into the point it names, rather than the moment it arrives.
 end(event){
  const p=this.objects.get(event.id);if(!p)return;
  if(p.generation!==event.generation){this.remove(event.id);return;}
  this.ordnance.draw.netEnd(p.slot,event);
 }
 receive(event){
  if(event.schema===2||event.schema===7){this.end(event);return;}
  let p=this.objects.get(event.id);
  if(p&&p.generation!==event.generation){this.remove(event.id);p=null;}
  const draw=this.ordnance.draw;
  if(!p){const def=this.definition(event.definitionId);if(!def)throw Error('missing projectile definition');
   p={id:event.id,def,generation:event.generation,slot:-1,mesh:null,trail:null};
   // Every slot is held: this one is not drawn, and the page goes on.
   if((p.slot=draw.netAllocate(p))<0)return;
   if(event.type!=='beam'){p.mesh=this.ordnance.projectileMesh(def);p.trail=def.TrailEffect?this.ordnance.effects.play(def.TrailEffect,new Vector3(...event.position),null,new Vector3(...event.velocity),new Quaternion(...event.quaternion)):null;}
   this.objects.set(event.id,p);
  }
  draw.netSample(p.slot,event,p.def,(p.mesh?1<<NET_FLAG.MESH:0)|(p.trail?1<<NET_FLAG.TRAIL:0));
 }
 // The frame's projectiles are drawn where the kernel finds this among the
 // other draws, at the playout clock.
 present(tick){this.ordnance.draw.presentNet(tick);}
}
