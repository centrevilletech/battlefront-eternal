import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';
import { particleVertexShader, particleFragmentShader } from './particles.js';

// Spawn state stays in instance buffers. Authored tracks and the much smaller
// array of emitter transforms/times are read in the vertex shader. One draw per
// texture/blend, including effects with different tracks. No simulation readback.
const KEYS = 8, WIDTH = 64, MAX_EFFECTS = 1024, MAX_DEFINITIONS = 1024;
const ATTRIBUTES = { spawnPosition: 3, spawnVelocity: 3, spawnColor: 4, spawnShape: 4, spawnMeta: 4 };
const power2 = n => 2 ** Math.ceil(Math.log2(Math.max(1, n)));
function dataTexture(data, width, height) {
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat, THREE.FloatType);
  texture.needsUpdate = true;
  return texture;
}
const INPUTS = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 spawnPosition, spawnVelocity;
  attribute vec4 spawnColor, spawnShape, spawnMeta;
  uniform sampler2D tracks, emitters;
  uniform float trackRows, emitterRows;
  vec4 track(float column) { return texture2D(tracks, vec2((column + 0.5) / ${WIDTH}.0, (spawnMeta.w + 0.5) / trackRows)); }
  vec4 emitter(float column) { return texture2D(emitters, vec2((column + 0.5) / 3.0, (spawnMeta.z + 0.5) / emitterRows)); }
  vec3 rotateBy(vec3 p, vec4 q) { return p + 2.0 * cross(q.xyz, cross(q.xyz, p) + q.w * p); }
`;
const INITIALIZE = /* glsl */ `
  vec4 origin = emitter(0.0), turn = emitter(1.0), clock = emitter(2.0);
  float age = max(0.0, clock.x - spawnMeta.x);
  vec4 counts = track(0.0), settings = track(1.0), atlas = track(2.0);
  vec3 velocity = spawnVelocity, displacement = vec3(0.0);
  float left = age;
  for (int k = 0; k < ${KEYS}; k++) {
    if (float(k) >= counts.x || left <= 0.0) break;
    vec4 key = track(4.0 + float(k) * 2.0), value = track(5.0 + float(k) * 2.0);
    float span = key.x, op = key.y;
    if (span <= 0.0) {
      if (op == 1.0) velocity += value.xyz;
      else if (op == 2.0) velocity = value.xyz;
      else if (op == 3.0) velocity *= value.x;
    } else {
      float t = min(left, span), f = t / span;
      vec3 change = vec3(0.0);
      if (op == 1.0) change = value.xyz;
      else if (op == 2.0) change = value.xyz - velocity;
      else if (op == 3.0) change = velocity * (value.x - 1.0);
      displacement += velocity * t + change * (t * f * 0.5);
      velocity += change * f;
      left -= t;
    }
  }
  if (left > 0.0) displacement += velocity * left;
  vec3 iPosition = spawnPosition + displacement + origin.xyz * settings.y;
  float particleSize = spawnShape.x;
  float effectScale = clock.y;
  left = age;
  for (int k = 0; k < ${KEYS}; k++) {
    if (float(k) >= counts.y || left < 0.0) break;
    vec4 key = track(20.0 + float(k) * 2.0);
    float value = track(21.0 + float(k) * 2.0).x;
    float f = key.x > 0.0 ? min(left / key.x, 1.0) : 1.0;
    if (key.y == 1.0) particleSize *= 1.0 + (value - 1.0) * f;
    else if (key.y == 2.0) particleSize += value * f;
    else if (key.y == 3.0) particleSize += (value - particleSize) * f;
    left -= key.x;
    if (f < 1.0) break;
  }
  vec4 iColor = spawnColor;
  left = age;
  for (int k = 0; k < ${KEYS}; k++) {
    if (float(k) >= counts.z) break;
    vec4 key = track(36.0 + float(k) * 2.0), value = track(37.0 + float(k) * 2.0);
    float f = key.x > 0.0 ? min(left / key.x, 1.0) : 1.0;
    if (key.y == 1.0) iColor += (value - iColor) * f;
    else if (key.y == 2.0) iColor = clamp(iColor + value * f, 0.0, 1.0);
    left -= key.x;
    if (f < 1.0) break;
  }
  iColor *= origin.w;
  particleSize *= effectScale;
  float rotation = spawnShape.y + spawnShape.z * age;
  float speed = length(velocity);
  vec4 iShape = vec4(particleSize, rotation, speed * settings.z, settings.x);
  vec3 iAxis = vec3(0.0, 0.0, 1.0), iAxis2 = vec3(1.0, 0.0, 0.0);
  if (settings.x > 1.5) {
    iAxis = rotateBy(vec3(cos(rotation), 0.0, sin(rotation)), turn);
    iAxis2 = rotateBy(vec3(-sin(rotation), 0.0, cos(rotation)), turn);
  } else if (settings.x > 0.5 && speed > 0.0) iAxis = velocity / speed;
  vec4 iFrame = vec4(0.0, 0.0, 1.0, 1.0);
  if (atlas.x > 0.0) {
    float index = min(floor(age / atlas.x), atlas.z - 1.0);
    iFrame = vec4(mod(index, atlas.y), floor(index / atlas.y), 1.0, 1.0) / atlas.y;
  }
  // The CPU owns expiry: a float-rounded GPU lifetime comparison can remove
  // a still-live flash one frame early. Only invisible shapes are clipped here.
  if (particleSize <= 0.0 || (settings.x == 1.0 && speed < 0.0001)) {
    gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return;
  }
`;

export class AnalyticParticleBatch {
  constructor(texture, blending, owner) {
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute('corner', new THREE.Float32BufferAttribute([-1,-1,1,-1,1,1,-1,1],2));
    this.geometry.setIndex([0,1,2,0,2,3]);
    this.material = new THREE.ShaderMaterial({
      uniforms: { map:{value:texture}, fade:lighting.fade, ...owner.uniforms,
        sceneDepth:particleDepth.map, softParticles:particleDepth.enabled,
        depthResolution:particleDepth.resolution, cameraRange:particleDepth.cameraRange },
      vertexShader:particleVertexShader(INPUTS, INITIALIZE), fragmentShader:particleFragmentShader,
      transparent:true, depthWrite:false, side:THREE.DoubleSide, forceSinglePass:true,
      blending:blending==='additive'?THREE.AdditiveBlending:THREE.NormalBlending,
    });
    this.mesh = new THREE.Mesh(this.geometry,this.material);
    this.mesh.visible=false;this.geometry.instanceCount=0;
    this.mesh.frustumCulled=false; this.mesh.renderOrder=10;
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);
    this.arrays={}; this.records=[]; this.slots=[]; this.epochs=[]; this.count=0;
    this.grow(256); this.begin();
  }
  grow(capacity) {
    // Dispose replaced GPU buffers too; replacing attributes alone leaks them.
    this.geometry.dispose();
    for (const [name,size] of Object.entries(ATTRIBUTES)) {
      const array=new Float32Array(capacity*size);
      if(this.arrays[name])array.set(this.arrays[name]);
      this.arrays[name]=array;
      this.geometry.setAttribute(name,new THREE.InstancedBufferAttribute(array,size).setUsage(THREE.DynamicDrawUsage));
    }
    this.capacity=capacity; this.dirty=0;
  }
  begin() { this.count=0; this.dirty=Infinity; }
  pushParticle(p, slot, definition, epoch = 0) {
    const i=this.count++;
    if(i===this.capacity)this.grow(this.capacity*2);
    if(this.records[i]===p && this.slots[i]===slot && this.epochs[i]===epoch)return;
    this.records[i]=p;this.slots[i]=slot;this.epochs[i]=epoch;this.dirty=Math.min(this.dirty,i);
    const a=this.arrays;
    p.position.toArray(a.spawnPosition,i*3);p.velocity.toArray(a.spawnVelocity,i*3);
    a.spawnColor.set(p.color,i*4);
    a.spawnShape.set([p.size,p.rotation,p.spin,0],i*4);
    a.spawnMeta.set([p.birth-epoch,p.life,slot,definition],i*4);
  }
  end() {
    // Release dead references; capacity is the historical peak as with CPU batches.
    this.records.length=this.slots.length=this.epochs.length=this.count;
    this.mesh.visible=this.count>0;this.geometry.instanceCount=this.count;
    if(this.dirty>=this.count)return;
    for(const name of Object.keys(ATTRIBUTES)) {
      const a=this.geometry.getAttribute(name);
      // Merge into one pending range even if many updates happen without a draw.
      const start=Math.min(this.dirty*a.itemSize,a.updateRanges[0]?.start??Infinity);
      a.clearUpdateRanges();a.addUpdateRange(start,this.count*a.itemSize-start);a.needsUpdate=true;
    }
  }
}

export class GpuEffects {
  constructor(library, renderer) {
    this.renderer=renderer;this.definitions=new Map();
    const rows=[];
    const visit=defs=>{for(const def of defs){
      visit(def.geometry?.emitters||[]);
      const g=def.geometry,t=def.transformer;
      if(!g || !t || g.blend==='blur' || !g.texture || !['particle','animated','spark','streak','billboard'].includes(g.type))continue;
      if(['position','size','color'].some(k=>(t[k]?.length||0)>KEYS))continue;
      if(this.definitions.has(def))continue;
      if(rows.length>=MAX_DEFINITIONS)continue;
      this.definitions.set(def,rows.length);rows.push(def);
    }};
    for(const defs of Object.values(library))visit(defs);
    const height=power2(rows.length), data=new Float32Array(WIDTH*height*4);
    rows.forEach((def,row)=>{
      const {geometry:g,transformer:t}=def;
      const put=(column,values)=>data.set(values,(row*WIDTH+column)*4);
      put(0,[t.position?.length||0,t.size?.length||0,t.color?.length||0,0]);
      put(1,[g.type==='billboard'?2:['spark','streak'].includes(g.type)?1:0,def.registerStep?0:1,g.sparkLength||.1,0]);
      put(2,[g.type==='animated'?(g.timePerFrame||0):0,Math.max(Math.round(g.frameSize||1),1),Math.max(Math.round(g.totalFrames||1),1),0]);
      for(const [kind,offset,ops] of [['position',4,['accelerate','reach','scale']],['size',20,['scale','move','reach']],['color',36,['reach','move']]]) {
        (t[kind]||[]).forEach((key,i)=>{
          const op=ops.findIndex(name=>key[name]!==undefined), value=key[ops[op]];
          put(offset+i*2,[key.lifetime,op+1,0,0]);
          const values=Array.isArray(value)?value:[value||0];
          put(offset+i*2+1,kind==='color'?values.map(v=>v/255):values);
        });
      }
    });
    this.uniforms={tracks:{value:dataTexture(data,WIDTH,height)},trackRows:{value:height},emitters:{value:null},emitterRows:{value:0}};
    this.resizeEmitters(16);this.count=0;this.epochs=[];
  }
  resizeEmitters(rows) {
    const data=new Float32Array(rows*12);
    if(this.data)data.set(this.data);
    this.data=data;this.uniforms.emitters.value?.dispose();
    this.uniforms.emitters.value=dataTexture(data,3,rows);this.uniforms.emitterRows.value=rows;
  }
  begin() { this.count=0; }
  register(effect) {
    const slot=this.count++;
    if(slot>=MAX_EFFECTS)return -1; // CPU fallback for pathological source counts.
    if(slot>=this.uniforms.emitterRows.value)this.resizeEmitters(this.uniforms.emitterRows.value*2);
    const at=slot*12;
    effect.position.toArray(this.data,at);this.data[at+3]=effect.tint;
    // Keep subtraction precise in long sessions; rebase live births together
    // with their emitter clock once per 256 seconds, never wall-clock time.
    const epoch=Math.floor(effect.time/256)*256;this.epochs[slot]=epoch;
    effect.quaternion.toArray(this.data,at+4);this.data[at+8]=effect.time-epoch;
    this.data[at+9]=effect.scale??1; // every particle's drawn size (Effect.scale)
    return slot;
  }
  batch(def, texture) {
    const blend=def.geometry.blend==='additive'?'additive':'normal';
    return this.renderer.custom(`analytic:${texture.uuid}:${blend}`,()=>new AnalyticParticleBatch(texture,blend,this));
  }
  async warmup(renderer, camera, texture) {
    if(this.warmBatch)return;
    // Keep a reference to the compiled program without inserting an empty
    // batch into the live transparent draw order. Every track uses this shader.
    this.warmBatch=new AnalyticParticleBatch(texture,'normal',this);
    const stage=new THREE.Scene();stage.add(this.warmBatch.mesh);
    await renderer.compileAsync(stage,camera);
    renderer.initTexture(this.uniforms.tracks.value);
    renderer.initTexture(this.uniforms.emitters.value);
  }
  end() { if(this.count)this.uniforms.emitters.value.needsUpdate=true; }
  dispose() {
    this.uniforms.tracks.value.dispose();this.uniforms.emitters.value.dispose();
    this.warmBatch?.geometry.dispose();this.warmBatch?.material.dispose();
  }
}
