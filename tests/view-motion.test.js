import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Clip, loadClips } from '../src/anim.js';
import { existsSync, readFileSync } from 'node:fs';
import { FirstPersonView } from '../src/firstperson.js';
import { FirstPersonMotion } from '../src/camera.js';

function constantClip(name, x) {
  return new Clip(name, {frames:2, tracks:[1], offset:0, flags:0, travel:[0,0,0]},
    new Float32Array([0,0,0,1,x,0,0, 0,0,0,1,x,0,0]), 1);
}
function arms() {
  const identity=new THREE.Matrix4().toArray();
  const clips=new Map(['_1st_handsdown','_1st_rifle_idle','_1st_rifle_run'].map((name,i)=>[name,constantClip(name,i===2?1:0)]));
  const view=new FirstPersonView({rig:{model:'arms',bones:[{name:'root',parent:-1,local:identity},{name:'hp_weapons',parent:0,local:identity}]},parts:new Map(),clips,texture:()=>null,weaponModels:new Map(),firePoints:{}});
  const soldier={velocity:new THREE.Vector3(4,0,0),air:null,dive:0,posture:'stand'};
  const camera=new THREE.PerspectiveCamera();camera.updateMatrixWorld();
  const shown={bank:'rifle',def:{}};
  view.wasActive=true;view.holding=shown;view.start('idle','_1st_rifle_idle',true);
  return {view,args:{active:true,camera,soldier,shown,zoomed:false}};
}

test('first-person clip crossfade has the same pose at 30, 60 and 144 FPS',()=>{
  const poses=[];
  for(const fps of [30,60,144]) {
    const {view,args}=arms();for(let i=0;i<fps/6;i++)view.update(1/fps,args);
    poses.push(view.bones[1].position.x);
  }
  assert.ok(Math.max(...poses)-Math.min(...poses)<1e-6,JSON.stringify(poses));
});

test('zero-time draws cannot advance a first-person transition',()=>{
  const {view,args}=arms();view.update(.1,args);const before=view.bones[1].position.clone();
  for(let i=0;i<10;i++)view.update(0,args);
  assert.ok(view.bones[1].position.distanceTo(before)<1e-8);
});

test('stopping head bob settles from its current offset without a snap',()=>{
  const motion=new FirstPersonMotion();
  const soldier={velocity:new THREE.Vector3(2,0,0),air:null,dive:0,posture:'stand'};
  motion.update(.15,soldier,true);const before=motion.offset.clone();
  assert.ok(before.length()>.05);
  soldier.velocity.set(0,0,0);motion.update(0,soldier,true);
  assert.ok(motion.offset.distanceTo(before)<1e-8);
  for(let i=0;i<30;i++)motion.update(1/60,soldier,true);
  assert.ok(motion.offset.length()<1e-6);
});

test('head-bob phase preserves elapsed time across cycle boundaries',()=>{
  const soldier={velocity:new THREE.Vector3(2,0,0),air:null,dive:0,posture:'stand'};
  const poses=[];
  for(const fps of [30,60,144]) {
    const motion=new FirstPersonMotion();for(let i=0;i<fps*3/2;i++)motion.update(1/fps,soldier,true);
    poses.push(motion.offset.clone());
  }
  assert.ok(poses[0].distanceTo(poses[1])<1e-6&&poses[0].distanceTo(poses[2])<1e-6,JSON.stringify(poses));
});

test('camera motion velocity stays continuous across unequal key durations',()=>{
  const zero=[0,0,0];
  const motion=new FirstPersonMotion();
  motion.play({Vec:[[1,0,0],[2,0,0]],Slope:[[.2,0,0],zero],Ore:[zero,zero],RSlope:[zero,zero],Time:[.1,.5]});
  motion.step(.1);const position=motion.offset.x,velocity=motion.slope[0];
  motion.step(1e-5);
  assert.ok(Math.abs((motion.offset.x-position)/1e-5-velocity)<1e-3);
});

test('a brief stair-contact fall does not trigger a landing camera plunge',()=>{
  const zero=[0,0,0];
  const motion=new FirstPersonMotion({Land:{Vec:[[0,-1,0],zero],Slope:[zero,zero],Ore:[zero,zero],RSlope:[zero,zero],Time:[.17,.17]}});
  const soldier={velocity:new THREE.Vector3(0,-.5,0),air:'fall',dive:0,posture:'stand'};
  motion.update(1/60,soldier,true);soldier.air=null;soldier.velocity.y=0;motion.update(1/60,soldier,true);
  assert.equal(motion.keys,null);assert.equal(motion.offset.length(),0);
  soldier.air='jump';soldier.velocity.y=-8;motion.update(.4,soldier,true);
  soldier.air=null;soldier.velocity.y=0;motion.update(.17,soldier,true);
  assert.ok(motion.offset.y<-.1&&motion.offset.y>-.36,`dip ${motion.offset.y}`);
});

test('brief stair-contact flicker does not lower the first-person weapon',()=>{
  const {view,args}=arms();args.soldier.velocity.set(0,-.5,0);args.soldier.air='fall';
  view.update(1/60,args);args.soldier.air=null;args.soldier.velocity.y=0;view.update(1/60,args);
  assert.equal(view.forced,null);
});

const rigFile=new URL('../public/sides/rep/firstperson/rep_1st_trooper.json',import.meta.url);
test('retail first-person arms retain the same transition pose at different render rates',{skip:!existsSync(rigFile)},()=>{
  const rig=JSON.parse(readFileSync(rigFile));
  const meta=JSON.parse(readFileSync(new URL('../public/common/anims_fp.json',import.meta.url)));
  const bytes=readFileSync(new URL('../public/common/anims_fp.bin',import.meta.url));
  const clips=loadClips(meta,bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),rig.bones.map(b=>b.name));
  const poses=[];
  for(const fps of [30,60,144]) {
    const view=new FirstPersonView({rig,parts:new Map(),clips,texture:()=>null,weaponModels:new Map(),firePoints:{}});
    const {args}=arms();view.wasActive=true;view.holding=args.shown;view.start('idle','_1st_rifle_idle',true);view.apply(view.clip,0,true,1);
    for(let i=0;i<fps/6;i++)view.update(1/fps,args);
    poses.push(view.bones.map(b=>({p:b.position.clone(),q:b.quaternion.clone()})));
  }
  for(const pose of poses.slice(1))for(let i=0;i<pose.length;i++) {
    assert.ok(pose[i].p.distanceTo(poses[0][i].p)<1e-6);
    assert.ok(pose[i].q.angleTo(poses[0][i].q)<1e-6);
  }
});

test('a roll/posture cue blends the arms rather than teleporting them down',()=>{
  const {view,args}=arms();view.clips.set('_1st_rifle_jump_land',constantClip('_1st_rifle_jump_land',0));
  view.update(.2,args);const before=view.bones[1].position.clone();
  view.cue(1);
  assert.ok(view.bones[1].position.distanceTo(before)<1e-8);
  view.update(1/144,args);
  assert.ok(view.bones[1].position.distanceTo(before)<.01);
});
