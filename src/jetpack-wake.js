import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";
import { particleDepth } from "./particle-depth.js";

import {EffectPhase} from './effects-gpu.js';
import {WakeFrame as W,JetNozzle as N,WakeParticle as P,VisualLimits as L} from './presentation/visual-effects-layout.js';
const WAKE_BANKS=[['input',Float64Array],['nozzles',Float64Array],['particles',Float32Array]];
const POOL_BASE=[0,96,224];
// Nozzle rows carry authored facts; interpolation and normalization are native.
export function writeNozzles(out,nozzles) {
  for(let i=0;i<4;i++){
    const at=i*N.STRIDE,n=nozzles[i];out.fill(0,at,at+N.STRIDE);if(!n)continue;
    n.position.toArray(out,at+N.POSITION);n.direction.toArray(out,at+N.DIRECTION);out[at+N.RADIUS]=n.radius??0.034;
    const previous=n.previous?.position||n.previous;
    if(previous?.isVector3){previous.toArray(out,at+N.PREVIOUS);out[at+N.HAS_PREVIOUS]=1;}
    if(n.bendWorld)n.bendWorld.toArray(out,at+N.BEND);
  }
}
const ATTRIBUTES = { birth: 4, flow: 4, axis: 4, style: 2 };
const TYPES=[{name:'combustion',capacity:L.WAKE_COMBUSTION,drag:7},{name:'smoke',capacity:L.WAKE_SMOKE,drag:3},{name:'embers',capacity:L.WAKE_EMBERS,drag:1.8}];

// The pools by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a pool is `external`, three is shown
// it never and uploads it nothing.
export const jetpackWakes = new WeakMap();

// A pool of one kind of wake particle. Their clock is the wake's own
// (wakeTime, not the frame's time).
class WakePool {
  constructor(scene, kind) {
    this.kind = kind;
    this.spec = TYPES[kind];
    this.capacity = this.spec.capacity;
    this.count = 0;
    this.external = false;
    this.dirty = false;
    // WakePool's capacities and birth/flow records are gameplay presentation
    // facts. Native descriptors own the common plane and each kind's blend.
    this.nativeKind=EFFECT.WAKE.ID;this.family='wake';this.variant=kind;this.texture=null;
    this.values=new Float64Array(EFFECT.WAKE.LENGTH);
    this.values[EFFECT.WAKE.KIND]=kind;this.values[EFFECT.WAKE.DRAG]=this.spec.drag;
    this.arrays={};
    for(const [name,size] of Object.entries(ATTRIBUTES))this.arrays[name]=new Float32Array(this.capacity*size);
    this.streams=Object.values(this.arrays);this.widths=Object.values(ATTRIBUTES);
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name = `jetpack-wake-${this.spec.name}`;
    sceneProperty(this.mesh,"visible",false);
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",kind === 1 ? 8 : 11);
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    jetpackWakes.set(this.mesh, this);
    scene.add(this.mesh);
  }

  publish(records, base, count, time, dirty) {
    this.count=count;this.dirty=Boolean(dirty);this.values[EFFECT.WAKE.WAKE_TIME]=time;
    for(let i=0;i<count;i++){
      const at=(base+i)*P.STRIDE,j=i*4;
      for(let k=0;k<4;k++){this.arrays.birth[j+k]=records[at+P.POSITION+k];this.arrays.flow[j+k]=records[at+P.VELOCITY+k];this.arrays.axis[j+k]=records[at+P.DIRECTION+k];}
      this.arrays.style[i*2]=records[at+P.SEED];this.arrays.style[i*2+1]=records[at+P.OPACITY];
    }
    sceneProperty(this.mesh,"visible",count>0);
  }
  clear() {this.count=0;sceneProperty(this.mesh,"visible",false);this.dirty=false;}

}

export class JetpackWake {
  constructor({ scene, random = Math.random }) {
    this.random = random;
    this.time = 0;
    this.emitted = 0;
    this.pools = TYPES.map((_, kind) => new WakePool(scene, kind));
    [this.wisps, this.smoke, this.embers] = this.pools;
    this.phase=new EffectPhase('PageWake',WAKE_BANKS);
  }
  publish(frame,records) {
    this.time=frame[W.TIME];this.emitted=frame[W.EMITTED];
    for(let k=0;k<3;k++)this.pools[k].publish(records,POOL_BASE[k],frame[W.COUNTS+k],this.time,frame[W.DIRTY+k]);
  }
  update(dt, facts) {
    this.phase.refresh();const x=this.phase.input;
    x[W.DT]=dt;x[W.THROTTLE]=facts.throttle??1;x[W.ACTIVE]=facts.active?1:0;x[W.NOZZLES]=facts.nozzles.length;
    facts.velocity.toArray(x,W.VELOCITY);
    writeNozzles(this.phase.nozzles,facts.nozzles);
    this.phase.frame(this.random);this.publish(this.phase.input,this.phase.particles);
  }
  clear() {
    this.phase.refresh();this.phase.input[W.CLEAR]=1;this.phase.frame(this.random);
    for(const pool of this.pools)pool.clear();
  }
  dispose() {for(const pool of this.pools)pool.mesh.removeFromParent();this.phase.dispose();}
}
