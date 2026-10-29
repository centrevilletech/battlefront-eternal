import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import * as THREE from 'three';
import {loadClips,Pose} from '../src/anim.js';
import {SoldierAnimator} from '../src/animator.js';
import {SoldierPresentation} from '../src/soldier-presentation.js';
import {Unit} from '../src/units.js';

const metaURL=new URL('../public/common/anims.json',import.meta.url);
const actorURL=new URL('../public/sides/cis/actors/cis_inf_bdroid.json',import.meta.url);
const available=existsSync(metaURL)&&existsSync(actorURL);
function fixture() {
 const meta=JSON.parse(readFileSync(metaURL));
 const model=JSON.parse(readFileSync(actorURL));
 const bytes=readFileSync(new URL('../public/common/anims.bin',import.meta.url));
 const clips=loadClips(meta,bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),model.bones.map(b=>b.name));
 const root=new THREE.Group(),byName={};
 const bones=model.bones.map((desc,index)=>{const bone=new THREE.Bone();bone.name=desc.name;bone.userData.index=index;new THREE.Matrix4().fromArray(desc.local).decompose(bone.position,bone.quaternion,bone.scale);byName[bone.name]=bone;return bone;});
 model.bones.forEach((desc,i)=>{if(desc.parent>=0)bones[desc.parent].add(bones[i]);else root.add(bones[i]);});
 const animator=new SoldierAnimator({root,bones,byName},clips,meta.sets);animator.setBank('bdroid');
 const soldier={aimYaw:0,aimPitch:0,velocity:new THREE.Vector3(0,0,4),posture:'stand',dive:0,diveTime:0.85,air:null};
 return {animator,soldier,clips};
}
const deg=r=>r*180/Math.PI;

export function rollMetrics() {
 const {animator:a,soldier:s}=fixture();
 const previous=new THREE.Quaternion();
 let entry=0,recovery=0,recoveryLegs=0;
 for(let frame=0;frame<240;frame++) {
  if(frame===120){s.dive=1;s.posture='crouch';s.velocity.set(-7.65,0,0);}
  if(frame===171){s.dive=0;s.posture='stand';s.velocity.set(-4.5,0,0);}
  previous.copy(a.actor.root.quaternion);
  a.update(1/60,s);
  if(frame>=120&&frame<130)entry=Math.max(entry,deg(previous.angleTo(a.actor.root.quaternion)));
  if(frame>=171&&frame<190){recovery=Math.max(recovery,deg(previous.angleTo(a.actor.root.quaternion)));recoveryLegs=a.legs;}
 }
 return {entry,recovery,recoveryLegs:deg(recoveryLegs)};
}

export function gaitMetrics() {
 const {animator:a,soldier:s}=fixture();
 const arm=a.byName.bone_l_upperarm,previous=new THREE.Quaternion();
 let maxStep=0,maxSwitchStep=0,minWeights=1;
 for(let frame=0;frame<360;frame++) {
  const phase=Math.floor(frame/8)%3;
  s.velocity.set(0,0,phase===0?4:phase===1?6.5:0);
  previous.copy(arm.quaternion);
  a.update(1/60,s);
  const angle=deg(previous.angleTo(arm.quaternion));
  if(frame>60){maxStep=Math.max(maxStep,angle);if(frame%8===0)maxSwitchStep=Math.max(maxSwitchStep,angle);}
  minWeights=Math.min(minWeights,a.layers.reduce((sum,layer)=>sum+layer.weight,0));
 }
 return {maxStep,maxSwitchStep,minWeights};
}

test('side roll enters without a one-frame body snap and recovers on the same leg heading',{skip:!available},()=>{
 const metrics=rollMetrics();
 assert.ok(metrics.entry<30,`roll entry body step ${metrics.entry.toFixed(2)} degrees`);
 assert.ok(metrics.recovery<8,`roll recovery body step ${metrics.recovery.toFixed(2)} degrees`);
 assert.ok(metrics.recoveryLegs<0,'right roll should retain a right-facing leg heading during sideways recovery');
});

test('retail B1 gait crossfades keep normalized weights and smooth upper-body transitions',{skip:!available},()=>{
 const metrics=gaitMetrics();
 assert.ok(Math.abs(metrics.minWeights-1)<1e-5,`minimum layer weight sum ${metrics.minWeights}`);
 assert.ok(metrics.maxSwitchStep<18,`upper arm changes ${metrics.maxSwitchStep.toFixed(2)} degrees on gait selection`);
});

test('fading locomotion keeps its authored playback rate and reused strides stay in phase',{skip:!available},()=>{
 const {animator:a,clips}=fixture();
 const walk=clips.get('human_rifle_stand_walkforward'),run=clips.get('human_rifle_stand_runforward');
 a.play(walk,0.02,null,1.8);
 const walkLayer=a.layers.at(-1),before=walkLayer.time;
 a.play(run,0.02,null,0.8);
 assert.ok(Math.abs(walkLayer.time-before-0.036)<1e-8,'outgoing walk must keep its 1.8x playback');
 const phase=a.layers.at(-1).time/run.duration%1;
 a.play(walk,0.02,null,1.8);
 assert.ok(Math.abs(a.layers.at(-1).time-(phase*walk.duration+0.036))<1e-8);
});

test('bank idle over foot turns keeps real-time playback instead of compressing its whole clip',{skip:!available},()=>{
 const {animator:a,soldier:s,clips}=fixture();s.velocity.set(0,0,0);
 const idle=clips.get('human_bdroid_stand_idle_emote');
 const expected=new Pose(a.bones.length,a.byName.DummyRoot.userData.index);
 const index=a.byName.bone_l_upperarm.userData.index;
 const q=new THREE.Quaternion();let turns=0;
 for(let frame=0;frame<180;frame++) {
  s.aimYaw=frame/60*1.2;a.update(1/60,s);
  if(frame<30||!a.turn)continue;
  turns++;
  expected.clear();expected.add(idle,a.time,1,true);
  q.fromArray(expected.q,index*4).normalize();
  assert.ok(deg(q.angleTo(a.bones[index].quaternion))<0.02,'upper idle should retain its natural clock while the feet turn');
 }
 assert.ok(turns>20);
});

test('pitch changes and crossing the yaw wrap do not disturb the stride phase',{skip:!available},()=>{
 const first=fixture(),second=fixture();
 for(let frame=0;frame<300;frame++) {
  const yaw=frame/60*1.5;
  first.soldier.aimYaw=yaw;
  second.soldier.aimYaw=Math.atan2(Math.sin(yaw),Math.cos(yaw));
  second.soldier.aimPitch=Math.sin(frame/30)*0.8;
  for(const f of [first,second]) {f.soldier.velocity.set(Math.sin(yaw)*4,0,Math.cos(yaw)*4);f.animator.update(1/60,f.soldier);}
  assert.ok(Math.abs(first.animator.layers.at(-1).time-second.animator.layers.at(-1).time)<1e-8);
  assert.ok(first.animator.actor.root.quaternion.angleTo(second.animator.actor.root.quaternion)<1e-6);
  for(const name of ['bone_l_thigh','bone_l_calf','bone_r_thigh','bone_r_calf'])assert.ok(first.animator.byName[name].quaternion.angleTo(second.animator.byName[name].quaternion)<1e-6);
 }
});

test('recent firing does not make right-roll recovery choose the opposite heading',{skip:!available},()=>{
 const {animator:a,soldier:s}=fixture();
 for(let i=0;i<30;i++)a.update(1/60,s);
 a.fire();s.dive=1;s.posture='crouch';s.velocity.set(-7.65,0,0);
 for(let i=0;i<51;i++)a.update(1/60,s);
 s.dive=0;s.posture='stand';s.velocity.set(-4.5,0,0);
 for(let i=0;i<20;i++) {a.update(1/60,s);assert.ok(a.legs<0);assert.notEqual(a.gait,'runBack');}
});

test('small contact-speed noise does not repeatedly restart the walking cycle',{skip:!available},()=>{
 const {animator:a,soldier:s}=fixture();
 for(let i=0;i<30;i++)a.update(1/60,s);
 for(let i=0;i<60;i++) {
  s.velocity.set(0,0,i%2?0.28:0.32);a.update(1/60,s);
  assert.notEqual(a.gait,'idle');
 }
 s.velocity.set(0,0,0);a.update(1/60,s);
 assert.equal(a.gait,'idle','a real stop still takes effect immediately');
});

test('a brief stop at a step does not reverse sideways roll recovery',{skip:!available},()=>{
 const {animator:a,soldier:s}=fixture();
 for(let i=0;i<30;i++)a.update(1/60,s);
 s.dive=1;s.posture='crouch';s.velocity.set(-7.65,0,0);
 for(let i=0;i<51;i++)a.update(1/60,s);
 s.dive=0;s.posture='stand';s.velocity.set(0,0,0);
 a.update(1/60,s);assert.equal(a.gait,'idle');
 s.velocity.set(-4.5,0,0);
 for(let i=0;i<20;i++){a.update(1/60,s);assert.ok(a.legs<0);assert.notEqual(a.gait,'runBack');}
});

test('crossing forward/backward gait selection produces a bounded visual pivot',{skip:!available},()=>{
 const {animator:a,soldier:s}=fixture();let previous=0,maxStep=0;
 for(let frame=0;frame<180;frame++) {
  const angle=frame<90?1.75:1.95;
  s.velocity.set(Math.sin(angle)*4,0,Math.cos(angle)*4);
  a.update(1/60,s);
  if(frame>0)maxStep=Math.max(maxStep,Math.abs(a.legs-previous)*180/Math.PI);
  previous=a.legs;
 }
 assert.ok(maxStep<8,`visual leg pivot ${maxStep.toFixed(2)} degrees in one frame`);
 assert.equal(s.aimYaw,0,'aim input stays immediate and independent of the visual pivot');
});

test('a replicated firing counter restores the strafing rifle direction after lost shot events',{skip:!available},()=>{
 const {animator,soldier}=fixture();soldier.position=new THREE.Vector3();
 const reference=fixture();reference.soldier.velocity.set(4,0,0);
 const presentation=new SoldierPresentation(soldier);
 const unit=new Unit({def:{},actor:animator.actor,soldier,animator,presentation});
 unit.removed=false;unit.alive=true;
 const pose={kind:'soldier',generation:1,fromTick:10,fireCounter:0,position:[0,0,0],velocity:[4,0,0],yaw:0,pitch:0,posture:0,state:0};
 presentation.networkPose=pose;
 const q=new THREE.Quaternion(),forward=new THREE.Vector3();
 const yawError=()=>{animator.byName.hp_weapons.getWorldQuaternion(q);forward.set(0,0,1).applyQuaternion(q);return Math.abs(Math.atan2(forward.x,forward.z));};
 for(let i=0;i<60;i++){unit.present(1/60,1);reference.animator.update(1/60,reference.soldier);}
 const relaxed=yawError();
 // No shot event, flash or projectile callback runs: only its authoritative
 // counter eventually reaches the body sample being rendered.
 pose.fromTick=20;pose.fireCounter=1;
 reference.animator.fire();
 for(let i=0;i<60;i++){unit.present(1/60,1);reference.animator.update(1/60,reference.soldier);}
 const firing=yawError();
 assert.ok(relaxed>.5,`relaxed strafing rifle yaw ${deg(relaxed).toFixed(2)} degrees`);
 assert.ok(firing<relaxed*.5,`rifle error falls from ${deg(relaxed).toFixed(2)} to ${deg(firing).toFixed(2)} degrees`);
 const expected=new THREE.Quaternion();reference.animator.byName.hp_weapons.getWorldQuaternion(expected);
 assert.ok(q.normalize().angleTo(expected.normalize())<1e-6,'recovered rifle matches the actual firing stance, including authored clip sway');
 assert.equal(soldier.aimYaw,0,'visual recovery never changes the authoritative shot direction');
});

if(process.env.ANIMATION_METRICS==='1'&&available)console.log(JSON.stringify({roll:rollMetrics(),gait:gaitMetrics()}));
