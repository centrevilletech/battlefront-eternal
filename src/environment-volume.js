import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT, EFFECT_TARGET } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

import { Wakes as LIMITS, WakeFrame as FRAME, WakeActor as ACTOR, WakeResult as RESULT } from './environment/layout.js';

export const WAKE_COUNT = LIMITS.COUNT;
// EnvironmentWakes is now only a uniform shell around native wake records.
// The shared surface frame advances placed effects and their wake records.
export class EnvironmentWakes {
  constructor(host) {
    this.host = host;
    this.points = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4());
    this.motion = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4());
    this.ids = new WeakMap(); this.nextId = 0;
    this.synced();
  }
  get time() { this.host.refresh();return this.host.wakeResult[RESULT.TIME]; }
  set time(value) { this.host.refresh();const f=this.host.wakeFrame;f[FRAME.SET_TIME]=1;f[FRAME.TIME]=value;this.host.x.vis_wake_apply();this.synced(); }
  get cursor() { this.host.refresh();return this.host.wakeResult[RESULT.CURSOR]; }
  prepare(actors) {
    this.host.refresh();const rows=this.host.wakeActors;
    let count=0;
    for(let i=0;i<actors.length&&count<LIMITS.ACTORS;i++) {
      const actor=actors[i];if(!actor?.position)continue;
      let id=this.ids.get(actor);if(!id){id=++this.nextId;this.ids.set(actor,id);}
      const at=count++*ACTOR.STRIDE,p=actor.position;
      rows[at+ACTOR.RELEVANT]=1;rows[at+ACTOR.ID]=id;rows[at+ACTOR.POSITION]=p.x;rows[at+ACTOR.POSITION+1]=p.y;rows[at+ACTOR.POSITION+2]=p.z;
    }
    this.host.wakeFrame[FRAME.ACTORS]=count;
  }
  synced() {
    this.host.refresh();
    for(let i=0;i<WAKE_COUNT;i++) {
      this.points[i].fromArray(this.host.wakePoints,i*4);this.motion[i].fromArray(this.host.wakeMotion,i*4);
    }
  }
  add(position,velocity,strength=1) {
    this.host.refresh();const f=this.host.wakeFrame;f[FRAME.ADD]=1;f[FRAME.STRENGTH]=strength;
    f[FRAME.POSITION]=position.x;f[FRAME.POSITION+1]=position.y;f[FRAME.POSITION+2]=position.z;
    f[FRAME.VELOCITY]=velocity.x;f[FRAME.VELOCITY+1]=velocity.y;f[FRAME.VELOCITY+2]=velocity.z;
    this.host.x.vis_wake_apply();this.synced();
  }
  clear() { this.host.refresh();this.host.wakeFrame[FRAME.CLEAR]=1;this.host.x.vis_wake_apply();this.synced(); }
}

// The composite anchor is registered with its native offscreen batch. Native
// descriptors own the density field, box, full-screen quad and target policy.
export const environmentVolumes=new WeakMap();
export class EnvironmentVolume {
  constructor(scene,capacity,wakes,reflectionUniforms={}) {
    this.nativeKind=EFFECT.VOLUME_COMPOSITE.ID;this.family='volumeComposite';this.variant=0;this.texture=null;
    this.capacity=this.count=1;this.arrays={};this.visibility='anchor';this.resources=null;this.target=null;this.scale=.5;
    this.values=new Float64Array(EFFECT.VOLUME_COMPOSITE.LENGTH);this.samples=new Int32Array(1);this.samples.fill(-1);
    this.wakes=wakes;this.reflectionUniforms=reflectionUniforms;
    const mesh=this.volumeMesh=new THREE.Object3D();
    this.volumeBatch={nativeKind:EFFECT.VOLUME.ID,family:'volume',variant:0,texture:null,count:0,capacity,
      arrays:{origin:new Float32Array(capacity*3),direction:new Float32Array(capacity*3),extent:new Float32Array(capacity*3),kind:new Float32Array(capacity)},
      mesh,values:new Float64Array(EFFECT.VOLUME.LENGTH),prepareResources:resources=>this.prepareVolumeResources(resources)};
    const v=this.volumeBatch.values;v[EFFECT.VOLUME.HAS_DEPTH]=1;
    for(let i=0;i<4;i++)v[EFFECT.VOLUME.REFLECTION_MATRIX+i*5]=1;
    this.volumeSamples=new Int32Array(2);this.volumeSamples.fill(-1);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;this.mesh.name='environment-volumes';
    sceneProperty(this.mesh,"frustumCulled",false);sceneProperty(this.mesh,"renderOrder",10);sceneProperty(this.mesh,"visible",false);
    environmentVolumes.set(this.mesh,this);scene.add(this.mesh);
  }
  // Ports EnvironmentVolume.prepare. Target descriptors remain native; only
  // the actual viewport dimensions are retained by the shared context host.
  prepare(renderer,scene,camera) {
    const captured=particleDepth.capture(renderer,scene,camera,true);
    this.values[EFFECT.VOLUME_COMPOSITE.ENABLED]=captured?1:0;if(!captured)return false;
    const source=renderer.getRenderTarget();
    this.resources.resizeEffectTarget(this.target,Math.max(1,Math.ceil(source.width*this.scale)),Math.max(1,Math.ceil(source.height*this.scale)));
    this.values[EFFECT.VOLUME_COMPOSITE.VOLUME_RESOLUTION]=this.volumeBatch.values[EFFECT.VOLUME.VOLUME_RESOLUTION]=this.target.width;
    this.values[EFFECT.VOLUME_COMPOSITE.VOLUME_RESOLUTION+1]=this.volumeBatch.values[EFFECT.VOLUME.VOLUME_RESOLUTION+1]=this.target.height;
    return true;
  }
  update(time,volumes) {
    const batch=this.volumeBatch,v=batch.values,a=batch.arrays;
    sceneProperty(this.mesh,"visible",volumes.length>0);batch.count=Math.min(volumes.length,batch.capacity);
    v[EFFECT.VOLUME.VOLUME_TIME]=time;v.set(this.wakes.host.wakePoints,EFFECT.VOLUME.WAKES);v.set(this.wakes.host.wakeMotion,EFFECT.VOLUME.MOTION);
    const reflection=this.reflectionUniforms;
    if(reflection.reflectionMatrix)v.set(reflection.reflectionMatrix.value.elements,EFFECT.VOLUME.REFLECTION_MATRIX);
    v[EFFECT.VOLUME.HAS_REFLECTION]=reflection.hasReflection?.value||0;
    for(let i=0;i<batch.count;i++){const source=volumes[i];source.position.toArray(a.origin,i*3);source.direction.toArray(a.direction,i*3);source.extent.toArray(a.extent,i*3);a.kind[i]=source.kind;}
  }
  prepareResources(resources) {
    this.resources=resources;this.target=resources.effectTarget(EFFECT_TARGET.VOLUME);this.samples[0]=this.target.texture;return this.samples;
  }
  prepareVolumeResources(resources) {
    this.volumeSamples[0]=resources.gpu.effect_noise(1);
    const reflection=this.reflectionUniforms.reflection?.value;
    this.volumeSamples[1]=reflection?resources.staticSample(reflection):-1;return this.volumeSamples;
  }
  dispose() {
    this.mesh.removeFromParent();
    if(this.resources){this.resources.gpu.dispose_sample(this.volumeSamples[0]);this.resources.disposeEffectTarget(this.target);}
  }
}
