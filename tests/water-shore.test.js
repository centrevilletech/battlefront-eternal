import { SurfaceHost, SurfaceOwner, stepSurface } from './helpers/surfaces.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { WaterShore } from '../src/water-shore.js';
import { loadEnvironment } from '../src/environment/module.js';
import { SurfaceResult as RESULT } from '../src/environment/layout.js';
await loadEnvironment();

function fixture(walls=true,dry=true) {
  let quality='high',gathers=0;
  const water={host:new SurfaceHost(new SurfaceOwner(undefined,true)),height:0,group:new THREE.Group(),over:()=>true};
  const a=new THREE.Triangle(new THREE.Vector3(0,-2,-20),new THREE.Vector3(0,2,-20),new THREE.Vector3(0,2,20));
  const b=new THREE.Triangle(new THREE.Vector3(0,-2,-20),new THREE.Vector3(0,2,20),new THREE.Vector3(0,-2,20));
  const polygons=[{triangles:[a,b]}];
  const collision={gather(){gathers++;return walls?polygons:[];},cast(origin,direction,far,hit){hit.point.set(origin.x,dry&&origin.x<0?2:-2,origin.z);return hit;}};
  const shore=new WaterShore({water,collision,graphics:{settings:{get:()=>quality}}});
  const camera=new THREE.PerspectiveCamera();camera.position.set(2,3,0);camera.updateMatrixWorld();
  return {shore,water,camera,setQuality:q=>{quality=q;},gathers:()=>gathers};
}
test('native shore records publish exposed wall patches, cache tiles, and dispose their resources',()=>{
  const f=fixture();
  for(let i=0;i<60;i++)stepSurface(f.shore,1/60,f.camera);
  assert.ok(f.shore.count>0&&f.shore.count<=192);
  const points=f.shore.host.shore;
  for(let i=0;i<f.shore.count;i++){assert.equal(points[i*4],0);assert.ok(points[i*4+2]>.99);}
  const before=f.gathers();for(let i=0;i<60;i++)stepSurface(f.shore,1/60,f.camera);assert.equal(f.gathers(),before);
  f.setQuality('low');stepSurface(f.shore,1/60,f.camera);assert.equal(f.shore.group.visible,false);assert.equal(f.gathers(),before);
  f.setQuality('ultra');for(let i=0;i<100;i++){f.camera.position.x+=8;stepSurface(f.shore,1/60,f.camera);}
  assert.ok(f.shore.host.result[RESULT.SHORE_TILES]<=169);f.shore.dispose();assert.equal(f.water.group.children.length,0);
});
test('native shore phase produces no patches without a wall or exposed water',()=>{
  for(const f of [fixture(false),fixture(true,false)]){for(let i=0;i<60;i++)stepSurface(f.shore,1/60,f.camera);assert.equal(f.shore.count,0);f.shore.dispose();}
});
