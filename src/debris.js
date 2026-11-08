import {sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from 'three';
import {loadEnvironment,loaded} from './environment/module.js';
import {Debris as LIMITS,DebrisLaunch as INPUT,DebrisPiece as OUTPUT,DebrisEvent as EVENT,DebrisScratch as SCRATCH} from './environment/layout.js';

// Classic chunk physics belongs to the environment phase. This host resolves
// named models/bones, answers casts and bounds queries, and applies scene and
// particle commands. A headless battle never creates the presentation bank.
export class Debris {
  static load(){return loadEnvironment();}
  constructor({scene,collision,effects,model}) {
    Object.assign(this,{scene,collision,effects,model});this.pieces=[];this.bySlot=new Array(LIMITS.CAPACITY).fill(null);
    this.hit={distance:0,point:new THREE.Vector3(),normal:new THREE.Vector3()};
    this.ray=new THREE.Vector3();this.start=new THREE.Vector3();this.out=new THREE.Vector3();
    this.box=new THREE.Box3();this.saved=new THREE.Quaternion();this.impact=new THREE.Vector3();
    this.x=null;this.prepare();
  }
  // Allocate the complete bounded bank at first visual admission. All frames
  // thereafter share these records; a full burst drops excess visual pieces.
  prepare() {
    if(this.x)return;
    this.x=new WebAssembly.Instance(loaded(),{
      surface_math:{surface_sin:Math.sin,surface_cos:Math.cos,surface_tan:Math.tan,surface_atan2:Math.atan2,surface_hypot:Math.hypot},
      env:{host_cast:()=>0,host_cast_out:()=>0,host_ground:()=>NaN,host_cover:()=>-100000,host_impact:()=>-Infinity,host_impact_normal:()=>0,host_shore_tile:()=>0,host_shore_exposed:()=>0,
        host_debris_cast:(slot,ox,oy,oz,dx,dy,dz,far)=>{
          this.sync(slot);this.start.set(ox,oy,oz);this.ray.set(dx,dy,dz);
          return this.collision.cast(this.start,this.ray,far,this.hit,this.bySlot[slot].ignore)?1:0;
        },
        host_debris_hit:i=>i<3?this.hit.point.getComponent(i):this.hit.normal.getComponent(i-3),
        host_debris_low:(slot,x,y,z,w)=>{
          const p=this.sync(slot),q=p.mesh.quaternion;this.saved.copy(q);sceneTransform(q.set(x,y,z,w));p.mesh.updateMatrixWorld(true);
          const low=this.box.setFromObject(p.mesh).min.y;sceneTransform(q.copy(this.saved));return low;
        },
        host_debris_event:(slot,kind)=>this.event(slot,kind),
      },
    }).exports;
    this.x.debris_new();
    this.input=new Float64Array(this.x.memory.buffer,this.x.debris_input(),INPUT.STRIDE);
    this.scratch=new Float64Array(this.x.memory.buffer,this.x.debris_scratch(),SCRATCH.STRIDE);
    this.output=new Float64Array(this.x.memory.buffer,this.x.debris_output(),LIMITS.CAPACITY*OUTPUT.STRIDE);
  }
  // The public piece facade is a readback for existing effects/capture readers.
  // Native slots, never compacted list positions, qualify every query/result.
  sync(slot) {
    this.ray.fromArray(this.scratch,SCRATCH.RAY);this.start.fromArray(this.scratch,SCRATCH.START);this.out.fromArray(this.scratch,SCRATCH.OUT);
    const p=this.bySlot[slot],r=this.output,at=slot*OUTPUT.STRIDE;if(!p)return null;
    sceneTransform(p.position.fromArray(r,at+OUTPUT.POSITION));p.previous.fromArray(r,at+OUTPUT.PREVIOUS);p.velocity.fromArray(r,at+OUTPUT.VELOCITY);
    sceneTransform(p.mesh.quaternion.fromArray(r,at+OUTPUT.QUATERNION));p.omega.fromArray(r,at+OUTPUT.OMEGA);
    p.hits=r[at+OUTPUT.HITS];p.lift=r[at+OUTPUT.LIFT];p.thin=r[at+OUTPUT.THIN];p.resting=r[at+OUTPUT.RESTING]!==0;p.flight=r[at+OUTPUT.FLIGHT];p.left=r[at+OUTPUT.LEFT];
    if(r[at+OUTPUT.HAS_SETTLE]){const s=p.settleRecord;s.from.fromArray(r,at+OUTPUT.SETTLE_FROM);s.to.fromArray(r,at+OUTPUT.SETTLE_TO);s.y0=r[at+OUTPUT.SETTLE_Y];s.y1=r[at+OUTPUT.SETTLE_Y+1];s.t=r[at+OUTPUT.SETTLE_T];p.settle=s;}
    return p;
  }
  event(slot,kind) {
    const p=this.sync(slot);if(!p)return;
    if(kind===EVENT.ADMIT){
      const at=slot*OUTPUT.STRIDE;
      if(this.output[at+OUTPUT.HAS_SHIFT])for(let i=0;i<p.mesh.children.length&&i<LIMITS.CHILDREN;i++)sceneTransform(p.mesh.children[i].position.fromArray(this.input,INPUT.CHILDREN+i*3));
      sceneTransform(p.mesh.scale.set(1,1,1));this.scene.add(p.mesh);
      if(typeof p.chunk.ChunkTrailEffect==='string'&&p.chunk.ChunkTrailEffect)p.trail=this.effects.play(p.chunk.ChunkTrailEffect,p.position,null,p.velocity);
      if(typeof p.chunk.ChunkSmokeEffect==='string')p.smoke=this.effects.play(p.chunk.ChunkSmokeEffect,p.position);
    }else if(kind===EVENT.MOVE){p.trail?.move(p.position,null,p.velocity);p.smoke?.move(p.position);}
    else if(kind===EVENT.IMPACT){this.impact.fromArray(this.output,slot*OUTPUT.STRIDE+OUTPUT.IMPACT_POINT);this.effects.play(p.chunk.ChunkTerrainEffect,this.impact);}
    else if(kind===EVENT.STOP_TRAIL){p.trail?.stop();p.trail=null;}
    else if(kind===EVENT.SETTLE)p.smoke?.move(p.position);
    else if(kind===EVENT.RETIRE||kind===EVENT.CLEAR){this.scene.remove(p.mesh);p.trail?.stop();p.smoke?.stop();if(kind===EVENT.RETIRE)p.gone=true;this.bySlot[slot]=null;}
  }
  // Debris.blowApart publishes authored chunk facts once, after resolving the
  // real model and posed bone. The native launch owns damage scaling, placement,
  // velocity, recentering, inertia-side selection and the lifetime policy.
  blowApart(chunks,bones,origin,velocity,health,ignore=null,{centre=false,floor=-Infinity}={}) {
    this.prepare();const names=new Map(Object.entries(bones).map(([name,bone])=>[name.toLowerCase(),bone]));
    for(const chunk of chunks){
      const make=chunk.ChunkGeometryName?this.model(chunk.ChunkGeometryName):null,bone=names.get(String(chunk.ChunkNodeName||'').toLowerCase());
      if(!make||!bone)continue;const slot=this.x.debris_free();if(slot>=LIMITS.CAPACITY)break;
      const mesh=make();this.box.makeEmpty();if(centre){mesh.updateMatrixWorld(true);this.box.setFromObject(mesh);}bone.updateWorldMatrix(true,false);
      const p={slot,chunk,mesh,position:mesh.position,previous:new THREE.Vector3(),velocity:new THREE.Vector3(),omega:new THREE.Vector3(),ignore,
        hits:0,lift:0,thin:1,resting:false,flight:0,left:0,trail:null,smoke:null,settle:null,
        settleRecord:{from:new THREE.Quaternion(),to:new THREE.Quaternion(),y0:0,y1:0,t:0}};
      this.bySlot[slot]=p;this.pieces.push(p);const c=this.input;
      c.set(bone.matrixWorld.elements,INPUT.MATRIX);this.box.min.toArray(c,INPUT.BOUNDS);this.box.max.toArray(c,INPUT.BOUNDS+3);
      origin.toArray(c,INPUT.ORIGIN);velocity.toArray(c,INPUT.VELOCITY);c[INPUT.HEALTH]=health;c[INPUT.CENTER]=centre?1:0;c[INPUT.FLOOR]=floor;
      c[INPUT.SPEED]=chunk.ChunkSpeed??0;const omega=Array.isArray(chunk.ChunkOmega)?chunk.ChunkOmega:[0,0,0];
      for(let i=0;i<3;i++)c[INPUT.OMEGA+i]=omega[i]===undefined?0:omega[i];c[INPUT.STILL]=String(chunk.ChunkPhysics||'').toUpperCase()==='STATIC'?1:0;
      c[INPUT.COLLISIONS]=chunk.ChunkTerrainCollisions??0;c[INPUT.TRAIL]=typeof chunk.ChunkTrailEffect==='string'&&chunk.ChunkTrailEffect?1:0;
      c[INPUT.SMOKE]=typeof chunk.ChunkSmokeEffect==='string'?1:0;c[INPUT.IMPACT]=typeof chunk.ChunkTerrainEffect==='string'?1:0;
      c[INPUT.CHILD_COUNT]=mesh.children.length;
      for(let i=0;i<mesh.children.length&&i<LIMITS.CHILDREN;i++)mesh.children[i].position.toArray(c,INPUT.CHILDREN+i*3);
      this.x.debris_launch(slot);this.sync(slot);
    }
  }
  update(dt) {
    if(!this.x)return;this.x.debris_update(dt);let count=0;
    for(let i=0;i<this.pieces.length;i++){const p=this.pieces[i];if(!p.gone){this.sync(p.slot);this.pieces[count++]=p;}}
    this.pieces.length=count;
  }
  // Explicit diagnostic commands reuse the native contact/rest phase bodies.
  bounce(p){this.x.debris_contact(p.slot);this.sync(p.slot);}
  rest(p,floor=-Infinity){this.x.debris_settle(p.slot,floor);this.sync(p.slot);}
  clear(){if(this.x)this.x.debris_clear();this.pieces.length=0;}
}
