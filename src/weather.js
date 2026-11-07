import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { RainImpacts } from './rain-impacts.js';
import { RainSheets } from './rain-sheets.js';
import { WeatherHost } from './environment/module.js';
import { WeatherFrame as WEATHER_FRAME, WeatherResult as RESULT } from './environment/layout.js';

// The rain's clock, the camera's motion, the cover cache, the splashes and
// which of the draws show are a Rust phase's (rust/bfe-environment,
// weather.rs); the page keeps the meshes, materials and uniforms, and answers
// the phase's two queries of its collision world.
export class Weather {
  constructor({data, scene, collision, water, settings, surfaceOwner}) {
    // The rain's authored numbers: its range (drops fall within this of the eye) and
    // how many drops the classic rain has; the Enhanced rain uses up to 32 times
    // as many (the geometry's capacity), within 8192.
    Object.assign(this,{collision,water,settings});const p=data.precipitation;
    this.nativeRange=THREE.MathUtils.clamp(p.Range?.[0]??12.5,4,40);
    this.nativeCount=THREE.MathUtils.clamp(Math.round(p.ParticleDensity?.[0]??256),32,2048);
    this.capacity=Math.min(8192,this.nativeCount*32);
    // The mode decides the cover cache's range and size: the Enhanced rain reaches
    // farther and needs a finer cache.
    const enhanced=settings?.get('rainEffects')==='enhanced';
    // A roof or floor over a cell: what a cast straight down first hits, or
    // open water over it if higher.
    // The phase asks the page two things of its collision world. A roof or floor
    // over a cell: what a cast straight down first hits, or open water over it
    // if higher.
    const down=new THREE.Vector3(0,-1,0),from=new THREE.Vector3(),hit=makeHit(),normal=new Float64Array(4);
    const height=(x,y,z)=>{const found=collision.cast(from.set(x,y,z),down,512,hit);let at=found?found.point.y:-100000;if(water?.over(x,z))at=Math.max(at,water.level);return at;};
    // A splash's cast: where it ends, its normal and whether that is water.
    // A splash's cast: where it ends, its normal and whether that is water (the
    // normal and the flag are left in `normal` for the phase to read).
    const impact=(x,y,z)=>{
      const found=collision.cast(from.set(x,y,z),down,180,hit);
      let at=found?.point.y??-Infinity;const n=found?.normal;
      const wet=Boolean(water?.over(x,z)&&water.level>at);
      if(wet)at=water.level;
      if(n){normal[0]=n.x;normal[1]=n.y;normal[2]=n.z;}else normal[0]=normal[1]=normal[2]=0;
      normal[3]=wet?1:0;return at;
    };
    // The rain's clock, camera motion, cover cache and splashes are Rust's; this
    // keeps the meshes, materials and uniforms the shaders read.
    this.host=new WeatherHost(surfaceOwner,this.capacity,enhanced,this.rangeFor(enhanced),enhanced?32:16,height,impact,normal);
    // The shader's values: where the eye is, how it moves, the wind, the drops'
    // look, and the cover texture its drops are clipped against.
    // Weather owns only phase facts and persistent seed/texel arrays. Rust
    // selects the rain plane, material, sampler slots and cover image layout.
    this.nativeKind=EFFECT.RAIN.ID;this.family='rain';this.variant=0;this.texture=null;
    this.visibility='anchor';this.arrays={seed:this.host.seeds};this.resources=null;
    this.samples=new Int32Array(1);this.samples.fill(-1);
    this.values=new Float64Array(EFFECT.RAIN.LENGTH);
    const v=this.values,c=p.Color||[216,220,228],wind=data.wind||[0,0],alpha=p.AlphaMinMax||[.3,.45];
    v.set(wind,EFFECT.RAIN.WIND);v.set(alpha,EFFECT.RAIN.ALPHA);
    for(let i=0;i<3;i++)v[EFFECT.RAIN.COLOR+i]=c[i]/255;
    v[EFFECT.RAIN.VELOCITY]=p.Velocity?.[0]??9;v[EFFECT.RAIN.VELOCITY_RANGE]=p.VelocityRange?.[0]??1;
    v[EFFECT.RAIN.WIDTH]=p.ParticleSize?.[0]??.02;v[EFFECT.RAIN.STREAK]=p.StreakLength?.[0]??1;
    this.mesh=new THREE.Object3D();this.mesh.name='map-precipitation';this.mesh.userData.effectBatch=this;
    this.mist=new THREE.Object3D();this.mist.name='rain-ground-mist';
    const mist=this.mistBatch={nativeKind:EFFECT.RAIN.ID,family:'rain',variant:0,texture:null,visibility:'anchor',
      count:48,capacity:48,arrays:{seed:this.host.mist},values:new Float64Array(EFFECT.RAIN.LENGTH),mesh:this.mist,
      prepareResources:resources=>this.prepareResources(resources)};
    this.mist.userData.effectBatch=mist;
    // None of them is culled by three: the drops fill the view, and their
    // visibility is the phase's call (it hides all of them until the cover cache
    // holds every cell around the eye).
    for(const mesh of [this.mesh,this.mist]){mesh.frustumCulled=false;mesh.visible=false;mesh.renderOrder=20;mesh.onBeforeRender=(r,s,c)=>{if(this.enhanced)particleDepth.capture(r,s,c);};scene.add(mesh);}
    // The splashes and the mid-distance sheets, drawn only in the Enhanced rain.
    this.impacts=new RainImpacts({scene,locations:this.host.locations,normals:this.host.normals});
    this.sheets=new RainSheets(scene,this);
    this.cover={heights:null,size:0,cell:0,queries:0};
    this.applyMode(true);
    this.host.bindArray(this.arrays,'seed',h=>h.seeds);this.host.bindArray(this.mistBatch.arrays,'seed',h=>h.mist);
    this.host.bindArray(this.impacts,'locations',h=>h.locations);this.host.bindArray(this.impacts,'normals',h=>h.normals);
    this.host.bindArray(this.impacts.arrays,'impact',h=>h.locations);this.host.bindArray(this.impacts.arrays,'surfaceNormal',h=>h.normals);
    this.host.bindArray(this.cover,'heights',h=>h.cover.subarray(0,this.cover.size*this.cover.size));
    this.unsubscribe=settings?.onChange(key=>{if(key==='rainEffects')this.applyMode(false);});
  }
  // How far the rain reaches in a mode.
  rangeFor(enhanced){return enhanced?Math.min(40,this.nativeRange*3.2):this.nativeRange;}
  // Classic or Enhanced rain, live: a new cover cache of the mode's range and
  // size (filled whole in the next frame), the splashes hidden until then.
  // Classic or Enhanced rain, live: a new cover cache of the mode's range and size
  // (filled whole in the next frame), the splashes hidden until then. The mode's
  // first call (the constructor's) leaves the phase as it was built.
  applyMode(first){
    this.enhanced=this.settings?.get('rainEffects')==='enhanced';
    this.range=this.rangeFor(this.enhanced);
    this.count=this.enhanced?this.capacity:this.nativeCount;

    const size=this.enhanced?32:16;
    if(!first)this.host.mode(this.enhanced,this.range,size);
    const heights=this.host.cover.subarray(0,size*size);
    Object.assign(this.cover,{heights,size,cell:this.host.cell,queries:0});
    const v=this.values;v[EFFECT.RAIN.ENHANCED]=this.enhanced?1:0;v[EFFECT.RAIN.RANGE]=this.range;
    v[EFFECT.RAIN.COVER_CELL]=this.cover.cell;v[EFFECT.RAIN.COVER_SIZE]=size;
    this.mistBatch.values.set(v);this.mistBatch.values[EFFECT.RAIN.EFFECT]=1;
    if(this.resources){this.resources.gpu.weather_cover(this.samples[0],size,heights);this.imageBuffer=this.host.buffer;}
    this.mist.visible=false;this.impacts.mesh.visible=false;
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
    const r=this.host.result,v=this.values;
    v[EFFECT.RAIN.RAIN_TIME]=r[RESULT.RAIN_TIME];
    for(let i=0;i<3;i++){v[EFFECT.RAIN.EYE+i]=r[RESULT.EYE+i];v[EFFECT.RAIN.CAMERA_MOTION+i]=r[RESULT.CAMERA_MOTION+i];}
    this.mistBatch.values.set(v);this.mistBatch.values[EFFECT.RAIN.EFFECT]=1;this.sheets.publishFrame();
    if(r[RESULT.CHANGED]&&this.resources)this.resources.gpu.effect_pixels(this.samples[0],this.cover.heights);
    this.cover.queries=r[RESULT.COVER_QUERIES];
    this.mesh.visible=r[RESULT.COVERED]===1;
    this.mist.visible=r[RESULT.MIST_VISIBLE]===1;
    this.sheets.mesh.visible=r[RESULT.SHEETS_VISIBLE]===1;
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
