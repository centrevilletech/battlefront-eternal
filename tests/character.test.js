import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { Droideka } from '../src/droideka.js';
import { SoldierAnimator } from '../src/animator.js';
import { PlayerCombat } from '../src/combat.js';
import { FirstPersonView } from '../src/firstperson.js';
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

const floor = {
  gather: () => [],
  cast(from, direction, reach, hit) {
    if (direction.y >= 0) return null;
    const distance = -from.y / direction.y;
    if (distance < 0 || distance > reach) return null;
    hit.distance = distance;
    hit.point.copy(from).addScaledVector(direction, distance);
    hit.normal.set(0, 1, 0);
    hit.terrain = true;
    return hit;
  },
};

function droideka() {
  return new Droideka(floor, { BallRollingFriction: 50, BallAcceleration: 100 }, new Map(), actor());
}

function aimState() {
  return Object.fromEntries(['aimDir', 'left', 'up', 'origin', 'pivot', 'from', 'target', 'shotDir'].map(k => [k, new THREE.Vector3()]));
}

test('droideka aim is independent of world origin and stays ahead of both barrels', () => {
  const d = droideka();
  let ray;
  d.world = { cast(from) { ray = from.clone(); return null; } };
  for (const yaw of [0, Math.PI / 2, -Math.PI / 2, Math.PI]) {
    d.position.set(1200, 60, -1900);
    d.aimYaw = yaw;
    const c = aimState();
    const forward = new THREE.Vector3(Math.sin(yaw), 0, Math.cos(yaw));
    c.camera = { position: d.position.clone().add(new THREE.Vector3(0, 2.3, 0)).addScaledVector(forward, -4.5) };
    d.aim(c);
    assert.ok(Math.abs(ray.clone().sub(c.origin).dot(forward)) < 1e-8);
    assert.ok(c.shotDir.dot(forward) > 0.999);
  }
});

test('turn-only droideka input pivots correctly without propulsion', () => {
  const d = droideka();
  d.enter(12);
  for (let i = 0; i < 60; i++) d.tick({ right: 1 }, 1 / 60);
  assert.ok(d.aimYaw < -1.9, 'right input must turn right, matching soldier controls');
  assert.equal(d.spin, 0);
  assert.ok(Math.hypot(d.position.x, d.position.z) < 1e-8);
  assert.ok(Math.abs(d.position.y) < 1e-8, 'the final physics position must be resolved onto ground');
});

test('a target behind a droideka muzzle cannot reverse its emitted bolt', () => {
  const c = Object.create(PlayerCombat.prototype);
  let emitted;
  Object.assign(c, aimState(), {
    soldier: { nextGun: out => out.set(0, 1, 2), posture: 'stand' },
    fireDir: new THREE.Vector3(), side: new THREE.Vector3(), gunPoint: new THREE.Vector3(),
    held: new Map(), kick: { offset: 0 },
    animator: { weaponAction() {}, fire() {} }, flash() {},
    ordnance: { fire(def, from, direction) { emitted = direction.clone(); } },
  });
  c.aimDir.set(0, 0, 1);
  c.shotDir.set(0, 0, 1);
  c.target.set(0, 1, 1);
  c.fire({ weapon: { def: { ordnance: { base: 'bolt' } }, bank: 'bdroid' }, yaw: 0, pitch: 0 });
  assert.ok(emitted.z > 0.99);
});

test('rolling steering follows heading, retains top speed, and brakes without throttle', () => {
  const d = droideka();
  d.enter(12);
  for (let i = 0; i < 60; i++) d.tick({ forward: 1 }, 1 / 60);
  assert.ok(Math.abs(d.velocity.length() - 13) < 1e-6);
  d.tick({ forward: 1, right: 1 }, 1 / 60);
  assert.ok(d.velocity.x < 0);
  const before = d.aimYaw;
  for (let i = 0; i < 60; i++) d.tick({ right: 1 }, 1 / 60);
  assert.equal(d.spin, 0);
  assert.ok(d.aimYaw < before - 1.9);
});

test('rolling final positions and speeds are stable across physics rates', () => {
  const runs = [30, 60, 120].map(hz => {
    const d = droideka();
    d.enter(12);
    for (let i = 0; i < hz * 2; i++) d.tick({ forward: 1 }, 1 / hz);
    assert.ok(Math.abs(d.position.y) < 1e-8);
    return d.position.z;
  });
  assert.ok(Math.max(...runs) - Math.min(...runs) < 0.2);
});

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
  assert.ok(a.layers.at(-1).time > 0.9, 'the recovery must not be cut off at 75% of the clip');
  assert.equal(a.legs, -Math.PI / 2);
  soldier.dive = 0;
  for (let i = 0; i < 30; i++) a.update(1 / 60, soldier);
  assert.equal(a.action, 'loco');
  assert.notEqual(a.gait, 'idle', 'residual roll velocity must not force the idle pose');
  assert.ok(a.layers.every(l => Number.isFinite(l.time)));
  assert.ok(a.actor.root.quaternion.toArray().every(Number.isFinite));
});

test('active locomotion layer stays last when a fading walk is reused', () => {
  const a = animator();
  const walk = a.clips.get('human_rifle_stand_walkforward');
  a.play(walk, 0.01);
  a.play(a.diveClip, 0.01, 0);
  a.play(walk, 0.01);
  assert.equal(a.layers.at(-1).clip, walk);
});

test('grenade action survives the bank switch in the real combat update', () => {
  const a = animator();
  const primary = { def: {}, bank: 'rifle' };
  const grenade = { def: {}, bank: 'grenade', thrown: true, charge: 1, busy: false };
  const c = Object.create(PlayerCombat.prototype);
  Object.assign(c, {
    soldier: { posture: 'stand', air: null, dive: 0, lock: 0 }, animator: a,
    arsenal: { weapons: [primary, grenade], current: ch => ch ? grenade : primary, update() { grenade.busy = true; return []; } },
    held: new Map(), seen: new Map(),
    aim() {}, updateGlow() {}, updateKick() {}, updateFlashes() {},
  });
  c.update(1 / 60, { take: () => false, held: () => false }, 0);
  assert.equal(a.bank, 'grenade');
  assert.equal(a.weaponLayer?.clip.name, 'human_grenade_stand_shoot3');
});

test('first-person grenade starts windup before release', () => {
  const view = Object.create(FirstPersonView.prototype);
  const windup = clip('_1st_grenade_shoot');
  const release = clip('_1st_grenade_shoot2');
  Object.assign(view, { wasActive: true, bones: [], transitionFrom: [], clips: new Map([[windup.name, windup], [release.name, release]]) });
  const weapon = { bank: 'grenade' };
  view.play('prepareThrow', weapon);
  assert.equal(view.clip, windup);
  view.play('throw', weapon);
  assert.equal(view.clip, release);
});

const assetPath = new URL('../public/common/anims.json', import.meta.url);
test('retail animation samples remain finite through alternating rolls and throws', { skip: !existsSync(assetPath) }, () => {
  const meta = JSON.parse(readFileSync(assetPath));
  const bytes = readFileSync(new URL('../public/common/anims.bin', import.meta.url));
  const clips = loadClips(meta, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  const a = new SoldierAnimator(actor(meta.bones), clips, meta.sets);
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
    assert.ok(a.weaponLayer === null, 'throw must finish and fade out');
  }
});
