import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import {SniperTrace} from '../src/sniper-trace.js';
import {lighting} from '../src/materials.js';
import {SniperCounts as H} from '../src/presentation/sniper-layout.js';

// Frozen synthetic frames retain the original RNG order through deferred
// impact births, duplicate head reports, sparks, smoke and final expiry.
test('native sniper phases publish the frozen flight and impact buffers',()=>{
 const golden=JSON.parse(fs.readFileSync(new URL('./fixtures/presentation/sniper-goldens.json',import.meta.url)));
 const previous=Math.random;let seed=1234;Math.random=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
 try{
 const lights=[],plays=[],trace=new SniperTrace({add(p,c,r){lights.push(['add',p.toArray(),c.slice(),r]);},flash(p,c,r,t){lights.push(['flash',p.toArray(),c.slice(),r,t]);}});
 const source={},def={Velocity:2000,LifeSpan:.15,LaserGlowColor:[224,224,160,255]},start=new THREE.Vector3(2,1.6,3),dir=new THREE.Vector3(.2,.1,1).normalize(),head=start.clone().addScaledVector(dir,40);
 for(const expected of golden){const {t}=expected;lighting.time.value=t;lights.length=0;trace.begin();if(t<10.1)trace.touch(source,def,start,head,dir,t>10,1);
  if(t===10.04){trace.defer(head,(value)=>plays.push(value/4294967296),null,true);trace.headshot(head);trace.headshot(head.clone().addScalar(.1));}
  trace.end();
  assert.deepEqual(Array.from(trace.streaks.data.slice(0,trace.streaks.count*18)),expected.streaks,`streaks at ${t}`);
  assert.deepEqual(Array.from(trace.flares.data.slice(0,trace.flares.count*14)),expected.flares,`flares at ${t}`);
  assert.deepEqual(lights,expected.lights,`lights at ${t}`);assert.deepEqual(plays,expected.plays);assert.equal(seed,expected.seed);
 }
 }finally{Math.random=previous;}
});

// A paused burst fills command and source storage. It must keep drawing the
// newest retained records without resizing either interleaved GPU buffer.
test('a full sniper command block degrades with fixed buffers',()=>{
 const trace=new SniperTrace(),start=new THREE.Vector3(),direction=new THREE.Vector3(0,0,1),head=new THREE.Vector3(0,0,30),def={Velocity:2000,LifeSpan:.15};
 const streaks=trace.streaks.data,flares=trace.flares.data;lighting.time.value=20;trace.begin();
 for(let i=0;i<20000;i++)trace.touch({},def,start,head,direction,false,1);
 trace.end();assert.ok(trace.dropped>0);assert.equal(trace.counts[H.RECORDS],48);
 assert.equal(trace.streaks.data,streaks);assert.equal(trace.flares.data,flares);assert.ok(trace.streaks.count<=512);
});

// The own bolt follows its moving barrel, with the camera-local position
// retained through a frame that has no muzzle resource. Smoke stays fired.
test('native eye-view traces retain the frozen moving muzzle buffers',()=>{
 const golden=JSON.parse(fs.readFileSync(new URL('./fixtures/presentation/sniper-eye-goldens.json',import.meta.url)));
 const previous=Math.random;let seed=789;Math.random=()=>((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
 try{
 const camera=new THREE.PerspectiveCamera(),muzzle=new THREE.Vector3(),lights=[],source={},def={Velocity:2000,LifeSpan:.15,LaserGlowColor:[30,120,255,255]},start=new THREE.Vector3(.05,1.5,1),dir=new THREE.Vector3(0,0,1),head=new THREE.Vector3(0,1.5,40);
 let hasMuzzle=true;
 const trace=new SniperTrace({add(p,c,r){lights.push(['add',p.toArray(),c.slice(),r]);},flash(p,c,r,t){lights.push(['flash',p.toArray(),c.slice(),r,t]);}},()=>camera,()=>hasMuzzle?muzzle:null);
 for(const [index,expected]of golden.entries()){
  camera.position.set(index*.07,1.6,index*.03);camera.rotation.y=index*.01;camera.updateMatrixWorld(true);
  muzzle.set(.05,-.1,.8).applyMatrix4(camera.matrixWorld);hasMuzzle=index!==2;
  const {t}=expected;lighting.time.value=t;lights.length=0;trace.begin();if(t<10.1)trace.touch(source,def,start,head,dir,t>10,1);trace.end();
  assert.deepEqual(Array.from(trace.streaks.data.slice(0,trace.streaks.count*18)),expected.streaks,`eye streaks at ${t}`);
  assert.deepEqual(Array.from(trace.flares.data.slice(0,trace.flares.count*14)),expected.flares,`eye flares at ${t}`);
  assert.deepEqual(lights,expected.lights);assert.equal(seed,expected.seed);
 }
 }finally{Math.random=previous;}
});
