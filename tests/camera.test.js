import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { SoldierCamera } from '../src/camera.js';

// A wall that enters the old camera position as the player approaches an arch.
function fixture(responsive = false) {
  let wall = -10;
  const world = {
    cast(origin, direction, far, hit) {
      if (direction.z >= 0) return null;
      const distance = (wall - origin.z) / direction.z;
      if (distance < 0 || distance > far) return null;
      hit.distance = distance;
      return hit;
    },
  };
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.05, 1000);
  const view = new SoldierCamera(camera, world, {
    cameras: {
      STAND: { TrackCenter: [0, 1.8, 0], TrackOffset: [0, 0.5, 4], TiltValue: 0 },
      PRONE: { TrackCenter: [0, 0.5, 0], TrackOffset: [0, 0.2, 3], TiltValue: 0 },
    },
  }, { responsive });
  return { camera, view, wall: z => { wall = z; } };
}

for (const posture of ['stand', 'prone']) {
  test(`chase camera resolves its rendered ${posture} position immediately at a wall`, () => {
    const f = fixture();
    const feet = new THREE.Vector3();
    f.view.update(1 / 60, feet, 0, 0, 'stand');
    assert.ok(f.camera.position.z < -3.9);
    f.wall(-1);
    f.view.update(1 / 60, feet, 0, 0, posture);
    assert.ok(f.camera.position.z > -1, 'easing must never leave the rendered camera behind the wall');
    assert.ok(f.camera.position.z < -0.8, 'retain the unobstructed portion of the boom');
    f.wall(-10);
    const close = f.camera.position.z;
    f.view.update(1 / 60, feet, 0, 0, posture);
    assert.ok(f.camera.position.z < close, 'ease back out after clearing the obstruction');
    assert.ok(f.camera.position.z > -2, 'do not jump straight to the full boom');
  });
}

test('pitching and orbiting near the floor does not pump the camera across its safety skin', () => {
  const world = { cast(origin, direction, far, hit) {
    const distance = -origin.y / direction.y;
    if (direction.y >= 0 || distance < 0 || distance > far) return null;
    hit.distance = distance;
    return hit;
  } };
  for (const responsive of [false, true]) for (const hz of [30, 60, 144]) {
    const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.05, 1000);
    const view = new SoldierCamera(camera, world, { cameras: {
      STAND: { TrackCenter: [0, 1.8, 0], TrackOffset: [0, 0.5, 4], TiltValue: 5 },
    } }, { responsive });
    const feet = new THREE.Vector3(), samples = [];
    for (let i = 0; i < hz * 1.3; i++) {
      const t = i / hz;
      view.update(1 / hz, feet, t * 0.8, Math.sin(t * 1.5) * 1.2, 'stand');
      samples.push(camera.position.clone());
      if (t > 0.85) {
        const acceleration = samples[i].clone().sub(samples[i - 1]).sub(
          samples[i - 1].clone().sub(samples[i - 2])).length() * hz * hz;
        assert.ok(acceleration < 20, `${hz} Hz, responsive=${responsive}: floor contact jolted at ${acceleration.toFixed(1)} m/s²`);
        assert.ok(camera.position.y > 0.05, 'camera stays above the floor');
      }
    }
  }
});

test('responsive chase view follows mouse aim and an unobstructed orbit on the same frame', () => {
  for (const hz of [30, 60, 144, 240]) {
    const {view,camera} = fixture(true), feet = new THREE.Vector3();
    view.update(1/hz,feet,0,0,'stand');
    for (const yaw of [.5, -.3, 1.2, 0]) {
      view.update(1/hz,feet,yaw,.2,'stand');
      const expected = new THREE.Quaternion().setFromEuler(new THREE.Euler(.2,yaw+Math.PI,0,'YXZ'));
      assert.ok(camera.quaternion.angleTo(expected)<1e-7, `${hz} Hz: no filtered mouse rotation`);
      const position = new THREE.Vector3(0,.5,4).applyQuaternion(expected).add(new THREE.Vector3(0,1.8,0));
      assert.ok(camera.position.distanceTo(position)<1e-7, `${hz} Hz: orbit must not trail the aim`);
    }
    const before = camera.position.clone(); feet.set(.2,0,.1);
    view.update(1/hz,feet,0,.2,'stand');
    assert.ok(camera.position.clone().sub(before).distanceTo(feet)<1e-7,'interpolated feet must not get filtered twice');
  }
});

test('responsive chase view still retracts at walls and eases only its release and stance changes', () => {
  const f=fixture(true), feet=new THREE.Vector3();
  f.view.update(1/60,feet,0,0,'stand'); f.wall(-1);
  f.view.update(1/60,feet,0,0,'stand');
  assert.ok(f.camera.position.z> -1 && f.camera.position.z<-.8);
  f.wall(-10); const close=f.camera.position.z;
  f.view.update(1/60,feet,0,0,'stand');
  assert.ok(f.camera.position.z<close && f.camera.position.z> -2);
  f.view.update(1/60,feet,0,0,'prone');
  assert.ok(f.view.followCenter.y>.5 && f.view.followCenter.y<1.8,'stance height blends without filtering the feet');
});

test('camera releases continuously as clearance grows instead of waiting for a boom dead band', () => {
  const f = fixture(), feet = new THREE.Vector3();
  f.wall(-1);
  for (let i = 0; i < 120; i++) f.view.update(1 / 120, feet, 0, 0, 'stand');
  for (let i = 1; i <= 120; i++) {
    const before = f.view.reach;
    f.wall(-1 - i * 0.005);
    f.view.update(1 / 120, feet, 0, 0, 'stand');
    assert.ok(f.view.reach > before, 'every increase in clearance should ease the boom outward');
    assert.ok(f.camera.position.z > -1 - i * 0.005, 'the camera remains in front of the wall');
  }
});
