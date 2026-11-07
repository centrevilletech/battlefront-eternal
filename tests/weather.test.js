import { SurfaceHost, SurfaceOwner, stepSurface } from './helpers/surfaces.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import fs from 'node:fs';
import {Weather} from '../src/weather.js';
import {runWeather} from './support/weather-scenario.js';
import {oceanGeometry,OceanSurface} from '../src/ocean-surface.js';
import {loadEnvironment} from '../src/environment/module.js';

await loadEnvironment();

test('rain cover caches stationary roofs, refreshes entered cells, and budgets teleports',()=>{
 let queries=0;
 const collision={cast(from,down,range,hit){queries++;hit.point.set(from.x,20,from.z);return hit;}};
 const scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();camera.position.set(0,10,0);
 const weather=new Weather({surfaceOwner:new SurfaceOwner(),data:{precipitation:{ParticleDensity:[256],Range:[12.5]}},scene,collision});
 const cover=weather.cover;
 stepSurface(weather,1/60,camera);assert.equal(weather.mesh.visible,true);assert.equal(queries,256);assert.equal(cover.queries,256);
 assert.ok(cover.heights.every(h=>h===20),'roof height, not indoor floor, clips falling rain');
 queries=0;stepSurface(weather,1/60,camera);assert.equal(queries,0);
 camera.position.x+=cover.cell;stepSurface(weather,1/60,camera);assert.equal(queries,16);
 queries=0;camera.position.x+=1000;stepSurface(weather,1/60,camera);assert.equal(queries,96);assert.equal(weather.mesh.visible,false,'drawn again once its cover is held');
 while(!weather.mesh.visible){stepSurface(weather,1/60,camera);assert.ok(cover.queries<=96);}
 queries=0;stepSurface(weather,1/60,camera);assert.equal(queries,0);weather.dispose();
});

// The storm, scripted: rain over roofed and open
// ground with a pond, a walking and jumping camera, frames of every length and
// the rain effects switched live, digested per frame (uniforms, which draws
// show, a hash of the splashes' and the cover cache's records), held to a recorded run.
test('the rain clock, cover and splashes are kept exactly as the recorded storm has them',()=>{
 const golden=JSON.parse(fs.readFileSync(new URL('./fixtures/weather-golden.json',import.meta.url),'utf8'));
 const rows=runWeather(Weather);
 assert.equal(rows.length,golden.length);
 rows.forEach((row,i)=>assert.deepEqual(row,golden[i],`frame ${i}`));
});

test('open-water rain stops at the water level and uses one bounded instance batch',()=>{
 const scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();camera.position.y=10;
 const weather=new Weather({surfaceOwner:new SurfaceOwner(),data:{precipitation:{ParticleDensity:[256],Range:[12.5]}},scene,
  collision:{cast(){return null}},water:{over:()=>true,level:3}});
 stepSurface(weather,1/60,camera);
 assert.equal(weather.mesh.geometry.instanceCount,256);assert.equal(scene.children.filter(m=>m.visible).length,1);
 assert.ok(weather.cover.heights.every(h=>h===3));assert.ok(weather.mesh.visible);
 weather.dispose();assert.equal(scene.children.length,0);
});

test('ocean geometry cost is bounded independently of the authored water-grid size',()=>{
 const geometry=oceanGeometry();assert.ok(geometry.index.count/3<8300);geometry.dispose();
 const scene=new THREE.Scene(),farScene=new THREE.Scene(),graphics={scene,farScene};
 const texture=new THREE.Texture();
 const data={grid:128,cell:160,height:-8,offset:[10240,10240],cells:[[64,64]],look:{OceanEnable:[1],MainTexture:['sea'],FoamTexture:['foam'],WindSpeed:[25]}};
 const ocean=new OceanSurface({surfaceHost:new SurfaceHost(new SurfaceOwner(undefined,true)),data,texture:()=>texture,graphics});
 assert.equal(ocean.mask.image.data[64*128+64],255);assert.equal(ocean.mask.image.data[0],0);
 const camera=new THREE.PerspectiveCamera();camera.position.set(523,50,-251);stepSurface(ocean,.5,camera);
 assert.deepEqual(ocean.near.position.toArray(),[520,-8,-256]);
 assert.deepEqual(ocean.far.position.toArray(),ocean.near.position.toArray());
 assert.equal(ocean.material.uniforms.time.value,.5);
 ocean.dispose();assert.equal(scene.children.length,0);assert.equal(farScene.children.length,0);
});

test('enhanced rain switches live with bounded instances, cached cover and impact raycasts',()=>{
 let mode='enhanced',listener;const settings={get:()=>mode,onChange:fn=>{listener=fn;return()=>{listener=null}}};
 const scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();camera.position.y=2;
 const collision={cast(from,down,range,hit){hit.point.set(from.x,0,from.z);hit.normal.set(0,1,0);return hit;}};
 const weather=new Weather({surfaceOwner:new SurfaceOwner(),data:{precipitation:{ParticleDensity:[256],Range:[12.5]}},scene,collision,settings});
 const geometry=weather.mesh.geometry;
 for(let i=0;i<60;i++)stepSurface(weather,1/60,camera);
 assert.equal(weather.count,8192);assert.equal(weather.cover.size,32);assert.equal(weather.cover.queries,0);
 assert.equal(scene.children.filter(m=>m.visible).length,4);assert.ok(weather.sheets.mesh.visible,'enhanced rain draws its mid-distance sheets');assert.ok(weather.impacts.next>0);assert.ok(weather.impacts.queries<=4);
 mode='classic';listener('rainEffects');stepSurface(weather,1/60,camera);
 assert.equal(weather.count,256);assert.equal(weather.cover.size,16);assert.equal(scene.children.filter(m=>m.visible).length,1);
 assert.equal(weather.mesh.geometry,geometry,'mode switches reuse the rain geometry');
 mode='enhanced';listener('rainEffects');stepSurface(weather,1/60,camera);assert.equal(weather.count,8192);
 weather.dispose();assert.equal(listener,null);assert.equal(scene.children.length,0);
});
