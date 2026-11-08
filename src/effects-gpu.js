import {sceneProperty} from "./rust-render/scene-changes.js";
import {presentationModule} from './presentation/module.js';
import {Draw as D,Effect as E} from './presentation/layout.js';
import {AnalyticFrame as F,AnalyticBatch as B,AnalyticSpawn as S,AnalyticArrays as A,VisualLimits as L} from './presentation/visual-effects-layout.js';
import * as THREE from 'three';
import {particleDepth} from './particle-depth.js';
import {EFFECT} from './rust-render/effect-layout.js';
const BANKS=[['input',Float64Array],['emitters',Float64Array],['data',Float32Array],['epochs',Float64Array],['definitions',Int32Array],['tracks',Float32Array],['batches',Float64Array],['spawns',Float64Array],['arrays',Float32Array]];
const ATTRIBUTES={spawnPosition:[A.POSITION,3],spawnVelocity:[A.VELOCITY,3],spawnColor:[A.COLOR,4],spawnShape:[A.SHAPE,4],spawnMeta:[A.META,4]};

// The anchor and texture identity stay on the page. Rust retains particle
// identities, epoch rebasing and dirty/upload spans for every texture batch.
export class AnalyticParticleBatch {
  constructor(texture,blending,owner){
    this.nativeKind=EFFECT.ANALYTIC.ID;this.family='analytic';this.variant=blending==='additive'?1:0;
    this.texture=texture;this.owner=owner;this.values=new Float64Array(0);this.capacity=L.ANALYTIC_PARTICLES;
    this.mesh=new THREE.Object3D();sceneProperty(this.mesh,"visible",false);sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",10);
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);
    this.arrays={};this.records={length:0};this.count=0;this.unread=0;
    this.serials=new Float64Array(this.capacity);this.slots=new Float64Array(this.capacity);this.epochKeys=new Float64Array(this.capacity);
    this.serials.fill(NaN);this.slots.fill(NaN);this.epochKeys.fill(NaN);
    const resource=owner.resource(texture,blending);owner.admit(resource);
    this.id=resource.id;this.admitted=this.id<L.ANALYTIC_BATCHES;
    if(this.admitted)owner.batches.push(this);
    else{this.capacity=0;for(const name of Object.keys(ATTRIBUTES))this.arrays[name]=new Float32Array(0);}
    owner.refresh();this.bind();this.begin();
  }
  bind(){
    if(!this.admitted)return;
    const phase=this.owner.phase,base=this.id*A.STRIDE;
    for(const [name,layout]of Object.entries(ATTRIBUTES))this.arrays[name]=new Float32Array(phase.loaded.memory.buffer,phase.arrays.byteOffset+(base+layout[0])*4,this.capacity*layout[1]);
  }
  begin(){this.count=0;}
  // These are input facts, not packed GPU attributes. Their final float32
  // arrays and stable-row decision are published once by GpuEffects.end.
  pushRecord(data,at,slot,definition){
    if(!this.admitted||this.count===this.capacity)return;
    this.owner.prepareParticles();const index=this.count++,serial=data[at+D.PARTICLE],epoch=this.owner.epochs[slot];
    // This is a resource receipt: stable native spawn facts cross no second
    // numeric boundary. Epoch choice and final upload dirtiness stay native.
    if(this.serials[index]===serial&&this.slots[index]===slot&&this.epochKeys[index]===epoch)return;
    this.serials[index]=serial;this.slots[index]=slot;this.epochKeys[index]=epoch;
    const x=this.owner.phase.spawns,to=(this.id*this.capacity+index)*S.STRIDE;
    x[to+S.SERIAL]=serial;
    for(let k=0;k<3;k++){x[to+S.POSITION+k]=data[at+D.POSITION+k];x[to+S.VELOCITY+k]=data[at+D.VELOCITY+k];}
    for(let k=0;k<4;k++)x[to+S.COLOR+k]=data[at+D.COLOR+k];
    x[to+S.SIZE]=data[at+D.SIZE];x[to+S.ROTATION]=data[at+D.ROTATION];x[to+S.SPIN]=data[at+D.SPIN];x[to+S.BIRTH]=data[at+D.BIRTH];x[to+S.LIFE]=data[at+D.LIFE];x[to+S.SLOT]=slot;x[to+S.DEFINITION]=definition;
  }
  end(){}
  publish(){const x=this.owner.phase.batches,at=this.id*B.STRIDE;this.count=x[at+B.COUNT];this.dirty=x[at+B.DIRTY];this.unread=x[at+B.UNREAD];this.records.length=this.count;sceneProperty(this.mesh,"visible",this.count>0);}
}

export class GpuEffects {
  constructor(library,renderer,source=null){
    this.renderer=renderer;renderer.gpu=this;this.definitions=new Map();this.batches=[];this.resourcesByKey=new Map();
    // Resource identity discovery preserves authored child-before-parent order.
    // Eligibility, track operations and texel encoding are decided natively.
    const rows=[],seen=new Set();const visit=defs=>{for(const def of defs){visit(def.geometry?.emitters||[]);if(!seen.has(def)){seen.add(def);rows.push(def);}}};
    for(const defs of Object.values(library))visit(defs);
    this.phase=new EffectPhase('PageAnalyticEffects',BANKS,JSON.stringify(rows));this.buffer=null;this.refresh();
    for(let k=0;k<rows.length;k++)if(this.phase.definitions[k]>=0)this.definitions.set(rows[k],this.phase.definitions[k]);
    // Admit all immutable texture/blend identities before the producer's
    // input views are constructed. Displayed frames never allocate a bank.
    if(source)for(const def of this.definitions.keys()){
      const texture=source.emitterTexture(def);
      if(texture)this.admit(this.resource(texture,def.geometry.blend==='additive'?'additive':'normal'));
    }
    this.trackRows=this.phase.kernel.track_rows();this.emitterRows=L.ANALYTIC_EFFECTS;this.count=0;this.trackSample=-1;this.resources=null;
  }
  resource(texture,blend){
    const key=`analytic:${texture.uuid}:${blend}`;let resource=this.resourcesByKey.get(key);
    if(!resource){resource={key,id:-1};this.resourcesByKey.set(key,resource);}return resource;
  }
  admit(resource){
    if(resource.id>=0)return;resource.id=this.phase.kernel.add_batch();
    this.phase.buffer=null;this.buffer=null;this.refresh();
  }
  refresh(){this.phase.refresh();if(this.buffer===this.phase.buffer)return;this.buffer=this.phase.buffer;
    this.data=this.phase.data;this.epochs=this.phase.epochs;this.tracks=this.phase.tracks;for(const batch of this.batches)batch.bind();}
  prepareResources(resources){this.refresh();if(this.trackSample<0){this.resources=resources;this.trackSample=resources.gpu.effect_data(0,this.trackRows,this.tracks);}return this.trackSample;}
  begin(){this.refresh();this.count=0;this.emittersReady=false;}
  prepareParticles(){if(this.emittersReady)return;this.refresh();this.phase.input[F.EMITTERS]=this.count;this.phase.kernel.emitters();this.emittersReady=true;}
  registerRecord(data,at){
    this.refresh();const slot=this.count++;if(slot>=L.ANALYTIC_EFFECTS)return -1;
    const x=this.phase.emitters,to=slot*E.STRIDE;for(let k=0;k<E.STRIDE;k++)x[to+k]=data[at+k];return slot;
  }
  batch(def,texture){const blend=def.geometry.blend==='additive'?'additive':'normal';return this.renderer.custom(`analytic:${texture.uuid}:${blend}`,()=>new AnalyticParticleBatch(texture,blend,this));}
  async warmup(renderer){this.prepareResources(renderer.resources);}
  end(){
    this.prepareParticles();
    for(const batch of this.batches){const at=batch.id*B.STRIDE;this.phase.batches[at+B.COUNT]=batch.count;this.phase.batches[at+B.UNREAD]=batch.unread??0;}
    this.phase.frame();this.refresh();for(const batch of this.batches)batch.publish();
  }
  dispose(){if(this.trackSample>=0)this.resources.gpu.dispose_sample(this.trackSample);this.phase.dispose();}
}

// Fixed native phase banks are views, refreshed only after memory growth.
// Resource hosts keep their ordinary renderer arrays and copy active output.
export class EffectPhase {
  constructor(name, banks, content) {
    this.loaded = presentationModule();
    this.kernel = content === undefined ? new this.loaded.module[name]() : new this.loaded.module[name](content);
    this.banks = banks;this.buffer = null;this.refresh();
  }
  refresh() {
    const buffer = this.loaded.memory.buffer;
    if (buffer === this.buffer) return;
    this.buffer = buffer;
    for (const [name, Type] of this.banks) this[name] = new Type(buffer, this.kernel[name + '_ptr'](), this.kernel[name + '_len']());
  }
  frame(random) { this.kernel.frame(random);this.refresh(); }
  dispose() { this.kernel.free(); }
}
