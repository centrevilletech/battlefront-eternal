import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentDraw as DRAW} from './environment/layout.js';
import {drawValues} from './environment/module.js';
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
// RainSheets' three open cylinders and repeat/mipmap streak image are native
// resources. The page publishes only the rain's live environmental facts.
export class RainSheets {
  constructor(scene,weather) {
    this.weather=weather;this.nativeKind=EFFECT.RAIN_SHEETS.ID;this.family='rainSheets';
    this.variant=0;this.texture=null;this.arrays={};this.count=1;this.capacity=1;this.visibility='anchor';
    drawValues(weather.host,this,DRAW.SHEETS,EFFECT.RAIN_SHEETS);this.resources=null;
    this.samples=new Int32Array(2);this.samples.fill(-1);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;this.mesh.name='rain-sheets';
    sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",19);sceneProperty(this.mesh,"visible",false);
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);scene.add(this.mesh);
  }
  prepareResources(resources) {
    this.resources=resources;this.samples[0]=resources.gpu.effect_data(4,256,this.weather.host.streaks);
    this.samples[1]=this.weather.prepareResources(resources)[0];return this.samples;
  }
  dispose(){this.mesh.removeFromParent();if(this.resources)this.resources.gpu.dispose_sample(this.samples[0]);}
}
