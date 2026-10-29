import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { SoldierPresentation, HeadlessSoldierPresentation } from '../src/soldier-presentation.js';
import {ENTITY_SOLDIER_STATES} from '../src/runtime/network-entity-codec.js';

test('headless presentation follows authoritative movement and respawn without a pose cache', () => {
  const { soldier } = fixture();
  soldier.mode = 'walk'; soldier.legs = 0;
  const view = new HeadlessSoldierPresentation(soldier);
  view.beforeStep();
  soldier.position.set(2, 3, 4); soldier.velocity.set(1, 0, 2);
  soldier.aimYaw = 1; soldier.legs = 2;
  view.afterStep();
  assert.equal(view.update(1 / 60, 0), soldier);
  assert.equal(view.position, soldier.position);
  assert.equal(view.velocity, soldier.velocity);
  soldier.position.set(100, 20, -10); soldier.aimYaw = -2;
  view.reset();
  assert.equal(view.previousYaw, -2);
  assert.equal(view.update().position.x, 100);
  assert.equal(view.previousPose, undefined);
});

function fixture() {
  const soldier = { position: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, groundAdjustment: 0, posture: 'stand', air: null };
  return { soldier, view: new SoldierPresentation(soldier) };
}

test('fixed-rate rendering interpolates movement, velocity and keyboard turn together', () => {
  const { soldier: s, view: v } = fixture();
  v.beforeStep();
  s.position.z = 0.1; s.velocity.z = 6; s.aimYaw = 0.04;
  v.afterStep();
  const samples = [0, 0.25, 0.5, 0.75, 1].map(alpha => {
    const state = v.update(1 / 240, alpha);
    return [state.position.z, state.velocity.z, state.aimYaw];
  });
  for (let i = 0; i < samples.length; i++) {
    assert.ok(Math.abs(samples[i][0] - i * 0.025) < 1e-10);
    assert.ok(Math.abs(samples[i][1] - i * 1.5) < 1e-10);
    assert.ok(Math.abs(samples[i][2] - i * 0.01) < 1e-10);
  }
  assert.equal(s.position.z, 0.1, 'rendering never alters authoritative position');
});

test('mouse aim received between physics ticks stays immediate', () => {
  const { soldier: s, view: v } = fixture();
  v.beforeStep(); s.aimYaw = 0.04; v.afterStep();
  const before = v.update(1 / 144, 0.2).aimYaw;
  s.aimYaw += 0.3; s.aimPitch = -0.4;
  const after = v.update(0, 0.2);
  assert.ok(Math.abs(after.aimYaw - before - 0.3) < 1e-10);
  assert.equal(after.aimPitch, -0.4);
});

test('stairs smooth the rendered body without changing collision or adding jump lag', () => {
  for (const height of [-0.35, 0.35]) {
    const { soldier: s, view: v } = fixture();
    v.beforeStep(); s.position.y = height; s.groundAdjustment = height; v.afterStep();
    const first = v.update(1 / 60).position.y;
    assert.ok(Math.abs(first) < 0.08, '35cm riser does not teleport the rendered body');
    assert.equal(s.position.y, height);
    for (let i = 0; i < 30; i++) v.update(1 / 60);
    assert.ok(Math.abs(v.position.y - height) < 0.001, 'visual foot settles promptly');
    v.beforeStep(); s.position.y += 0.12; s.air = 'jump'; v.afterStep();
    const beforeJump = v.position.y;
    v.update(1 / 60);
    assert.ok(Math.abs(v.position.y - beforeJump - 0.12) < 0.001, 'airborne travel remains immediate');
  }
});

test('repeated frozen draws preserve the pose and spawning clears stair lag', () => {
  const { soldier: s, view: v } = fixture();
  v.beforeStep(); s.position.y = 0.3; s.groundAdjustment = 0.3; v.afterStep();
  v.update(1 / 60);
  const position = v.position.clone();
  for (let i = 0; i < 20; i++) v.update(0);
  assert.ok(v.position.distanceTo(position) < 1e-12);
  s.position.set(100, 20, -10); s.groundAdjustment = 0; v.reset(); v.update(0, 0);
  assert.ok(v.position.equals(s.position));
});

test('angle wrapping takes the short turn while preserving live movement states', () => {
  const { soldier: s, view: v } = fixture();
  s.aimYaw = Math.PI - 0.01; v.reset(); v.beforeStep();
  s.aimYaw = -Math.PI + 0.01; s.posture = 'prone'; v.afterStep();
  const state = v.update(1 / 144, 0.5);
  assert.ok(Math.abs(Math.abs(state.aimYaw) - Math.PI) < 1e-10);
  assert.equal(state.posture, 'prone');
});

test('remote animation flags follow sampled motion while authoritative collision state stays current',()=>{
 const {soldier:s,view:v}=fixture();
 Object.assign(s,{air:null,dead:true,dive:0,posture:'stand'});
 for(const [mode,posture,air,dive,dead] of [
  ['ground',1,null,0,false],['diveLeft',1,null,-1,false],
  ['jump',0,'jump',0,false],['thrownDead',0,'thrown',0,true],['dead',0,null,0,true]
 ]) {
  v.networkPose={kind:'soldier',position:[1,2,3],velocity:[4,5,6],yaw:.2,pitch:.1,posture,state:ENTITY_SOLDIER_STATES.indexOf(mode)};
  const state=v.update(1/60);
  assert.equal(state.air,air,mode);assert.equal(state.dead,dead,mode);assert.equal(state.dive,dive,mode);
  assert.equal(state.posture,posture===1?'crouch':'stand');
  assert.equal(s.air,null);assert.equal(s.dead,true);assert.equal(s.posture,'stand');
 }
 v.networkPose=null;Object.assign(s,{air:'jet',dead:false,posture:'prone',dive:1});
 const local=v.update(1/60);
 assert.equal(local.air,'jet');assert.equal(local.dead,false);assert.equal(local.posture,'prone');assert.equal(local.dive,1);
 v.deathPending=true;v.reset();
 assert.equal(v.deathPending,false,'a new life cannot inherit delayed death');
 assert.equal(Object.hasOwn(v.state,'air'),false,'reset resumes live state inheritance');
});

test('droideka axle and lean interpolate at display rate without wrapping the spin or mixing mode clocks', () => {
  const { soldier: s, view: v } = fixture();
  Object.assign(s, { mode: 12, roll: 6.2, lean: 0, legs: 0, time: 0.4 });
  v.reset(); v.beforeStep();
  Object.assign(s, { roll: 6.6, lean: 0.1, legs: 0.05, time: 0.42 }); v.afterStep();
  v.update(1 / 120, 0.5);
  assert.ok(Math.abs(v.state.roll - 6.4) < 1e-10);
  assert.equal(v.state.lean, 0.05);
  assert.equal(v.state.legs, 0.025);
  assert.ok(Math.abs(v.state.time - 0.41) < 1e-10);
  v.beforeStep(); s.mode = 13; s.time = 0.01; v.afterStep(); v.update(1 / 120, 0.5);
  assert.equal(v.state.time, 0.01);
});

test('a body whose records stop is drawn where it is, not where the last stepped tick began', () => {
  const { soldier, view } = fixture();
  // Riding 7 m up (the AT-TE's gunner), then off the seat and on records.
  soldier.position.set(0, 7, 0);
  view.beforeStep();
  soldier.position.set(2, 0, 1);
  view.networkPose = { kind: 'soldier', position: [2, 0, 1], velocity: [0, 0, 0], yaw: 0, pitch: 0, state: 0, posture: 0 };
  assert.deepEqual(view.update(1 / 60, 0).position.toArray(), [2, 0, 1]);
  // At rest the records stop; the page steps no tick for him.
  view.networkPose = null;
  assert.deepEqual(view.update(1 / 60, 0).position.toArray(), [2, 0, 1], 'on the ground, not back on the seat');
});
