import {SurfaceHost} from '../src/environment/surfaces.js';
import {SkyHost,VisibilityHost} from '../src/environment/module.js';
import {SkyConfig as SKY,SkyClass as CLASS,EnvironmentInstance as INSTANCE} from '../src/environment/layout.js';
// A module can grow after resource admission. Exercise the displayed owner,
// including numeric draw packets which borrowed arrays before detachment.
import test from 'node:test';
import {standinThree,standinResources} from './support/rust-standin.mjs';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { loadEnvironment, SurfaceOwner } from '../src/environment/module.js';
import { SurfaceFrame } from '../src/environment/frame.js';
import { Water } from '../src/water.js';
import { WaterShore } from '../src/water-shore.js';
import { StormSky } from '../src/storm-sky.js';
import { SurfaceResult as RESULT } from '../src/environment/layout.js';
await loadEnvironment();

test('surface growth preserves queued rings and rebinds native shore and lightning packets',()=>{
  const owner=new SurfaceOwner(undefined,true),texture=new THREE.Texture();
  const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),renderSize:new THREE.Vector2(320,240),
    settings:{get:key=>key==='waterQuality'?'high':'classic',onChange:()=>()=>{}},
    rust:{byName:{far:{admitEffect(){}}},cameras:{flash(){},restoreLight(){}},frame:{cloud(){}}}};
  const {renderer}=standinThree([]);standinResources(renderer,[]);graphics.renderer=renderer;
  const water=new Water({data:{height:0,cell:16,offset:[0,0],grid:4,cells:[[0,0]],look:{}},
    texture:()=>texture,graphics,surfaceOwner:owner});
  const shore=new WaterShore({water,collision:{gather:()=>[],cast:()=>null},graphics});
  const storm=new StormSky({graphics,uniforms:{},surfaceHost:water.host});
  const phase=new SurfaceFrame(owner,null,water,shore,null,null,null,null,null);
  const camera=new THREE.PerspectiveCamera();camera.position.set(2,3,2);camera.updateMatrixWorld();
  const position=new THREE.Vector3(2,0,2),host=water.host;
  water.ripple(position,4,1);
  const stale=[host.config,host.frame,host.commands,host.result,host.positions,host.shore,host.stormPositions];
  const rings=water.rings.userData.effectMesh,bolt=storm.boltSource;
  const packets=[rings.arrays,...shore.batches.map(b=>b.arrays),bolt.arrays];
  owner.x.memory.grow(1);
  assert.ok(stale.every(a=>a.byteLength===0),'growth detached every old surface view');
  // A producer can run before the next frame. Its refresh must preserve the
  // command already queued in native storage and append the second birth.
  water.ripple(position,4,2);
  phase.update(.1,camera,null);phase.publishWater(camera);
  assert.equal(host.result[RESULT.QUADS],80);
  assert.equal(rings.index,480);
  assert.equal(water.time,.1);
  assert.equal(rings.positions,host.positions);assert.equal(rings.arrays.position,host.positions);
  assert.ok(shore.batches.every(b=>b.arrays.shore===host.shore&&b.arrays.phase===host.phases));
  assert.equal(bolt.arrays.position,host.stormPositions);assert.equal(bolt.arrays.tangent,host.stormTangents);assert.equal(bolt.arrays.energy,host.stormEnergies);
  assert.ok(packets.every(p=>Object.values(p).every(a=>a.buffer===owner.x.memory.buffer&&a.length>0)));
  const bindings=host.bindings.length,frame=host.frame,points=host.positions;
  for(let i=0;i<100;i++){phase.update(0,camera,null);phase.publishWater(camera);}
  assert.equal(host.bindings.length,bindings);assert.equal(host.frame,frame);assert.equal(host.positions,points);
  storm.dispose();shore.dispose();texture.dispose();
});

// Loading-time warm draws can run before the first SurfaceFrame. A later
// source admission must repair retained sky/ring aliases at that boundary.
test('cold environment admission refreshes borrowed motion and surface arrays before a frame',()=>{
  const owner=new SurfaceOwner(),surface=new SurfaceHost(owner),sky=new SkyHost(owner,1);
  sky.config[SKY.CLASSES]=1;sky.config[SKY.STRIDE+CLASS.COUNT]=2;
  sky.config[SKY.STRIDE+CLASS.DISTANCE]=1000;sky.config[SKY.STRIDE+CLASS.LIFE_TIME]=100;
  sky.config[SKY.STRIDE+CLASS.CLASSIC]=1;sky.start();
  const packet={matrices:sky.arrays(0)[4]},rings={positions:surface.positions};
  sky.bindArray(packet,'matrices',h=>h.arrays(0)[4]);surface.bindArray(rings,'positions','positions');
  const previous=packet.matrices;
  owner.x.memory.grow(1);
  const placed=new VisibilityHost(owner,1);placed.config[INSTANCE.SPHERE+3]=16;placed.start();
  assert.equal(previous.byteLength,0);
  assert.equal(packet.matrices.buffer,owner.x.memory.buffer);assert.equal(packet.matrices.length,32);
  assert.equal(rings.positions,surface.positions);assert.equal(rings.positions.buffer,owner.x.memory.buffer);
});
