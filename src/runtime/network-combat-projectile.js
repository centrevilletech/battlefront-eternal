import { Vector3, Quaternion } from 'three';
import { networkShotFor } from './network-shot-aim.js';
const TYPES=['grenade','round','beam'];
const COAST_TICKS=6,COAST_FULL_TICKS=3,SMOOTH_SECONDS=.08,MAX_OFFSET=6;
// A sharp turn or authoritative stop can be a bounce/contact. Never carry
// a correction from the old trajectory across that new authority sample.
const changedFlight=(a,b)=>{
 const speedA=Math.hypot(...a),speedB=Math.hypot(...b);
 return speedA>1e-6&&(speedB<speedA*.25||a.reduce((sum,n,i)=>sum+n*b[i],0)<speedA*speedB*.5);
};
const u32=n=>{if(!Number.isSafeInteger(n)||n<0||n>0xffffffff)throw Error('invalid projectile integer');return n;};
export function projectileRecord({id,generation,tick,definitionId,type,ownerId=0,ownerGeneration=0,fireId=0,channel=0,launchTick=tick,position,velocity=[0,0,0],quaternion=[0,0,0,1],end=position,life=0,terminal=false}){
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
 launch(projectile,type){
  if(this.active.has(projectile))return;
  const c=this.capture;if(this.active.size>=c.maxRecords)throw Error('dynamic projectile capture capacity exceeded');
  const owner=projectile.owner,channel=projectile.networkFireChannel??0,shot=networkShotFor(owner,channel);
  this.active.set(projectile,{id:c.nextId++,generation:c.generation,type,launchTick:c.tick(),definitionId:c.definitionId(projectile.def),ownerId:typeof owner==='object'?owner?.id??0:0,ownerGeneration:typeof owner==='object'?owner?.generation??0:0,fireId:shot?.id??0,channel});
 }
 sample(){
  const c=this.capture,tick=c.tick(),live=new Set();
  for(const[type,list]of[['grenade',this.ordnance.grenades],['round',this.ordnance.rounds],['beam',this.ordnance.beams]])for(const p of list){
   if(p.gone||type==='beam'&&p.life<=0)continue;live.add(p);this.launch(p,type);const entry=this.active.get(p);if(entry.terminal)continue;
   c.emit(projectileRecord({...entry,tick,position:p.position.toArray(),velocity:p.velocity?.toArray()??[0,0,0],quaternion:(p.quaternion??p.mesh?.quaternion)?.toArray()??[0,0,0,1],end:p.end?.toArray()??p.position.toArray(),life:Math.max(0,p.life??p.fuse??0)}));
  }
  for(const[p,entry]of this.active)if(!live.has(p)){if(!entry.terminal)c.emit(projectileRecord({...entry,tick,terminal:true}));this.active.delete(p);}
 }
}

// Rounds whose flight a page can follow from one sample: rockets ("bullet")
// and fat rays fly dead straight, shells fall at their Gravity
// (Ordnance.round, stepRound). Missiles steer and waver, grenades bounce:
// those keep the short coast and wait for their next sample.
const FOLLOWED={bullet:'straight',fatray:'straight',shell:'ballistic'};
export class ProjectilePresentation {
 constructor(ordnance,definition){this.ordnance=ordnance;this.definition=definition;this.objects=new Map();this.samplePosition=new Vector3();this.sampleQuaternion=new Quaternion();this.expected=new Vector3();}
 remove(id){const p=this.objects.get(id);if(!p)return;if(p.mesh)this.ordnance.scene.remove(p.mesh);p.trail?.stop();this.objects.delete(id);}
 reset(){for(const id of this.objects.keys())this.remove(id);}
 // A terminal (an impact, schema 2, or a removal, 7) ends the projectile
 // when this page's playout clock reaches its tick, an impact's last stretch
 // flown into the point it names, rather than the moment it arrives.
 end(event){
  const p=this.objects.get(event.id);if(!p)return;
  if(p.generation!==event.generation){this.remove(event.id);return;}
  const last=p.samples.at(-1);p.endTick=Math.min(p.endTick??Infinity,event.tick);
  if(event.schema===2&&event.tick>last.tick){p.samples.push({...last,tick:event.tick,expiryTick:event.tick+18,position:event.position,end:event.position,life:0,changedFlight:false});if(p.samples.length>8)p.samples.shift();}
 }
 receive(event){
  if(event.schema===2||event.schema===7){this.end(event);return;}
  let p=this.objects.get(event.id);
  if(p&&p.generation!==event.generation){this.remove(event.id);p=null;}
  if(!p){const def=this.definition(event.definitionId);if(!def)throw Error('missing projectile definition');
   p={def,generation:event.generation,type:event.type,position:new Vector3(),previous:new Vector3(),velocity:new Vector3(),quaternion:new Quaternion(),end:new Vector3(),direction:new Vector3(),axis:[0,0,1],life:event.life,samples:[],offset:new Vector3(),pathVelocity:new Vector3(),shown:null};
   if(p.type!=='beam'){p.mesh=this.ordnance.projectileMesh(def);p.trail=def.TrailEffect?this.ordnance.effects.play(def.TrailEffect,new Vector3(...event.position),null,new Vector3(...event.velocity),new Quaternion(...event.quaternion)):null;}
   this.objects.set(event.id,p);
  }
  const previous=p.samples.at(-1);
  p.samples.push({...event,changedFlight:!!previous&&changedFlight(previous.velocity,event.velocity)});if(p.samples.length>8)p.samples.shift();
 }
 present(tick){
  for(const[id,p]of this.objects){const last=p.samples.at(-1),flight=p.type==='round'?FOLLOWED[p.def.base]:null;
   if(tick>=p.endTick||tick>(flight?last.tick+last.life*60+18:last.expiryTick)){this.remove(id);continue;}
   const renderTick=tick;let a=p.samples[0],b=a;for(const s of p.samples){if(s.tick<=renderTick)a=b=s;else{b=s;break;}}
   const fraction=b.tick>a.tick?Math.max(0,Math.min(1,(renderTick-a.tick)/(b.tick-a.tick))):0;
   p.position.fromArray(a.position).lerp(this.samplePosition.fromArray(b.position),fraction);
   p.velocity.fromArray(a.velocity).lerp(this.samplePosition.fromArray(b.velocity),fraction);
   p.pathVelocity.set(0,0,0);
   if(b.tick>a.tick&&renderTick>=a.tick)p.pathVelocity.fromArray(b.position).sub(this.samplePosition.fromArray(a.position)).multiplyScalar(60/(b.tick-a.tick));
   if(renderTick>=last.tick&&flight){
    // A rocket or shell flies on along its path for the life it had left,
    // so a sample that is late, or never sent, costs nothing to see.
    const age=Math.min(last.life*60,renderTick-last.tick)/60,gravity=flight==='ballistic'?-9.8*(p.def.Gravity??1):0;
    p.position.addScaledVector(p.velocity,age);p.position.y+=gravity*age*age/2;
    if(renderTick-last.tick<last.life*60){p.pathVelocity.copy(p.velocity);p.pathVelocity.y+=gravity*age;}
   }else if(renderTick>=last.tick&&p.type!=='beam'){
    // Short, bounded cosmetic coast only. Brake the final three ticks and
    // never extrapolate longer than the authority's remaining sampled life.
    const limit=Math.min(COAST_TICKS,last.life*60),full=Math.min(COAST_FULL_TICKS,limit),tail=limit-full;
    const age=Math.min(limit,renderTick-last.tick),brake=Math.max(0,age-full);
    p.position.addScaledVector(p.velocity,(age-(tail?brake*brake/(2*tail):0))/60);
    if(age<limit)p.pathVelocity.copy(p.velocity).multiplyScalar(tail?1-brake/tail:1);
   }
   if(p.type!=='beam')this.smooth(p,tick,last.tick,a.changedFlight||b.changedFlight);
   p.previous.copy(p.position);p.quaternion.fromArray(a.quaternion).slerp(this.sampleQuaternion.fromArray(b.quaternion),fraction);
   p.end.fromArray(a.end).lerp(this.samplePosition.fromArray(b.end),fraction);p.life=last.life;p.direction.subVectors(p.end,p.position).normalize();
   if(p.type==='beam')this.ordnance.drawBeam(p);else this.ordnance.drawProjectileView(p,1,true);
  }
 }
 smooth(p,tick,latestTick,discontinuous){
  const shown=p.shown;
  if(!shown||tick<shown.tick||discontinuous)p.offset.set(0,0,0);
  else{
   const dt=(tick-shown.tick)/60;p.offset.multiplyScalar(Math.exp(-dt/SMOOTH_SECONDS));
   if(latestTick!==shown.latestTick){
    this.expected.copy(shown.raw).addScaledVector(shown.velocity,dt).sub(p.position);
    const bound=Math.min(MAX_OFFSET,Math.max(.25,p.velocity.length()*.1));
    if(this.expected.length()>bound*2)p.offset.set(0,0,0);
    else p.offset.add(this.expected).clampLength(0,bound);
   }
  }
  p.shown??={raw:new Vector3(),velocity:new Vector3()};
  p.shown.raw.copy(p.position);p.shown.velocity.copy(p.pathVelocity);p.shown.tick=tick;p.shown.latestTick=latestTick;
  p.position.add(p.offset);
 }
}
