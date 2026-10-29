import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { PlayerCombat } from '../src/combat.js';
import { SoldierCamera } from '../src/camera.js';

function fixture() {
  const combat = Object.create(PlayerCombat.prototype);
  const soldier = { position: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, posture: 'stand', air: null, jet: { recharge: 0 } };
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, .05, 2000);
  const def = { cameras: { STAND: { TrackCenter: [0, 1.8, 0], TrackOffset: [0, .5, 4], TiltValue: 5 } } };
  const weapon = { def: {} }, values = [];
  Object.assign(combat, { soldier, camera, def, health: 100, maxHealth: 100, zoomStep: 0,
    arsenal: { current: () => weapon }, hud: { setVehicle() {}, setPlayer() {}, setSecondBar() {}, setScope() {}, setReticle(y) { values.push(y); } } });
  for (const key of ['origin', 'pivot', 'aimDir', 'left', 'up', 'worldPoint', 'screen']) combat[key] = new THREE.Vector3();
  combat.aimBasis();
  return { combat, soldier, camera, values, view: new SoldierCamera(camera, { cast: () => null }, def, { responsive: true }) };
}

test('reticle stays with displayed aim while authoritative aim arrives in uneven batches', () => {
  for (const firstPerson of [true, false]) {
    const { combat, soldier, camera, values, view } = fixture();
    const pose = Object.assign(Object.create(soldier), { position: new THREE.Vector3(), aimYaw: 0, aimPitch: 0 });
    const eye = new THREE.Vector3();
    combat.firstPerson = firstPerson;
    for (let frame = 0; frame < 120; frame++) {
      pose.aimYaw = frame * .025; pose.aimPitch = Math.sin(frame * .025) * .4;
      if (frame % 9 === 0) {
        soldier.aimYaw = pose.aimYaw; soldier.aimPitch = pose.aimPitch;
        combat.aimBasis();
      }
      const authoritative = [combat.origin.toArray(), combat.aimDir.toArray()];
      combat.viewEye(pose.position, eye, pose);
      view.update(1 / 120, pose.position, pose.aimYaw, pose.aimPitch, 'stand', 1, firstPerson, eye);
      camera.updateMatrixWorld();
      combat.updateHud(null, pose);
      assert.deepEqual([combat.origin.toArray(), combat.aimDir.toArray()], authoritative);
      assert.notEqual(values.at(-1), null);
      if (firstPerson) assert.ok(Math.abs(values.at(-1) - .5) < 1e-8);
      if (values.length > 1) assert.ok(Math.abs(values.at(-1) - values.at(-2)) < .001, 'no packet-timed crosshair jump');
    }
  }
});

test('predicted eye follows displayed feet despite a distant acknowledged position', () => {
  const { combat, soldier } = fixture();
  soldier.position.set(20, 3, 40); combat.aimBasis();
  const pose = Object.assign(Object.create(soldier), { position: new THREE.Vector3(24, 4, 45), aimYaw: 1.2, aimPitch: .3 });
  const eye = combat.viewEye(pose.position, new THREE.Vector3(), pose);
  assert.ok(eye.distanceTo(pose.position) < 2, 'eye is attached to the displayed body');
  const next = pose.position.clone().add(new THREE.Vector3(1, 0, 0));
  const moved = combat.viewEye(next, new THREE.Vector3(), pose);
  assert.ok(moved.sub(eye).distanceTo(new THREE.Vector3(1, 0, 0)) < 1e-8);
});
