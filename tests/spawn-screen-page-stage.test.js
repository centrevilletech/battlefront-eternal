import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CameraRig, Spring, BODY } from "../src/spawn-screen/page/rig.js";
import { ShotDirector, HOLD } from "../src/spawn-screen/page/director.js";

const near = (a, b, e = 1e-3) => Math.abs(a - b) <= e;

test("framed, his feet and the top of his head land on the page's place for him, wherever it is", () => {
  const rig = new CameraRig();
  for (const [aspect, figure] of [[16 / 9, { x: -0.1, top: 0.75, bottom: -0.7 }], [4 / 3, { x: 0.2, top: 0.4, bottom: -0.9 }], [21 / 9, { x: 0, top: 0.9, bottom: -0.2 }]]) {
    assert.ok(rig.frame(aspect, figure));
    const camera = rig.rest();
    const feet = new THREE.Vector3(0, 0, 0).project(camera);
    const head = new THREE.Vector3(0, BODY, 0).project(camera);
    assert.ok(near(feet.x, figure.x), `across, ${aspect}`);
    assert.ok(near(feet.y, figure.bottom, 0.02) && near(head.y, figure.top, 0.02), `up, ${aspect}: ${feet.y} ${head.y}`);
  }
  assert.equal(rig.frame(16 / 9, { x: 0, top: -0.2, bottom: 0.3 }), false, "no place, no framing");
});

test("the camera turns about him: he keeps his place while it swings, drifts and sweeps in", () => {
  const rig = new CameraRig();
  rig.frame(16 / 9, { x: 0.1, top: 0.7, bottom: -0.7 });
  const at = () => new THREE.Vector3(0, 0.95, 0).project(rig.camera);
  const rest = at();
  rig.sweepIn();
  rig.update(0);
  const swept = at();
  assert.ok(near(swept.x, rest.x, 0.02), "his middle stays put as it comes round");
  const from = rig.camera.position.clone();
  for (let i = 0; i < 300; i++) rig.update(1 / 60);
  assert.ok(rig.camera.position.distanceTo(from) > 1, "it came in");
  assert.ok(near(rig.yaw.value, 0, 0.02) && near(rig.dolly.value, 1, 0.02), "and settled");
  rig.kick("spawn");
  for (let i = 0; i < 60; i++) rig.update(1 / 60);
  assert.ok(rig.dolly.value < 0.7, "SPAWN pushes in");
  rig.settle();
  for (let i = 0; i < 300; i++) rig.update(1 / 60);
  assert.ok(near(rig.dolly.value, 1, 0.02), "and back if the world didn't take it");
});

test("a spring settles on its target without overshooting", () => {
  const spring = new Spring(1, 3);
  spring.target = 0;
  let low = Infinity;
  for (let i = 0; i < 240; i++) {
    spring.step(1 / 60);
    low = Math.min(low, spring.value);
  }
  assert.ok(near(spring.value, 0, 1e-3));
  assert.ok(low > -1e-6, "critically damped");
});

const SHOTS = [
  { position: [0, 10, 0], quaternion: [0, 0, 0, 1] }, // looking -z
  { position: [100, 10, 0], quaternion: new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), Math.PI / 2).toArray() }, // looking -x
];

test("the battle behind: a shot held, drifting, then a cut to the next", () => {
  const director = new ShotDirector(SHOTS);
  const first = director.update(0.016);
  assert.equal(first.cut, true);
  assert.equal(director.index, 0);
  const later = director.update(HOLD / 2);
  assert.equal(later.cut, false);
  assert.notDeepEqual(later.position, first.position, "it pushes in");
  assert.notDeepEqual(later.quaternion, first.quaternion, "and pans");
  const next = director.update(HOLD);
  assert.equal(next.cut, true);
  assert.equal(director.index, 1);
  assert.equal(new ShotDirector([]).update(1), null, "no shots, no view");
});

test("a post picked brings up the shot that sees it best, turned partway to it", () => {
  const director = new ShotDirector(SHOTS);
  director.update(0.016);
  // West of the second shot, which looks -x at it.
  director.follow([60, 0, 0]);
  assert.equal(director.index, 1);
  const view = director.update(0.016);
  assert.equal(view.cut, true);
  const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(new THREE.Quaternion().fromArray(view.quaternion));
  assert.ok(forward.y < -0.05, "turned down toward it");
  // In front of the first.
  director.follow([0, 10, -50]);
  assert.equal(director.index, 0);
});
