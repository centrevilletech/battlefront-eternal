import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";
import { makeHit } from "./collision.js";
import { particleDepth } from "./particle-depth.js";

import {EffectPhase} from './effects-gpu.js';
import {TrailFrame as F,TrailParticle as P} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['records',Float32Array],['rays',Float64Array]];
const CAPACITY = 160;
const UP = new THREE.Vector3(0, 1, 0), DOWN = new THREE.Vector3(0, -1, 0);



// The trails by their meshes, for a renderer that draws them itself
// (src/rust-render/effects.js): while a trail is `external`, three is shown
// it never and uploads it nothing.
export const droidekaTrails = new WeakMap();

// A fixed-size pool of dust wisps. Physics only emits particles; their motion,
// expansion and turbulent breakup happen on the GPU. Roof/floor probes are
// cached, and paved outdoor courtyards produce a lighter trail than terrain.
// Its clock is its own (trailTime, not the frame's time).
export class DroidekaTrail {
  constructor({ scene, collision, water = null, settings, mapName = "", random = Math.random }) {
    this.collision = collision;
    this.water = water;
    this.settings = settings;
    this.random = random;
    this.time = 0;
    this.count = 0;
    this.external = false;
    this.emitted = 0;
    this.phase=new EffectPhase('PageTrail',BANKS,mapName);
    this.origin=new THREE.Vector3();this.hit=makeHit();
    this.records=new Float32Array(CAPACITY*P.STRIDE);
    // DroidekaTrail keeps only simulation/draw arrays. Its plane, raster
    // state and tint layout are one native descriptor admitted before drawing.
    this.nativeKind=EFFECT.TRAIL.ID;this.family='trail';this.variant=0;this.texture=null;
    this.values=new Float64Array(EFFECT.TRAIL.LENGTH);for(let k=0;k<3;k++)this.values[EFFECT.TRAIL.TINT+k]=this.phase.input[F.TINT+k];
    this.capacity=CAPACITY;
    this.arrays={birth:new Float32Array(CAPACITY*4),drift:new Float32Array(CAPACITY*4),shape:new Float32Array(CAPACITY*4)};
    this.streams=Object.values(this.arrays);
    this.mesh = new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name = "droideka-rolling-dust";
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",9);
    sceneProperty(this.mesh,"visible",false);
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    droidekaTrails.set(this.mesh, this);
    scene.add(this.mesh);
  }

  clear() {
    this.phase.refresh();this.phase.input[F.CLEAR]=1;this.phase.frame(this.random);
    this.count=0;sceneProperty(this.mesh,"visible",false);
  }
  update(dt,soldier,options) {
    const position=options?.position??soldier.position,dead=options?.dead??soldier.dead;
    const phase=this.phase;phase.refresh();const x=phase.input;
    x[F.DT]=dt;x[F.ENHANCED]=(this.settings?.get('droidekaEffects')??'enhanced')==='enhanced'?1:0;
    position.toArray(x,F.POSITION);soldier.velocity.toArray(x,F.VELOCITY);
    x[F.ROLLING]=soldier.mode===12?1:0;x[F.DEAD]=dead?1:0;x[F.AIR]=soldier.air?1:0;x[F.GROUNDED]=soldier.grounded===false?0:1;
    phase.kernel.probe();
    if(x[F.PROBE]){
      this.origin.fromArray(phase.rays);const floor=this.collision.cast(this.origin,DOWN,1.3,this.hit);
      x[F.FLOOR_HIT]=floor?1:0;x[F.FLOOR_NORMAL]=floor?.normal.y??0;x[F.FLOOR]=floor?.point.y??0;x[F.TERRAIN]=floor?.terrain?1:0;
      x[F.ROOF]=1;
      phase.kernel.roof();
      if(x[F.ROOF_PROBE]){this.origin.fromArray(phase.rays);x[F.ROOF]=this.collision.cast(this.origin,UP,80,this.hit)?1:0;}
    }
    x[F.WET]=this.water?.over(position.x,position.z)?1:0;x[F.WATER_LEVEL]=this.water?.level??NaN;
    phase.frame(this.random);this.time=x[F.TIME];this.count=x[F.COUNT];this.emitted=x[F.EMITTED];
    this.values[EFFECT.TRAIL.TRAIL_TIME]=this.time;this.records.set(phase.records);
    for(let i=0;i<this.count;i++)for(let a=0;a<3;a++)for(let k=0;k<4;k++)this.streams[a][i*4+k]=this.records[i*P.STRIDE+a*4+k];
    sceneProperty(this.mesh,"visible",this.count>0);
  }
  dispose(){this.phase.dispose();this.mesh.removeFromParent();}
}
