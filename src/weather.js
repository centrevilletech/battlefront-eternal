import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { RainImpacts } from './rain-impacts.js';
import { RainSheets } from './rain-sheets.js';
import { lighting } from './materials.js';
import { WeatherHost } from './environment/module.js';
import { WeatherFrame as WEATHER_FRAME, WeatherResult as RESULT } from './environment/layout.js';

// The instanced quads the drops are drawn as, their seeds the Rust phase's.
function geometry(count,seeds){
  const base=new THREE.PlaneGeometry(2,1),g=new THREE.InstancedBufferGeometry();g.setIndex(base.index.clone());
  g.setAttribute('position',base.attributes.position.clone());g.setAttribute('uv',base.attributes.uv.clone());base.dispose();
  g.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));g.instanceCount=count;return g;
}
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
    const c=p.Color||[216,220,228];
    const uniforms={eye:{value:new THREE.Vector3()},cameraMotion:{value:new THREE.Vector3()},wind:{value:new THREE.Vector2(...(data.wind||[0,0]))},
      rainTime:{value:0},range:{value:0},velocity:{value:p.Velocity?.[0]??9},velocityRange:{value:p.VelocityRange?.[0]??1},
      width:{value:p.ParticleSize?.[0]??.02},streak:{value:p.StreakLength?.[0]??1},color:{value:new THREE.Vector3(...c).multiplyScalar(1/255)},
      alpha:{value:new THREE.Vector2(...(p.AlphaMinMax||[.3,.45]))},cover:{value:null},coverCell:{value:0},coverSize:{value:0},
      enhanced:{value:0},effect:{value:0},ambient:lighting.ambient,sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange};
    const options={transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true};
    this.material=ownedEffectMaterial('effects/rain', {...options,uniforms});
    // The drops, and the mist they leave on the ground (the same shader in its
    // `effect` mode, forty-eight big soft quads).
    this.mesh=new THREE.Mesh(geometry(this.capacity,this.host.seeds),this.material);this.mesh.name='map-precipitation';
    this.mist=new THREE.Mesh(geometry(48,this.host.mist),ownedEffectMaterial('effects/rain', {...options,uniforms:{...uniforms,effect:{value:1}}}));this.mist.name='rain-ground-mist';
    // None of them is culled by three: the drops fill the view, and their
    // visibility is the phase's call (it hides all of them until the cover cache
    // holds every cell around the eye).
    for(const mesh of [this.mesh,this.mist]){mesh.frustumCulled=false;mesh.visible=false;mesh.renderOrder=20;mesh.onBeforeRender=(r,s,c)=>{if(this.enhanced)particleDepth.capture(r,s,c);};scene.add(mesh);}
    // The splashes and the mid-distance sheets, drawn only in the Enhanced rain.
    this.impacts=new RainImpacts({scene,locations:this.host.locations,normals:this.host.normals});
    this.sheets=new RainSheets(scene,uniforms,this.host.streaks);
    this.cover={heights:null,texture:null,size:0,cell:0,queries:0};
    this.applyMode(true);this.unsubscribe=settings?.onChange(key=>{if(key==='rainEffects')this.applyMode(false);});
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
    this.mesh.geometry.instanceCount=this.count;
    const size=this.enhanced?32:16;
    if(!first)this.host.mode(this.enhanced,this.range,size);
    this.cover.texture?.dispose();
    // The cache's texture reads the phase's own heights: no copy.
    const heights=this.host.cover.subarray(0,size*size);
    const texture=new THREE.DataTexture(heights,size,size,THREE.RedFormat,THREE.FloatType);
    texture.minFilter=texture.magFilter=THREE.NearestFilter;texture.needsUpdate=true;
    Object.assign(this.cover,{heights,texture,size,cell:this.host.cell,queries:0});
    const u=this.material.uniforms;u.enhanced.value=this.enhanced?1:0;u.range.value=this.range;
    u.cover.value=texture;u.coverCell.value=this.cover.cell;u.coverSize.value=size;
    this.mist.visible=false;this.impacts.mesh.visible=false;
  }
  // The frame: the camera's place and the time go to the phase, which advances
  // the clock, the camera's motion, the cover cache and the splashes; what it
  // left goes to the uniforms and the draws' visibility.
  prepareFrame(dt,camera){
    const frame=this.host.frame;frame[WEATHER_FRAME.DT]=dt;
    frame[WEATHER_FRAME.POSITION]=camera.position.x;frame[WEATHER_FRAME.POSITION+1]=camera.position.y;frame[WEATHER_FRAME.POSITION+2]=camera.position.z;
  }
  publishFrame() {
    const r=this.host.result,u=this.material.uniforms;
    u.rainTime.value=r[RESULT.RAIN_TIME];u.eye.value.fromArray(r,RESULT.EYE);u.cameraMotion.value.fromArray(r,RESULT.CAMERA_MOTION);
    if(r[RESULT.CHANGED])this.cover.texture.needsUpdate=true;this.cover.queries=r[RESULT.COVER_QUERIES];
    this.mesh.visible=r[RESULT.COVERED]===1;
    this.mist.visible=r[RESULT.MIST_VISIBLE]===1;
    this.sheets.mesh.visible=r[RESULT.SHEETS_VISIBLE]===1;
    this.impacts.synced(r[RESULT.IMPACT_TIME],r[RESULT.IMPACTS_SHOWN]===1,r[RESULT.IMPACTS_CHANGED]===1,r[RESULT.IMPACTS_NEXT],r[RESULT.IMPACTS_QUERIES]);
  }

  dispose(){this.unsubscribe?.();for(const mesh of [this.mesh,this.mist]){mesh.removeFromParent();mesh.geometry.dispose();mesh.material.dispose();}this.impacts.dispose();this.sheets.dispose();this.cover.texture.dispose();}
}
