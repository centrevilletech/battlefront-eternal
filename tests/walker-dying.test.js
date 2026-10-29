import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import {captureEntityVehicle} from '../src/runtime/network-entity-capture.js';
import {EntityGameState} from '../src/runtime/network-entity-game.js';

// A destroyed walker plays out its death animation before it is gone
// (docs/vehicles.md, "Dying"): the simulation keeps the clock (Vehicle.dying,
// a plain field every host replicates) and sets off its FinalExplosion at
// the end; whoever draws it poses the death clip from that clock. Synthetic
// data only: a two-bone walker whose death01 tips its body over in 31
// frames, played at AnimationSpeed 2.

const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
const FRAMES = 31;
const SPAN = FRAMES / 30 / 2;
const X = new THREE.Vector3(1, 0, 0);

function walker({ respawnTime = 30, fx = {} } = {}) {
  const ground = box(0, -1, 0, 400, 2, 400);
  const collision = new CollisionWorld(ground.positions, ground.indices, ground.polygons);
  const bones = [{ name: 'dummyroot', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'bone_body', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 3, 0).toArray() }];
  // death01: the body tips forward a quarter turn; the root stays.
  const floats = [];
  for (const track of [0, 1]) for (let f = 0; f < FRAMES; f++) {
    const q = new THREE.Quaternion().setFromAxisAngle(X, track ? (f / (FRAMES - 1)) * Math.PI / 2 : 0);
    floats.push(q.x, q.y, q.z, q.w, 0, track ? 3 : 0, 0);
  }
  const blasts = [];
  const data = {
    models: { walker: { bones, parts: [] } }, collision: { walker: box(0, 1, 0, 2, 2, 2) },
    animations: { walker: { fps: 30, bones: ['dummyroot', 'bone_body'], clips: { death01: { frames: FRAMES, flags: 0, tracks: [0, 1], travel: [0, 0, 0], offset: 0 } } } },
    classes: { atst: { model: 'walker', label: 'AT-ST', base: 'walker', settings: { MaxHealth: '1000', AnimationSpeed: '2' }, seats: [{ name: 'BODY', settings: {}, weapons: [] }],
      explosion: { class: 'atst_exp' }, finalExplosion: { class: 'atst_finalexp' },
      fx: { explosionOffset: [0, 5, 0], dying: { effects: ['atat_explosion'], interval: 0.4, finalOffset: [1, 2, 3], span: SPAN }, ...fx } } },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().makeTranslation(10, 0, 0).toArray(), team: 1, post: 'cp', respawnTime, classes: { 1: 'atst' } }],
  };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), animationBuffer: new Float32Array(floats).buffer,
    collision, texture: () => null, camera: new THREE.PerspectiveCamera(), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] },
    ordnance: { explosion: (def, at, owner) => blasts.push({ class: def.class, at: at.clone(), owner }) }, effects: { play() { return null; } } });
  const v = system.vehicles[0];
  return { system, v, blasts };
}
// The world's ticks (Vehicles.update takes at most 0.1 s at a time).
const run = (system, seconds) => { for (let n = Math.round(seconds * 600); n > 0; n--) system.update(1 / 600); };
const tilt = (v) => 2 * Math.atan2(Math.hypot(v.byName.bone_body.quaternion.x, v.byName.bone_body.quaternion.z), v.byName.bone_body.quaternion.w);

test('a destroyed walker dies over its death animation, then its FinalExplosion goes off at its offset and it is gone', () => {
  const { system, v, blasts } = walker();
  v.yaw = Math.PI / 2;
  v.damage(5000, 'player');
  assert.equal(v.alive, false);
  assert.equal(v.body.enabled, false, 'nothing to walk into while it falls');
  assert.equal(v.root.visible, true, 'still drawn');
  assert.ok(Math.abs(v.dying - SPAN) < 1e-9, 'its death animation: 31 frames at AnimationSpeed 2');
  assert.deepEqual(blasts.map(b => b.class), ['atst_exp'], 'its ExplosionName at once, at ExplosionOffset');
  const at = v.position.clone();
  assert.ok(blasts[0].at.distanceTo(at.clone().add(new THREE.Vector3(0, 5, 0))) < 1e-6);
  run(system, SPAN / 2);
  assert.ok(Math.abs(v.dying - SPAN / 2) < 2e-3);
  assert.equal(blasts.length, 1, 'the dying explosions are the page\'s, without damage');
  run(system, SPAN / 2 + 0.01);
  assert.equal(v.dying, 0);
  assert.equal(v.root.visible, false, 'gone');
  assert.deepEqual(blasts.map(b => b.class), ['atst_exp', 'atst_finalexp']);
  // FinalExplosionOffset in the hull's frame: a quarter turn takes +z to +x.
  assert.ok(blasts[1].at.distanceTo(at.clone().add(new THREE.Vector3(3, 2, -1))) < 1e-6, `${blasts[1].at.toArray()}`);
  assert.equal(blasts[1].owner, v);
  run(system, 1);
  assert.equal(blasts.length, 2, 'once');
});

test('a walker without a FinalExplosion ends with its ExplosionName; one without a death animation is gone at once', () => {
  const plain = walker({ fx: { dying: { effects: [], interval: 0.4, finalOffset: [0, 0, 0], span: SPAN } } });
  delete plain.v.def.finalExplosion;
  plain.v.damage(5000);
  run(plain.system, SPAN + 0.1);
  assert.deepEqual(plain.blasts.map(b => b.class), ['atst_exp', 'atst_exp'], 'the spider walker\'s twice, as the original game falls back');
  const sudden = walker({ fx: { dying: undefined } });
  sudden.v.damage(5000);
  assert.equal(sudden.v.dying, 0);
  assert.equal(sudden.v.root.visible, false);
  run(sudden.system, 1);
  assert.deepEqual(sudden.blasts.map(b => b.class), ['atst_exp']);
});

test('whoever draws it poses the death clip from the dying clock, holding its last frame', () => {
  const { system, v } = walker();
  v.damage(5000);
  assert.equal(v.present(1), true);
  assert.ok(tilt(v) < 1e-6, 'upright as it is destroyed');
  run(system, SPAN / 2);
  v.present(1);
  // The span counts the clip's frames (as the original game's animation does),
  // so halfway through it the clip is at frame 15.5 of 30.
  const half = (FRAMES / 2 / (FRAMES - 1)) * Math.PI / 2;
  assert.ok(Math.abs(tilt(v) - half) < 0.01, `half fallen at half its span, not ${tilt(v)}`);
  // The entity record carries the remaining death clock in milliseconds.
  const { v: page } = walker();
  const record=captureEntityVehicle(v,{tick:1,classId:1});
  EntityGameState.prototype.dyingHull.call({destroyHull(v){v.alive=false;},presentation:{unbind(){}}},page,record);
  assert.equal(page.alive, false);
  assert.ok(Math.abs(page.dying-v.dying)<=.001);
  page.present(1);
  const clockAngleError = .001 / SPAN * FRAMES / (FRAMES - 1) * Math.PI / 2;
  assert.ok(Math.abs(tilt(page) - tilt(v)) <= clockAngleError + 1e-6, 'the page poses it within the wire clock precision');
  run(system, SPAN / 2 - 0.01);
  v.present(1);
  assert.ok(Math.abs(tilt(v) - Math.PI / 2) < 0.04, `nearly down, not ${tilt(v)}`);
  run(system, 0.02);
  assert.equal(v.present(1), false, 'gone: nothing to draw');
});

test('its pad waits for it to be gone before the next one rolls out', () => {
  const { system, v } = walker({ respawnTime: 0.1 });
  v.damage(5000);
  run(system, 0.3);
  assert.equal(v.alive, false, 'still dying past the respawn time');
  assert.ok(v.dying > 0);
  run(system, SPAN);
  assert.equal(v.alive, true, 'its next life');
  assert.equal(v.dying, 0);
  assert.equal(v.root.visible, true);
});

test('the death clip takes over from the pose it was drawn in alive, not from a snap to its first frame', () => {
  const { system, v } = walker();
  // Drawn alive with its body leaning back (a stride, an aim).
  v.present(1, 1 / 60);
  v.byName.bone_body.quaternion.setFromAxisAngle(X, -0.4);
  v.damage(5000);
  v.present(1);
  assert.ok(Math.abs(tilt(v) - 0.4) < 1e-6, `where it was drawn as it is destroyed, not ${tilt(v)}`);
  run(system, SPAN / 16);
  v.present(1);
  assert.ok(tilt(v) < 0.4 && tilt(v) > 0, `on its way into the clip: ${tilt(v)}`);
  run(system, SPAN / 8);
  v.present(1);
  const later = ((3 * SPAN / 16) / SPAN) * (FRAMES / (FRAMES - 1)) * Math.PI / 2;
  assert.ok(Math.abs(tilt(v) - later) < 0.02, `the clip's own pose once an eighth of it has played: ${tilt(v)}, not ${later}`);
});
