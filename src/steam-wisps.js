import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
// SteamWisps retains only vent/wake facts. The renderer owns its seeded quad
// and material descriptors; the native environment phase owns wake motion.
export class SteamWisps {
  constructor(scene,wakes,texture) {
    this.nativeKind=EFFECT.WISPS.ID;this.family='wisps';this.variant=0;this.texture=texture;this.wakes=wakes;
    this.count=this.capacity=9*16;this.visibility='anchor';this.values=new Float64Array(EFFECT.WISPS.LENGTH);
    const seed=new Float32Array(this.count*4);let state=317;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<this.count;i++){const o=i*4;seed[o]=Math.floor(i/16);seed[o+1]=random();seed[o+2]=random();seed[o+3]=random();}
    this.arrays={seed};this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name='chamber-steam-wisps';sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"visible",false);sceneProperty(this.mesh,"renderOrder",11);
    this.mesh.onBeforeRender=(r,s,c)=>{this.values[EFFECT.WISPS.HAS_DEPTH]=particleDepth.capture(r,s,c,true)?1:0;};scene.add(this.mesh);
  }
  // Ports SteamWisps.update's live rows without uniform wrappers or scans.
  update(time,volumes) {
    const v=this.values;v[EFFECT.WISPS.WISP_TIME]=time;sceneProperty(this.mesh,"visible",volumes.length>0);
    v.set(this.wakes.host.wakePoints,EFFECT.WISPS.WAKES);v.set(this.wakes.host.wakeMotion,EFFECT.WISPS.MOTION);
    for(let i=0;i<9;i++){const source=volumes[i],o=EFFECT.WISPS.SOURCES+i*4,d=EFFECT.WISPS.DIRECTIONS+i*4;
      if(source){v[o]=source.position.x;v[o+1]=source.position.y;v[o+2]=source.position.z;v[o+3]=1;v[d]=source.direction.x;v[d+1]=source.direction.y;v[d+2]=source.direction.z;v[d+3]=source.kind;}else v[o+3]=0;}
  }
  dispose(){this.mesh.removeFromParent();}
}
