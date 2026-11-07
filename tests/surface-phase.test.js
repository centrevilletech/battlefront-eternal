// Whole-frame ownership checks: resource shells share one native environment,
// and no subsystem update export is called separately by the live binding.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { loadEnvironment, SurfaceOwner } from '../src/environment/module.js';
import { SurfaceFrame } from '../src/environment/frame.js';
import { Weather } from '../src/weather.js';
import { Snow } from '../src/snow.js';
import { Atmosphere } from '../src/atmosphere.js';
import { Foliage } from '../src/foliage.js';
import { Surface as LIMITS, SurfaceResult as RESULT } from '../src/environment/layout.js';
await loadEnvironment();

for (const snowing of [false,true]) test(`one surface call advances shared ${snowing?'snow':'rain'}, haze and foliage resources`,()=>{
  const owner=new SurfaceOwner(),scene=new THREE.Scene();
  let calls=0,selections=0;
  const advance=owner.update.bind(owner);owner.update=()=>{calls++;advance();};
  const camera=new THREE.PerspectiveCamera(65,1.6,.5,250);camera.position.set(2,3,4);camera.updateMatrixWorld();
  const collision={cast:()=>null},texture=new THREE.Texture();
  const data={precipitation:{ParticleDensity:[32],Range:[10]}};
  const weather=snowing?null:new Weather({data,scene,collision,settings:{get:()=> 'classic',onChange:()=>()=>{}},surfaceOwner:owner});
  const snow=snowing?new Snow({data,scene,collision,texture:()=>texture,surfaceOwner:owner}):null;
  const atmosphere=new Atmosphere({data:{},scene,collision,texture:()=>null,surfaceOwner:owner});
  const foliage=new Foliage(scene,{},{texture:()=>texture,nearEnd:()=>200,drawer:{select:()=>selections++},surfaceOwner:owner});
  for(const system of [weather,snow,atmosphere])if(system){assert.equal(system.update,undefined);assert.equal(system.host.update,undefined);}
  const phase=new SurfaceFrame(owner,foliage,null,null,weather,snow,atmosphere,null,null);
  // Construction can grow the shared heap; warm frames must retain it.
  const memory=owner.x.memory.buffer;
  for(let i=0;i<120;i++){phase.update(1/60,camera,null);phase.publishEnvironment();}
  assert.equal(calls,120);assert.equal(selections,120);assert.equal(foliage.time,phase.host.result[RESULT.TIME]);
  assert.equal(owner.x.memory.buffer,memory,'all borrowed resource views retain their memory');
  weather?.dispose();snow?.dispose();atmosphere.dispose();texture.dispose();
});

test('a paused command burst retains the newest bounded prefix and resumes in one call',()=>{
  const owner=new SurfaceOwner(),camera=new THREE.PerspectiveCamera();camera.updateMatrixWorld();
  const phase=new SurfaceFrame(owner,null,null,null,null,null,null,null,null),position=new THREE.Vector3();
  for(let i=0;i<LIMITS.COMMANDS+9000;i++)phase.host.ripple(position,4,1,null);
  assert.equal(phase.host.commandCount,LIMITS.COMMANDS);
  phase.update(.1,camera,null);
  assert.equal(phase.host.result[RESULT.QUADS],LIMITS.RIPPLES*LIMITS.SEGMENTS);
  assert.equal(phase.host.result[RESULT.OVERFLOW],9000);
  phase.update(10000,camera,null);assert.equal(phase.host.result[RESULT.QUADS],0);
});
