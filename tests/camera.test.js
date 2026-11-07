import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {SoldierCamera,CameraFrame,openCameraWorld} from './helpers/cameras.js';
import {CollisionWorld} from '../tools/collision-fixture.mjs';
// Collision cases use the native ray world rather than a JavaScript cast
// callback: the camera's fan of queries stays inside its coarse phase.
test('responsive chase view follows mouse aim and an unobstructed orbit on the same frame', () => {
  for (const hz of [30, 60, 144, 240]) {
    const camera = new THREE.PerspectiveCamera(60,16/9,.05,1000), view = new SoldierCamera(camera,openCameraWorld(),{cameras:{STAND:{TrackCenter:[0,1.8,0],TrackOffset:[0,.5,4],TiltValue:0}}},{responsive:true}), feet = new THREE.Vector3();
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


for(const responsive of [false,true])test(`native chase retracts against a wall and releases continuously: ${responsive}`,()=>{
  const wall=new THREE.BoxGeometry(40,40,.1).translate(0,0,-1.05);
  const world=new CollisionWorld(wall.attributes.position.array,wall.index.array,new Uint32Array(wall.index.count/3));
  const h=new CameraFrame(world,undefined,{cameras:{STAND:{TrackCenter:[0,1.8,0],TrackOffset:[0,.5,4],TiltValue:0}}});
  h.owner.view.responsive=responsive;h.frame(1/60);h.finish();
  assert.ok(h.camera.position.z> -1 && h.camera.position.z<-.8);
  const before=h.camera.position.z;h.state.position.z=2;h.frame(1/60);h.finish();
  assert.ok(h.camera.position.z>before,'moving away releases some boom immediately');
  assert.ok(h.camera.matrixWorld.elements.every(Number.isFinite));
});
