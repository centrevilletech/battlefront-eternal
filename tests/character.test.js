import './fixtures/native-anim.js';
import { contentURL } from './helpers/content.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { Droideka } from '../src/droideka.js';
import { SoldierAnimator } from './helpers/presentation.js';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { FirstPersonView } from './helpers/presentation.js';
import { Clip, loadClips } from '../src/anim.js';

function actor(names = ['DummyRoot']) {
  const root = new THREE.Group();
  const bones = names.map((name, index) => {
    const bone = new THREE.Bone();
    bone.name = name;
    bone.userData.index = index;
    root.add(bone);
    return bone;
  });
  return { root, bones, byName: Object.fromEntries(bones.map(b => [b.name, b])) };
}

function clip(name, travel = 1) {
  const data = new Float32Array([0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, travel]);
  return new Clip(name, { offset: 0, frames: 2, tracks: [0], flags: 1, travel: [0, 0, travel] }, data, 1);
}

function animator() {
  const names = ['stand_idle_emote', 'stand_walkforward', 'stand_runforward', 'stand_walkbackward', 'stand_runbackward', 'stand_turnleft', 'stand_turnright', 'diveforward'];
  const clips = new Map(names.map(name => [`human_rifle_${name}`, clip(`human_rifle_${name}`)]));
  clips.set('human_grenade_stand_shoot3', clip('human_grenade_stand_shoot3', 0));
  return new SoldierAnimator(actor(), clips, { banks: { rifle: {}, grenade: {} } });
}

test('roll fits recovery into physics duration and returns to a finite moving gait', () => {
  const a = animator();
  const soldier = { aimYaw: 0, aimPitch: 0, velocity: new THREE.Vector3(-10, 0, 0), posture: 'stand', dive: 1, diveTime: 0.75, air: null };
  a.update(1 / 60, soldier);
  for (let i = 0; i < 43; i++) a.update(1 / 60, soldier);
  assert.equal(a.action,'dive');
  assert.equal(a.legs, -Math.PI / 2);
  soldier.dive = 0;
  for (let i = 0; i < 30; i++) a.update(1 / 60, soldier);
  assert.equal(a.action, 'loco');
  assert.notEqual(a.gait, 'idle', 'residual roll velocity must not force the idle pose');
  assert.ok(Number.isFinite(a.time));
  assert.ok(a.actor.root.quaternion.toArray().every(Number.isFinite));
});

test('native grenade actions retain their selected animation bank', async () => {
  const grenade=weaponDefinition('grenade',1,{base:'grenade',AnimationBank:'grenade',InitialSalvoDelay:.1,
    ordnance:{class:'grenade-round',base:'grenade',Velocity:10,LifeSpan:5}});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,
    weapons:[weaponDefinition('rifle',0),grenade]}});
  const unit=world.spawn('soldier',0,0),actions=[];
  unit.combat.arsenal.weapons[1].random=world.ordnance.random;
  unit.combat.onPresentation=event=>{if(event.kind==='weaponAction')actions.push([event.action,event.bank,event.posture]);};
  unit.intent.buttons[1]=1;
  for(let tick=1;tick<=4;tick++)world.phase.phase(1/60,tick);
  assert.ok(actions.some(([action,bank])=>action==='shoot3'&&bank==='grenade'),JSON.stringify(actions));
});

test('first-person grenade starts windup before release',()=>{
  const identity=new THREE.Matrix4().toArray(),windup=clip('_1st_grenade_shoot'),release=clip('_1st_grenade_shoot2');
  const view=new FirstPersonView({rig:{model:'arms',bones:[{name:'dummyroot',parent:-1,local:identity}]},parts:new Map(),clips:new Map([[windup.name,windup],[release.name,release]]),texture:()=>null,weaponModels:new Map(),firePoints:{}});
  const camera=new THREE.Camera();camera.updateMatrixWorld();const weapon={name:'g',bank:'grenade',def:{}},args={active:true,camera,soldier:{velocity:new THREE.Vector3(),posture:'stand',air:null},shown:weapon,zoomed:false};
  view.update(0,args);view.play('prepareThrow',weapon);view.update(0,args);assert.equal(view.clip,windup);view.play('throw',weapon);view.update(0,args);assert.equal(view.clip,release);
});

const assetPath = contentURL('common/anims.json');
test('retail animation samples remain finite through alternating rolls and throws', { skip: !existsSync(assetPath) }, () => {
  const meta = JSON.parse(readFileSync(assetPath));
  const bytes = readFileSync(contentURL('common/anims.bin'));
  // Authored clips need the actor's hierarchy and bind lengths. A flat list
  // of independent roots cannot represent the visible spine/neck chain.
  const model = JSON.parse(readFileSync(contentURL('sides/cis/actors/cis_inf_bdroid.json')));
  const body = actor(model.bones.map(b => b.name));
  model.bones.forEach((desc, i) => {
    new THREE.Matrix4().fromArray(desc.local).decompose(body.bones[i].position, body.bones[i].quaternion, body.bones[i].scale);
    if (desc.parent >= 0) body.bones[desc.parent].add(body.bones[i]);
  });
  const clips = loadClips(meta, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength), model.bones.map(b => b.name));
  const a = new SoldierAnimator(body, clips, meta.sets);
  const soldier = { aimYaw: 0, aimPitch: 0.2, velocity: new THREE.Vector3(), posture: 'stand', dive: 0, diveTime: clips.get('human_rifle_diveforward').duration * 0.75, air: null };
  for (const direction of [-1, 1, -1, 1]) {
    soldier.dive = direction;
    soldier.velocity.set(-direction * 10, 0, 0);
    for (let t = 0; t < soldier.diveTime; t += 1 / 60) a.update(1 / 60, soldier);
    soldier.dive = 0;
    soldier.velocity.set(0, 0, 4);
    a.setBank('grenade');
    a.weaponAction('shoot3', 'grenade', 'stand');
    for (let t = 0; t < 2.5; t += 1 / 60) {
      a.update(1 / 60, soldier);
      for (const bone of a.bones) assert.ok([...bone.quaternion.toArray(), ...bone.position.toArray()].every(Number.isFinite));
    }
    assert.equal(a.action, 'loco');
    assert.ok(Number.isFinite(a.time));
  }
});
