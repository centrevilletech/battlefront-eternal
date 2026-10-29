import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { buildActor } from '../src/actor.js';
import { JetpackNozzles } from '../src/jetpack-nozzles.js';

const path = 'public/sides/rep/actors/rep_inf_jettrooper';
const available = fs.existsSync(`${path}.bin`);
function fixture(pathOverride = path) {
  const path = pathOverride;
  const definition = JSON.parse(fs.readFileSync(`${path}.json`));
  const buffer = file => { const b = fs.readFileSync(file); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
  const actor = buildActor(definition, buffer(`${path}.bin`), buffer(`${path}_index.bin`), () => null, { caster() {} });
  const nozzles = new JetpackNozzles(actor, definition);
  return { actor, nozzles };
}

test('retail pack outlet anchors come from the actual eight-vertex mouth rims', { skip: !available }, () => {
  const { actor, nozzles } = fixture();
  assert.equal(nozzles.frames.length, 2);
  const frames = nozzles.update();
  for (const frame of frames) {
    assert.equal(frame.rimIndices.length, 8);
    assert.ok(Math.abs(frame.radius - .0338494) < .00001);
    assert.ok(Math.abs(frame.rimCenter.y - 1.27409) < .00001);
    assert.ok(frame.position.distanceTo(frame.rimCenter) <= .002001);
    assert.ok(frame.direction.y < -.999);
  }
  const authored = new THREE.Vector3(-.24, -.21, .15).applyMatrix4(actor.byName.bone_ribcage.matrixWorld);
  assert.ok(authored.distanceTo(frames[1].rimCenter) > .028, 'retail effect origin was visibly below the mesh mouth');
});

test('anchors remain exactly at skinned outlets while the torso leans, rotates and scales', { skip: !available }, () => {
  const { actor, nozzles } = fixture();
  const original = actor.byName.bone_ribcage.quaternion.clone();
  const scratch = new THREE.Vector3();
  let maximumError = 0;
  for (let i = 0; i < 40; i++) {
    actor.root.position.set(Math.sin(i) * 13, -40 + i * .1, Math.cos(i) * 8);
    actor.root.rotation.y = i * .17;
    actor.root.scale.setScalar(1 + i * .005);
    actor.byName.bone_ribcage.quaternion.copy(original).multiply(new THREE.Quaternion().setFromEuler(new THREE.Euler(Math.sin(i) * .6, i * .08, Math.cos(i) * .4)));
    // Renderer updates the skinned mesh bind inverse before drawing vertices.
    actor.root.updateMatrixWorld(true);
    for (const frame of nozzles.update()) {
      const renderedCenter = new THREE.Vector3();
      for (const index of frame.rimIndices) {
        actor.body.getVertexPosition(index, scratch).applyMatrix4(actor.body.matrixWorld);
        renderedCenter.add(scratch);
      }
      renderedCenter.multiplyScalar(1 / frame.rimIndices.length);
      maximumError = Math.max(maximumError, renderedCenter.distanceTo(frame.rimCenter));
      assert.ok(frame.position.clone().sub(renderedCenter).dot(frame.direction) < 0, 'origin is just inside, never floating below the rim');
      for (const index of frame.rimIndices) {
        actor.body.getVertexPosition(index, scratch).applyMatrix4(actor.body.matrixWorld);
        assert.ok(Math.abs(scratch.distanceTo(renderedCenter) - frame.radius) < .00001);
        assert.ok(Math.abs(scratch.sub(renderedCenter).dot(frame.direction)) < .00001);
      }
    }
  }
  assert.ok(maximumError < 1e-10, `skinned outlet error ${maximumError}`);
});


test('Dark Trooper tilted pack mouths follow the skinned rims', { skip: !fs.existsSync('public/sides/imp/actors/imp_inf_darktrooper.bin') }, () => {
  const { actor, nozzles } = fixture('public/sides/imp/actors/imp_inf_darktrooper');
  assert.equal(nozzles.frames.length, 2);
  actor.root.rotation.set(.2, 1.3, -.1);
  actor.byName.bone_ribcage.rotation.z += .4;
  actor.root.updateMatrixWorld(true);
  for (const frame of nozzles.update()) {
    const center = new THREE.Vector3(), vertex = new THREE.Vector3();
    for (const index of frame.rimIndices) center.add(actor.body.getVertexPosition(index, vertex).applyMatrix4(actor.body.matrixWorld));
    center.divideScalar(frame.rimIndices.length);
    assert.ok(center.distanceTo(frame.rimCenter) < 1e-7);
    assert.ok(frame.radius > .035 && frame.radius < .037);
    assert.ok(frame.position.distanceTo(center) < .002001);
  }
});
