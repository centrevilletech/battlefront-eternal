import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { DeathCamera, DEATH_BODY, deadFov } from '../src/camera.js';
import { DeathShot, drawnKiller, spring, classicBlend, ease } from '../src/death-shot.js';
import { feedbackRecord, decodeFeedback, presentFeedback } from '../src/runtime/network-combat-feedback.js';

// The death camera's traced rules (docs/formats.md, "Death camera"): it rises
// to look down on the body over the first second, holds, and from 1.5 s turns
// to look over the body at the killer, all within 6 m rays' reach.

const STEP = 1 / 60;
const open = { cast: () => null };
// A wall across z = at, facing +z: what a ray going -z meets.
const wallAt = (at) => ({
  cast(origin, direction, far, hit) {
    if (direction.z >= 0) return null;
    const distance = (at - origin.z) / direction.z;
    if (distance < 0 || distance > far) return null;
    hit.distance = distance;
    return hit;
  },
});

// A chase view 3.5 m behind a soldier at the origin who faces +z.
function chase() {
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.1, 1000);
  camera.rotation.order = 'YXZ';
  camera.rotation.set(-THREE.MathUtils.degToRad(5), Math.PI, 0);
  camera.position.set(0, 1.8, -3.5);
  camera.updateMatrixWorld();
  return camera;
}

const forward = (q) => new THREE.Vector3(0, 0, -1).applyQuaternion(q);
const degrees = (a, b) => THREE.MathUtils.radToDeg(a.angleTo(b));

function run(death, seconds, feet, killer) {
  const steps = Math.round(seconds / STEP);
  for (let i = 0; i < steps; i++) death.update(STEP, feet, killer);
}

test('it rises and backs off to look down on the body, keeping its heading', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3();
  death.start(chase(), feet, false, THREE.MathUtils.degToRad(65));
  run(death, 1.2, feet, null);
  // Near point (0, 1.5, -1), far point (0, 0.8, 0): 5 m back along that line.
  const dir = new THREE.Vector3(0, -0.7, 1).normalize();
  const want = new THREE.Vector3(0, 1.5, -1).addScaledVector(dir, -5);
  assert.ok(death.position.distanceTo(want) < 1e-6, `at ${death.position.toArray()}`);
  assert.ok(degrees(forward(death.quaternion), dir) < 1e-4);
  assert.ok(Math.abs(want.y - 4.367) < 0.01 && Math.abs(want.z + 5.096) < 0.01);
  // No roll: its right stays level.
  assert.ok(Math.abs(new THREE.Vector3(1, 0, 0).applyQuaternion(death.quaternion).y) < 1e-9);
  // Without a killer it stays there.
  run(death, 2.5, feet, null);
  assert.ok(death.position.distanceTo(want) < 1e-6);
});

test('the first second eases in, a fraction of time / 1 s each step', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3();
  const from = chase();
  death.start(from, feet, false, 1);
  death.update(STEP, feet, null);
  const want = new THREE.Vector3(0, 1.5, -1).addScaledVector(new THREE.Vector3(0, -0.7, 1).normalize(), -5);
  const moved = death.position.distanceTo(from.position) / want.distanceTo(from.position);
  assert.ok(Math.abs(moved - STEP) < 1e-9, `first step moved ${moved}`);
  // Most of the way by half a second, all of it by one.
  run(death, 0.5 - STEP, feet, null);
  assert.ok(death.position.distanceTo(want) < 0.01 * want.distanceTo(from.position));
  run(death, 0.5, feet, null);
  assert.ok(death.position.distanceTo(want) < 1e-9);
});

test('from 1.5 s it looks over the body at the killer and follows him', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3();
  const killer = new THREE.Vector3(10, 1.7, 30);
  death.start(chase(), feet, true, 1);
  run(death, 1.5, feet, killer);
  const down = new THREE.Vector3(0, -0.7, 1).normalize();
  assert.ok(degrees(forward(death.quaternion), down) < 1e-3, 'still on the body at 1.5 s');
  const toward = (q, p) => degrees(forward(q), killer.clone().sub(p));
  run(death, 0.25, feet, killer);
  const quarter = toward(death.quaternion, death.position);
  run(death, 0.25, feet, killer);
  const half = toward(death.quaternion, death.position);
  assert.ok(quarter > 5 && quarter < 30, `turning at 1.75 s (${quarter.toFixed(1)} deg off)`);
  assert.ok(half < 3, `framed by 2 s (${half.toFixed(2)} deg off)`);
  run(death, 1.5, feet, killer);
  // 5 m behind the body's middle on the line from the killer.
  const dir = killer.clone().sub(new THREE.Vector3(0, 0.8, 0)).normalize();
  const want = new THREE.Vector3(0, 0.8, 0).addScaledVector(dir, -5);
  assert.ok(death.position.distanceTo(want) < 1e-6);
  assert.ok(degrees(forward(death.quaternion), dir) < 1e-4);
  // He moves; it follows at once now.
  killer.set(-20, 1.7, 25);
  death.update(STEP, feet, killer);
  assert.ok(degrees(forward(death.quaternion), killer.clone().sub(new THREE.Vector3(0, 0.8, 0))) < 1e-4);
});

test('a killer with no body leaves it looking down on the body', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3();
  death.start(chase(), feet, true, 1);
  run(death, 4, feet, null);
  assert.ok(degrees(forward(death.quaternion), new THREE.Vector3(0, -0.7, 1)) < 1e-3);
});

test('two rays stop it 1 m short of a wall behind it', () => {
  const death = new DeathCamera(wallAt(-3));
  const feet = new THREE.Vector3();
  death.start(chase(), feet, false, 1);
  run(death, 1.2, feet, null);
  // Back from (0, 1.5 +- 0.75, -1) along -dir; the wall is 2 m back in z.
  const dir = new THREE.Vector3(0, -0.7, 1).normalize();
  const hit = 2 / dir.z;
  const want = new THREE.Vector3(0, 1.5, -1).addScaledVector(dir, 1 - hit);
  assert.ok(death.position.distanceTo(want) < 1e-6, `at ${death.position.toArray()}`);
});

test('the body point follows the body while it is in the world', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3(0, 10, 0);
  death.start(chase(), feet, false, 1);
  // Falling: the first near point stays, the camera tips down after him.
  feet.y = 0;
  run(death, 1.2, feet, null);
  const fell = forward(death.quaternion);
  assert.ok(fell.y < new THREE.Vector3(0, -0.7, 1).normalize().y, 'looks further down');
  // Gone from the world: the last point holds.
  const at = death.position.clone();
  run(death, 0.5, null, null);
  assert.ok(death.position.distanceTo(at) < 1e-9);
});

test('drawn between steps, with its field of view for the camera', () => {
  const death = new DeathCamera(open);
  const feet = new THREE.Vector3();
  death.start(chase(), feet, false, THREE.MathUtils.degToRad(70));
  death.update(0.5, feet, null);
  const camera = new THREE.PerspectiveCamera(10, 2, 0.1, 100);
  death.apply(camera, 0.5);
  assert.ok(camera.position.distanceTo(death.previousPosition.clone().lerp(death.position, 0.5)) < 1e-9);
  assert.ok(Math.abs(Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) * 2 - Math.tan(THREE.MathUtils.degToRad(35))) < 1e-9);
});

test("its field of view is the chase camera's after one step toward a dead soldier's 48 degrees", () => {
  const deg = (r) => THREE.MathUtils.radToDeg(r);
  const fov = (dt) => deg(deadFov(THREE.MathUtils.degToRad(65), dt));
  // At 60 Hz, a narrowing of about 6 degrees; at the 0.1 s frames of the
  // live capture, most of the way to 48.
  assert.ok(Math.abs(fov(1 / 60) - 59.2) < 0.1, `${fov(1 / 60)}`);
  assert.ok(Math.abs(fov(0.1) - 49.4) < 0.1, `${fov(0.1)}`);
  assert.ok(Math.abs(fov(0) - 65) < 1e-9);
});

// ------------------------------------------------------------ the drawn one

// The death camera as the page draws it (src/death-shot.js), from what is
// drawn, on the frame's clock: one path whatever the frames, no jumps, the
// original's framing and timing as Classic, the replay as Enhanced.

const killerRuns = (t) => new THREE.Vector3(10 + 5 * Math.max(0, t - 2), 1.7, -30);
const HFOV = THREE.MathUtils.degToRad(59);
const PATTERNS = { '30 Hz': [1 / 30], '60 Hz': [1 / 60], '144 Hz': [1 / 144], '4 ms / 21 ms': [1 / 240, 1 / 48], jittered: [0.004, 0.011, 0.0167, 0.025, 0.007] };

// A body at the origin, the view taken over 3.5 m behind it; the killer
// (where, as a function of time, or null) behind and to one side. Every
// frame's camera.
function shoot({ style = 'enhanced', pattern = [1 / 60], seconds = 4, world = open, killer = killerRuns, styleAt = null, trace = null, before = null, across = null } = {}) {
  const shot = new DeathShot({ world, trace });
  const camera = chase();
  if (across) {
    camera.fov = THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(across) / 2) / camera.aspect));
    camera.updateProjectionMatrix();
  }
  if (before) for (const [dt, move] of before) { move(camera); shot.watch(camera, dt); }
  const feet = new THREE.Vector3();
  const seen = { point: new THREE.Vector3(), size: 1.9, living: true, friendly: false };
  const input = { serial: 1, hfov: HFOV, style, feet, chest: null, killer: null };
  const frames = [{ t: 0, p: camera.position.clone(), q: camera.quaternion.clone(), fov: camera.fov }];
  let t = 0;
  for (let i = 0; t < seconds - 1e-9; i++) {
    const dt = pattern[i % pattern.length];
    t += dt;
    const where = killer?.(t) ?? null;
    if (where) seen.point.copy(where);
    input.killer = where ? seen : null;
    input.style = styleAt ? styleAt(t) : style;
    shot.update(dt, camera, input);
    frames.push({ t, p: camera.position.clone(), q: camera.quaternion.clone(), fov: camera.fov });
  }
  return { frames, shot, camera };
}

// The camera at time t, between the frames either side.
function at(frames, t) {
  const i = frames.findIndex((f) => f.t >= t - 1e-9);
  const b = frames[i], a = frames[Math.max(0, i - 1)];
  const u = b.t > a.t ? (t - a.t) / (b.t - a.t) : 1;
  return { p: a.p.clone().lerp(b.p, u), q: a.q.clone().slerp(b.q, u), fov: THREE.MathUtils.lerp(a.fov, b.fov, u) };
}

// Frame to frame: the largest change of velocity and of turning rate (per
// second, so any frame rate compares), velocity reversals, and single-frame
// spikes (a frame far faster than both its neighbours).
function motion(frames) {
  const v = [], w = [];
  for (let i = 1; i < frames.length; i++) {
    const dt = frames[i].t - frames[i - 1].t;
    v.push({ dt, v: frames[i].p.clone().sub(frames[i - 1].p).divideScalar(dt) });
    const turn = frames[i].q.clone().multiply(frames[i - 1].q.clone().invert());
    if (turn.w < 0) turn.set(-turn.x, -turn.y, -turn.z, -turn.w);
    const s = Math.hypot(turn.x, turn.y, turn.z);
    const angle = 2 * Math.atan2(s, turn.w);
    w.push(s > 1e-12 ? new THREE.Vector3(turn.x, turn.y, turn.z).multiplyScalar(angle / s / dt) : new THREE.Vector3());
  }
  let accel = 0, spin = 0, reversals = 0, spikes = 0;
  for (let i = 1; i < v.length; i++) {
    const dt = (v[i].dt + v[i - 1].dt) / 2;
    accel = Math.max(accel, v[i].v.distanceTo(v[i - 1].v) / dt);
    spin = Math.max(spin, THREE.MathUtils.radToDeg(w[i].distanceTo(w[i - 1]) / dt));
    if (v[i].v.length() > 0.25 && v[i - 1].v.length() > 0.25 && v[i].v.dot(v[i - 1].v) < 0) reversals++;
    if (w[i].length() > 0.05 && w[i - 1].length() > 0.05 && w[i].dot(w[i - 1]) < 0) reversals++;
  }
  for (let i = 1; i + 1 < v.length; i++) {
    const speed = v[i].v.length();
    if (speed > 0.5 && speed > 3 * Math.max(v[i - 1].v.length(), v[i + 1].v.length(), 0.05)) spikes++;
  }
  return { accel, spin, reversals, spikes };
}

const offCentre = (pose, point) => degrees(forward(pose.q), point.clone().sub(pose.p));

test('a critically damped step is exact for any cut of the time', () => {
  // One 0.3 s step toward a goal moving evenly equals 300 steps of 1 ms.
  const one = spring(0.2, -1, 1, 4, 7, 0.3, [0, 0]);
  let x = 0.2, v = -1;
  for (let i = 0; i < 300; i++) [x, v] = spring(x, v, 1 + (3 * i) / 300, 1 + (3 * (i + 1)) / 300, 7, 0.001, [0, 0]);
  assert.ok(Math.abs(one[0] - x) < 1e-9 && Math.abs(one[1] - v) < 1e-9, `${one} vs ${x}, ${v}`);
  // A goal that jumps moves nothing at once: the step starts from rest.
  const jump = spring(0, 0, 0, 10, 7, 1e-6, [0, 0]);
  assert.ok(Math.abs(jump[0]) < 1e-6 && Math.abs(jump[1]) < 1e-3, `${jump}`);
  assert.equal(ease(0), 0);
  assert.equal(ease(1), 1);
  assert.equal(ease(0.5), 0.5);
});

test("Classic's blends are the original's per-frame fractions at 60 a second", () => {
  // The rise: t / 1 s a frame, nearly done by 0.4 s.
  assert.ok(classicBlend(0.4, 1) > 0.97, `${classicBlend(0.4, 1)}`);
  // The turn: (t - 1.5 s) / 2 s a frame, about 65% done a quarter second
  // in, 98% at half a second (docs/formats.md).
  assert.ok(Math.abs(classicBlend(0.25, 0.5) - 0.62) < 0.03, `${classicBlend(0.25, 0.5)}`);
  assert.ok(classicBlend(0.5, 0.5) > 0.98, `${classicBlend(0.5, 0.5)}`);
  // Starting from rest.
  assert.ok(classicBlend(1 / 600, 0.5) < 1e-3);
  assert.equal(classicBlend(0, 1), 0);
  assert.equal(classicBlend(1.2, 1), 1);
});

for (const style of ['enhanced', 'classic']) {
  test(`${style}: the same path at any frame rate, steady or uneven`, () => {
    const runs = Object.fromEntries(Object.entries(PATTERNS).map(([name, pattern]) => [name, shoot({ style, pattern }).frames]));
    const reference = runs['144 Hz'];
    const [far, turn] = style === 'enhanced' ? [0.015, 0.15] : [0.06, 0.3];
    for (const [name, frames] of Object.entries(runs)) {
      for (let t = 0.25; t <= 3.9; t += 0.25) {
        const a = at(frames, t), b = at(reference, t);
        assert.ok(a.p.distanceTo(b.p) < far, `${name} at ${t} s: ${a.p.distanceTo(b.p).toFixed(4)} m from 144 Hz`);
        assert.ok(THREE.MathUtils.radToDeg(a.q.angleTo(b.q)) < turn, `${name} at ${t} s: ${THREE.MathUtils.radToDeg(a.q.angleTo(b.q)).toFixed(3)} deg`);
      }
    }
  });

  test(`${style}: no jumps, reversals or spikes, its jerk bounded the same at every frame rate`, () => {
    // Enhanced moves as a crane; Classic keeps the original's half-second
    // swing to the killer, so it accelerates harder, but evenly.
    const [accel, spin] = style === 'enhanced' ? [45, 650] : [320, 4500];
    const peaks = [];
    for (const [name, pattern] of Object.entries(PATTERNS)) {
      const m = motion(shoot({ style, pattern }).frames);
      assert.equal(m.reversals, 0, `${name}: reversals`);
      assert.equal(m.spikes, 0, `${name}: spikes`);
      assert.ok(m.accel < accel && m.spin < spin, `${name}: ${m.accel.toFixed(1)} m/s2, ${m.spin.toFixed(0)} deg/s2`);
      peaks.push(m.accel);
    }
    // Frame rate changes neither: the peaks agree.
    assert.ok(Math.max(...peaks) / Math.min(...peaks) < 1.25, `${peaks.map((p) => p.toFixed(1))}`);
  });
}

test('it starts from the view it takes over, moving on as it moved, its field of view easing', () => {
  // A 65-degree chase view moving at 5 m/s and turning when he died.
  const before = Array.from({ length: 10 }, () => [1 / 60, (c) => { c.position.x += 5 / 60; c.rotation.y += 0.01; c.updateMatrixWorld(); }]);
  for (const style of ['enhanced', 'classic']) {
    const { frames, camera } = shoot({ style, before, seconds: 0.5, across: 65 });
    const start = frames[0];
    // First frame: on from there at the speed it had, no jump.
    assert.ok(frames[1].p.distanceTo(start.p) < 6 / 60, `${style}: ${frames[1].p.distanceTo(start.p)}`);
    assert.ok(frames[1].p.x > start.p.x, `${style}: keeps its way`);
    const m = motion(frames);
    assert.equal(m.spikes, 0, style);
    assert.ok(m.accel < (style === 'enhanced' ? 45 : 320), `${style}: ${m.accel}`);
    // The field of view: no pop at the hand-off, easing to the death
    // camera's 59 across.
    for (let i = 1; i < frames.length; i++) assert.ok(Math.abs(frames[i].fov - frames[i - 1].fov) < 0.3, `${style}: fov step at ${frames[i].t}`);
    assert.ok(camera.fov < frames[0].fov, `${style}: toward the death camera field of view`);
  }
});

test('classic: down on the body by 0.6 s, still from 1 to 1.5 s, the killer framed by 2.1 s and followed', () => {
  const { frames } = shoot({ style: 'classic' });
  const body = new THREE.Vector3(0, DEATH_BODY, 0);
  const down = at(frames, 0.6);
  assert.ok(offCentre(down, body) < 2, `on the body at 0.6 s (${offCentre(down, body).toFixed(2)})`);
  assert.ok(Math.abs(THREE.MathUtils.radToDeg(Math.asin(forward(down.q).y)) + 35) < 3, 'about 35 degrees down');
  assert.ok(at(frames, 1).p.distanceTo(at(frames, 1.5).p) < 0.01, 'held from 1 s to 1.5 s');
  for (const t of [2.1, 2.6, 3.4]) assert.ok(offCentre(at(frames, t), killerRuns(t)) < 4, `on the killer at ${t} s (${offCentre(at(frames, t), killerRuns(t)).toFixed(2)})`);
  // Without a killer it holds the look down.
  const alone = shoot({ style: 'classic', killer: null }).frames;
  assert.ok(offCentre(at(alone, 3.9), body) < 1);
});

test('enhanced: a hold on the body, a two-shot over it, then the killer framed close', () => {
  const { frames } = shoot({ style: 'enhanced' });
  const body = new THREE.Vector3(0, DEATH_BODY, 0);
  assert.ok(offCentre(at(frames, 1), body) < 12, `the body at 1 s (${offCentre(at(frames, 1), body).toFixed(1)})`);
  // Over the body at the killer: both in frame (half the height is 16.8
  // degrees at 59 across and 16:9).
  const two = at(frames, 2.5);
  assert.ok(offCentre(two, body) < 17 && offCentre(two, killerRuns(2.5)) < 15, `both at 2.5 s (${offCentre(two, body).toFixed(1)}, ${offCentre(two, killerRuns(2.5)).toFixed(1)})`);
  // The camera went round to the far side of the body from him.
  assert.ok(two.p.z > 1, `across the body (${two.p.z.toFixed(2)})`);
  // Framed close, followed as he runs.
  for (const t of [3.3, 3.9]) assert.ok(offCentre(at(frames, t), killerRuns(t)) < 3, `on the killer at ${t} s (${offCentre(at(frames, t), killerRuns(t)).toFixed(2)})`);
  assert.ok(at(frames, 3.9).fov < 24, `the lens narrowed (${at(frames, 3.9).fov.toFixed(1)})`);
  // No killer: on the body throughout.
  const alone = shoot({ style: 'enhanced', killer: null }).frames;
  for (const t of [1, 2, 3, 3.9]) assert.ok(offCentre(at(alone, t), body) < 12, `alone at ${t} s`);
});

test('a killer lost mid-shot, or jumping into a hull, moves it smoothly', () => {
  // Enhanced holds where he was; Classic goes back down to the body, as the
  // original does when he has no body, but without its snap.
  const lose = (t) => (t < 2.6 ? killerRuns(t) : null);
  for (const style of ['enhanced', 'classic']) {
    const m = motion(shoot({ style, killer: lose }).frames);
    assert.equal(m.spikes, 0, style);
    assert.equal(m.reversals, 0, style);
    assert.ok(m.accel < (style === 'enhanced' ? 45 : 320), `${style}: ${m.accel.toFixed(1)}`);
  }
  const held = shoot({ style: 'enhanced', killer: lose }).frames;
  assert.ok(offCentre(at(held, 3.9), killerRuns(2.6)) < 4, 'Enhanced stays on where he was');
  const back = shoot({ style: 'classic', killer: lose }).frames;
  assert.ok(offCentre(at(back, 3.9), new THREE.Vector3(0, DEATH_BODY, 0)) < 2, 'Classic looks down on the body again');
  // He gets into a hull 6 m away at 3 s: the eyes jump; the camera doesn't.
  const hull = (t) => killerRuns(t).add(new THREE.Vector3(t < 3 ? 0 : 6, t < 3 ? 0 : 1.5, 0));
  for (const style of ['enhanced', 'classic']) {
    const m = motion(shoot({ style, killer: hull }).frames);
    assert.equal(m.spikes, 0, style);
    assert.ok(m.accel < (style === 'enhanced' ? 45 : 320), `${style}: ${m.accel.toFixed(1)}`);
  }
});

test('a wall coming between pulls the camera in quickly, and it lets out slowly', () => {
  // A wall at z = -2 behind the body from 0.5 s to 2 s.
  let clock = 0;
  const world = {
    cast(origin, direction, far, hit) {
      if (clock < 0.5 || clock > 2 || direction.z >= 0) return null;
      const distance = (-2 - origin.z) / direction.z;
      if (distance < 0 || distance > far) return null;
      hit.distance = distance;
      return hit;
    },
  };
  for (const style of ['enhanced', 'classic']) {
    const shot = new DeathShot({ world });
    const camera = chase();
    const input = { serial: 1, hfov: HFOV, style, feet: new THREE.Vector3(), chest: null, killer: null };
    const frames = [];
    for (let i = 1; i <= 240; i++) {
      clock = i / 60;
      shot.update(1 / 60, camera, input);
      frames.push({ t: clock, p: camera.position.clone(), q: camera.quaternion.clone(), fov: camera.fov });
    }
    const z = (t) => at(frames, t).p.z;
    assert.ok(z(0.45) < -2.5, `${style}: out past the wall's place before it (${z(0.45).toFixed(2)})`);
    assert.ok(z(0.8) > -2.05, `${style}: in front of the wall 0.3 s after it came (${z(0.8).toFixed(2)})`);
    assert.ok(z(2.3) > z(3.9) + 0.3, `${style}: easing back out after it went`);
    const m = motion(frames);
    assert.equal(m.spikes, 0, style);
    assert.equal(m.reversals, 0, style);
  }
});

test('changing the style mid-death blends; the trace shows only in the Enhanced reveal', () => {
  const m = motion(shoot({ styleAt: (t) => (t < 2.5 ? 'enhanced' : 'classic') }).frames);
  assert.equal(m.spikes, 0);
  assert.ok(m.accel < 320, `${m.accel}`);
  const calls = [];
  const trace = { show: (from, to, friendly, head, opacity, time) => calls.push({ from: from.clone(), to: to.clone(), friendly, head, opacity, time }), hide: () => calls.push(null) };
  const { frames } = shoot({ trace, killer: killerRuns });
  const shown = calls.filter(Boolean);
  assert.ok(shown.length > 0, 'shown');
  // From where he stood when named to the body; its head runs to the body,
  // then it fades.
  assert.ok(shown[0].from.distanceTo(killerRuns(0)) < 1e-6);
  assert.ok(shown.every((c) => c.to.distanceTo(new THREE.Vector3(0, DEATH_BODY, 0)) < 0.01));
  assert.ok(shown[0].head < 0.2 && shown.some((c) => c.head === 1), 'the head runs');
  assert.ok(calls.at(-1) === null && frames.at(-1).t > 3.3, 'hidden once it has faded');
  const classic = [];
  shoot({ style: 'classic', trace: { show: () => classic.push(1), hide: () => {} } });
  assert.equal(classic.length, 0, 'not in Classic');
});

// ------------------------------------------------------------ the killer

test("the world names the killer in plain numbers: a unit and its life, his Character's slot, or a hull and its life", () => {
  const death = new DeathCamera(open);
  const character = {}, other = {};
  death.start(chase(), new THREE.Vector3(), true, 1);
  assert.equal(death.deaths, 1);
  assert.deepEqual([death.killerUnit, death.killerCharacter, death.killerHull], [-1, -1, -1]);
  death.blame({ unit: { id: 9 }, generation: 2, character }, [other, character], []);
  assert.deepEqual([death.killerUnit, death.killerLife, death.killerCharacter, death.killerHull], [9, 2, 1, -1]);
  const hull = {};
  death.blame({ vehicle: hull, life: 5 }, [], [{}, hull]);
  assert.deepEqual([death.killerUnit, death.killerHull, death.killerHullLife], [-1, 1, 5]);
  death.blame(null);
  assert.deepEqual([death.killerUnit, death.killerCharacter, death.killerHull], [-1, -1, -1]);
  // A new death starts with none named.
  death.blame({ unit: { id: 9 }, generation: 2, character: null });
  death.start(chase(), new THREE.Vector3(), false, 1);
  assert.equal(death.deaths, 2);
  assert.equal(death.killerUnit, -1);
});

test('the page finds the killer where it draws him: his body, his seat, his corpse, his next body', () => {
  const point = (x, y, z) => new THREE.Vector3(x, y, z);
  const unit = { id: 7, generation: 3, alive: true, removed: false, team: 2, soldier: { posture: 'stand' }, actor: { root: { position: point(5, 0, 5) } } };
  const next = { id: 11, generation: 1, alive: true, removed: false, team: 2, soldier: { posture: 'stand' }, actor: { root: { position: point(40, 0, 0) } } };
  const character = { unit: null };
  next.character = character;
  const units = new Map([[7, unit], [11, next]]);
  const hull = { alive: true, team: 1, radius: 4, aiTarget: { generation: 2 }, seatEye: (i, out, presented) => out.set(i, presented ? 9 : -9, 0) };
  const refs = { units: { byId: (id) => units.get(id) ?? null }, characters: [character], vehicles: [hull], relation: (team) => (team === 1 ? 1 : -1) };
  const out = { point: new THREE.Vector3(), size: 0, living: false, friendly: false };
  const death = new DeathCamera(open);
  death.blame({ unit, generation: 3, character }, refs.characters, refs.vehicles);
  // Standing, or prone: his eyes over his drawn feet.
  let k = drawnKiller(death, refs, false, out);
  assert.ok(k && k.point.distanceTo(point(5, 1.7, 5)) < 1e-9 && k.living && !k.friendly);
  unit.soldier.posture = 'prone';
  assert.ok(drawnKiller(death, refs, false, out).point.distanceTo(point(5, 0.6, 5)) < 1e-9);
  // In a hull's seat: the seat's eye as the hull is drawn.
  unit.seat = { vehicle: hull, index: 1 };
  k = drawnKiller(death, refs, false, out);
  assert.ok(k.point.distanceTo(point(1, 9, 0)) < 1e-9 && k.size === 8, 'the presented seat');
  unit.seat = null;
  // Dead: Enhanced shows the corpse; Classic, as the original, looks for
  // his Character's next body.
  unit.alive = false;
  k = drawnKiller(death, refs, false, out);
  assert.ok(k && !k.living && k.point.distanceTo(point(5, 0.5, 5)) < 1e-9, 'the corpse');
  assert.equal(drawnKiller(death, refs, true, out)?.living, false, 'no next body yet');
  character.unit = next;
  k = drawnKiller(death, refs, true, out);
  assert.ok(k && k.living && k.point.distanceTo(point(40, 1.7, 0)) < 1e-9, 'his next body');
  // His unit taken by another life, or gone from the world: not him.
  unit.generation = 4;
  assert.equal(drawnKiller(death, refs, false, out), null);
  unit.generation = 3;
  unit.removed = true;
  assert.equal(drawnKiller(death, refs, false, out), null);
  // A hull credited by itself, while it is the same hull's life.
  death.blame({ vehicle: hull, life: 2 }, refs.characters, refs.vehicles);
  k = drawnKiller(death, refs, false, out);
  assert.ok(k && k.point.distanceTo(point(0, 9, 0)) < 1e-9 && k.friendly);
  hull.aiTarget.generation = 3;
  assert.equal(drawnKiller(death, refs, false, out), null, 'a wreck come back is another hull');
});

test("online, the server's damage and death records name who hurt the page's player", () => {
  const own = { id: 8, generation: 3, team: 1, soldier: { aimYaw: 0 } }, bot = { id: 7, generation: 2, team: 2 };
  const notes = [];
  const game = {
    playerUnit: own, units: { byId: (id) => ({ 7: bot, 8: own })[id] ?? null, relation: () => -1 },
    hud: { hit() {}, damageFrom() {}, killMessage() {} },
    noteAttacker: (...args) => notes.push(args),
  };
  const record = (kind, attacker, tick, target = own) => decodeFeedback(feedbackRecord({ id: 1, generation: 1, tick, event: {
    kind, target: { unit: target, generation: target.generation }, attacker: attacker ? { unit: attacker, generation: attacker.generation } : null,
    team: 1, attackerTeam: 2, position: [0, 0, 0], source: [1, 0, 0],
  } }));
  presentFeedback(game, record('damage', bot, 10));
  presentFeedback(game, record('death', bot, 12));
  presentFeedback(game, record('death', null, 14));
  presentFeedback(game, record('death', { id: 99, generation: 1 }, 15));
  // Someone else's death names no one to this page.
  presentFeedback(game, record('death', bot, 16, bot));
  assert.deepEqual(notes, [[bot, false, 10], [bot, true, 12], [null, true, 14], [undefined, true, 15]]);
});
