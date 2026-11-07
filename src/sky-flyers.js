import * as THREE from "three";
import {actorDescription,actorNode} from "./actor-resources.js";
import {ACTOR} from "./rust-render/actor-layout.js";
import { SkyHost } from "./environment/module.js";
import { SkyClass as SKY_CLASS, SkyConfig as SKY_CONFIG, SkyFrame as SKY_FRAME, SkyResult as SKY_RESULT } from "./environment/layout.js";
import { lighting } from "./materials.js";
import { modelPack } from "./models.js";
import { motionUniforms, motionVectors } from "./motion-vectors.js";
import { SkyProbe } from "./sky-probe.js";

// The sky's flyers: the sky config's SkyObjects (maps/<map>/sky/flyers.json,
// tools/zero_sky.py), flown as the 2004 game flies them (SkyObjectClass;
// docs/formats.md, "Sky flyers"):
//
//   - A class keeps NumObjects ships in the sky all the time. A ship starts
//     Distance from the world's origin in a random direction, Height above
//     it, fading in over 4 s. It heads across that direction, either way,
//     turned inward by a random InDirectionFactor / 2 to InDirectionFactor
//     times the inward direction, level, nose along its heading.
//   - It flies at a velocity drawn once in its own axes (VelocityX right,
//     VelocityY up, VelocityZ forward, each between its min and max), which
//     Acceleration (in its axes too) adds to every frame. It never turns or
//     banks.
//   - Once it is more than Distance + 10 m from the origin across the
//     ground, or older than LifeTime (its age starts at 80% to 100% of it,
//     so it lives up to a fifth of LifeTime), its fade is set to -1: drawn
//     as its absolute value, it jumps to full and fades out over 4 s, still
//     flying, and the ship starts again somewhere else.
//   - It is drawn in the far scene, after the dome, depth tested against
//     the far terrain, without fog; lit by the map's ambient and sun, never
//     by lamps; blended while it fades, still writing depth.
//
// That is Classic: the battle's own classes (those its script loads), cut
// off by the far scene's far plane. Graphics > Ocean and sky's Enhanced
// (the default) keeps the flight and adds:
//
//   - every class of the battle's era (flyers.json `enhanced`), so Bespin:
//     Platforms has capital ships and Cloud City's Galactic Civil War battle
//     Star Destroyers;
//   - ships past the far plane drawn anyway, scaled toward the eye (their
//     image unchanged) with their depth just inside the far plane, so the
//     far scene's terrain still hides them; far off, low over the horizon,
//     they take on the sky behind them (sky-probe.js);
//   - smooth fades: a ship starts its fade-in 4 s back along its course, so
//     it is solid where the original starts it, and fades out from where it
//     is, without the jump;
//   - fading ships write no depth (drawn after the solid ones), so they never
//     hide a ship behind them.
//
// In both, ships draw their own motion for the temporal upscaler
// (motion-vectors.js), so a crossing fighter doesn't smear.
//
// Presentation only, in the native environment phase: no simulation reads it, it isn't in the
// content fingerprint, and each page draws its own random ships. A class is
// one native instanced draw per model part, sorted far to near (Enhanced: another
// for its fading ships).

// The setting whose Enhanced choice turns the Enhanced sky on.
export const SKY_SETTING = "oceanEffects";



// The flyers a battle draws: its mission's classes from flyers.json, as the
// original draws them or (`enhanced`) as the Enhanced sky does. A map
// prepared before the Enhanced lists has only the original's.
export function battleFlyers(data, mission, enhanced = false) {
  const table = enhanced && data?.enhanced ? data.enhanced : data?.missions;
  const indices = table?.[mission] || [];
  return indices.map((i) => data.flyers[i]).filter((def) => def && def.count > 0);
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
  // src/atmosphere.js); motion: the temporal upscaler's motion pass.
  constructor({ data, mission, buffer, indexBuffer, texture = () => null, scene = null, seed = 1,
    graphics = null, settings = graphics?.settings ?? null, motion = motionVectors, surfaceOwner }) {
    this.graphics = graphics;
    this.settings = settings;
    this.motion = motion;
    this.group = new THREE.Group();
    this.group.name = "sky-flyers";
    this.group.matrixAutoUpdate = false;
    this.classes = [];
    this.time = 0;
    this.stepped = false;
    this.probe = null;
    this.enhanced = enhancedSky(settings);
    const classic = new Set(battleFlyers(data, mission, false));
    const enhanced = new Set(battleFlyers(data, mission, true));
    const models = modelPack(data?.models, buffer, indexBuffer);
    const kept = [];
    for (const def of data?.flyers || []) {
      if (!classic.has(def) && !enhanced.has(def)) continue;
      const parts = models.get(def.model);
      if (!parts?.length) continue;
      kept.push({ def, parts, modes: { classic: classic.has(def), enhanced: enhanced.has(def) } });
    }
    // Rust keeps the ships; the page's draws are built over its records.
    this.host = new SkyHost(surfaceOwner, kept.length);
    const config = this.host.config;
    config[SKY_CONFIG.SEED] = seed;
    config[SKY_CONFIG.CLASSES] = kept.length;
    config[SKY_CONFIG.ENHANCED] = this.enhanced ? 1 : 0;
    const radii = kept.map(({ parts }) => parts.reduce((r, part) => {
      const b=actorDescription(part.asset).bounds;
      return Math.max(r,Math.sqrt(b[0]*b[0]+b[1]*b[1]+b[2]*b[2])+b[3]);
    }, 0));
    kept.forEach(({ def, modes }, i) => {
      const at = SKY_CONFIG.STRIDE + i * SKY_CLASS.STRIDE;
      config[at + SKY_CLASS.COUNT] = def.count;
      config[at + SKY_CLASS.DISTANCE] = def.distance;
      config.set(def.height, at + SKY_CLASS.HEIGHT);
      config[at + SKY_CLASS.IN_DIRECTION] = def.inDirection;
      def.velocity.forEach((range, k) => config.set(range, at + SKY_CLASS.VELOCITY + k * 2));
      config.set(def.acceleration, at + SKY_CLASS.ACCELERATION);
      config[at + SKY_CLASS.LIFE_TIME] = def.lifeTime;
      config[at + SKY_CLASS.RADIUS] = radii[i];
      config[at + SKY_CLASS.CLASSIC] = modes.classic ? 1 : 0;
      config[at + SKY_CLASS.ENHANCED] = modes.enhanced ? 1 : 0;
    });
    this.host.start();
    kept.forEach(({ def, parts, modes }, i) => this.classes.push(this.build(def, parts, texture, modes, radii[i], i)));
    this.motionChecked = -1;
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
      const m=actorNode(part.asset,null);m.name=def.model;m.frustumCulled=false;m.matrixAutoUpdate=false;m.renderOrder=order;m.count=count;
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
    return this.enhanced ? kind.enhanced : kind.classic;
  }

  // Classic or Enhanced, as the setting changes: classes coming in start
  // afresh; ships flying on keep their course under the new rules.
  setEnhanced(enhanced) {
    if (enhanced === this.enhanced) return;
    this.enhanced = enhanced;
    this.host.x.sky_set_enhanced(enhanced ? 1 : 0);this.host.refresh(true);
    this.place(null);
  }

  get count() {
    return this.classes.reduce((sum, kind) => sum + (this.active(kind) ? kind.def.count : 0), 0);
  }

  prepareFrame(dt, camera = null) {
    this.host.refresh();
    if (dt > 0) this.stepped = true;
    this.frameCamera = camera;
    if (this.enhanced && camera) this.prepareProbe();
    this.writeFrame(camera?.position || null, dt);
  }
  prepareProbe() {
    if (!this.graphics?.renderer || !this.classes.length || this.probe) return;
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
    this.time = r[SKY_RESULT.TIME] || 0;
    if (this.enhanced && this.frameCamera && this.probe) this.probe.update(this.time, this.frameCamera);
    const far = this.graphics?.farCamera?.far || 0;
    for (const kind of this.classes) {
      const at = kind.index * SKY_RESULT.STRIDE;
      if (!r[at + SKY_RESULT.ON]) {
        for (const m of kind.meshes) m.visible = false;
        continue;
      }
      const solids = r[at + SKY_RESULT.SOLIDS], fadings = r[at + SKY_RESULT.FADINGS];
      for (const m of kind.draws) {
        m.count = solids;
        m.visible = solids > 0;
        m.userData.flyerSource.values[ACTOR.FLYER.REACH]=far;
      }
      for (const m of kind.fades) {
        m.count = fadings;
        m.visible = fadings > 0;
        m.userData.flyerSource.values[ACTOR.FLYER.REACH]=far;
      }
      for (const m of kind.motions) m.visible = true;

    }
  }

  // Before the motion pass draws a class (once a frame, motion-vectors.js):
  // a frame with no flight since the last one (paused) shows no motion.
  motionFrame() {
    const frame = this.motion?.frame ?? 0;
    if (frame === this.motionChecked) return;
    this.motionChecked = frame;
    this.still=this.stepped?0:1;
    this.stepped = false;
  }

  dispose() {
    this.unsubscribe?.();
    this.group.removeFromParent();
    for (const kind of this.classes) {
      for (const m of [...kind.draws, ...kind.fades]) {

      }
      for (const m of kind.motions) {
        this.motion?.removeExtra?.(m);

      }
    }
    this.probe?.dispose();
    this.probe = null;
    this.classes = [];
  }
}
