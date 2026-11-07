import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
const ZERO=new THREE.Vector3();
export const jetpackVolumes=new WeakMap();
// JetpackVolume's nozzles are live facts. Rust owns the box, 3D noise bytes,
// repeat/linear sampler and premultiplied material descriptors.
export class JetpackVolume {
  constructor({scene,count}) {
    this.nativeKind=EFFECT.FLAME.ID;this.family='flame';this.variant=0;this.texture=null;
    this.external=false;this.count=0;this.capacity=count;this.values=new Float64Array(EFFECT.FLAME.LENGTH);
    this.values[EFFECT.FLAME.POWER]=1;this.resources=null;this.samples=new Int32Array(1);this.samples.fill(-1);
    this.arrays={nozzle:new Float32Array(count*3),direction:new Float32Array(count*3),bend:new Float32Array(count*3),nozzleRadius:new Float32Array(count),phase:new Float32Array(count)};
    for(let i=0;i<count;i++)this.arrays.phase[i]=i*17.31;
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;this.mesh.name='jetpack-volumetric-combustion';
    sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",10);sceneProperty(this.mesh,"visible",false);
    this.mesh.onBeforeRender=(r,s,c)=>{this.values[EFFECT.FLAME.HAS_DEPTH]=particleDepth.capture(r,s,c,true)?1:0;};
    jetpackVolumes.set(this.mesh,this);scene.add(this.mesh);
  }
  // No BufferAttribute versions are written. The effects phase reads one
  // bounded active prefix and publishes all nozzles in the same coarse call.
  update(time,power,nozzles) {
    const shown=power>0.015,a=this.arrays;this.count=shown?Math.min(nozzles.length,this.capacity):0;
    sceneProperty(this.mesh,"visible",shown);if(!shown)return;
    this.values[EFFECT.FLAME.FLAME_TIME]=time;this.values[EFFECT.FLAME.POWER]=Math.max(power,0.025);
    for(let i=0;i<this.count;i++){const n=nozzles[i];n.position.toArray(a.nozzle,i*3);n.direction.toArray(a.direction,i*3);(n.bendWorld||ZERO).toArray(a.bend,i*3);a.nozzleRadius[i]=n.radius||0.034;}
  }
  prepareResources(resources){this.resources=resources;this.samples[0]=resources.gpu.effect_noise(0);return this.samples;}
  clear(){this.count=0;sceneProperty(this.mesh,"visible",false);}
  dispose(){this.mesh.removeFromParent();if(this.resources)this.resources.gpu.dispose_sample(this.samples[0]);}
}
