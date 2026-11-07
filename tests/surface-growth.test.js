// A module can grow after resource admission. Exercise the displayed owner,
// including renderer attributes which borrowed its arrays before detachment.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { loadEnvironment, SurfaceOwner } from '../src/environment/module.js';
import { SurfaceFrame } from '../src/environment/frame.js';
import { Water } from '../src/water.js';
import { WaterShore } from '../src/water-shore.js';
import { StormSky } from '../src/storm-sky.js';
import { SurfaceResult as RESULT } from '../src/environment/layout.js';
await loadEnvironment();

test('surface growth preserves queued rings and rebinds shore and lightning attributes',()=>{
  const owner=new SurfaceOwner(undefined,true),texture=new THREE.Texture();
  const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),renderSize:new THREE.Vector2(320,240),
    settings:{get:key=>key==='waterQuality'?'high':'classic',onChange:()=>()=>{}},
    rust:{cameras:{flash(){},restoreLight(){}},frame:{cloud(){}}}};
  const water=new Water({data:{height:0,cell:16,offset:[0,0],grid:4,cells:[[0,0]],look:{}},
    texture:()=>texture,graphics,surfaceOwner:owner});
  const shore=new WaterShore({water,collision:{gather:()=>[],cast:()=>null},graphics});
  const storm=new StormSky({graphics,uniforms:{},surfaceHost:water.host});
  const phase=new SurfaceFrame(owner,null,water,shore,null,null,null,null,null);
  const camera=new THREE.PerspectiveCamera();camera.position.set(2,3,2);camera.updateMatrixWorld();
  const position=new THREE.Vector3(2,0,2),host=water.host;
  water.ripple(position,4,1);
  const stale=[host.config,host.frame,host.commands,host.result,host.positions,host.shore,host.stormPositions];
  const attributes=[...Object.values(water.rings.geometry.attributes),shore.shore,shore.phase,...Object.values(storm.boltGeometry.attributes)];
  const versions=attributes.map(a=>a.version);
  owner.x.memory.grow(1);
  assert.ok(stale.every(a=>a.byteLength===0),'growth detached every old surface view');
  // A producer can run before the next frame. Its refresh must preserve the
  // command already queued in native storage and append the second birth.
  water.ripple(position,4,2);
  phase.update(.1,camera,null);phase.publishWater(camera);
  assert.equal(host.result[RESULT.QUADS],80);
  assert.equal(water.rings.geometry.drawRange.count,480);
  assert.equal(water.time,.1);
  assert.equal(water.rings.geometry.attributes.position.array,host.positions);
  assert.equal(shore.shore.array,host.shore);assert.equal(shore.phase.array,host.phases);
  assert.equal(storm.boltGeometry.attributes.position.array,host.stormPositions);
  assert.equal(storm.boltGeometry.attributes.tangent.array,host.stormTangents);
  assert.equal(storm.boltGeometry.attributes.energy.array,host.stormEnergies);
  assert.ok(attributes.every((a,i)=>a.array.buffer===owner.x.memory.buffer&&a.count>0&&a.version>versions[i]));
  const bindings=host.bindings.length,frame=host.frame,points=host.positions;
  for(let i=0;i<100;i++){phase.update(0,camera,null);phase.publishWater(camera);}
  assert.equal(host.bindings.length,bindings);assert.equal(host.frame,frame);assert.equal(host.positions,points);
  storm.dispose();shore.dispose();texture.dispose();
});
