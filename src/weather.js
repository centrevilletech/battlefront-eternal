import {sceneProperty} from "./rust-render/scene-changes.js";
import {EnvironmentDraw as DRAW, WeatherMode as MODE} from "./environment/layout.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { RainImpacts } from './rain-impacts.js';
import { RainSheets } from './rain-sheets.js';
import { WeatherHost,drawValues } from './environment/module.js';
import { WeatherFrame as WEATHER_FRAME, WeatherResult as RESULT } from './environment/layout.js';

// The rain's clock, the camera's motion, the cover cache, the splashes and
// which of the draws show are a Rust phase's (rust/bfe-environment,
// weather.rs); the page keeps the meshes, materials and uniforms, and answers
// the phase's two queries of its collision world.
export class Weather {
  constructor({data, scene, collision, water, settings, surfaceOwner}) {
    Object.assign(this,{collision,water,settings});
    const enhanced=settings?.get('rainEffects')==='enhanced';
    // These callbacks copy actual collision resources; Rust combines heights.
    const down=new THREE.Vector3(0,-1,0),from=new THREE.Vector3(),hit=makeHit(),normal=new Float64Array(4);
    surfaceOwner.attach({host_water_over:(x,z)=>water?.over(x,z)?1:0,host_water_level:()=>water?.level??0});
    const height=(x,y,z)=>collision.cast(from.set(x,y,z),down,512,hit)?.point.y??-100000;
    const impact=(x,y,z)=>{
      const found=collision.cast(from.set(x,y,z),down,180,hit),n=found?.normal;
      if(n){normal[0]=n.x;normal[1]=n.y;normal[2]=n.z;}else normal[0]=normal[1]=normal[2]=0;
      return found?.point.y??-Infinity;
    };
    // The rain's clock, camera motion, cover cache and splashes are Rust's; this
    // keeps the meshes, materials and uniforms the shaders read.
    this.host=new WeatherHost(surfaceOwner,0,false,0,0,height,impact,normal,{data,enhanced});
    this.recipe=this.host.recipe;this.nativeRange=this.recipe.range;this.nativeCount=this.recipe.count;this.capacity=this.recipe.capacity;
    // The shader's values: where the eye is, how it moves, the wind, the drops'
    // look, and the cover texture its drops are clipped against.
    // Weather owns only phase facts and persistent seed/texel arrays. Rust
    // selects the rain plane, material, sampler slots and cover image layout.
    this.nativeKind=EFFECT.RAIN.ID;this.family='rain';this.variant=0;this.texture=null;
    this.visibility='anchor';this.arrays={seed:this.host.seeds};this.resources=null;
    this.samples=new Int32Array(1);this.samples.fill(-1);
    drawValues(this.host,this,DRAW.RAIN,EFFECT.RAIN);
    this.mesh=new THREE.Object3D();this.mesh.name='map-precipitation';this.mesh.userData.effectBatch=this;
    this.mist=new THREE.Object3D();this.mist.name='rain-ground-mist';
    const mist=this.mistBatch={nativeKind:EFFECT.RAIN.ID,family:'rain',variant:0,texture:null,visibility:'anchor',
      count:48,capacity:48,arrays:{seed:this.host.mist},mesh:this.mist,
      prepareResources:resources=>this.prepareResources(resources)};
    drawValues(this.host,mist,DRAW.MIST,EFFECT.RAIN);
    this.mist.userData.effectBatch=mist;
    // None of them is culled by three: the drops fill the view, and their
    // visibility is the phase's call (it hides all of them until the cover cache
    // holds every cell around the eye).
    for(const mesh of [this.mesh,this.mist]){sceneProperty(mesh,"frustumCulled",false);sceneProperty(mesh,"visible",false);sceneProperty(mesh,"renderOrder",20);mesh.onBeforeRender=(r,s,c)=>{if(this.enhanced)particleDepth.capture(r,s,c);};scene.add(mesh);}
    // The splashes and the mid-distance sheets, drawn only in the Enhanced rain.
    this.impacts=new RainImpacts({scene,locations:this.host.locations,normals:this.host.normals});
    this.sheets=new RainSheets(scene,this);
    this.cover={heights:null,size:0,cell:0,queries:0};
    this.applyMode(true);
    this.host.bindArray(this.arrays,'seed',h=>h.seeds);this.host.bindArray(this.mistBatch.arrays,'seed',h=>h.mist);
    this.host.bindArray(this.impacts,'locations',h=>h.locations);this.host.bindArray(this.impacts,'normals',h=>h.normals);
    this.host.bindArray(this.impacts.arrays,'impact',h=>h.locations);this.host.bindArray(this.impacts.arrays,'surfaceNormal',h=>h.normals);
    this.host.bindArray(this.cover,'heights',h=>h.cover);
    this.unsubscribe=settings?.onChange(key=>{if(key==='rainEffects')this.applyMode(false);});
  }
  // Settings cross as a raw mode observation. Count, reach and cache shape
  // come back from the retained native policy, including changes while paused.
  applyMode(first){
    if(!first){this.host.x.page_weather_mode(this.settings?.get('rainEffects')==='enhanced'?1:0);this.host.refresh(true);}
    const m=this.host.modeResult;this.enhanced=Boolean(m[MODE.ENHANCED]);this.range=m[MODE.RANGE];this.count=m[MODE.COUNT];
    const size=m[MODE.SIZE],heights=this.host.cover;
    Object.assign(this.cover,{heights,size,cell:m[MODE.CELL],queries:0});
    if(this.resources){this.resources.gpu.weather_cover(this.samples[0],size,heights);this.imageBuffer=this.host.buffer;}
    sceneProperty(this.mist,'visible',false);sceneProperty(this.impacts.mesh,'visible',false);
  }
  // The frame: the camera's place and the time go to the phase, which advances
  // the clock, the camera's motion, the cover cache and the splashes; what it
  // left goes to the uniforms and the draws' visibility.
  prepareFrame(dt,camera){
    this.host.refresh();
    const frame=this.host.frame;frame[WEATHER_FRAME.DT]=dt;
    frame[WEATHER_FRAME.POSITION]=camera.position.x;frame[WEATHER_FRAME.POSITION+1]=camera.position.y;frame[WEATHER_FRAME.POSITION+2]=camera.position.z;
  }
  publishFrame() {
    this.host.refresh();
    if(this.resources&&this.imageBuffer!==this.host.buffer){
      this.resources.gpu.weather_cover(this.samples[0],this.cover.size,this.cover.heights);this.imageBuffer=this.host.buffer;
    }
    const r=this.host.result;
    if(r[RESULT.CHANGED]&&this.resources)this.resources.gpu.effect_pixels(this.samples[0],this.cover.heights);
    this.cover.queries=r[RESULT.COVER_QUERIES];
    sceneProperty(this.mesh,"visible",r[RESULT.COVERED]===1);
    sceneProperty(this.mist,"visible",r[RESULT.MIST_VISIBLE]===1);
    sceneProperty(this.sheets.mesh,"visible",r[RESULT.SHEETS_VISIBLE]===1);
    this.impacts.synced(r[RESULT.IMPACT_TIME],r[RESULT.IMPACTS_SHOWN]===1,r[RESULT.IMPACTS_CHANGED]===1,r[RESULT.IMPACTS_NEXT],r[RESULT.IMPACTS_QUERIES]);
  }
  // Stable native sampler identity survives mode changes. Resizing publishes
  // a typed cover command, without a replacement Texture or a version scan.
  prepareResources(resources){
    if(!this.resources){this.host.refresh();this.resources=resources;this.imageBuffer=this.host.buffer;this.samples[0]=resources.gpu.effect_data(3,this.cover.size,this.cover.heights);}
    return this.samples;
  }
  dispose(){this.unsubscribe?.();this.mesh.removeFromParent();this.mist.removeFromParent();this.impacts.dispose();this.sheets.dispose();if(this.resources)this.resources.gpu.dispose_sample(this.samples[0]);}
}
