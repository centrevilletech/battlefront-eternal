import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { JetpackEffects } from '../src/jetpack-effects.js';

function fixture() {
  let mode = 'enhanced';
  const effect = new JetpackEffects({ scene: new THREE.Scene(), settings: { get: () => mode }, random: () => .5 });
  const state = { active: true, position: new THREE.Vector3(2, 5, 3), quaternion: new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), Math.PI / 2), velocity: new THREE.Vector3(3, 1, 0) };
  const run = (seconds) => { for (let i = 0; i < Math.round(seconds * 60); i++) effect.update(1 / 60, state); };
  return { effect, state, run, mode: value => { mode = value; } };
}

test('enhanced jets follow the original ribcage nozzle offsets and exhaust axis', () => {
  const f = fixture(); f.effect.update(1 / 60, f.state);
  assert.equal(f.effect.flame.geometry.instanceCount, 2);
  for (let i = 0; i < 2; i++) {
    const nozzle = f.effect.nozzles[i];
    assert.ok(nozzle.direction.distanceTo(new THREE.Vector3(0, -1, 0)) < 1e-6);
    assert.ok(nozzle.position.distanceTo(new THREE.Vector3(2.21, 4.76, i ? 2.85 : 3.15)) < 1e-6);
  }
});

test('authored nozzle positions override defaults without assuming world-down', () => {
  const effect = new JetpackEffects({ scene: new THREE.Scene(), definition: [{ name: 'JetExhaust', spawner: { offset: [[1, 1], [2, 2], [3, 3]], box: [[0, 0], [0, 0], [-1, -1]] } }] });
  effect.update(0, { active: true, position: new THREE.Vector3(4, 5, 6), quaternion: new THREE.Quaternion(), velocity: new THREE.Vector3() });
  assert.equal(effect.nozzles.length, 1);
  assert.deepEqual(effect.nozzles[0].position.toArray(), [5, 7, 9]);
  assert.deepEqual(effect.nozzles[0].direction.toArray(), [0, 0, -1]);
});

test('mesh outlet frames override the legacy emitters and set the visible exhaust width', () => {
  const f = fixture();
  const nozzleFrames = [
    { position: new THREE.Vector3(1, 2, 3), direction: new THREE.Vector3(0, -1, 0), radius: .034 },
    { position: new THREE.Vector3(4, 5, 6), direction: new THREE.Vector3(1, 0, 0), radius: .032 },
  ];
  f.effect.update(1 / 60, { ...f.state, nozzleFrames });
  for (let i = 0; i < 2; i++) {
    assert.ok(f.effect.nozzles[i].position.equals(nozzleFrames[i].position));
    assert.ok(f.effect.nozzles[i].direction.equals(nozzleFrames[i].direction));
    assert.ok(Math.abs(f.effect.flame.geometry.attributes.nozzleRadius.array[i] - nozzleFrames[i].radius) < 1e-8);
  }
});

test('sparks and smoke persist briefly after ignition stops; Classic clears enhancements', () => {
  const f = fixture(); f.run(.8);
  assert.ok(f.effect.smoke.count > 0); assert.ok(f.effect.sparks.count > 0);
  const emitted = f.effect.emitted;
  f.state.active = false; f.run(.1);
  assert.ok(f.effect.throttle > 0 && f.effect.throttle < .3, 'hot core winds down smoothly'); assert.equal(f.effect.emitted, emitted);
  assert.ok(f.effect.smoke.count > 0);
  f.run(2); assert.equal(f.effect.smoke.count, 0); assert.equal(f.effect.sparks.count, 0);
  f.state.active = true; f.run(.5); f.mode('classic'); f.run(1 / 60);
  assert.equal(f.effect.enhanced, false); assert.equal(f.effect.flame.visible, false);
  assert.equal(f.effect.smoke.count, 0); assert.equal(f.effect.sparks.count, 0);
});

test('long burns keep fixed buffers bounded and zero-time frames do not age particles', () => {
  const f = fixture(), smoke = f.effect.smoke.attributes.birth.array, sparks = f.effect.sparks.attributes.birth.array;
  f.run(15);
  assert.ok(f.effect.smoke.count <= 128); assert.ok(f.effect.sparks.count <= 64);
  assert.equal(f.effect.smoke.attributes.birth.array, smoke); assert.equal(f.effect.sparks.attributes.birth.array, sparks);
  const before = Array.from(smoke); const emitted = f.effect.emitted;
  f.effect.update(0, f.state); assert.deepEqual(Array.from(smoke), before); assert.equal(f.effect.emitted, emitted);
  for (const value of [...smoke, ...sparks]) assert.ok(Number.isFinite(value));
});

test('teleporting cannot leave a smoke trail connected across the map', () => {
  const f = fixture(); f.run(.5); f.state.position.x += 100; f.state.active = false;
  f.effect.update(1 / 60, f.state); assert.equal(f.effect.smoke.count, 0); assert.equal(f.effect.sparks.count, 0);
});

test('enhanced exhaust requests one bounded local light only while burning', () => {
  const requests = [];
  let mode = 'enhanced';
  const effect = new JetpackEffects({ scene: new THREE.Scene(), settings: { get: () => mode }, lights: { add(position, rgb, radius, strength) { requests.push({ position: position.clone(), rgb, radius, strength }); } } });
  const state = { active: true, position: new THREE.Vector3(0, 3, 0), quaternion: new THREE.Quaternion(), velocity: new THREE.Vector3() };
  effect.update(1 / 60, state);
  assert.equal(requests.length, 1); assert.equal(requests[0].radius, 1.5);
  assert.ok(requests[0].position.distanceTo(state.position) < 1);
  state.active = false; effect.update(1 / 60, state);
  mode = 'classic'; state.active = true; effect.update(1 / 60, state);
  assert.equal(requests.length, 1);
});

test('ignition and cutoff envelopes are stable across render rates', () => {
  const traces = [30, 60, 144].map(rate => {
    const f = fixture(), trace = [];
    for (const [active, duration] of [[true,.12],[true,.30],[false,.10],[false,.30]]) {
      f.state.active = active;
      for (let left=duration;left>1e-8;) {const dt=Math.min(left,1/rate);f.effect.update(dt,f.state);left-=dt;}
      trace.push(f.effect.throttle);
    }
    assert.ok(trace[0] > .85 && trace[0] < 1);
    assert.ok(trace[2] > 0 && trace[2] < .3);
    assert.equal(trace[3],0);
    return trace;
  });
  for(const trace of traces.slice(1))for(let i=0;i<trace.length;i++)assert.ok(Math.abs(trace[i]-traces[0][i])<1e-8);
});

test('turning bends the combustion tail while the nozzle remains attached; teleport clears history', () => {
  const f=fixture();f.run(.3);
  f.state.position.x+=.08;f.effect.update(1/60,f.state);
  assert.ok(f.effect.nozzles[0].bendWorld.length()>.01);
  assert.ok(f.effect.nozzles[0].bendWorld.length()<=.141);
  assert.ok(f.effect.nozzles[0].bendWorld.dot(f.effect.nozzles[0].direction)<1e-8);
  f.state.position.x+=100;f.effect.update(1/60,f.state);
  assert.ok(f.effect.nozzles[0].bendWorld.length()<1e-8);
});
