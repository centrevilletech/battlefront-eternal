import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {RemoteDroids} from '../src/remote.js';

// Flight, weapons and the use edge are covered by the native recon replay.
// This fixture observes only the live resource shell's cosmetic/control state.
test('recon hover, eye and zoom visibility, and control handback preserve the live hull',()=>{
  const definitions=[],bound=[];
  const remote=new RemoteDroids({scene:new THREE.Scene(),camera:new THREE.PerspectiveCamera(),
    cameraPhase:{remoteDefinition:d=>definitions.push(d),bindCamera:c=>bound.push(c)},
    model:()=>()=>new THREE.Group()});
  remote.aimCamera=new THREE.PerspectiveCamera();
  const position=new THREE.Vector3(40,2,70),velocity=new THREE.Vector3(1,0,2);
  const droid=remote.createDroid({GeometryName:'probe',weapons:[{channel:0,ZoomMin:2.5}]},{},position,velocity,.4,1);
  remote.droids.push(droid);
  assert.equal(remote.active,false);
  remote.enter(droid);
  assert.equal(remote.active,true);assert.equal(remote.piloted,droid);
  assert.ok(remote.view);assert.ok(remote.simulationView);
  assert.deepEqual(definitions,[droid]);assert.deepEqual(bound,[remote.aimCamera]);

  remote.present(.5,true,false);
  const t=.5+position.x*.13+position.z*.09;
  assert.equal(remote.hoverTime,.5);
  assert.equal(droid.mesh.position.y,position.y+Math.sin(t*2.6)*.045);
  assert.equal(droid.mesh.rotation.x,Math.sin(t*1.7)*.025);
  assert.equal(droid.mesh.rotation.z,Math.sin(t*2.1)*.035);
  assert.deepEqual(droid.position,position);assert.deepEqual(droid.velocity,velocity);
  assert.equal(droid.mesh.visible,true);
  remote.present(0,true,true);assert.equal(droid.mesh.visible,false);
  remote.present(0,true,false);assert.equal(droid.mesh.visible,true);
  remote.zoomStep=1;assert.equal(remote.zoom,2.5);
  remote.present(0,true,false);assert.equal(droid.mesh.visible,false);

  // Exit hands the view back without killing the replicated resource. The
  // same surviving droid can be entered again, with fresh local input state.
  remote.exit();assert.equal(remote.active,false);assert.equal(remote.piloted,null);
  assert.equal(remote.view,null);assert.equal(remote.simulationView,null);
  assert.equal(remote.zoomStep,0);assert.equal(droid.mesh.visible,true);assert.equal(droid.dead,false);
  remote.present(0,false,false);
  assert.deepEqual(droid.mesh.position,position);assert.equal(droid.mesh.rotation.x,0);
  assert.equal(droid.mesh.rotation.y,.4);assert.equal(droid.mesh.rotation.z,0);
  remote.look(20,10,.5,false);remote.zoomStep=1;remote.enter(droid);
  assert.equal(remote.active,true);assert.equal(remote.piloted,droid);
  assert.equal(remote.lookX,0);assert.equal(remote.lookY,0);assert.equal(remote.zoomStep,0);
  assert.deepEqual(definitions,[droid,droid]);assert.deepEqual(bound,[remote.aimCamera,remote.aimCamera]);
  droid.dead=true;remote.exit();assert.equal(remote.active,false);assert.equal(droid.mesh.visible,false);
  remote.enter(droid);assert.equal(remote.active,false);
  remote.present(0,false,false);assert.equal(droid.mesh.visible,false);
});
