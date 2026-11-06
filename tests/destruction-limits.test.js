import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Destruction } from '../src/destruction.js';
import { Dst } from '../src/environment/layout.js';
import { loadEnvironment } from '../src/environment/module.js';
import { zeroMaterial } from '../src/materials.js';
import { listenSounds } from '../src/audio/events.js';

await loadEnvironment();

// The most the game can throw at the phase between two frames (a load, a
// pause, a slow PC) and more than any map has of models, bones and droids:
// nothing may throw, the queue must never fill, and the frames after must
// carry on.
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
const material = zeroMaterial('dynamic', {});
const model = () => () => {
  const group = new THREE.Group();
  group.add(new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.12, 0.12).translate(0.15, 0, 0), material));
  return group;
};
const enhanced = { get: () => 'enhanced', onChange: () => () => {} };
const quiet = { flash() {}, add() {} };

// A droid with `bones` bones in a chain and `chunks` chunks, each its own model.
function droid(at, { bones = 4, chunks = 3, tag = '' } = {}) {
  const names = Array.from({ length: bones }, (_, i) => `bone_${i}`);
  const list = names.map((name) => Object.assign(new THREE.Bone(), { name }));
  list.forEach((bone, i) => { if (i) list[i - 1].add(bone); bone.position.set(0, 0.2, 0); });
  const root = new THREE.Group();
  root.position.fromArray(at);
  root.add(list[0]);
  root.updateMatrixWorld(true);
  const positions = [], skin = [];
  list.forEach((bone, i) => { for (let k = 0; k < 16; k++) { positions.push(0.01 * k, 0.01 * i, 0); skin.push(i, 0, 0, 0); } });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(skin, 4));
  return {
    classDef: {
      chunks: Array.from({ length: chunks }, (_, i) => ({ ChunkGeometryName: `${tag}chunk${i}`, ChunkNodeName: `bone_${i % bones}`, ChunkSpeed: 8 })),
      explosion: { class: 'com_inf_droid_exp', base: 'explosion' },
    },
    actor: { root, bones: list, body: { geometry } },
    position: new THREE.Vector3().fromArray(at),
    velocity: new THREE.Vector3(),
    walker: false, generation: 1,
    combat: { dead: true }, animator: {}, goUp() {},
  };
}

function make(lights = quiet) {
  const scene = new THREE.Scene();
  return { scene, d: new Destruction({ scene, collision: FLOOR, lights, settings: enhanced, model, seed: 5 }) };
}

test('a long load or pause with events and no frame never fills the queue or stops the next frame', () => {
  const { d } = make();
  const memory = d.host.x.memory.buffer;
  let flashes = 0;
  d.lights = { flash() { flashes++; }, add() { flashes++; } };
  // Far more than the queue holds: blasts, deaths, droids blown apart, droideka blasts.
  const units = Array.from({ length: Dst.SLOTS * 3 }, (_, i) => droid([i % 9, 0, Math.floor(i / 9)]));
  for (let i = 0; i < Dst.COMMANDS * 3; i++) d.blast(new THREE.Vector3(i * 0.01, 0.4, 0), 0.5);
  for (const unit of units) d.died(unit);
  for (const unit of units.slice(0, 40)) d.blowApart(unit);
  for (const unit of units.slice(40, 60)) d.deathBlast(unit);
  assert.doesNotThrow(() => d.update(1 / 60));
  for (let i = 0; i < 90; i++) assert.doesNotThrow(() => d.update(1 / 60));
  assert.ok(d.blasts >= Dst.COMMANDS * 3, 'every blast was made');
  assert.ok(d.pieces.count <= Dst.PIECES, 'the pieces are bounded');
  assert.ok(flashes > 0, 'their lights were asked for');
  assert.equal(d.host.x.memory.buffer, memory, 'the module\'s memory never moved, so the views stay good');
});

test('more models, bones, chunks, droids and wrecks than any map has degrade and never throw', () => {
  const { d, scene } = make();
  // 100 distinct piece models where the most any map has is 23.
  const models = Array.from({ length: 100 }, (_, i) => droid([i, 0, 0], { chunks: 1, tag: `m${i}_` }));
  assert.doesNotThrow(() => d.prepare(models.map((u) => u.classDef)));
  assert.equal(d.pieces.names.length, Dst.SHAPES, 'the shapes the phase has room for');
  // A body of 100 bones and 40 chunks, dying with the others.
  const big = droid([0, 0, 0], { bones: 100, chunks: 40, tag: 'm0_' });
  assert.doesNotThrow(() => { d.died(big); for (let i = 0; i < 60; i++) d.update(1 / 60); });
  assert.equal(big.actor.root.visible, false, 'it still blew apart');
  assert.ok(d.pieces.count > 0 && d.pieces.count <= Dst.PIECES);
  // More droids dying at once than there are slots: the extra ones blow apart at once.
  const crowd = Array.from({ length: Dst.SLOTS + 10 }, (_, i) => droid([i, 0, 5]));
  for (const unit of crowd) d.died(unit);
  d.update(0);
  assert.ok(crowd.slice(Dst.SLOTS).every((unit) => unit.actor.root.visible === false), 'the ones with no slot blew apart');
  // More stations than any map has.
  const stations = Array.from({ length: Dst.STATIONS + 20 }, (_, i) => ({
    state: 'built', health: 1, maxHealth: 1, looks: { destroyed: { root: new THREE.Group(), model: { box: [[0, 0, 0], [1, 1, 1]] } } },
    matrix: new THREE.Matrix4().makeTranslation(i, 0, 0), center: (out) => out.set(i, 0.5, 0),
  }));
  assert.doesNotThrow(() => d.watch({ stations }));
  stations.forEach((s) => { s.state = 'destroyed'; s.health = 0; });
  assert.doesNotThrow(() => { for (let i = 0; i < 30; i++) d.update(1 / 60); });
  assert.equal(d.sites.length, Dst.STATIONS, 'the wrecks it has room for smoulder');
  d.dispose();
  assert.equal(scene.children.length, 0);
});

test('a light or a sound that throws mid-frame is skipped: the rest of the frame and every later frame go on', () => {
  const sounds = [];
  const stop = listenSounds((type) => { sounds.push(type); throw new Error('the audio engine failed'); });
  const quietLog = console.error;
  console.error = () => {};
  try {
    let calls = 0;
    const { d } = make({ flash() { calls++; throw new Error('the lights failed'); }, add() { calls++; throw new Error('the lights failed'); } });
    const units = [droid([0, 0, 0]), droid([3, 0, 0])];
    d.died(units[0]);
    d.blowApart(units[1]);
    d.blast(new THREE.Vector3(1, 0.4, 1), 1);
    assert.doesNotThrow(() => d.update(1 / 60));
    assert.ok(calls >= 3 && d.failures >= 3, 'each failure counted, none stopped the rest');
    assert.ok(d.pieces.count > 0 && d.fx.hot.count > 0, 'the pieces and the effects were still made and applied');
    for (let i = 0; i < 60; i++) assert.doesNotThrow(() => d.update(1 / 60));
    assert.equal(units[0].actor.root.visible, false, 'the short-out ended in its burst although its sound threw');
    assert.ok(sounds.length > 0);
    d.pieces.sync(d.host.result, 0, 0);
    assert.equal(d.pieces.count, 0, 'a page that lost every piece is brought back in step by the records');
  } finally { stop(); console.error = quietLog; }
});

test('switching to Classic clears the effects at once, with no frame to carry it (the pause menu)', () => {
  const { d } = make();
  d.blowApart(droid([0, 0, 0]));
  d.blast(new THREE.Vector3(0, 0.4, 0), 1);
  d.update(1 / 60);
  assert.ok(d.pieces.count > 0 && d.fx.hot.count > 0);
  d.clear();
  assert.deepEqual([d.pieces.count, d.fx.hot.count, d.fx.smoke.count, d.fx.arcs.live], [0, 0, 0, 0], 'gone without another update');
});
