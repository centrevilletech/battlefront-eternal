import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { RainImpacts } from './rain-impacts.js';
import { RainSheets } from './rain-sheets.js';
import { lighting } from './materials.js';
import { WeatherHost, loaded } from './environment/module.js';
import { WeatherFrame as WEATHER_FRAME, WeatherResult as RESULT } from './environment/layout.js';

const VERTEX = /* glsl */ `
  attribute vec4 seed;
  uniform vec3 eye, cameraMotion;
  uniform vec2 wind, depthResolution;
  uniform float rainTime, range, velocity, velocityRange, width, streak, enhanced, effect;
  uniform sampler2D cover;
  uniform float coverCell,coverSize;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying float vOpacity,vDepth,vGlint;
  void main() {
    float radius = mix(range,range*mix(.22,1.0,seed.w*seed.w),enhanced);
    float span = radius * 2.0;
    float speed = max(0.1, velocity + (seed.w - 0.5) * velocityRange);
    speed *= mix(1.0,1.25+seed.z*.6,enhanced);
    vec2 gust = wind*(1.0+enhanced*2.0) + enhanced*vec2(sin(rainTime*.7)*4.5,cos(rainTime*.43)*2.0);
    vec2 drift = wind*rainTime*(1.0+enhanced*2.0)+enhanced*vec2(-cos(rainTime*.7)*6.43,sin(rainTime*.43)*4.65);
    vec3 center;
    center.xz = mod(seed.xy * span + drift - eye.xz + radius, span) + eye.xz - radius;
    center.y = eye.y + radius - mod(seed.z * span + rainTime * speed, span);
    vec3 fall = normalize(vec3(gust.x, -speed, gust.y)-cameraMotion*.2*enhanced);
    vec3 viewFall = normalize(mat3(viewMatrix) * fall);
    vec3 side = normalize(vec3(-viewFall.y, viewFall.x + 0.00001, 0.0));
    float length = streak*mix(1.0,mix(.18,.45+seed.z*.55,step(.18,seed.w)),enhanced);
    float thickness = width*mix(1.0,.35+seed.w*.65,enhanced);
    vOpacity = (1.0 - smoothstep(radius * 0.7, radius, distance(center.xz, eye.xz)))
      * smoothstep(0.35, 1.2, distance(center, eye));
    if(effect>.5){
      vec2 cell=mod(floor(center.xz/coverCell),coverSize);
      float floorY=texture2D(cover,(cell+.5)/coverSize).r;
      center.y=floorY+.4+seed.z;
      vOpacity*=step(floorY,eye.y+.5)*step(eye.y-10.0,floorY);
    }
    vec4 view = viewMatrix * vec4(center, 1.0);
    // Keep fine rain visible through MSAA/FXAA and the map's soft-focus pass.
    // Bound the minimum in pixels, so distant drops never become thick rods.
    if(enhanced>.5)thickness=max(thickness,.85*max(0.0,-view.z)/(projectionMatrix[1][1]*max(1.0,depthResolution.y)));
    if(effect>.5)view.xy+=position.xy*vec2(3.0,2.0);
    else view.xyz += side * position.x * thickness + viewFall * position.y * length;
    vWorld = center + (effect>.5?vec3(0,position.y*2.0,0):fall * position.y * length);
    vGlint=seed.w;
    vDepth=-view.z;vUv = uv;
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D cover,sceneDepth;
  uniform float coverCell, coverSize,enhanced,effect,rainTime,softParticles;
  uniform vec2 depthResolution,cameraRange;
  uniform vec3 color;
  // The map's ambient light (materials.js lighting), on a line of its own.
  uniform vec3 ambient;
  uniform vec2 alpha;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying float vOpacity,vDepth,vGlint;
  float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
  float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
  void main() {
    vec2 cell = mod(floor(vWorld.xz / coverCell), coverSize);
    float roof = texture2D(cover, (cell + 0.5) / coverSize).r;
    if (vWorld.y < roof + 0.04) discard;
    float edge = 1.0 - abs(vUv.x * 2.0 - 1.0);
    float tip = sin(vUv.y * 3.14159265);
    float a = mix(alpha.x, alpha.y, vUv.y) * edge * tip * vOpacity;
    vec3 light=color;
    if(enhanced>.5){
      a*=mix(1.05,1.45,vGlint);
      // Drops pass on the light of the sky all around them: brighter than the
      // ambient they fall through, and far brighter in a lightning flash.
      light*=(.8+vGlint*.3)*clamp(.55+dot(ambient,vec3(.3,.59,.11))*1.6,0.,1.4);
      if(effect>.5){
        float cloud=noise(vUv*5.0+vec2(rainTime*.35,0.0))*.65+noise(vUv*12.0-vec2(rainTime*.2,0.0))*.35;
        float boundary=pow(max(0.0,1.0-dot(vUv*2.0-1.0,vUv*2.0-1.0)),2.0);
        a=cloud*boundary*vOpacity*.055;
      }
      if(softParticles>.5){float d=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).r;
        float z=cameraRange.x*cameraRange.y/(cameraRange.y-d*(cameraRange.y-cameraRange.x));
        a*=clamp((z-vDepth)/(effect>.5?.7:.12),0.0,1.0);}
    }
    if(a<.001)discard;
    gl_FragColor = vec4(light, a);
  }
`;
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
  constructor({data, scene, collision, water, settings}) {
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
    this.host=new WeatherHost(loaded(),this.capacity,enhanced,this.rangeFor(enhanced),enhanced?32:16,height,impact,normal);
    // The shader's values: where the eye is, how it moves, the wind, the drops'
    // look, and the cover texture its drops are clipped against.
    const c=p.Color||[216,220,228];
    const uniforms={eye:{value:new THREE.Vector3()},cameraMotion:{value:new THREE.Vector3()},wind:{value:new THREE.Vector2(...(data.wind||[0,0]))},
      rainTime:{value:0},range:{value:0},velocity:{value:p.Velocity?.[0]??9},velocityRange:{value:p.VelocityRange?.[0]??1},
      width:{value:p.ParticleSize?.[0]??.02},streak:{value:p.StreakLength?.[0]??1},color:{value:new THREE.Vector3(...c).multiplyScalar(1/255)},
      alpha:{value:new THREE.Vector2(...(p.AlphaMinMax||[.3,.45]))},cover:{value:null},coverCell:{value:0},coverSize:{value:0},
      enhanced:{value:0},effect:{value:0},ambient:lighting.ambient,sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange};
    const options={vertexShader:VERTEX,fragmentShader:FRAGMENT,transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true};
    this.material=new THREE.ShaderMaterial({...options,uniforms});
    // The drops, and the mist they leave on the ground (the same shader in its
    // `effect` mode, forty-eight big soft quads).
    this.mesh=new THREE.Mesh(geometry(this.capacity,this.host.seeds),this.material);this.mesh.name='map-precipitation';
    this.mist=new THREE.Mesh(geometry(48,this.host.mist),new THREE.ShaderMaterial({...options,uniforms:{...uniforms,effect:{value:1}}}));this.mist.name='rain-ground-mist';
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
  update(dt,camera){
    const frame=this.host.frame;frame[WEATHER_FRAME.DT]=dt;
    frame[WEATHER_FRAME.POSITION]=camera.position.x;frame[WEATHER_FRAME.POSITION+1]=camera.position.y;frame[WEATHER_FRAME.POSITION+2]=camera.position.z;
    this.host.update();
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
