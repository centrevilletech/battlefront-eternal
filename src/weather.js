import * as THREE from 'three';
import { makeHit } from './collision.js';
import { particleDepth } from './particle-depth.js';
import { RainImpacts } from './rain-impacts.js';
import { RainSheets } from './rain-sheets.js';
import { lighting } from './materials.js';

const DOWN = new THREE.Vector3(0, -1, 0);
const mod = (n, size) => ((n % size) + size) % size;

// A small toroidal height cache shares roof/floor queries across every drop.
// Only newly entered cells are queried; the shader clips rain below cover.
export class RainCover {
  constructor(collision, range, water = null, size = 16) {
    this.collision = collision; this.water = water;
    this.size = size; this.cell = (range * 2 + 8) / this.size;
    this.heights = new Float32Array(this.size * this.size).fill(-100000);
    this.keysX = new Int32Array(this.heights.length).fill(2147483647);
    this.keysZ = new Int32Array(this.heights.length).fill(2147483647);
    this.from = new THREE.Vector3(); this.hit = makeHit(); this.band = null;
    this.texture = new THREE.DataTexture(this.heights, this.size, this.size, THREE.RedFormat, THREE.FloatType);
    this.texture.minFilter = this.texture.magFilter = THREE.NearestFilter;
    this.texture.needsUpdate = true;
    this.queries = 0;
  }
  update(camera, budget = 64) {
    const band = Math.floor(camera.y / 32);
    if (band !== this.band) { this.keysX.fill(2147483647); this.band = band; }
    const x0 = Math.floor(camera.x / this.cell) - this.size / 2;
    const z0 = Math.floor(camera.z / this.cell) - this.size / 2;
    let changed = false, pending = 0; this.queries = 0;
    for (let z = z0; z < z0 + this.size; z++) for (let x = x0; x < x0 + this.size; x++) {
      const at = mod(z, this.size) * this.size + mod(x, this.size);
      if (this.keysX[at] === x && this.keysZ[at] === z) continue;
      if (this.queries >= budget) { pending++; continue; }
      this.from.set((x + .5) * this.cell, band * 32 + 160, (z + .5) * this.cell);
      const hit = this.collision.cast(this.from, DOWN, 512, this.hit);
      let height = hit ? hit.point.y : -100000;
      if (this.water?.over(this.from.x, this.from.z)) height = Math.max(height, this.water.level);
      this.heights[at] = height; this.keysX[at] = x; this.keysZ[at] = z; this.queries++; changed = true;
    }
    if (changed) this.texture.needsUpdate = true;
    return pending === 0;
  }
  dispose() { this.texture.dispose(); }
}

const VERTEX = /* glsl */ `
  attribute vec4 seed;
  uniform vec3 eye, cameraMotion;
  uniform vec2 wind, depthResolution;
  uniform float time, range, velocity, velocityRange, width, streak, enhanced, effect;
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
    vec2 gust = wind*(1.0+enhanced*2.0) + enhanced*vec2(sin(time*.7)*4.5,cos(time*.43)*2.0);
    vec2 drift = wind*time*(1.0+enhanced*2.0)+enhanced*vec2(-cos(time*.7)*6.43,sin(time*.43)*4.65);
    vec3 center;
    center.xz = mod(seed.xy * span + drift - eye.xz + radius, span) + eye.xz - radius;
    center.y = eye.y + radius - mod(seed.z * span + time * speed, span);
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
  uniform float coverCell, coverSize,enhanced,effect,time,softParticles;
  uniform vec2 depthResolution,cameraRange;
  uniform vec3 color, ambient;
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
        float cloud=noise(vUv*5.0+vec2(time*.35,0.0))*.65+noise(vUv*12.0-vec2(time*.2,0.0))*.35;
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
function geometry(count){
  const base=new THREE.PlaneGeometry(2,1),g=new THREE.InstancedBufferGeometry();g.setIndex(base.index.clone());
  g.setAttribute('position',base.attributes.position.clone());g.setAttribute('uv',base.attributes.uv.clone());base.dispose();
  const seeds=new Float32Array(count*4);let state=7351;
  for(let i=0;i<seeds.length;i++){state=Math.imul(state,1664525)+1013904223>>>0;seeds[i]=state/4294967296;}
  g.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));g.instanceCount=count;return g;
}
export class Weather {
  constructor({data, scene, collision, water, settings}) {
    Object.assign(this,{collision,water,settings});const p=data.precipitation;
    this.nativeRange=THREE.MathUtils.clamp(p.Range?.[0]??12.5,4,40);
    this.nativeCount=THREE.MathUtils.clamp(Math.round(p.ParticleDensity?.[0]??256),32,2048);
    this.capacity=Math.min(8192,this.nativeCount*32);
    const c=p.Color||[216,220,228];
    const uniforms={eye:{value:new THREE.Vector3()},cameraMotion:{value:new THREE.Vector3()},wind:{value:new THREE.Vector2(...(data.wind||[0,0]))},
      time:{value:0},range:{value:0},velocity:{value:p.Velocity?.[0]??9},velocityRange:{value:p.VelocityRange?.[0]??1},
      width:{value:p.ParticleSize?.[0]??.02},streak:{value:p.StreakLength?.[0]??1},color:{value:new THREE.Vector3(...c).multiplyScalar(1/255)},
      alpha:{value:new THREE.Vector2(...(p.AlphaMinMax||[.3,.45]))},cover:{value:null},coverCell:{value:0},coverSize:{value:0},
      enhanced:{value:0},effect:{value:0},ambient:lighting.ambient,sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange};
    const options={vertexShader:VERTEX,fragmentShader:FRAGMENT,transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true};
    this.material=new THREE.ShaderMaterial({...options,uniforms});
    this.mesh=new THREE.Mesh(geometry(this.capacity),this.material);this.mesh.name='map-precipitation';
    this.mist=new THREE.Mesh(geometry(48),new THREE.ShaderMaterial({...options,uniforms:{...uniforms,effect:{value:1}}}));this.mist.name='rain-ground-mist';
    for(const mesh of [this.mesh,this.mist]){mesh.frustumCulled=false;mesh.visible=false;mesh.renderOrder=20;mesh.onBeforeRender=(...args)=>{if(this.enhanced)particleDepth.capture(...args);};scene.add(mesh);}
    this.impacts=new RainImpacts({scene,collision,water});this.previousEye=new THREE.Vector3();
    this.sheets=new RainSheets(scene,uniforms);
    this.applyMode();this.unsubscribe=settings?.onChange(key=>{if(key==='rainEffects')this.applyMode();});
  }
  applyMode(){
    this.enhanced=this.settings?.get('rainEffects')==='enhanced';
    this.range=this.enhanced?Math.min(40,this.nativeRange*3.2):this.nativeRange;
    this.count=this.enhanced?this.capacity:this.nativeCount;
    this.mesh.geometry.instanceCount=this.count;
    this.cover?.dispose();this.cover=new RainCover(this.collision,this.range,this.water,this.enhanced?32:16);
    const u=this.material.uniforms;u.enhanced.value=this.enhanced?1:0;u.range.value=this.range;
    u.cover.value=this.cover.texture;u.coverCell.value=this.cover.cell;u.coverSize.value=this.cover.size;
    this.mist.visible=false;this.impacts.mesh.visible=false;this.initial=true;
  }
  update(dt,camera){
    const u=this.material.uniforms;u.time.value+=dt;u.eye.value.copy(camera.position);
    if(!this.initial&&dt>0&&this.previousEye.distanceToSquared(camera.position)<100)u.cameraMotion.value.subVectors(camera.position,this.previousEye).divideScalar(dt).clampLength(0,30);
    else u.cameraMotion.value.set(0,0,0);
    this.previousEye.copy(camera.position);
    this.mesh.visible=this.cover.update(camera.position,this.initial?this.cover.heights.length:96);
    this.mist.visible=this.mesh.visible&&this.enhanced;
    this.sheets.mesh.visible=this.mesh.visible&&this.enhanced;
    this.impacts.update(dt,camera,this.mesh.visible&&this.enhanced);this.initial=false;
  }
  dispose(){this.unsubscribe?.();for(const mesh of [this.mesh,this.mist]){mesh.removeFromParent();mesh.geometry.dispose();mesh.material.dispose();}this.impacts.dispose();this.sheets.dispose();this.cover.dispose();}
}
