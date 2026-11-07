import {sceneProperty} from "./rust-render/scene-changes.js";
import { Draw as D, Effect as E } from './presentation/layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
import { EFFECT } from './rust-render/effect-layout.js';

// Spawn state stays in instance buffers. Authored tracks and the much smaller
// array of emitter transforms/times are read in the vertex shader. One draw per
// texture/blend, including effects with different tracks. No simulation readback.
const KEYS = 8, WIDTH = 64, MAX_EFFECTS = EFFECT.ANALYTIC.CAPACITY, MAX_DEFINITIONS = 1024;
const ATTRIBUTES = { spawnPosition: 3, spawnVelocity: 3, spawnColor: 4, spawnShape: 4, spawnMeta: 4 };
const power2 = n => 2 ** Math.ceil(Math.log2(Math.max(1, n)));
export class AnalyticParticleBatch {
  constructor(texture, blending, owner) {
    // AnalyticParticleBatch keeps authored births and an owner identity. Rust
    // owns its quad, material and sampler descriptors, including emitter data.
    this.nativeKind=EFFECT.ANALYTIC.ID;this.family='analytic';this.variant=blending==='additive'?1:0;
    this.texture=texture;this.owner=owner;this.values=new Float64Array(0);
    this.mesh=new THREE.Object3D();sceneProperty(this.mesh,"visible",false);
    sceneProperty(this.mesh,"frustumCulled",false); sceneProperty(this.mesh,"renderOrder",10);
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);
    this.arrays={}; this.records=[]; this.slots=[]; this.epochs=[]; this.count=0;
    this.grow(16384); this.begin();
  }
  grow(capacity) {
    for (const [name,size] of Object.entries(ATTRIBUTES)) {
      const array=new Float32Array(capacity*size);
      if(this.arrays[name])array.set(this.arrays[name]);
      this.arrays[name]=array;
    }
    this.capacity=capacity; this.dirty=0;
  }
  begin() { this.count=0; this.dirty=Infinity; }
  // Packed spawn output from the host-neutral presentation kernel.
  pushRecord(data, at, slot, definition, epoch) {
    if(this.count===this.capacity)return;
    const i=this.count++;
    const serial=data[at+D.PARTICLE];
    if(this.records[i]===serial && this.slots[i]===slot && this.epochs[i]===epoch)return;
    this.records[i]=serial;this.slots[i]=slot;this.epochs[i]=epoch;this.dirty=Math.min(this.dirty,i);
    const a=this.arrays,k=i*4;
    for(let j=0;j<3;j++){a.spawnPosition[i*3+j]=data[at+D.POSITION+j];a.spawnVelocity[i*3+j]=data[at+D.VELOCITY+j];}
    for(let j=0;j<4;j++)a.spawnColor[k+j]=data[at+D.COLOR+j];
    a.spawnShape[k]=data[at+D.SIZE];a.spawnShape[k+1]=data[at+D.ROTATION];a.spawnShape[k+2]=data[at+D.SPIN];a.spawnShape[k+3]=0;
    a.spawnMeta[k]=data[at+D.BIRTH]-epoch;a.spawnMeta[k+1]=data[at+D.LIFE];a.spawnMeta[k+2]=slot;a.spawnMeta[k+3]=definition;
  }
  end() {
    // Release dead references; capacity is the historical peak as with CPU batches.
    this.records.length=this.slots.length=this.epochs.length=this.count;
    sceneProperty(this.mesh,"visible",this.count>0);
    this.unread=Math.min(this.unread??0,this.dirty);
  }
}

export class GpuEffects {
  constructor(library, renderer) {
    this.renderer=renderer;this.definitions=new Map();
    // Its batches' owner, for a renderer that draws them (src/rust-render/effects.js).
    renderer.gpu=this;
    const rows=[];
    const visit=defs=>{for(const def of defs){
      visit(def.geometry?.emitters||[]);
      const g=def.geometry,t=def.transformer;
      if(!g || !t || g.blend==='blur' || !g.texture || !['particle','animated','spark','streak','billboard'].includes(g.type))continue;
      if(['position','size','color'].some(k=>(t[k]?.length||0)>KEYS))continue;
      if(this.definitions.has(def))continue;
      if(rows.length>=MAX_DEFINITIONS)continue;
      this.definitions.set(def,rows.length);rows.push(def);
    }};
    for(const defs of Object.values(library))visit(defs);
    const height=power2(rows.length), data=new Float32Array(WIDTH*height*4);
    rows.forEach((def,row)=>{
      const {geometry:g,transformer:t}=def;
      const put=(column,values)=>data.set(values,(row*WIDTH+column)*4);
      put(0,[t.position?.length||0,t.size?.length||0,t.color?.length||0,0]);
      put(1,[g.type==='billboard'?2:['spark','streak'].includes(g.type)?1:0,def.registerStep?0:1,g.sparkLength||.1,0]);
      put(2,[g.type==='animated'?(g.timePerFrame||0):0,Math.max(Math.round(g.frameSize||1),1),Math.max(Math.round(g.totalFrames||1),1),0]);
      for(const [kind,offset,ops] of [['position',4,['accelerate','reach','scale']],['size',20,['scale','move','reach']],['color',36,['reach','move']]]) {
        (t[kind]||[]).forEach((key,i)=>{
          const op=ops.findIndex(name=>key[name]!==undefined), value=key[ops[op]];
          put(offset+i*2,[key.lifetime,op+1,0,0]);
          const values=Array.isArray(value)?value:[value||0];
          put(offset+i*2+1,kind==='color'?values.map(v=>v/255):values);
        });
      }
    });
    // The native owner admits the maximum 1024 emitters while loading. This
    // removes resizeEmitters and its repeated DataTexture allocation in battle.
    this.tracks=data;this.trackRows=height;this.emitterRows=MAX_EFFECTS;
    this.data=new Float32Array(MAX_EFFECTS*12);this.count=0;this.epochs=[];
    this.trackSample=-1;this.resources=null;
  }
  prepareResources(resources) {
    if(this.trackSample<0){this.resources=resources;this.trackSample=resources.gpu.effect_data(0,this.trackRows,this.tracks);}
    return this.trackSample;
  }
  begin() { this.count=0; }
  registerRecord(data, at) {
    const slot=this.count++;
    if(slot>=MAX_EFFECTS)return -1;
    const dst=slot*12;
    for(let j=0;j<3;j++)this.data[dst+j]=data[at+E.POSITION+j];
    this.data[dst+3]=data[at+E.TINT];
    // Rebase live births with the emitter's clock every 256 seconds so a
    // long session retains precise GPU ages without a wall-clock dependency.
    const time=data[at+E.TIME],epoch=Math.floor(time/256)*256;this.epochs[slot]=epoch;
    for(let j=0;j<4;j++)this.data[dst+4+j]=data[at+E.QUATERNION+j];
    this.data[dst+8]=time-epoch;this.data[dst+9]=data[at+E.SCALE];return slot;
  }
  batch(def, texture) {
    const blend=def.geometry.blend==='additive'?'additive':'normal';
    return this.renderer.custom(`analytic:${texture.uuid}:${blend}`,()=>new AnalyticParticleBatch(texture,blend,this));
  }
  async warmup(renderer) { this.prepareResources(renderer.resources); }
  // Emitters publish once with the effects phase; no redundant texture version
  // or browser upload is scheduled by GpuEffects.end.
  end() {}
  dispose() {if(this.trackSample>=0)this.resources.gpu.dispose_sample(this.trackSample);}
}
