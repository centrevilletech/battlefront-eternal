import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CameraShake, shakeShare, SHAKE_SLOTS } from '../src/camera-shake.js';

// The orbital strike's explosion (every side's: Shake 2 for 1 s, 10 to 30 m).
const STRIKE = { Shake: 2, ShakeLength: 1, ShakeRadiusInner: 10, ShakeRadiusOuter: 30 };
const near = (a, b, message) => assert.ok(Math.abs(a - b) < 1e-9, `${message}: ${a} vs ${b}`);

test('the share falls off from what the camera follows, at its origin less half its sphere', () => {
  near(shakeShare(STRIKE, 10.5, 1.0), 1, 'inside the inner radius once half the sphere is off');
  near(shakeShare(STRIKE, 20.73, 1.46), (30 - 20) / 20, 'halfway out for a standing soldier 20.73 m off');
  near(shakeShare(STRIKE, 30.5, 0), 0, 'nothing past the outer radius');
  assert.equal(shakeShare(STRIKE, 0.00005, 0), 0, 'nothing at the blast itself');
  assert.equal(shakeShare({ Shake: 1, ShakeRadiusInner: 5 }, 1, 0), 0, 'nothing without an outer radius');
  assert.equal(shakeShare({ ShakeRadiusOuter: 10 }, 1, 0), 0, 'nothing without a shake');
});

test('an explosion takes two shakes of its share that die away over its length; four at most', () => {
  const shake = new CameraShake(() => 1);
  const target = { position: new THREE.Vector3(0, 0, 20.73), radius: 1.46 };
  near(shake.explode(STRIKE, [0, 0, 0], target), 0.5, 'half a share');
  assert.deepEqual(shake.amounts, [1, 1], 'Shake x share, twice over');
  near(shake.rates[0], 2, 'falling at Shake / ShakeLength, whatever the share');
  // The frame's sum is taken before its decay.
  near(shake.update(0.25), 2, 'the first frame takes both whole');
  near(shake.amounts[0], 0.5, 'then each has fallen by dt x its rate');
  near(shake.update(0.25), 1, 'the next frame');
  assert.equal(shake.amounts.length, 0, 'gone once at 0: ShakeLength x share = 0.5 s');
  // A fifth shake is dropped.
  shake.explode(STRIKE, [0, 0, 0], target);
  shake.explode(STRIKE, [0, 0, 0], target);
  shake.explode(STRIKE, [0, 0, 0], target);
  assert.equal(shake.amounts.length, SHAKE_SLOTS);
  // Without a ShakeLength the class's 0.5 s.
  const other = new CameraShake();
  other.explode({ Shake: 4, ShakeRadiusOuter: 7 }, [0, 0, 0], { position: new THREE.Vector3(0, 1, 0), radius: 0 });
  near(other.rates[0], 4 * (6 / 7) / (0.5 * (6 / 7)), 'default length 0.5 s');
});

test('the view turns about its own axes by (2r - 1)^3 x 0.1 x the sum; never paused or in the death camera', () => {
  const camera = new THREE.PerspectiveCamera();
  const shake = new CameraShake(() => 1);
  shake.add(2, 1);
  assert.equal(shake.apply(camera, 0), 0, 'paused, nothing moves');
  assert.ok(camera.quaternion.equals(new THREE.Quaternion()), 'and the view stays');
  near(shake.apply(camera, 1 / 60), 2, 'the sum this frame');
  // r = 1: 0.2 rad about x, then y, then z.
  const expected = new THREE.Quaternion().setFromEuler(new THREE.Euler(0.2, 0.2, 0.2, 'XYZ'));
  assert.ok(camera.quaternion.angleTo(expected) < 1e-6, 'turned 0.2 rad about each axis');
  const still = new THREE.PerspectiveCamera();
  shake.apply(still, 1 / 60, true);
  assert.ok(still.quaternion.equals(new THREE.Quaternion()), 'the death camera is not shaken');
  near(shake.amounts[0], 2 - 2 * 2 / 60, 'though its shake still dies away');
});
