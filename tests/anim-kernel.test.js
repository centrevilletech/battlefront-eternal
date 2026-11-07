import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { readFileSync } from 'node:fs';
import * as THREE from 'three';
import { Clip, Pose, setAnimKernel, releaseAnimKernel } from '../src/anim.js';
import { loadAnimKernel } from '../src/sim/wasm/anim-kernel.js';

// Synthetic signed quaternions and root travel retain the original f32
// accumulator stores, wrapping, weight cutoff and normalized bone writes.
function random(seed) {
  return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
}

// A clip of seeded frames: quaternions wandering off the unit sphere (the
// sampler normalizes), some negated (the sign flips), translations of
// either sign, and tracks that include the root and bones a body lacks.
function makeClip(name, { frames = 17, tracks = [0, 1, 2, -1, 5, 3], fps = 30, rootMotion = false, seed = 7 } = {}) {
  const rand = random(seed);
  const data = new Float32Array(tracks.length * frames * 7);
  for (let i = 0; i < data.length; i += 7) {
    const s = rand() < 0.25 ? -1 : 1;
    data[i] = s * (rand() * 2 - 1); data[i + 1] = s * (rand() * 2 - 1);
    data[i + 2] = s * (rand() * 2 - 1); data[i + 3] = s * (rand() * 2 - 1);
    data[i + 4] = rand() * 4 - 2; data[i + 5] = rand() * 4 - 2; data[i + 6] = rand() * 4 - 2;
  }
  return new Clip(name, { frames, tracks, offset: 0, flags: rootMotion ? 1 : 0, travel: [1, 0, 2] }, data, fps);
}


const kernel = await loadAnimKernel();
setAnimKernel(kernel);
const golden = JSON.parse(readFileSync(new URL('./fixtures/pose-golden.json', import.meta.url)));
test('native pose blending preserves the synthetic golden results', () => {
  const clips = [makeClip('a'), makeClip('b', { frames:2, seed:11, rootMotion:true }),
    makeClip('c', { frames:1, seed:23 }), makeClip('d', { frames:41, seed:31, fps:24, tracks:[5,4,0] })];
  let index = 0;
  for (const loop of [true, false]) for (const time of [0, 1/60, .317, -.25, -13.7, 123.456, 1e6, .999999]) for (let c = 0; c < clips.length; c++) {
    const pose = new Pose(6, 0);
    pose.clear();
    pose.add(clips[c], time, .35, loop);
    pose.add(clips[(c + 1) % 4], time * .5 + .01, .65, loop);
    pose.add(clips[c], -time, .00001, loop);
    pose.add(null, time, 1, loop);
    const hash = crypto.createHash('sha256');
    for (const values of [pose.q, pose.t, pose.w]) hash.update(Buffer.from(values.buffer, values.byteOffset, values.byteLength));
    const bones = Array.from({length:6}, () => new THREE.Bone());
    pose.apply(bones);
    hash.update(JSON.stringify(bones.map(b => [...b.quaternion.toArray(), ...b.position.toArray(), b.rotation.x, b.rotation.y, b.rotation.z])));
    assert.equal(hash.digest('hex'), golden[index++], `clip ${c}, time ${time}, loop ${loop}`);
  }
  assert.equal(index, golden.length);
});
test('an existing pose retains its battle sampler across later construction', async () => {
  const pose = new Pose(1, 0), first = pose.kernel;
  setAnimKernel(await loadAnimKernel());
  pose.clear();
  assert.equal(pose.kernel, first);
  assert.notEqual(new Pose(1, 0).kernel, first);
  setAnimKernel(kernel);
});

test('teardown releases its sampler without clearing a newer battle', async () => {
  const first = await loadAnimKernel(), second = await loadAnimKernel();
  try {
    setAnimKernel(first);const retained = new Pose(1, 0);
    setAnimKernel(second);
    assert.equal(releaseAnimKernel(first), false);
    assert.equal(new Pose(1, 0).kernel, second);
    assert.equal(releaseAnimKernel(second), true);
    assert.equal(releaseAnimKernel(second), false);
    assert.throws(() => new Pose(1, 0), /not ready/);
    retained.clear();assert.equal(retained.kernel, first);
  } finally { setAnimKernel(kernel); }
});
