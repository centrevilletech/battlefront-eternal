import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { Clip, Pose, setAnimKernel } from '../src/anim.js';
import { loadAnimKernel } from '../src/anim-kernel.js';

// The anim kernel (src/anim-kernel.js, rust/sim/src/pose) must blend clips and
// apply poses exactly as anim.js's JavaScript does: every accumulator and
// every bone's quaternion and position to the last bit. Poses feed both
// the page's skeletons and the simulation's rider shapes, so a single
// differing bit is a failure here.

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

const u32 = (f32) => new Uint32Array(f32.buffer, f32.byteOffset, f32.length);
function sameBits(a, b, what) {
  assert.equal(a.length, b.length, `${what}: length`);
  const [ua, ub] = [u32(a), u32(b)];
  for (let i = 0; i < ua.length; i++) assert.ok(ua[i] === ub[i], `${what}[${i}]: ${a[i]} != ${b[i]}`);
}
function sameBone(a, b, what) {
  for (const [name, va, vb] of [['q', a.quaternion, b.quaternion], ['p', a.position, b.position], ['r', a.rotation, b.rotation]]) {
    for (const axis of ['x', 'y', 'z', 'w']) {
      if (va[axis] === undefined) continue;
      assert.ok(Object.is(va[axis], vb[axis]), `${what}.${name}.${axis}: ${va[axis]} != ${vb[axis]}`);
    }
  }
}

const kernel = await loadAnimKernel();

function pair(count = 6, root = 0) {
  setAnimKernel(null);
  const js = new Pose(count, root);
  setAnimKernel(kernel);
  const wasm = new Pose(count, root);
  setAnimKernel(null);
  return [js, wasm];
}

test('kernel poses blend every sample as the JavaScript does, bit for bit', () => {
  const clips = [
    makeClip('a'),
    makeClip('b', { frames: 2, seed: 11, rootMotion: true }),
    makeClip('c', { frames: 1, seed: 23 }),
    makeClip('d', { frames: 41, seed: 31, fps: 24, tracks: [5, 4, 0] }),
  ];
  const times = [0, 1 / 60, 0.317, -0.25, -13.7, 123.456, 1e6, 0.999999];
  const bones = () => Array.from({ length: 6 }, () => new THREE.Bone());
  for (const loop of [true, false]) for (const time of times) for (let c = 0; c < clips.length; c++) {
    const [js, wasm] = pair();
    for (const pose of [js, wasm]) {
      pose.clear();
      pose.add(clips[c], time, 0.35, loop);
      pose.add(clips[(c + 1) % clips.length], time * 0.5 + 0.01, 0.65, loop);
      pose.add(clips[c], -time, 0.00001, loop); // under the weight cutoff
      pose.add(null, time, 1, loop);
    }
    const what = `clip ${c} time ${time} loop ${loop}`;
    // Copy before comparing: a kernel pose's arrays view its memory.
    sameBits(Float32Array.from(js.q), Float32Array.from(wasm.q), `${what} q`);
    sameBits(Float32Array.from(js.t), Float32Array.from(wasm.t), `${what} t`);
    sameBits(Float32Array.from(js.w), Float32Array.from(wasm.w), `${what} w`);
    const [a, b] = [bones(), bones()];
    js.apply(a); wasm.apply(b);
    a.forEach((bone, i) => sameBone(bone, b[i], `${what} bone ${i}`));
  }
});

test('a pose takes the kernel up and drops it as it clears', () => {
  const clip = makeClip('a');
  setAnimKernel(null);
  const pose = new Pose(6, 0);
  pose.clear(); pose.add(clip, 0.2, 1, true);
  const plain = Float32Array.from(pose.q);
  setAnimKernel(kernel);
  pose.clear(); pose.add(clip, 0.2, 1, true);
  sameBits(plain, Float32Array.from(pose.q), 'taken up q');
  setAnimKernel(null);
  pose.clear(); pose.add(clip, 0.2, 1, true);
  sameBits(plain, Float32Array.from(pose.q), 'dropped q');
});
