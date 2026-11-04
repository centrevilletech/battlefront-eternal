import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Debris } from '../src/debris.js';
import { attachDebrisSim } from '../src/debris-sim.js';
import { loadSim } from '../src/sim/module.js';

test('expired and cleared debris releases its native host handles', async () => {
  const d = new Debris({ scene: new THREE.Scene(), collision: { cast() { return null; } }, model: () => () => new THREE.Group(),
    effects: { play() { return { stopped: false, move() {}, stop() { this.stopped = true; } }; } } });
  const adapter = attachDebrisSim(await loadSim(), d), bone = new THREE.Object3D();
  for (let n = 0; n < 20; n++) {
    d.blowApart([{ ChunkGeometryName: 'piece', ChunkTrailEffect: 'trail', ChunkSmokeEffect: 'smoke' }], { '': bone },
      new THREE.Vector3(), new THREE.Vector3(), -50, {});
    const piece = d.pieces[0], trail = piece.trail, smoke = piece.smoke;
    if (n % 2) d.clear();
    else { d.rest(piece); d.update(11); assert.equal(piece.gone, true); }
    assert.equal(d.pieces.length, 0); assert.equal(piece.mesh.parent, null);
    assert.equal(trail.stopped, true); assert.equal(smoke.stopped, true);
    assert.equal(adapter.pieces.size, 0); assert.equal(adapter.chunks.size, 0);
    for (const kind of ['mesh', 'body', 'effect']) assert.equal(adapter.ids[`${kind}Map`].size, 0);
  }
  assert.equal(adapter.ids.meshes.length, 1); assert.equal(adapter.ids.bodies.length, 1); assert.equal(adapter.ids.effects.length, 2);
});
