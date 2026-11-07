import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
// FountainSpray publishes source and wake facts on a fixed seed set. Native
// admission owns its plane, blend, sampler slots and value layout.
export class FountainSpray {
  constructor(scene,wakes) {
    this.nativeKind=EFFECT.SPRAY.ID;this.family='spray';this.variant=0;this.texture=null;this.wakes=wakes;
    this.count=this.capacity=8*24;this.visibility='anchor';this.values=new Float64Array(EFFECT.SPRAY.LENGTH);
    const seed=new Float32Array(this.count*4);let state=137;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<this.count;i++){const o=i*4;seed[o]=Math.floor(i/24);seed[o+1]=random();seed[o+2]=random();seed[o+3]=random();}
    // FountainJets still consumes these shared placement vectors until its
    // own descriptor is admitted. They contain no raster or sampler policy.
    this.sources=Array.from({length:5},()=>new THREE.Vector4());
    this.arrays={seed};this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name='fountain-wake-spray';sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"visible",false);sceneProperty(this.mesh,"renderOrder",11);
    this.mesh.onBeforeRender=(r,s,c)=>{this.values[EFFECT.SPRAY.HAS_DEPTH]=particleDepth.capture(r,s,c,true)?1:0;};scene.add(this.mesh);
  }
  update(time,fountains) {
    const v=this.values;v[EFFECT.SPRAY.SPRAY_TIME]=time;sceneProperty(this.mesh,"visible",fountains.length>0);
    v.set(this.wakes.host.wakePoints,EFFECT.SPRAY.WAKES);v.set(this.wakes.host.wakeMotion,EFFECT.SPRAY.MOTION);
    for(const source of this.sources)source.w=0;
    for(let i=0;i<fountains.length;i++){const f=fountains[i];this.sources[f.slot]?.set(f.position.x,f.position.y,f.position.z,1);}
    for(let i=0;i<5;i++)this.sources[i].toArray(v,EFFECT.SPRAY.SOURCES+i*4);
  }
  dispose(){this.mesh.removeFromParent();}
}
