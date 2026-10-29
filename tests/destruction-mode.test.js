import test, { beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Destruction } from '../src/destruction.js';
import { DEFAULTS, PERFORMANCE_PRESETS, Settings } from '../src/settings.js';
import { zeroMaterial } from '../src/materials.js';
import { listenSounds } from '../src/audio/events.js';
import { RandomStreams } from '../src/ai/random.js';

// Geometry, launch directions and cosmetic effects all draw randomness,
// including Three.js helpers that use Math.random directly. Keep each test
// reproducible without consuming the simulation's random stream.
beforeEach(context => {
  const random = new RandomStreams(20261012).get(context.name);
  context.mock.method(Math, 'random', () => random.uniform());
});

const FLOOR = {
  cast(origin, direction, far, out) {
    if (direction.y >= 0 || origin.y < 0) return null;
    const t = origin.y / -direction.y;
    if (t > far) return null;
    out.distance = t;
    out.point.copy(origin).addScaledVector(direction, t);
    out.normal.set(0, 1, 0);
    return out;
  },
};

function settings(mode = 'enhanced') {
  const values = { destructionEffects: mode };
  const listeners = new Set();
  return {
    get: (key) => values[key],
    set(key, value) { values[key] = value; for (const listener of listeners) listener(key, value); },
    onChange(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    listeners,
  };
}

const material = zeroMaterial('dynamic', {});
const model = () => () => {
  const group = new THREE.Group();
  group.add(new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.12, 0.12).translate(0.15, 0, 0), material));
  return group;
};

function lights() {
  return {
    flashes: 0, added: 0, at: [],
    flash(position) { this.flashes++; this.at.push(position.clone()); },
    add(position) { this.added++; this.at.push(position.clone()); },
  };
}

// A droid: a little skeleton, a body whose vertices hang from its bones, and
// a piece on each limb. Its simulation randomness must never be drawn on.
// Bodies of one model share their geometry (actor.js), as `geometry` lets a
// second droid here; `at` places it.
function droid({ geometry: shared = null, at = [0, 0, 0] } = {}) {
  const names = ['bone_root', 'bone_a_spine', 'bone_r_upperarm', 'bone_l_calf'];
  const bones = names.map((name) => Object.assign(new THREE.Bone(), { name }));
  bones[0].add(bones[1]); bones[1].add(bones[2]); bones[0].add(bones[3]);
  bones[0].position.set(0, 0.1, 0);
  bones[1].position.set(0, 1, 0); bones[2].position.set(0.25, 0.4, 0); bones[3].position.set(-0.15, 0.3, 0);
  const root = new THREE.Group();
  root.position.fromArray(at);
  root.add(bones[0]);
  root.updateMatrixWorld(true);
  const positions = [], skin = [];
  bones.forEach((bone, i) => { for (let k = 0; k < 16; k++) { positions.push(Math.random() * 0.1, Math.random() * 0.1, Math.random() * 0.1); skin.push(i, 0, 0, 0); } });
  const geometry = shared || new THREE.BufferGeometry();
  if (!shared) {
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    geometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(skin, 4));
  }
  const chunk = (name, node) => ({ ChunkGeometryName: name, ChunkNodeName: node, ChunkSpeed: 8 });
  return {
    classDef: {
      chunks: [chunk('torso', 'bone_a_spine'), chunk('arm', 'bone_r_upperarm'), chunk('leg', 'bone_l_calf')],
      explosion: { class: 'com_inf_droid_exp', base: 'explosion', Effect: 'droid_explosion', Damage: 0, Push: 0 },
    },
    actor: { root, bones, body: { geometry } },
    position: new THREE.Vector3().fromArray(at),
    velocity: new THREE.Vector3(),
    walker: false,
    generation: 1,
    combat: { dead: true, random: () => { throw new Error('the simulation randomness was used'); } },
    animator: {},
  };
}

function destruction(mode = 'enhanced') {
  const scene = new THREE.Scene();
  const light = lights();
  const options = settings(mode);
  const result = new Destruction({ scene, collision: FLOOR, lights: light, settings: options, model, random: Math.random });
  return { destruction: result, scene, lights: light, settings: options };
}

function run(d, seconds) {
  for (let i = 0; i < Math.round(seconds * 60); i++) d.update(1 / 60, null);
}

test('the setting defaults to enhanced and the presets carry it', () => {
  assert.equal(DEFAULTS.destructionEffects, 'enhanced');
  assert.equal(PERFORMANCE_PRESETS.performance.values.destructionEffects, 'classic');
  assert.equal(PERFORMANCE_PRESETS.balanced.values.destructionEffects, 'enhanced');
  assert.equal(PERFORMANCE_PRESETS.quality.values.destructionEffects, 'enhanced');
  const options = new Settings();
  options.set('performancePreset', 'performance');
  assert.equal(options.get('destructionEffects'), 'classic');
  options.set('destructionEffects', 'enhanced');
  assert.equal(options.get('performancePreset'), 'custom', 'changing it leaves the preset');
});

test('classic leaves droid deaths to the stock effects and pieces', () => {
  const { destruction: d, lights: light } = destruction('classic');
  const unit = droid();
  assert.equal(d.explosion(unit.classDef.explosion), unit.classDef.explosion);
  assert.equal(d.blowApart(unit), false);
  assert.equal(unit.actor.root.visible, true);
  d.died(unit);
  assert.equal(d.deaths.length, 0);
  assert.equal(d.deathBlast(unit), false);
  run(d, 1);
  assert.deepEqual([d.pieces.count, d.fx.hot.count, d.fx.smoke.count, d.fx.arcs.live, light.flashes], [0, 0, 0, 0, 0]);
});

test('enhanced blows a droid apart into its pieces and draws its own blast, touching nothing it simulates', () => {
  const { destruction: d, lights: light } = destruction();
  const unit = droid();
  const quiet = d.explosion(unit.classDef.explosion);
  assert.notEqual(quiet, unit.classDef.explosion);
  assert.equal(quiet.Effect, null, 'the stock sprites are left out');
  for (const key of ['class', 'Damage', 'Push']) assert.equal(quiet[key], unit.classDef.explosion[key]);
  assert.equal(d.explosion(unit.classDef.explosion), quiet, 'one copy per explosion');
  const before = [unit.position.clone(), unit.velocity.clone()];
  assert.equal(d.blowApart(unit), true);
  assert.equal(unit.actor.root.visible, false);
  assert.equal(d.pieces.count, 3, 'a piece for each chunk');
  assert.ok(d.fx.hot.count > 50 && d.fx.smoke.count > 20 && d.fx.arcs.live > 0);
  assert.ok(light.flashes >= 2);
  run(d, 3);
  assert.ok(d.pieces.pieces.every((piece) => piece.body.sleeping), 'the pieces have settled');
  assert.ok(d.pieces.pieces.every((piece) => piece.body.position.y < 0.3 && piece.body.position.y > 0), 'on the floor');
  assert.deepEqual([unit.position, unit.velocity], before);
});

test('a droid killed on its feet shorts out, then blows apart with its explosion sound', () => {
  const sounds = [];
  const stop = listenSounds((type, ...args) => sounds.push([type, args[0]]));
  try {
    const { destruction: d } = destruction();
    const unit = droid();
    d.died(unit);
    assert.equal(d.deaths.length, 1);
    run(d, 0.3);
    assert.equal(unit.actor.root.visible, true, 'still shorting out');
    assert.ok(d.fx.arcs.spawned > 4, 'arcs crawl over it');
    assert.equal(d.pieces.count, 0);
    run(d, 0.2);
    assert.equal(unit.actor.root.visible, false);
    assert.equal(d.pieces.count, 3);
    assert.deepEqual(sounds, [['explosion', unit.classDef.explosion]]);
    d.died(unit);
    assert.equal(d.deaths.length, 0, 'a body already blown apart does not short out again');
  } finally { stop(); }
});

test('a short-out stops if the body is reused before it blows', () => {
  const { destruction: d } = destruction();
  const unit = droid();
  d.died(unit);
  unit.generation++;
  run(d, 1);
  assert.equal(d.deaths.length, 0);
  assert.equal(d.pieces.count, 0);
  assert.equal(unit.actor.root.visible, true);
});

test('pieces sharing a node follow the bone whose part of the body they cover', () => {
  const { destruction: d } = destruction();
  const unit = droid();
  unit.walker = true;
  // All three on the root, modelled in its frame at the standing pose.
  for (const chunk of unit.classDef.chunks) chunk.ChunkNodeName = 'bone_root';
  unit.animator.bind = unit.actor.bones.map((bone) => ({ q: bone.quaternion.clone(), p: bone.position.clone() }));
  const shape = d.pieces.shape('arm');
  // The arm chunk's model sits where the arm's vertices are at bind.
  d.pieces.shapes.set('arm', { ...shape, center: new THREE.Vector3(0.3, 1.45, 0.05) });
  // The arm swings down; the piece on it goes with it.
  unit.actor.bones[2].quaternion.setFromAxisAngle(new THREE.Vector3(0, 0, 1), -Math.PI / 2);
  unit.actor.root.updateMatrixWorld(true);
  const frames = d.chunkFrames(unit);
  const arm = frames.find((frame) => frame.name === 'arm');
  const centre = new THREE.Vector3(0.3, 1.45, 0.05).applyMatrix4(arm.matrix);
  const vertex = new THREE.Vector3(0.05, 0.05, 0.05).applyMatrix4(unit.actor.bones[2].matrixWorld);
  assert.ok(centre.distanceTo(vertex) < 0.08, `the arm piece stayed ${centre.distanceTo(vertex)} m from the arm`);
  const torso = frames.find((frame) => frame.name === 'torso');
  assert.notDeepEqual(torso.matrix.elements, arm.matrix.elements);
});

test('switching to classic clears what enhanced drew', () => {
  const { destruction: d, settings: options } = destruction();
  d.blowApart(droid());
  d.died(droid());
  run(d, 0.1);
  options.set('destructionEffects', 'classic');
  assert.deepEqual([d.pieces.count, d.fx.hot.count, d.fx.smoke.count, d.fx.arcs.live, d.deaths.length], [0, 0, 0, 0, 0]);
});

function station() {
  const root = new THREE.Group();
  const original = zeroMaterial('dynamic', {});
  root.add(new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), original));
  return {
    state: 'built', health: 80, maxHealth: 80, original,
    looks: { destroyed: { root, model: { box: [[-0.5, 0, -0.5], [0.5, 1.2, 0.5]] } } },
    matrix: new THREE.Matrix4(),
    center: (out) => out.set(0, 0.6, 0),
  };
}

test('a wrecked power-up droid smoulders, dies down while repaired, and stops when rebuilt', () => {
  const { destruction: d } = destruction();
  const droidStation = station();
  d.watch({ stations: [droidStation] });
  run(d, 0.5);
  assert.equal(d.sites.length, 0);
  droidStation.state = 'destroyed';
  droidStation.health = 0;
  run(d, 0.1);
  assert.equal(d.sites.length, 1);
  assert.equal(d.blasts, 1);
  const mesh = droidStation.looks.destroyed.root.children[0];
  assert.notEqual(mesh.material, droidStation.original, 'the wreck glows');
  run(d, 6);
  const smoking = d.fx.smoke.emitted;
  run(d, 6);
  const whole = d.fx.smoke.emitted - smoking;
  droidStation.health = 72;
  const repairing = d.fx.smoke.emitted;
  run(d, 6);
  assert.ok(d.fx.smoke.emitted - repairing < whole * 0.6, 'repair smothers it');
  droidStation.state = 'built';
  run(d, 0.1);
  assert.equal(d.sites.length, 0);
  assert.equal(mesh.material, droidStation.original, 'its own material back');
});

test('a wreck goes out by itself in a few minutes', () => {
  const { destruction: d } = destruction();
  const droidStation = station();
  d.watch({ stations: [droidStation] });
  droidStation.state = 'destroyed';
  droidStation.health = 0;
  for (let i = 0; i < 1900; i++) d.update(0.1, null);
  assert.equal(d.sites.length, 0);
});

// Everything a droid shorting out has drawn so far: the ends of its arcs,
// where its sparks and flashes were born, and its lights.
function drawn(d, light) {
  const ends = [];
  for (const arc of d.fx.arcs.arcs) if (arc.live) for (const end of [arc.a, arc.b]) if (end.object) ends.push(end.object);
  const births = [];
  for (let i = 0; i < d.fx.hot.count; i++) births.push(new THREE.Vector3().fromArray(d.fx.hot.arrays[0], i * 4));
  return { ends, births, lights: light.at };
}

test('a second droid of the same model shorts out on its own body, not the first one\'s', () => {
  const { destruction: d, lights: light } = destruction();
  const first = droid();
  const second = droid({ geometry: first.actor.body.geometry, at: [50, 0, 0] });
  const third = droid({ geometry: first.actor.body.geometry, at: [0, 0, 50] });
  assert.equal(second.actor.body.geometry, first.actor.body.geometry);
  for (const [unit, others] of [[first, [second, third]], [second, [first, third]], [third, [first, second]]]) {
    d.clear();
    light.at.length = 0;
    d.died(unit);
    run(d, 0.3);
    const { ends, births, lights: lit } = drawn(d, light);
    assert.ok(ends.length > 0 && births.length > 0 && lit.length > 0, 'it shorts out');
    assert.ok(ends.every((bone) => unit.actor.bones.includes(bone)), 'its arcs are on its own bones');
    for (const other of others) assert.ok(!ends.some((bone) => other.actor.bones.includes(bone)), 'never on another body');
    for (const at of [...births, ...lit]) assert.ok(at.distanceTo(unit.position) < 3, `drawn ${at.distanceTo(unit.position).toFixed(1)} m from the droid dying`);
    run(d, 0.2);
    assert.equal(unit.actor.root.visible, false, 'and then it blows apart');
  }
});

test('a second droideka of the same model pops on its own body', () => {
  const { destruction: d } = destruction();
  const first = droid();
  const second = droid({ geometry: first.actor.body.geometry, at: [50, 0, 0] });
  assert.equal(d.deathBlast(first), true);
  d.fx.clear();
  assert.equal(d.deathBlast(second), true);
  d.fx.update(0);
  const { births } = drawn(d, lights());
  assert.ok(births.length > 0 && births.every((at) => at.distanceTo(second.position) < 3));
});

test('a droid blown apart in the air draws no floor glow and nothing skips off a floor that is not there', () => {
  const { destruction: d } = destruction();
  const unit = droid({ at: [0, 30, 0] });
  // Nothing below it within 6 m.
  d.collision = { cast: () => null };
  d.pieces.solver.collision = d.collision;
  d.blowApart(unit);
  const hot = d.fx.hot.arrays[3];
  const styles = [], floors = [];
  for (let i = 0; i < d.fx.hot.count; i++) { styles.push(hot[i * 4]); if (hot[i * 4] === 0) floors.push(hot[i * 4 + 3]); }
  assert.ok(!styles.includes(3), 'no glow on a floor');
  assert.ok(floors.length > 0 && floors.every((y) => y < -1000), 'sparks fall freely');
  const smoke = d.fx.smoke.arrays[3];
  for (let i = 0; i < d.fx.smoke.count; i++) assert.notEqual(smoke[i * 4], 2, 'no scorch mark in mid-air');
});

test('dispose takes everything out of the scene and stops listening to the settings', () => {
  const { destruction: d, scene, settings: options } = destruction();
  d.blowApart(droid());
  run(d, 0.1);
  d.dispose();
  assert.equal(d.pieces.count, 0);
  let meshes = 0;
  scene.traverse((object) => { if (object.isMesh) meshes++; });
  assert.equal(meshes, 0, 'no meshes left behind');
  assert.equal(options.listeners.size, 0, 'no longer listening');
});
