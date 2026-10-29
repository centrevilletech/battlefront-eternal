import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { weaponMuzzle, descendantMuzzles, vehicleMuzzleTips } from '../src/muzzles.js';
import { Turret } from '../src/props.js';
import { Effect } from '../src/effects.js';
import { RemoteDroids } from '../src/remote.js';

test('missing weapon markers recover a mouth and imported anchors stay authoritative', () => {
  const part = length => ({ geometry: new THREE.CylinderGeometry(.02, .02, length, 8).rotateX(Math.PI / 2).translate(.01, .05, length / 2) });
  const def = { ordnance: { base: 'bolt' } };
  for (const authored of [null, [0, 0, 0], [0, .05, .1]]) {
    if (authored?.[2] > .05) assert.equal(weaponMuzzle([part(.4)], authored, def), authored);
    else {
      assert.ok(new THREE.Vector3(...weaponMuzzle([part(.4)], authored, def)).distanceTo(new THREE.Vector3(.01, .05, .4)) < 1e-6);
      assert.ok(Math.abs(weaponMuzzle([part(.8)], authored, def)[2] - .8) < 1e-6);
    }
  }
  const marker = [.2, .3, .4];
  assert.equal(weaponMuzzle([part(.4)], marker, { base: 'grenade', ordnance: {} }), marker);
  assert.equal(weaponMuzzle([part(.01)], marker, def), marker, 'invisible SBD marker is not a barrel');
});

test('vehicle hardpoints follow the owning pivot without stealing another turret', () => {
  const root = new THREE.Bone(), head = new THREE.Bone(), cannon = new THREE.Bone(), side = new THREE.Bone(), sideGun = new THREE.Bone();
  root.add(head); head.add(cannon, side); side.add(sideGun);
  const names = { aimer_head: head, hp_fire_chin: cannon, aimer_side: side, hp_fire_side: sideGun };
  assert.deepEqual(descendantMuzzles(head, names, new Set([side])), [cannon]);
});

test('skinned vehicle barrel tips use inverse bind space', () => {
  const rig = { bones: [{ name: 'gun', parent: -1, local: new THREE.Matrix4().makeTranslation(4, 5, 6).toArray() }] };
  const geometry = new THREE.CylinderGeometry(.1, .1, 2, 8).rotateX(Math.PI / 2).translate(4, 5, 7);
  const parts = [{ geometry, joints: new Uint16Array(geometry.attributes.position.count) }];
  const tip = vehicleMuzzleTips(rig, parts).get('gun');
  assert.ok(new THREE.Vector3(...tip).distanceTo(new THREE.Vector3(0, 0, 2)) < 1e-5);
});

test('turret aiming reads do not consume barrels; only shots alternate', () => {
  const root = new THREE.Group(), left = new THREE.Object3D(), right = new THREE.Object3D();
  left.position.set(-1, 2, 3); right.position.set(1, 2, 3); root.add(left, right);
  const turret = { rig: { root }, gun: root, firePoints: [left, right], barrel: 0 };
  const p = new THREE.Vector3(), d = new THREE.Vector3();
  for (let shot = 0; shot < 6; shot++) {
    for (let read = 0; read < 5; read++) Turret.prototype.muzzle.call(turret, p, d);
    assert.equal(turret.barrel, shot);
    Turret.prototype.muzzle.call(turret, p, d, true);
    assert.equal(p.x, shot % 2 ? 1 : -1);
  }
});

test('classic exhaust shifts and rotates each emitter without changing shared definitions', () => {
  const def = { maxParticles: [-1, -1], startDelay: [0, 0], size: [1, 1], color: { channels: [[255, 255], [255, 255], [255, 255], [255, 255]] }, spawner: { offset: [[-1, -1], [0, 0], [0, 0]] } };
  const snapshot = JSON.stringify(def), effect = new Effect([def], new THREE.Vector3(3, 4, 5), new THREE.Quaternion(), new THREE.Vector3());
  const frame = { position: new THREE.Vector3(2, 4.1, 5), direction: new THREE.Vector3(0, -1, 0) };
  effect.attachNozzles([frame]);
  const em = effect.emitters[0];
  const at = new THREE.Vector3(-1, 0, 0).applyQuaternion(em.nozzleTurn).add(em.nozzleOffset).add(effect.position);
  assert.ok(at.distanceTo(frame.position) < 1e-8);
  assert.ok(new THREE.Vector3(-1, 0, 0).applyQuaternion(em.nozzleTurn).distanceTo(frame.direction) < 1e-8);
  assert.equal(JSON.stringify(def), snapshot);
});

test('remote droid rounds and flash start at its rotated hardpoint without a render mesh', () => {
  const fired = [], flashes = [], weapon = { def: { ordnance: { base: 'bolt' }, FlashLightRadius: 1 } };
  const system = { v: Array.from({ length: 5 }, () => new THREE.Vector3()), firePoints: { probe: [0, .2, .5] }, fire: (def, p, d) => fired.push({ p: p.clone(), d: d.clone() }), lights: { flash: p => flashes.push(p.clone()) } };
  const droid = { def: { GeometryName: 'probe' }, position: new THREE.Vector3(3, 4, 5), yaw: Math.PI / 2, target: new THREE.Vector3(20, 4, 5), arsenal: { update: () => [{ weapon, yaw: 0, pitch: 0 }], current: () => null }, countdown: -1 };
  RemoteDroids.prototype.shoot.call(system, droid, 1 / 60, null);
  assert.ok(fired[0].p.distanceTo(new THREE.Vector3(3.5, 4.2, 5)) < 1e-8);
  assert.ok(fired[0].d.distanceTo(droid.target.clone().sub(fired[0].p).normalize()) < 1e-8);
  assert.ok(flashes[0].equals(fired[0].p));
});

test('the imported barrel cap is not displaced by a longer underslung rail', () => {
  const barrel = { geometry: new THREE.CylinderGeometry(.03, .03, .4, 8).rotateX(Math.PI / 2).translate(0, .06, .2) };
  const rail = { geometry: new THREE.BoxGeometry(.03, .01, .6).translate(0, 0, .3) };
  const point = weaponMuzzle([barrel, rail], [0, .06, .4], { ordnance: {} });
  assert.ok(new THREE.Vector3(...point).distanceTo(new THREE.Vector3(0, .06, .4)) < 1e-6);
});
