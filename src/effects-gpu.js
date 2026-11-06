import { Draw as D, Effect as E } from './presentation/layout.js';
import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';
import { ownedEffectMaterial } from './rust-render/effect-material.js';

// Spawn state stays in instance buffers. Authored tracks and the much smaller
// array of emitter transforms/times are read in the vertex shader. One draw per
// texture/blend, including effects with different tracks. No simulation readback.
const KEYS = 8, WIDTH = 64, MAX_EFFECTS = 1024, MAX_DEFINITIONS = 1024;
const ATTRIBUTES = { spawnPosition: 3, spawnVelocity: 3, spawnColor: 4, spawnShape: 4, spawnMeta: 4 };
const power2 = n => 2 ** Math.ceil(Math.log2(Math.max(1, n)));
function dataTexture(data, width, height) {
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.FloatType);
  texture.needsUpdate = true;
  return texture;
}
export class AnalyticParticleBatch {
  constructor(texture, blending, owner) {
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute('corner', new THREE.Float32BufferAttribute([-1,-1,1,-1,1,1,-1,1],2));
    this.geometry.setIndex([0,1,2,0,2,3]);
    this.material = ownedEffectMaterial('effects/particle-analytic', {
      uniforms: { map:{value:texture}, fade:lighting.fade, ...owner.uniforms,
        sceneDepth:particleDepth.map, softParticles:particleDepth.enabled,
        depthResolution:particleDepth.resolution, cameraRange:particleDepth.cameraRange },
      transparent:true, depthWrite:false, side:THREE.DoubleSide, forceSinglePass:true,
      blending:blending==='additive'?THREE.AdditiveBlending:THREE.NormalBlending,
    });
    this.mesh = new THREE.Mesh(this.geometry,this.material);
    this.mesh.visible=false;this.geometry.instanceCount=0;
    this.mesh.frustumCulled=false; this.mesh.renderOrder=10;
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);
    this.arrays={}; this.records=[]; this.slots=[]; this.epochs=[]; this.count=0;
    this.grow(256); this.begin();
  }
  grow(capacity) {
    // Dispose replaced GPU buffers too; replacing attributes alone leaks them.
    this.geometry.dispose();
    for (const [name,size] of Object.entries(ATTRIBUTES)) {
      const array=new Float32Array(capacity*size);
      if(this.arrays[name])array.set(this.arrays[name]);
      this.arrays[name]=array;
      this.geometry.setAttribute(name,new THREE.InstancedBufferAttribute(array,size).setUsage(THREE.DynamicDrawUsage));
    }
    this.capacity=capacity; this.dirty=0;
  }
  begin() { this.count=0; this.dirty=Infinity; }
  // Packed spawn output from the host-neutral presentation kernel.
  pushRecord(data, at, slot, definition, epoch) {
    const i=this.count++;
    if(i===this.capacity)this.grow(this.capacity*2);
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
    // Drawn by another renderer (external, as particles.js's batches): never
    // shown to three, nothing marked for it to upload.
    this.mesh.visible=this.count>0&&!this.external;this.geometry.instanceCount=this.count;
    // What it has not read yet: from the first instance changed since (it
    // sets unread to Infinity as it reads).
    if(this.external){this.unread=Math.min(this.unread??0,this.dirty);return;}
    if(this.dirty>=this.count)return;
    for(const name of Object.keys(ATTRIBUTES)) {
      const a=this.geometry.getAttribute(name);
      // Merge into one pending range even if many updates happen without a draw.
      const start=Math.min(this.dirty*a.itemSize,a.updateRanges[0]?.start??Infinity);
      a.clearUpdateRanges();a.addUpdateRange(start,this.count*a.itemSize-start);a.needsUpdate=true;
    }
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
    this.uniforms={tracks:{value:dataTexture(data,WIDTH,height)},trackRows:{value:height},emitters:{value:null},emitterRows:{value:0}};
    this.resizeEmitters(16);this.count=0;this.epochs=[];
  }
  resizeEmitters(rows) {
    const data=new Float32Array(rows*12);
    if(this.data)data.set(this.data);
    this.data=data;this.uniforms.emitters.value?.dispose();
    this.uniforms.emitters.value=dataTexture(data,3,rows);this.uniforms.emitterRows.value=rows;
  }
  begin() { this.count=0; }
  registerRecord(data, at) {
    const slot=this.count++;
    if(slot>=MAX_EFFECTS)return -1;
    if(slot>=this.uniforms.emitterRows.value)this.resizeEmitters(this.uniforms.emitterRows.value*2);
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
  async warmup(renderer) {
    // Source-owned analytic programs prewarm in the Rust effects pass. Only
    // the authored track and emitter textures still need browser allocation.
    renderer.initTexture(this.uniforms.tracks.value);
    renderer.initTexture(this.uniforms.emitters.value);
  }
  end() { if(this.count)this.uniforms.emitters.value.needsUpdate=true; }
  dispose() {
    this.uniforms.tracks.value.dispose();this.uniforms.emitters.value.dispose();
  }
}
