import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentRecipe as RECIPE} from "./environment/layout.js";
import { environmentRecipe } from "./environment/module.js";
import * as THREE from "three";
import {actorDescription,actorNode} from "./actor-resources.js";
import {ACTOR} from "./rust-render/actor-layout.js";
import { SkyHost } from "./environment/module.js";
import { SkyFrame as SKY_FRAME, SkyResult as SKY_RESULT, SkyPublication as PUBLICATION } from "./environment/layout.js";
import { modelPack } from "./models.js";
import { motionVectors } from "./motion-vectors.js";
import { SkyProbe } from "./sky-probe.js";

// Sky flight, fading, class selection, depth order and motion live in Rust.
// The page retains model/texture identities and borrows the resulting arrays.
// The setting whose Enhanced choice turns the Enhanced sky on.
export const SKY_SETTING = "oceanEffects";



// The flyers a battle draws: its mission's classes from flyers.json, as the
// original draws them or (`enhanced`) as the Enhanced sky does. A map
// prepared before the Enhanced lists has only the original's.
export function battleFlyers(data, mission, enhanced = false) {
  return environmentRecipe(RECIPE.FLYERS, {data,mission,enhanced}).map(i => data.flyers[i]);
}

// Whether the Enhanced sky is on.
export const enhancedSky = (settings) => settings?.get?.(SKY_SETTING) === "enhanced";

// The flight (launch, fly, fade, placement, the far plane's reach, the haze),
// the order of the draws and their instance records are a Rust phase's
// (rust/bfe-environment, sky.rs). The page keeps the meshes, materials and
// the sky probe the haze reads.
export class SkyFlyers {
  // data: flyers.json; buffer, indexBuffer: flyers.bin and flyers_index.bin;
  // texture(name): the sky folder's textures; scene: the far scene;
  // graphics: its far camera (Enhanced's reach), renderer (the sky probe)
  // and settings; seed: the ships' random draws (the engine's generator,
  // native environment); motion: the temporal upscaler's motion pass.
  constructor({ data, mission, buffer, indexBuffer, texture = () => null, scene = null, seed = 1,
    graphics = null, settings = graphics?.settings ?? null, motion = motionVectors, surfaceOwner }) {
    this.graphics = graphics;
    this.settings = settings;
    this.motion = motion;
    this.group = new THREE.Group();
    this.group.name = "sky-flyers";
    sceneProperty(this.group,"matrixAutoUpdate",false);
    this.classes = [];
    this.time = 0;
    this.probe = null;
    this.enhanced = enhancedSky(settings);
    const models=modelPack(data?.models,buffer,indexBuffer),bounds={};
    for(const [name,parts]of models)bounds[name]=parts.map(part=>actorDescription(part.asset).bounds);
    this.host=new SkyHost(surfaceOwner,0,{data,mission,models:bounds,seed,enhanced:this.enhanced});
    this.host.recipe.classes.forEach((row,k)=>{
      const def=data.flyers[row.index];this.classes.push(this.build(def,models.get(def.model),texture,row,row.radius,k));
    });
    this.unsubscribe = settings?.onChange?.((key) => { if (key === SKY_SETTING) this.setEnhanced(enhancedSky(settings)); }) || null;
    this.place(null);
    scene?.add(this.group);
  }

  // A class's draws: per model part one instanced mesh for its ships (all of
  // them in Classic, the solid ones in Enhanced), one for Enhanced's fading
  // ships, and its motion. Their instance records are views onto the phase's.
  build(def, parts, texture, modes, radius, index) {
    const count = def.count;
    const records = this.host.arrays(index);
    const solid={matrices:records[0],looks:records[1]},fading={matrices:records[2],looks:records[3]},motion={matrices:records[4],previous:records[5],shown:records[6]};
    for(const [target,key,at]of [[solid,'matrices',0],[solid,'looks',1],[fading,'matrices',2],[fading,'looks',3],[motion,'matrices',4],[motion,'previous',5],[motion,'shown',6]])this.host.bindArray(target,key,h=>h.arrays(index)[at]);
    // SkyFlyers.build publishes borrowed environment arrays and authored model
    // facts. Native far/motion descriptors own every VBO, sampler and raster rule.
    const mesh=(part,arrays,order,fading=false,moving=false,map=null)=>{
      const m=actorNode(part.asset,null);m.name=def.model;sceneProperty(m,"frustumCulled",false);sceneProperty(m,"matrixAutoUpdate",false);sceneProperty(m,"renderOrder",order);m.count=count;
      const source={asset:part.asset,flags:part.flags??1,map,arrays,capacity:count,fading,moving,owner:this,values:new Float64Array(ACTOR.FLYER.LEN)};
      m.userData[moving?'flyerMotion':'flyerSource']=source;if(moving){m.userData.order=0;m.onBeforeRender=()=>this.motionFrame();}return m;
    };
    const draws=[],fades=[],motions=[];
    for(const part of parts){const map=texture(part.texture);draws.push(mesh(part,solid,1,false,false,map));fades.push(mesh(part,fading,2,true,false,map));motions.push(mesh(part,motion,0,false,true));}
    for(const m of [...draws,...fades])this.group.add(m);for(const m of motions)this.motion?.addExtra?.(m);
    return {def,...modes,index,radius,solid,fading,motion,draws,fades,motions,meshes:[...draws,...fades,...motions]};
  }

  // Whether a class flies in the current mode.
  active(kind) {
    return this.host.x.sky_active(kind.index)!==0;
  }

  // Classic or Enhanced, as the setting changes: classes coming in start
  // afresh; ships flying on keep their course under the new rules.
  setEnhanced(enhanced) {
    if(!this.host.x.sky_set_enhanced(enhanced?1:0))return;
    this.enhanced=enhanced;this.host.refresh(true);
    this.place(null);
  }

  get count() {
    return this.host.x.sky_count();
  }

  prepareFrame(dt, camera = null) {
    this.host.refresh();
    this.frameCamera = camera;
    if(this.host.x.sky_probe_needed()&&camera)this.prepareProbe();
    this.writeFrame(camera?.position || null, dt);
  }
  prepareProbe() {
    if (!this.graphics?.renderer || this.probe) return;
    this.probe = new SkyProbe(this.graphics, 64, 1); this.probe.enabled = true;

  }

  // Enhanced: the sky behind the ships (sky-probe.js), for their haze.
  // The phase's frame (flight over dt, then the layout of the draws from
  // the eye, none without one), and the meshes set from what it left.
  place(eye) {
    this.frameCamera = null;
    this.writeFrame(eye, 0); this.host.x.sky_update(); this.publishFrame();
  }
  writeFrame(eye, dt = 0) {
    this.host.refresh();
    const frame = this.host.frame;
    frame[SKY_FRAME.DT] = dt;
    frame[SKY_FRAME.HAS_EYE] = eye ? 1 : 0;
    if (eye) { frame[SKY_FRAME.EYE] = eye.x; frame[SKY_FRAME.EYE + 1] = eye.y; frame[SKY_FRAME.EYE + 2] = eye.z; }
    const far = this.graphics?.farCamera?.far || 0;
    frame[SKY_FRAME.FAR] = far;
    frame[SKY_FRAME.VISIBLE] = this.group.visible ? 1 : 0;
    frame[SKY_FRAME.MISTY] = this.probe ? 1 : 0;
  }
  publishFrame() {
    this.host.refresh();
    const r = this.host.result;
    this.time=r[SKY_RESULT.TIME]??0;
    if(this.host.x.sky_probe_needed()&&this.frameCamera&&this.probe) this.probe.update(this.time, this.frameCamera);
    const p=this.host.publication;
    for(const kind of this.classes){
      const pt=kind.index*PUBLICATION.STRIDE;
      for(const m of kind.draws){m.count=p[pt+PUBLICATION.SOLID_COUNT];sceneProperty(m,'visible',Boolean(p[pt+PUBLICATION.SOLID_VISIBLE]));m.userData.flyerSource.values[ACTOR.FLYER.REACH]=p[pt+PUBLICATION.REACH];}
      for(const m of kind.fades){m.count=p[pt+PUBLICATION.FADING_COUNT];sceneProperty(m,'visible',Boolean(p[pt+PUBLICATION.FADE_VISIBLE]));m.userData.flyerSource.values[ACTOR.FLYER.REACH]=p[pt+PUBLICATION.REACH];}
      for(const m of kind.motions)sceneProperty(m,'visible',Boolean(p[pt+PUBLICATION.MOTION_VISIBLE]));
    }
  }

  // Before the motion pass draws a class (once a frame, motion-vectors.js):
  // a frame with no flight since the last one (paused) shows no motion.
  motionFrame() {
    this.still=this.host.x.page_sky_motion(this.motion?.frame??0);
  }

  dispose() {
    this.unsubscribe?.();
    this.group.removeFromParent();
    for (const kind of this.classes) {
      for (const m of kind.motions) {
        this.motion?.removeExtra?.(m);

      }
    }
    this.probe?.dispose();
    this.probe = null;
    this.classes = [];
  }
}
