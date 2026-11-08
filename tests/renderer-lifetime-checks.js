import {Snow} from '../src/snow.js';
import {Weather} from '../src/weather.js';
import {SurfaceOwner,stepSurface} from './helpers/surfaces.js';
// Exercise real native resource owners over repeated admissions. No stand-in
// can prove GPU uploads, qualified handles or restoration journal bounds.
import * as THREE from 'three';
import {actorAsset,actorNode,actorLook,nativeActorSource,actorSkin,releaseActorSkin} from '../src/actor-resources.js';
import {EFFECT} from '../src/rust-render/effect-layout.js';
export function checkRendererLifetimes(g,assert){
 const core=g.rust,r=core.rust,resources=g.renderer.resources,gl=g.renderer.getContext();
 const texture=new THREE.DataTexture(new Uint8Array([255,255,255,255]),1,1);texture.needsUpdate=true;
 const sample=resources.staticSample(texture),alias=resources.gpu.repeat_sample(sample),native=core.texture(alias);
 const asset=actorAsset({kind:'sphere',radius:1,widthSegments:8,heightSegments:4});
 const source=nativeActorSource(core,asset);
 r.actor_effect(source.id,EFFECT.ACTOR_SHIELD.ID,true,false,native,'{}');
 const cold=r.recovery_commands(),counts=JSON.parse(r.resource_diagnostics()).resources;
 for(let i=0;i<2048;i++){
   const repeat=resources.gpu.repeat_sample(sample);if(repeat!==alias)throw Error('Shield sample changed');
   r.actor_effect(source.id,EFFECT.ACTOR_SHIELD.ID,true,false,core.texture(repeat),'{}');
 }
 assert('Shield spawn churn reuses model, texture and recovery recipes',r.recovery_commands()===cold&&JSON.stringify(JSON.parse(r.resource_diagnostics()).resources)===JSON.stringify(counts));
 const bone=new THREE.Bone(),skeleton=new THREE.Skeleton([bone]),skinNode=actorNode(asset,actorLook('body'),true);skinNode.add(bone);skinNode.bind(skeleton);
 actorSkin(core,skeleton);releaseActorSkin(core,skeleton);const skinCold=r.recovery_commands(),textureCold=core.textureSources.length;
 for(let i=0;i<10000;i++){
   skeleton.boneMatrices=Float32Array.from(new THREE.Matrix4().elements);
   if(!actorSkin(core,skeleton))throw Error('Skin allocation exhausted');releaseActorSkin(core,skeleton);
 }
 assert('Retired skin generations and images do not grow recovery history',r.recovery_commands()===skinCold&&core.textureSources.length===textureCold);
 const body=nativeActorSource(core,actorAsset({kind:'plane'})),crowd=r.actor_source_crowd();
 r.actor_source_crowd_add(body.id,0,1,new Uint32Array(),new Uint32Array(),new Uint32Array(),new Float64Array());
 r.actor_geometry(crowd,'actors/depth-skinned',true);
 const uploads=[],data=gl.bufferData,sub=gl.bufferSubData,dataOwn=Object.getOwnPropertyDescriptor(gl,"bufferData"),subOwn=Object.getOwnPropertyDescriptor(gl,"bufferSubData");
 gl.bufferData=function(...args){uploads.push({allocate:true});return data.apply(this,args);};
 gl.bufferSubData=function(...args){uploads.push({offset:args[1],bytes:args[2].byteLength});return sub.apply(this,args);};
 let appended;try{appended=r.actor_source_crowd_add(body.id,1,2,new Uint32Array(),new Uint32Array(),new Uint32Array(),new Float64Array());}finally{if(dataOwn)Object.defineProperty(gl,"bufferData",dataOwn);else delete gl.bufferData;if(subOwn)Object.defineProperty(gl,"bufferSubData",subOwn);else delete gl.bufferSubData;}
 assert('Crowd append uploads only added vertices',appended[0]===0&&!uploads.some(u=>u.allocate)&&uploads.reduce((n,u)=>n+u.bytes,0)===4*36,uploads);
 r.effect_batch(EFFECT.CPU.ID,0,true,-1,1,-1,new Int32Array());
 r.effect_mesh(EFFECT.LIGHTNING.ID,true,-1,4,6,new Float64Array());
 r.effect_settings(false);const off=JSON.parse(r.resource_diagnostics()).programs;
 r.effect_settings(true);const on=JSON.parse(r.resource_diagnostics()).programs;
 r.effect_settings(false);const again=JSON.parse(r.resource_diagnostics()).programs;
 assert('Existing batch, mesh and shield descriptors switch shadow programs and return exactly',off.includes('effects/shield-s2')&&on.includes('effects/shield-s2-shadow')&&off.includes('effects/particle-cpu')&&on.includes('effects/particle-cpu-shadow')&&off.some(key=>key.includes('basic-colors')&&!key.includes('-shadow'))&&on.some(key=>key.includes('basic-colors')&&key.includes('-shadow'))&&JSON.stringify(off)===JSON.stringify(again),{off,on,again});
 r.effect_settings(true);
 const clipped=JSON.parse(r.resource_diagnostics()).clipped;
 assert('Every bounded renderer owner exposes its overflow count',['actors','depth','crowd','interface','hud','effects','lines'].every(k=>Number.isFinite(clipped[k])));
 // Rebase retained image mirrors as well as CPU draw arrays after growth.
 const environment=new SurfaceOwner(),scene=new THREE.Scene(),collision={cast(from,down,range,hit){hit.point.set(from.x,0,from.z);return hit;}};
 const snow=new Snow({surfaceOwner:environment,data:{precipitation:{Range:[8],ParticleDensity:[4],ParticleSize:[.02],AlphaMinMax:[.3,.5]}},scene,collision,texture:()=>texture});
 const weather=new Weather({surfaceOwner:environment,data:{precipitation:{Range:[8],ParticleDensity:[4]}},scene,collision});
 stepSurface(snow,0,g.camera);stepSurface(weather,0,g.camera);snow.prepareResources(resources);weather.prepareResources(resources);
 const pixels=resources.gpu.effect_pixels,cover=resources.gpu.weather_cover;let rebound=0;
 resources.gpu.effect_pixels=function(sample,data){if(data.buffer!==environment.x.memory.buffer||!data.length)throw Error('Detached snow image');rebound++;return pixels.call(this,sample,data);};
 resources.gpu.weather_cover=function(sample,size,data){if(data.buffer!==environment.x.memory.buffer||!data.length)throw Error('Detached rain image');rebound++;return cover.call(this,sample,size,data);};
 try{environment.x.memory.grow(1);stepSurface(snow,0,g.camera);stepSurface(weather,0,g.camera);}finally{resources.gpu.effect_pixels=pixels;resources.gpu.weather_cover=cover;}
 assert('Memory growth rebinds all retained weather GPU images',rebound===4&&gl.getError()===gl.NO_ERROR,rebound);
 snow.dispose();weather.dispose();
 texture.dispose();
}
