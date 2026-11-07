import './fixtures/native-anim.js';
import { contentURL } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Clip, loadClips } from '../src/anim.js';
import { existsSync, readFileSync } from 'node:fs';
import { FirstPersonView } from './helpers/presentation.js';
import {CameraFrame,openCameraWorld} from './helpers/cameras.js';

function constantClip(name, x) {
  return new Clip(name, {frames:2, tracks:[1], offset:0, flags:0, travel:[0,0,0]},
    new Float32Array([0,0,0,1,x,0,0, 0,0,0,1,x,0,0]), 1);
}
function arms() {
  const identity=new THREE.Matrix4().toArray();
  const clips=new Map(['_1st_handsdown','_1st_rifle_idle','_1st_rifle_run','_1st_rifle_jump_land'].map((name,i)=>[name,constantClip(name,i===2?1:0)]));
  const view=new FirstPersonView({rig:{model:'arms',bones:[{name:'root',parent:-1,local:identity},{name:'hp_weapons',parent:0,local:identity}]},parts:new Map(),clips,texture:()=>null,weaponModels:new Map(),firePoints:{}});
  const soldier={velocity:new THREE.Vector3(4,0,0),air:null,dive:0,posture:'stand'};
  const camera=new THREE.PerspectiveCamera();camera.updateMatrixWorld();
  const shown={bank:'rifle',def:{}};
  const args={active:true,camera,soldier,shown,zoomed:false};soldier.velocity.set(0,0,0);for(let i=0;i<90;i++)view.update(1/60,args);soldier.velocity.set(4,0,0);return {view,args};
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

// These observations drive the full native camera phase, with an explicit
// zero eye so its published position measures the motion's displacement.
function motionFrame(motions={}){
  const h=new CameraFrame(openCameraWorld(),undefined,{cameraMotions:motions});h.first=h.ownEyes=true;
  return h;
}
test('stopping head bob settles from its current offset without a snap',()=>{
  const h=motionFrame();h.state.velocity.set(2,0,0);h.frame(.15);h.finish();const before=h.camera.position.clone();
  assert.ok(before.length()>.05);h.state.velocity.set(0,0,0);h.frame(0);h.finish();assert.ok(h.camera.position.distanceTo(before)<1e-8);
  for(let i=0;i<30;i++){h.frame(1/60);h.finish();}assert.ok(h.camera.position.length()<1e-6);
});
test('head-bob phase preserves elapsed time across cycle boundaries',()=>{
  const poses=[];for(const fps of [30,60,144]){const h=motionFrame();h.state.velocity.set(2,0,0);for(let i=0;i<fps*3/2;i++){h.frame(1/fps);h.finish();}poses.push(h.camera.position.clone());}
  assert.ok(poses[0].distanceTo(poses[1])<1e-6&&poses[0].distanceTo(poses[2])<1e-6,JSON.stringify(poses));
});
test('camera motion velocity stays continuous across unequal key durations',()=>{
  const z=[0,0,0],h=motionFrame({RollLeft:{Vec:[[1,0,0],[2,0,0]],Slope:[[.2,0,0],z],Ore:[z,z],RSlope:[z,z],Time:[.1,.5]}});
  h.frame(0);h.finish();h.state.dive=-1;h.frame(.1);h.finish();const position=h.camera.position.x;h.frame(1e-5);h.finish();
  assert.ok(Math.abs((h.camera.position.x-position)/1e-5+2)<1e-3);
});
test('a brief stair-contact fall does not trigger a landing camera plunge',()=>{
  const z=[0,0,0],h=motionFrame({Land:{Vec:[[0,-1,0],z],Slope:[z,z],Ore:[z,z],RSlope:[z,z],Time:[.17,.17]}}),s=h.state;
  s.velocity.set(0,-.5,0);s.air='fall';h.frame(1/60);h.finish();s.air=null;s.velocity.y=0;h.frame(1/60);h.finish();assert.equal(h.camera.position.length(),0);
  s.air='jump';s.velocity.y=-8;h.frame(.4);h.finish();s.air=null;s.velocity.y=0;h.frame(.17);h.finish();assert.ok(h.camera.position.y<-.1&&h.camera.position.y>-.36,`dip ${h.camera.position.y}`);
});

test('brief stair-contact flicker does not lower the first-person weapon',()=>{
  const {view,args}=arms();args.soldier.velocity.set(0,-.5,0);args.soldier.air='fall';
  view.update(1/60,args);args.soldier.air=null;args.soldier.velocity.y=0;view.update(1/60,args);
  assert.notEqual(view.slot,'jump_land');
});

const rigFile=contentURL('sides/rep/firstperson/rep_1st_trooper.json');
test('retail first-person arms retain the same transition pose at different render rates',{skip:!existsSync(rigFile)},()=>{
  const rig=JSON.parse(readFileSync(rigFile));
  const meta=JSON.parse(readFileSync(contentURL('common/anims_fp.json')));
  const bytes=readFileSync(contentURL('common/anims_fp.bin'));
  const clips=loadClips(meta,bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),rig.bones.map(b=>b.name));
  const poses=[];
  for(const fps of [30,60,144]) {
    const view=new FirstPersonView({rig,parts:new Map(),clips,texture:()=>null,weaponModels:new Map(),firePoints:{}});
    const {args}=arms();const velocity=args.soldier.velocity.clone();args.soldier.velocity.set(0,0,0);for(let i=0;i<90;i++)view.update(1/60,args);args.soldier.velocity.copy(velocity);
    for(let i=0;i<fps/6;i++)view.update(1/fps,args);
    poses.push(view.bones.map(b=>({p:b.position.clone(),q:b.quaternion.clone()})));
  }
  for(const pose of poses.slice(1))for(let i=0;i<pose.length;i++) {
    assert.ok(pose[i].p.distanceTo(poses[0][i].p)<1e-6);
    assert.ok(pose[i].q.angleTo(poses[0][i].q)<1e-6);
  }
});

test('a roll/posture cue blends the arms rather than teleporting them down',()=>{
  const {view,args}=arms();
  view.update(.2,args);const before=view.bones[1].position.clone();
  view.cue(1);
  assert.ok(view.bones[1].position.distanceTo(before)<1e-8);
  view.update(1/144,args);
  assert.ok(view.bones[1].position.distanceTo(before)<.01);
});
