import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
import {EffectPhase} from './effects-gpu.js';
import {writeNozzles} from './jetpack-wake.js';
import {VolumeFrame as V,VolumeNozzle as P} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['nozzles',Float64Array],['arrays',Float32Array]];
export const jetpackVolumes=new WeakMap();
// JetpackVolume's nozzles are live facts. Rust owns the box, 3D noise bytes,
// repeat/linear sampler and premultiplied material descriptors.
export class JetpackVolume {
  constructor({scene,count}) {
    this.nativeKind=EFFECT.FLAME.ID;this.family='flame';this.variant=0;this.texture=null;
    this.external=false;this.count=0;this.capacity=count;this.values=new Float64Array(EFFECT.FLAME.LENGTH);
    this.values[EFFECT.FLAME.POWER]=1;this.resources=null;this.samples=new Int32Array(1);this.samples.fill(-1);
    this.arrays={nozzle:new Float32Array(count*3),direction:new Float32Array(count*3),bend:new Float32Array(count*3),nozzleRadius:new Float32Array(count),phase:new Float32Array(count)};
    this.phase=new EffectPhase('PageJetVolume',BANKS);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;this.mesh.name='jetpack-volumetric-combustion';
    sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",10);sceneProperty(this.mesh,"visible",false);
    this.mesh.onBeforeRender=(r,s,c)=>{this.values[EFFECT.FLAME.HAS_DEPTH]=particleDepth.capture(r,s,c,true)?1:0;};
    jetpackVolumes.set(this.mesh,this);scene.add(this.mesh);
  }
  // No BufferAttribute versions are written. The effects phase reads one
  // bounded active prefix and publishes all nozzles in the same coarse call.
  update(time,power,nozzles) {
    this.phase.refresh();const x=this.phase.input;x[V.TIME]=time;x[V.POWER]=power;x[V.COUNT]=nozzles.length;x[V.CAPACITY]=this.capacity;
    writeNozzles(this.phase.nozzles,nozzles);this.phase.frame();this.publish(this.phase.input,this.phase.arrays);
  }
  publish(frame,data) {
    this.count=frame[V.COUNT];sceneProperty(this.mesh,"visible",Boolean(frame[V.SHOWN]));
    this.values[EFFECT.FLAME.FLAME_TIME]=frame[V.TIME];this.values[EFFECT.FLAME.POWER]=frame[V.POWER];
    for(let i=0;i<this.count&&i<this.capacity;i++){
      for(let k=0;k<3;k++){this.arrays.nozzle[i*3+k]=data[i*P.STRIDE+P.POSITION+k];this.arrays.direction[i*3+k]=data[i*P.STRIDE+P.DIRECTION+k];this.arrays.bend[i*3+k]=data[i*P.STRIDE+P.BEND+k];}
      this.arrays.nozzleRadius[i]=data[i*P.STRIDE+P.RADIUS];this.arrays.phase[i]=data[i*P.STRIDE+P.PHASE];
    }
  }
  prepareResources(resources){this.resources=resources;this.samples[0]=resources.gpu.effect_noise(0);return this.samples;}
  clear(){this.count=0;sceneProperty(this.mesh,"visible",false);}
  dispose(){this.phase.dispose();this.mesh.removeFromParent();if(this.resources)this.resources.gpu.dispose_sample(this.samples[0]);}
}
