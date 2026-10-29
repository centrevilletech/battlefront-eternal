import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import {EnvironmentWakes,WAKE_COUNT} from '../src/environment-volume.js';
import {EnvironmentEffects} from '../src/environment-effects.js';
import {TIER} from '../src/fountain-jets.js';

const run=(hz)=>{
  const wakes=new EnvironmentWakes(),actor={position:new THREE.Vector3()};
  wakes.update(0,[actor],()=>true);
  for(let i=1;i<=hz;i++){actor.position.set(i/hz*4,Math.sin(i/hz*Math.PI)*2,0);wakes.update(1/hz,[actor],()=>true);}
  return wakes;
};
test('walking and jumping displace bounded, world-space fog wakes',()=>{
  const wakes=run(60);assert.equal(wakes.points.length,WAKE_COUNT);assert.ok(wakes.cursor>=8);
  assert.ok(wakes.motion.some(m=>Math.abs(m.y)>3));assert.ok(wakes.points.some(p=>p.y>1));
  assert.ok(wakes.points.every(p=>p.w>=0&&p.w<=1.001));
});
test('stationary actors, teleports, and distant actors do not emit wakes',()=>{
  const w=new EnvironmentWakes(),actor={position:new THREE.Vector3()};
  for(let i=0;i<100;i++)w.update(.05,[actor],()=>true);assert.equal(w.cursor,0);
  actor.position.x=100;w.update(.05,[actor],()=>true);assert.equal(w.cursor,0);
  actor.position.x+=.2;w.update(.05,[actor],()=>false);assert.equal(w.cursor,0);
});
test('wakes expire, survive rate changes, and clear on mode changes',()=>{
  for(const hz of [30,60,144]){
    const w=run(hz);assert.ok(w.cursor>=8&&w.cursor<=10);w.update(3,[],()=>true);
    assert.ok(w.points.every(p=>w.time-p.w>2));w.clear();assert.ok(w.points.every(p=>p.w===-100));
  }
});
for(const [map,count,basins] of [['nab2',5,5],['bes2',9,0]]){
  const path=new URL(`../public/maps/${map}/environment/effects.json`,import.meta.url);
  test(`${map}: retail attachments, orientations and dependencies are complete`,{skip:!fs.existsSync(path)},()=>{
    const data=JSON.parse(fs.readFileSync(path));assert.equal(data.instances.length,count);assert.equal(data.basins.length,basins);
    for(const i of data.instances){assert.equal(i.matrix.length,16);assert.ok(i.matrix.every(Number.isFinite));assert.ok(data.effects[i.effect]?.length);}
    for(const defs of Object.values(data.effects))for(const d of defs){
      if(d.geometry.texture)assert.ok(data.textures[d.geometry.texture]);if(d.geometry.model)assert.ok(data.models[d.geometry.model]?.length);
    }
    if(map==='nab2'){assert.equal(data.effects.waterfountain.length,6);assert.ok(data.basins.every(b=>b.radius>6&&b.radius<7));}
    else {assert.equal(new Set(data.instances.map(i=>i.hardpoint)).size,9);const p=data.instances.find(i=>i.hardpoint==='hp_steam_2');assert.ok(Math.abs(p.matrix[9])>.3);}
  });
}

const chamberPath=new URL('../public/maps/bes2/environment/effects.json',import.meta.url);
test('mode changes and culling clear old draws; Classic starts fully flowing',{skip:!fs.existsSync(chamberPath)},()=>{
  const data=JSON.parse(fs.readFileSync(chamberPath)),scene=new THREE.Scene();let mode='classic';
  const texture=new THREE.DataTexture(new Uint8Array([255,255,255,255]),1,1);
  const e=new EnvironmentEffects({graphics:{scene},settings:{get:()=>mode},data,texture:()=>texture,model:()=>null});
  const camera=new THREE.PerspectiveCamera(70,1,.1,200);camera.position.set(-167,-59,-9);camera.lookAt(-178,-63,-20);
  e.update(0,camera,[]);assert.equal(e.stats.activeSources,9);assert.ok(e.stats.classicParticles>50);
  mode='enhanced';e.update(.05,camera,[]);assert.equal(e.stats.classicParticles,0);assert.equal(e.stats.volumes,9);
  assert.ok([...e.classic.renderer.batches.values()].every(b=>!b.mesh.visible));
  camera.position.set(1000,1000,1000);e.update(.05,camera,[]);assert.equal(e.stats.volumes,0);assert.equal(e.volume.mesh.visible,false);
  e.dispose();texture.dispose();assert.equal(scene.children.length,0);
});

const theedPath=new URL('../public/maps/nab2/environment/effects.json',import.meta.url);
test('Theed fountains keep fixed jet slots and their basin levels; jets follow mode and view',{skip:!fs.existsSync(theedPath)},()=>{
  const data=JSON.parse(fs.readFileSync(theedPath)),scene=new THREE.Scene();let mode='enhanced';
  const texture=new THREE.DataTexture(new Uint8Array([255,255,255,255]),1,1);
  const e=new EnvironmentEffects({graphics:{scene},settings:{get:()=>mode},data,texture:()=>texture,model:()=>null});
  const fountains=e.instances.filter(i=>i.jet);assert.deepEqual(fountains.map(f=>f.slot),[0,1,2,3,4]);
  // Each stock basin's water lies about 0.2 m under its nozzle; Enhanced
  // fills the inner tier to its lip, 0.8 m above it, and keeps the outer pool.
  for(const f of fountains){assert.ok(Math.abs(f.level-(f.position.y-.2+TIER.rise))<.05);assert.ok(Math.abs(f.low-(f.position.y-.2+TIER.outer))<.05);}
  const camera=new THREE.PerspectiveCamera(43,1.6,.5,220);camera.position.set(-25,-44,123);camera.lookAt(-36,-46.8,111);
  e.update(.05,camera,[]);const theed=fountains.find(f=>f.position.x===-36&&f.position.z===111);
  assert.ok(e.jets.meshes.every(m=>m.visible));assert.equal(e.spray.sources[theed.slot].w,1);
  assert.equal(e.jets.levels[theed.slot],Math.fround(theed.level));assert.equal(e.stats.volumes,0);
  // Only the fountains in view are drawn: one block of instances each.
  assert.equal(e.jets.visible[0],theed.slot);
  assert.ok(e.jets.meshes.every(m=>{const g=(m.userData.spray||m).geometry;return g.instanceCount===g.userData.perFountain*e.visibleFountains.length;}));
  camera.position.set(1000,1000,1000);e.update(.05,camera,[]);
  assert.ok(e.jets.meshes.every(m=>!m.visible));assert.ok(e.spray.sources.every(s=>s.w===0));
  camera.position.set(-25,-44,123);camera.lookAt(-36,-46.8,111);mode='classic';e.update(.05,camera,[]);
  assert.ok(e.jets.meshes.every(m=>!m.visible));assert.ok(e.stats.classicParticles>0);
  e.dispose();texture.dispose();assert.equal(scene.children.length,0);
});

// A placed object's stock effect with no Enhanced form (a campfire's
// smoke): it plays in either mode while in view, a mode switch doesn't
// restart it, and a map carrying only such effects builds no steam volumes.
const SMOKE=[{name:'Smoke',maxParticles:[-1,-1],startDelay:[0,0],burstDelay:[0.1,0.1],burstCount:[1,1],registerStep:true,size:[1,1],
  color:{hsv:false,channels:[[255,255],[255,255],[255,255],[255,255]]},
  spawner:{shape:'spread',box:[[-.1,.1],[.5,1],[-.1,.1]],offset:[[0,0],[0,0],[0,0]],positionScale:[0,0],velocityScale:[1,1],inheritVelocity:[0,0],
    size:[.5,.5],color:{hsv:false,channels:[[200,200],[200,200],[200,200],[128,128]]},startRotation:[0,0],rotationVelocity:[0,0],fadeInTime:0},
  transformer:{lifetime:2,position:[],size:[],color:[]},geometry:{type:'particle',blend:'normal',texture:'smoke',model:null}}];
test('placed stock effects play in both modes, out to the near scene, with no Enhanced volumes',()=>{
  const at=[1,0,0,0,0,1,0,0,0,0,1,0,40,0,0,1];
  const data={instances:[{effect:'campfire',hardpoint:'hp_smoke',owner:'fire1',matrix:at}],basins:[],effects:{campfire:SMOKE},textures:{smoke:'textures/smoke.png'},models:{}};
  const scene=new THREE.Scene();let mode='enhanced';
  const texture=new THREE.DataTexture(new Uint8Array([255,255,255,255]),1,1);
  const e=new EnvironmentEffects({graphics:{scene},settings:{get:()=>mode},data,texture:()=>texture,model:()=>null});
  assert.equal(e.volume,null);assert.equal(e.steam,null);assert.equal(e.water,null);
  const camera=new THREE.PerspectiveCamera(60,1,.5,300);camera.position.set(0,1.8,0);camera.lookAt(40,2,0);
  e.update(.05,camera,[]);assert.equal(e.stats.activeSources,1);assert.ok(e.stats.classicParticles>0,'Enhanced plays the stock effect');
  const handle=e.instances[0].handle;mode='classic';e.update(.05,camera,[]);
  assert.equal(e.instances[0].handle,handle,'a mode switch leaves it running');
  // Past 90 m it still plays while the near scene reaches it.
  camera.position.set(-110,1.8,0);camera.lookAt(40,2,0);e.update(.05,camera,[]);assert.equal(e.stats.activeSources,1);
  camera.lookAt(-300,2,0);e.update(.05,camera,[]);assert.equal(e.stats.activeSources,0);assert.equal(e.instances[0].handle,null);
  e.dispose();texture.dispose();assert.equal(scene.children.length,0);
});
