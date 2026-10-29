import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { EngineRandom } from '../src/atmosphere.js';
import { FADE_RATE, HAZE, KILL_MARGIN, PRE_ROLL, REACH, SKY_SETTING, SkyFlyers, battleFlyers, enhancedSky, flyEnhanced, flyerMaterial, fly, haze, launch,
  launchEnhanced, placement, withinReach } from '../src/sky-flyers.js';

// Geonosis's classes as tools/zero_sky.py writes them.
const ASSAULT = { model: 'rep_fly_assault_dome', count: 2, height: [200, 300], velocity: [[0, 0], [0, 0], [20, 50]], acceleration: [0, 0, 0], distance: 1500, inDirection: 2, lifeTime: 1000 };
const GUNSHIP = { model: 'rep_fly_gunship_dome', count: 20, height: [80, 140], velocity: [[0, 0], [-10, 10], [80, 120]], acceleration: [0, 0, 0], distance: 600, inDirection: 0.5, lifeTime: 1000 };
const CORE = { model: 'cis_fly_techounion_dome', count: 8, height: [0, 0], velocity: [[0, 0], [10, 12], [0, 0]], acceleration: [0, 2, 0], distance: 1000, inDirection: 3, lifeTime: 80 };

// A random source that hands out the values given, in order.
const scripted = (...values) => ({ float: () => { assert.ok(values.length, 'no more draws'); return values.shift(); } });
const near = (a, b, eps = 1e-9) => Math.abs(a - b) <= eps;

test('a ship starts Distance out across the ground, at a height in range, faded out', () => {
  const random = new EngineRandom(5);
  for (let i = 0; i < 200; i++) {
    const ship = launch(GUNSHIP, random);
    assert.ok(near(Math.hypot(ship.position.x, ship.position.z), 600, 1e-6));
    assert.ok(ship.position.y >= 80 && ship.position.y <= 140);
    assert.equal(ship.fade, 0);
    // Level, its heading a unit vector.
    assert.equal(ship.forward.y, 0);
    assert.ok(near(ship.forward.length(), 1, 1e-12));
    // Turned inward from across by InDirectionFactor / 2 to InDirectionFactor:
    // the inward part of its heading over the across part.
    const inward = new THREE.Vector3(-ship.position.x, 0, -ship.position.z).normalize();
    const along = ship.forward.dot(inward), across = Math.sqrt(1 - along * along);
    assert.ok(along > 0, 'heads inward');
    assert.ok(along / across >= 0.25 - 1e-9 && along / across <= 0.5 + 1e-9, `inward ${along / across}`);
    const [vx, vy, vz] = ship.velocity;
    assert.equal(vx, 0);
    assert.ok(vy >= -10 && vy <= 10 && vz >= 80 && vz <= 120);
    assert.ok(ship.age >= 800 && ship.age <= 1000, 'its age starts at 80% to 100% of LifeTime');
  }
});

test('the draws come in the original game order: z, x, height, side, inward, forward, up, right, age', () => {
  // z 0.9 - 0.5 = 0.4, x 0.2 - 0.5 = -0.3: the direction (-0.6, 0, 0.8).
  const ship = launch(ASSAULT, scripted(0.9, 0.2, 0.5, 0.75, 1, 0.5, 0.25, 0.125, 0));
  assert.ok(near(ship.position.x, -900) && near(ship.position.z, 1200) && near(ship.position.y, 250));
  // Across: up x direction = (0.8, 0, 0.6), flipped (0.75 > 0.5); then
  // turned inward by the whole factor (draw 1): -2 x direction.
  const tx = -0.8 + 1.2, tz = -0.6 - 1.6, length = Math.hypot(tx, tz);
  assert.ok(near(ship.forward.x, tx / length) && near(ship.forward.z, tz / length));
  assert.deepEqual(ship.velocity, [0, 0, 35]);
  assert.equal(ship.age, 800);
  // A zero direction is drawn again; no flip at exactly 0.5.
  const again = launch({ ...ASSAULT, inDirection: 0 }, scripted(0.5, 0.5, 0.5, 1, 0, 0.5, 0, 0, 0, 0.5));
  assert.ok(near(again.position.x, 1500) && near(again.position.z, 0) && near(again.position.y, 200), 'the second pair: z 0, x 0.5');
  assert.ok(near(again.forward.x, 0) && near(again.forward.z, -1), 'straight across with no inward factor');
  assert.equal(again.age, 900);
});

test('a ship flies its velocity in its own axes, and never turns', () => {
  const def = { ...GUNSHIP, velocity: [[3, 3], [-10, -10], [100, 100]] };
  const ship = launch(def, new EngineRandom(2));
  const start = ship.position.clone(), forward = ship.forward.clone();
  const right = new THREE.Vector3(forward.z, 0, -forward.x);
  for (let i = 1; i <= 120; i++) assert.equal(fly(def, ship, 1 / 60), true);
  const moved = ship.position.clone().sub(start);
  assert.ok(near(moved.dot(forward), 200, 1e-6) && near(moved.dot(right), 6, 1e-6) && near(moved.y, -20, 1e-6));
  assert.ok(ship.forward.equals(forward));
});

test('a ship fades in over 4 s and stays', () => {
  const def = { ...GUNSHIP, velocity: [[0, 0], [0, 0], [0, 0]] };
  const ship = launch(def, new EngineRandom(2));
  for (let i = 1; i <= 60; i++) fly(def, ship, 1 / 60);
  assert.ok(near(ship.fade, FADE_RATE, 1e-9), 'a quarter in after a second');
  for (let i = 1; i <= 180; i++) fly(def, ship, 1 / 60);
  assert.ok(near(ship.fade, 1, 1e-9));
  for (let i = 0; i < 600; i++) fly(def, ship, 1 / 60);
  assert.equal(ship.fade, 1, 'held at full');
});

test('Acceleration adds to the velocity before it moves: the core ships rise faster and faster', () => {
  const ship = launch(CORE, scripted(0.9, 0.2, 0.5, 0.25, 0.5, 0, 0.5, 0, 0));
  assert.deepEqual(ship.velocity, [0, 11, 0]);
  assert.equal(ship.position.y, 0);
  fly(CORE, ship, 0.5);
  assert.ok(near(ship.velocity[1], 12) && near(ship.position.y, 6));
  fly(CORE, ship, 0.5);
  assert.ok(near(ship.velocity[1], 13) && near(ship.position.y, 12.5));
});

test('past Distance + 10 m across the ground a ship fades out over 4 s, still flying, then starts again', () => {
  const def = { ...GUNSHIP, inDirection: 0, velocity: [[0, 0], [0, 0], [100, 100]] };
  const ship = launch(def, new EngineRandom(9));
  let frames = 0;
  while (ship.fade >= 0) {
    assert.equal(fly(def, ship, 0.05), true);
    assert.ok(++frames < 400);
  }
  // It went out of range on that frame's check, after the last move.
  const reach = def.distance + KILL_MARGIN;
  const away = () => Math.hypot(ship.position.x, ship.position.z);
  assert.ok(away() > reach && away() < reach + 5.1);
  assert.ok(near(ship.fade, -1 + 0.05 * FADE_RATE, 1e-9));
  const fading = [];
  let last = away();
  while (fly(def, ship, 0.05)) {
    fading.push(Math.abs(ship.fade));
    assert.ok(away() > last, 'still flying');
    last = away();
  }
  assert.ok(fading.length >= 78 && fading.length <= 80, `${fading.length} frames fading`);
  assert.ok(fading.every((a, i) => i === 0 || a < fading[i - 1]), 'its alpha falls');
  // Then it starts again, Distance out.
  launch(def, new EngineRandom(10), ship);
  assert.equal(ship.fade, 0);
  assert.ok(near(away(), def.distance, 1e-6));
});

test('a ship lives until its age passes LifeTime: at most a fifth of it', () => {
  const ship = launch(CORE, scripted(0.9, 0.2, 0.5, 0.25, 0.5, 0, 0.5, 0, 0.5));
  assert.equal(ship.age, 72);
  let t = 0;
  while (ship.fade >= 0) { fly(CORE, ship, 0.25); t += 0.25; }
  // Checked before each frame's step: 72 + 8 s is not past 80, 80.25 is.
  assert.ok(near(t, 8.5), `ended after ${t} s`);
});

test("a ship's placement: its model's z along its heading, x to its right, y up", () => {
  const ship = { position: new THREE.Vector3(10, 20, 30), forward: new THREE.Vector3(0.6, 0, 0.8) };
  const m = placement(ship);
  assert.ok(near(m.determinant(), 1, 1e-12), 'a turn, not a mirror');
  const nose = new THREE.Vector3(0, 0, 1).applyMatrix4(m), side = new THREE.Vector3(1, 0, 0).applyMatrix4(m), top = new THREE.Vector3(0, 1, 0).applyMatrix4(m);
  assert.ok(near(nose.x, 10.6) && near(nose.y, 20) && near(nose.z, 30.8));
  // Right is up x forward: (0.8, 0, -0.6).
  assert.ok(near(side.x, 10.8) && near(side.z, 29.4));
  assert.ok(near(top.y, 21));
  // Facing +z it is the model as it is.
  assert.ok(placement({ position: new THREE.Vector3(), forward: new THREE.Vector3(0, 0, 1) }).equals(new THREE.Matrix4()));
});

test('materials follow the MTRL flags and blend by the fade from the opaque queue', () => {
  const map = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
  const lit = flyerMaterial({ flags: 0x1 }, map), unlit = flyerMaterial({ flags: 0 }, map);
  const glass = flyerMaterial({ flags: 0x5 }, map), cut = flyerMaterial({ flags: 0x3 }, map), glow = flyerMaterial({ flags: 0x81 }, map);
  assert.ok('LIT' in lit.defines && !('LIT' in unlit.defines));
  assert.ok('BLENDED' in glass.defines && 'ALPHA_TEST' in cut.defines && 'ADDITIVE' in glow.defines && !('BLENDED' in glow.defines));
  for (const material of [lit, unlit, glass, cut]) {
    assert.equal(material.transparent, false);
    assert.equal(material.blending, THREE.CustomBlending);
    assert.equal(material.blendSrc, THREE.SrcAlphaFactor);
    assert.equal(material.blendDst, THREE.OneMinusSrcAlphaFactor);
    assert.equal(material.depthTest, true);
    assert.equal(material.uniforms.map.value, map);
  }
  // Opaque parts write depth, even fading, as in the original game; blended
  // ones (0x84) never do.
  assert.deepEqual([lit, unlit, cut, glass, glow].map(m => m.depthWrite), [true, true, true, false, false]);
  assert.equal(glow.blendDst, THREE.OneFactor);
  assert.equal(flyerMaterial({ flags: 0x5 | 0x80 }, map).side, THREE.DoubleSide);
  // Enhanced's fading ships write none at all.
  assert.equal(flyerMaterial({ flags: 0x1 }, map, { fading: true }).depthWrite, false);
  // No fog and no near-scene cut: the shader is the far scene's own.
  assert.ok(!/fog|fade\.x/.test(lit.fragmentShader));
});

test('Enhanced: a ship starts its fade-in 4 s back along its course, and is solid where the original starts it', () => {
  for (const def of [GUNSHIP, CORE, ASSAULT, { ...GUNSHIP, inDirection: 0 }]) {
    for (let seed = 1; seed < 20; seed++) {
      const original = launch(def, new EngineRandom(seed));
      const ship = launchEnhanced(def, new EngineRandom(seed));
      assert.equal(ship.fade, 0);
      assert.equal(ship.entered, false);
      assert.ok(near(ship.age, original.age - PRE_ROLL, 1e-9));
      assert.ok(ship.position.distanceTo(original.position) > 0 || def.velocity.every(([a, b]) => a === 0 && b === 0));
      // Outside its ring at first (unless it only climbs), never cut off there.
      const steps = Math.round(PRE_ROLL * 120);
      for (let i = 0; i < steps; i++) assert.equal(flyEnhanced(def, ship, 1 / 120), true);
      assert.ok(near(ship.fade, 1, 1e-9), 'faded in');
      assert.ok(ship.position.distanceTo(original.position) < 0.05 + 0.5 * Math.hypot(...def.acceleration) * PRE_ROLL / 120 * PRE_ROLL,
        `${def.model}: at the original start, ${ship.position.distanceTo(original.position)}`);
      ship.velocity.forEach((v, k) => assert.ok(near(v, original.velocity[k], 1e-9)));
      assert.ok(ship.forward.equals(original.forward));
    }
  }
});

test('Enhanced: once inside its ring a ship leaves it and fades out from where it is, without the jump', () => {
  const def = { ...GUNSHIP, inDirection: 0, velocity: [[0, 0], [0, 0], [100, 100]] };
  const ship = launchEnhanced(def, new EngineRandom(9));
  let frames = 0, entered = -1;
  while (ship.fade >= 0) {
    assert.equal(flyEnhanced(def, ship, 0.05), true);
    if (ship.entered && entered < 0) entered = frames;
    assert.ok(++frames < 600);
  }
  assert.ok(entered >= 0, 'it went inside first');
  // Out of range on that frame's check: it fades out from full.
  const away = () => Math.hypot(ship.position.x, ship.position.z);
  assert.ok(away() > def.distance + KILL_MARGIN);
  assert.ok(near(ship.fade, -1 + 0.05 * FADE_RATE, 1e-9));
  // Cut off mid fade-in (its life), it fades out from that alpha.
  const young = launchEnhanced({ ...def, lifeTime: 1000 }, new EngineRandom(3));
  for (let i = 0; i < 20; i++) flyEnhanced(def, young, 0.05);
  const alpha = young.fade;
  young.age = def.lifeTime + 1;
  flyEnhanced(def, young, 0.05);
  assert.ok(near(Math.abs(young.fade), alpha - 0.05 * FADE_RATE, 1e-9), 'no jump to full');
  let last = Math.abs(young.fade), steps = 0;
  while (flyEnhanced(def, young, 0.05)) { assert.ok(Math.abs(young.fade) < last); last = Math.abs(young.fade); steps++; }
  assert.ok(steps < 1 / (0.05 * FADE_RATE));
});

test('Enhanced: a ship past the far plane is drawn scaled toward the eye, its image unchanged, inside the far plane', () => {
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 2, 500);
  camera.position.set(10, 5, -20);
  const ship = { position: new THREE.Vector3(900, 400, 1200), forward: new THREE.Vector3(0.6, 0, 0.8) };
  camera.lookAt(ship.position);
  camera.updateMatrixWorld();
  const matrix = placement(ship), drawn = new THREE.Matrix4();
  const radius = 300;
  const scale = withinReach(matrix, camera.position, radius, 500, drawn);
  const distance = ship.position.distanceTo(camera.position);
  assert.ok(near(scale, 500 * REACH / (distance + radius), 1e-12));
  for (const corner of [[0, 0, 0], [150, 60, -270], [-150, 0, 270], [100, 180, 0]]) {
    const a = new THREE.Vector3(...corner).applyMatrix4(matrix), b = new THREE.Vector3(...corner).applyMatrix4(drawn);
    assert.ok(b.distanceTo(camera.position) <= 500 * REACH + 1e-6, 'inside the far plane');
    const pa = a.clone().project(camera), pb = b.clone().project(camera);
    assert.ok(near(pa.x, pb.x, 1e-9) && near(pa.y, pb.y, 1e-9), 'the same image');
  }
  // Inside the reach it stays where it is.
  const close = { position: new THREE.Vector3(100, 50, 100), forward: new THREE.Vector3(0, 0, 1) };
  assert.equal(withinReach(placement(close), camera.position, 20, 500, drawn), 1);
  assert.ok(drawn.equals(placement(close)));
});

test('Enhanced: far, low ships take on the sky behind them; high or near ones do not', () => {
  assert.equal(haze(400, 20, 1000), 0, 'near');
  assert.equal(haze(5000, 4000, 1000), 0, 'high');
  assert.ok(near(haze(3000, 0, 1000), HAZE, 1e-12), 'far, on the horizon');
  assert.ok(haze(2000, 100, 1000) > haze(1500, 100, 1000), 'more with distance');
  assert.ok(haze(2000, 100, 1000) > haze(2000, 400, 1000), 'less up the sky');
  assert.ok(haze(2000, 100, 1000) <= HAZE);
  assert.equal(haze(2000, 100, 0), 0);
});

// One triangle a model, packed as tools/extract.py write_meshes packs them.
function pack(names) {
  const floats = [], indices = [], models = {};
  for (const name of names) {
    const base = floats.length / 8;
    floats.push(0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, -1, 0, 0, 0, 1, 0, 0, 1);
    models[name] = [{ texture: `${name}_tex`, vertexOffset: base * 32, vertexCount: 3, indexOffset: indices.length * 4, indexCount: 3, flags: 1 }];
    indices.push(base, base + 1, base + 2);
  }
  return { models, buffer: new Float32Array(floats).buffer, indexBuffer: new Uint32Array(indices).buffer };
}

// The settings the flyers read, changed as the options menu changes them.
function settingsWith(value) {
  const listeners = [];
  return {
    value, get: (key) => (key === SKY_SETTING ? this?.value ?? value : undefined),
    onChange: (listener) => { listeners.push(listener); return () => listeners.splice(listeners.indexOf(listener), 1); },
    set(next) { value = next; this.get = (key) => (key === SKY_SETTING ? value : undefined); for (const l of listeners) l(SKY_SETTING); },
  };
}

// A motion pass that only counts what is added to it.
function motionPass() {
  return { frame: 0, extras: new Set(), addExtra(o) { this.extras.add(o); }, removeExtra(o) { this.extras.delete(o); } };
}

test('a battle draws its own classes: one instanced draw a part, far to near', () => {
  const { models, buffer, indexBuffer } = pack([ASSAULT.model, GUNSHIP.model, CORE.model]);
  const data = { flyers: [ASSAULT, GUNSHIP, CORE], missions: { geo1r: [0, 1, 2], other: [], solo: [1] }, models };
  assert.deepEqual(battleFlyers(data, 'solo'), [GUNSHIP]);
  assert.deepEqual(battleFlyers(data, 'solo', true), [GUNSHIP], 'no Enhanced lists: the original\'s');
  assert.deepEqual(battleFlyers(data, 'other'), []);
  assert.deepEqual(battleFlyers(data, 'missing'), []);
  assert.deepEqual(battleFlyers(null, 'geo1r'), []);
  const asked = [];
  const scene = new THREE.Scene(), motion = motionPass();
  const flyers = new SkyFlyers({ data, mission: 'geo1r', buffer, indexBuffer, scene, seed: 3, motion, texture: (name) => { asked.push(name); return null; } });
  assert.equal(scene.children.length, 1);
  assert.equal(flyers.count, 30);
  assert.equal(flyers.enhanced, false, 'Classic without the setting');
  // Classic: every ship in the one draw a part, writing depth as it fades.
  const solids = flyers.classes.map(kind => kind.draws[0]), fadings = flyers.classes.map(kind => kind.fades[0]);
  assert.deepEqual(solids.map(m => [m.name, m.count, m.isInstancedMesh, m.frustumCulled, m.renderOrder, m.visible]),
    [[ASSAULT.model, 2, true, false, 1, true], [GUNSHIP.model, 20, true, false, 1, true], [CORE.model, 8, true, false, 1, true]]);
  assert.ok(fadings.every(m => !m.visible && m.count === 0 && m.renderOrder === 2));
  assert.deepEqual(asked, [`${ASSAULT.model}_tex`, `${GUNSHIP.model}_tex`, `${CORE.model}_tex`]);
  assert.equal(motion.extras.size, 3, 'each part draws its motion');
  // Ships fade in together from the start.
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(0, 100, 0);
  flyers.update(2, camera);
  const gunships = flyers.classes[1], looks = solids[1].geometry.getAttribute('look');
  for (let i = 0; i < 20; i++) assert.ok(near(looks.array[i * 4], 2 * FADE_RATE, 1e-6));
  // Drawn far to near.
  const matrix = new THREE.Matrix4(), at = new THREE.Vector3();
  const distances = [];
  for (let i = 0; i < 20; i++) {
    solids[1].getMatrixAt(i, matrix);
    distances.push(at.setFromMatrixPosition(matrix).distanceTo(camera.position));
  }
  assert.ok(distances.every((d, i) => i === 0 || d <= distances[i - 1] + 1e-3), 'sorted far to near');
  // Their motion: where each was a frame ago, and where it is (as floats).
  const moving = gunships.motion;
  const same = (matrix, array, at) => matrix.elements.every((v, k) => Math.abs(v - array[at + k]) <= 1e-4 * Math.max(1, Math.abs(v)));
  assert.ok(gunships.ships.every((ship, i) => same(ship.matrix, moving.matrices.array, i * 16)));
  assert.ok(gunships.ships.every((ship, i) => same(ship.previous, moving.previous.array, i * 16) && !same(ship.matrix, moving.previous.array, i * 16)));
  // They go on flying, and come back: never more ships than NumObjects.
  for (let i = 0; i < 60 * 60; i++) flyers.update(1 / 60, camera);
  assert.equal(gunships.ships.length, 20);
  // Those not fading out are inside the range, give a frame's flight.
  for (const ship of gunships.ships) if (ship.fade >= 0) assert.ok(Math.hypot(ship.position.x, ship.position.z) <= GUNSHIP.distance + KILL_MARGIN + 2);
  // The same seed flies the same ships.
  const twin = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 3, motion: motionPass() });
  const other = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 4, motion: motionPass() });
  for (let i = 0; i < 600; i++) { twin.update(1 / 60, camera); other.update(1 / 60, camera); }
  const flock = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 3, motion: motionPass() });
  for (let i = 0; i < 600; i++) flock.update(1 / 60, camera);
  assert.deepEqual(flock.classes[1].ships.map(s => s.position.toArray()), twin.classes[1].ships.map(s => s.position.toArray()));
  assert.notDeepEqual(other.classes[1].ships.map(s => s.position.toArray()), twin.classes[1].ships.map(s => s.position.toArray()));
  // Paused (no time), nothing moves, and the motion pass shows none.
  const still = gunships.ships.map(s => s.position.clone());
  flyers.update(0, camera);
  assert.ok(gunships.ships.every((s, i) => s.position.equals(still[i])));
  const motionDraw = [...motion.extras][0];
  motion.frame = 5; motionDraw.onBeforeRender();
  assert.equal(motionDraw.material.uniforms.still.value, 0, 'a frame after flight moves');
  motion.frame = 6; motionDraw.onBeforeRender();
  assert.equal(motionDraw.material.uniforms.still.value, 1, 'a frame without flight holds still');
  flyers.dispose();
  assert.equal(scene.children.length, 0);
  assert.equal(motion.extras.size, 0);
  // A battle without flyers, or a map without the file, draws nothing.
  const none = new SkyFlyers({ data, mission: 'other', buffer, indexBuffer, scene, motion: motionPass() });
  assert.equal(none.count, 0);
  none.update(1 / 60, camera);
  assert.equal(new SkyFlyers({ data: null, mission: 'geo1r', scene, motion: motionPass() }).count, 0);
});

test('Ocean and sky Enhanced: the era\'s classes, solid and fading draws apart, ships past the far plane drawn, switched live', () => {
  const FEDLANDER = { ...ASSAULT, model: 'cis_fly_fedlander_dome' };
  const { models, buffer, indexBuffer } = pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]);
  const data = { flyers: [ASSAULT, FEDLANDER, GUNSHIP], missions: { bes2r: [0], bes2a: [] }, enhanced: { bes2r: [0, 1], bes2a: [2] }, models };
  assert.deepEqual(battleFlyers(data, 'bes2r', true), [ASSAULT, FEDLANDER]);
  assert.deepEqual(battleFlyers(data, 'bes2r', false), [ASSAULT]);
  const settings = settingsWith('enhanced');
  assert.equal(enhancedSky(settings), true);
  const graphics = { farCamera: { far: 500 }, renderer: null, settings };
  const scene = new THREE.Scene(), motion = motionPass();
  const flyers = new SkyFlyers({ data, mission: 'bes2r', ...pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]), scene, graphics, motion, seed: 5 });
  assert.equal(flyers.enhanced, true);
  assert.deepEqual(flyers.classes.map(kind => [kind.def.model, kind.classic, kind.enhanced]),
    [[ASSAULT.model, true, true], [FEDLANDER.model, false, true]]);
  assert.equal(flyers.count, 4);
  const camera = new THREE.PerspectiveCamera(60, 1, 2, 200);
  camera.position.set(0, 50, 0);
  // At first every ship is fading in: the fading draws, which write no depth.
  flyers.update(1 / 60, camera);
  const [assault] = flyers.classes;
  assert.equal(assault.draws[0].visible, false);
  assert.ok(assault.fades[0].visible && assault.fades[0].count === 2 && assault.fades[0].material.depthWrite === false);
  // Solid once faded in: the solid draws.
  for (let i = 0; i < Math.ceil(PRE_ROLL * 60) + 2; i++) flyers.update(1 / 60, camera);
  assert.ok(assault.draws[0].visible && assault.draws[0].count === 2);
  assert.equal(assault.fades[0].visible, false);
  // 1500 m out, past the 500 m far plane: drawn scaled toward the eye,
  // flagged for the depth just inside it.
  const looks = assault.solid.looks.array, matrix = new THREE.Matrix4();
  for (let i = 0; i < 2; i++) {
    assert.equal(looks[i * 4 + 2], 1);
    assert.ok(looks[i * 4 + 3] > 0 && looks[i * 4 + 3] < 1);
    assault.draws[0].getMatrixAt(i, matrix);
    assert.ok(new THREE.Vector3().setFromMatrixPosition(matrix).distanceTo(camera.position) < 500 * REACH);
  }
  // Classic again, live: only the battle's own class; its ships fly on.
  const position = assault.ships[0].position.clone();
  settings.set('classic');
  assert.equal(flyers.enhanced, false);
  assert.equal(flyers.count, 2);
  assert.ok(assault.ships[0].position.equals(position), 'kept its course');
  flyers.update(1 / 60, camera);
  assert.equal(flyers.classes[1].draws[0].visible, false, 'the Enhanced-only class is gone');
  assert.equal(looks[2], 0, 'Classic: cut off by the far plane');
  assert.ok(new THREE.Vector3().setFromMatrixPosition((assault.draws[0].getMatrixAt(0, matrix), matrix)).distanceTo(camera.position) > 1000);
  // And Enhanced again: the class comes back afresh.
  settings.set('enhanced');
  assert.equal(flyers.count, 4);
  assert.ok(flyers.classes[1].ships.every(ship => ship.fade === 0 && !ship.entered));
  flyers.dispose();
  // Without its Enhanced lists, a map prepared before them draws the original's.
  const old = new SkyFlyers({ data: { ...data, enhanced: undefined }, mission: 'bes2r', ...pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]), graphics, motion: motionPass() });
  assert.equal(old.count, 2);
  old.dispose();
});

// Prepared maps, where present (tools/zero_sky.py): what each battle draws
// as the original (classic) and as the Enhanced sky; every other map has
// no sky flyers.
const CAPITALS = { cw: ['rep_fly_assault_dome', 'cis_fly_fedlander_dome'], gcw: ['all_fly_moncalamari_dome', 'imp_fly_destroyer_dome'] };
const GEONOSIS = ['rep_fly_assault_dome', 'rep_fly_gunship_dome', 'cis_fly_droidfighter_dome', 'cis_fly_techounion_dome'];
const BATTLES = {
  bes1: { bes1r: [[], CAPITALS.cw], bes1a: [[], CAPITALS.gcw] },
  bes2: { bes2r: [['rep_fly_assault_dome'], CAPITALS.cw], bes2a: [[], CAPITALS.gcw] },
  geo1: { geo1r: [GEONOSIS, GEONOSIS] },
  nab1: { nab1c: [['cis_fly_fedlander_dome'], ['cis_fly_fedlander_dome']], nab1i: [['imp_fly_destroyer_dome'], ['imp_fly_destroyer_dome']] },
  tat1: { tat1r: [['rep_fly_assault_dome'], CAPITALS.cw], tat1i: [['imp_fly_destroyer_dome'], CAPITALS.gcw] },
  tat2: { tat2r: [['rep_fly_assault_dome'], CAPITALS.cw], tat2i: [['imp_fly_destroyer_dome'], CAPITALS.gcw], tat2i_h: [['all_fly_moncalamari_dome'], CAPITALS.gcw] },
};
for (const [map, battles] of Object.entries(BATTLES)) {
  const path = new URL(`../public/maps/${map}/sky/flyers.json`, import.meta.url);
  const data = fs.existsSync(path) ? JSON.parse(fs.readFileSync(path)) : null;
  test(`${map}: its prepared flyers`, { skip: !data?.enhanced }, () => {
    for (const [mission, [classic, enhanced]] of Object.entries(battles)) {
      assert.deepEqual(battleFlyers(data, mission).map(f => f.model), classic, `${mission} classic`);
      assert.deepEqual(battleFlyers(data, mission, true).map(f => f.model), enhanced, `${mission} enhanced`);
      // Never another era's ships.
      const era = { r: 'cw', c: 'cw', a: 'gcw', i: 'gcw' }[mission[4]];
      assert.ok(battleFlyers(data, mission, true).every(f => f.era === era), `${mission}: ${era} only`);
    }
    for (const def of data.flyers) {
      assert.ok(data.models[def.model]?.length, `${def.model} has parts`);
      for (const part of data.models[def.model]) assert.ok(data.textures[part.texture], `${part.texture} exported`);
    }
    const dir = new URL(`../public/maps/${map}/sky/`, import.meta.url);
    const buffer = fs.readFileSync(new URL('flyers.bin', dir)), indexBuffer = fs.readFileSync(new URL('flyers_index.bin', dir));
    const slice = (b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
    for (const mission of Object.keys(battles)) {
      for (const mode of ['classic', 'enhanced']) {
        const graphics = { farCamera: { far: 1000 }, renderer: null, settings: settingsWith(mode) };
        const flyers = new SkyFlyers({ data, mission, buffer: slice(buffer), indexBuffer: slice(indexBuffer), graphics, motion: motionPass() });
        assert.equal(flyers.count, battleFlyers(data, mission, mode === 'enhanced').reduce((sum, f) => sum + f.count, 0));
        flyers.update(1 / 60, new THREE.PerspectiveCamera());
        flyers.dispose();
      }
    }
  });
}
for (const map of ['end1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab2', 'rhn1', 'rhn2', 'tat3', 'yav1', 'yav2']) {
  const dir = new URL(`../public/maps/${map}/`, import.meta.url);
  test(`${map}: no battle draws sky flyers`, { skip: !fs.existsSync(new URL('map.json', dir)) }, () => {
    assert.equal(fs.existsSync(new URL('sky/flyers.json', dir)), false);
  });
}
