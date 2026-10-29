// Optional retail regression: exercise the actual rig and gait, not just a
// moving collider. Retail geometry/animation stays ignored under public/.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { packWith } from '../tools/vehicle-packs.mjs';
// Hoth's pack with the tauntaun (its battle's own, vehicles/<mission>/).
const pack = packWith(new URL('../public/maps/', import.meta.url), ['all_walk_tauntaun'], 'hot1');
const dir = pack ? new URL('./', pack) : null;
const available = Boolean(pack);
const binary = name => { const b = fs.readFileSync(new URL(name, dir)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
function fixture(slopeX = 0, slopeZ = 0) {
  const data = JSON.parse(fs.readFileSync(new URL('vehicles.json', dir)));
  const name = 'all_walk_tauntaun';
  const g = new THREE.PlaneGeometry(2000, 2000).rotateX(-Math.PI / 2);
  const positions = g.attributes.position;
  for (let i = 0; i < positions.count; i++) positions.setY(i, slopeX * positions.getX(i) + slopeZ * positions.getZ(i));
  const collision = new CollisionWorld(positions.array, g.index.array, new Uint32Array(g.index.count / 3));
  const system = new Vehicles({scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision,
    data: {...data, blockers: null, spawns: [{name: 'test', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: {1: name}}]},
    buffer: binary('models.bin'), indexBuffer: binary('models_index.bin'), animationBuffer: binary('animations.bin'),
    texture: () => null, effects: {play() {return null}}, ordnance: {explosion() {}}});
  return {system, v: system.vehicles[0], normal: new THREE.Vector3(-slopeX, 1, -slopeZ).normalize()};
}

test('tauntaun idle feet stay planted throughout breathing, on flat and sloped ground', {skip: !available}, () => {
  for (const [x, z] of [[0, 0], [.2, -.12]]) {
    const {system, v, normal} = fixture(x, z), point = new THREE.Vector3();
    const ranges = [[], []];
    for (let i = 0; i < 480; i++) {
      system.update(1 / 60);
      ['bone_l_pad', 'bone_r_pad'].forEach((name, side) => {
        v.byName[name].getWorldPosition(point);
        ranges[side].push(point.dot(normal));
      });
    }
    for (const range of ranges) {
      // Native pads have a 7cm local offset above their sole. Breathing must
      // not lift/lower the feet as averaging reference + animated roots did.
      assert.ok(Math.min(...range) > .06, String(Math.min(...range)));
      assert.ok(Math.max(...range) < .08, String(Math.max(...range)));
      assert.ok(Math.max(...range) - Math.min(...range) < .008);
    }
    assert.ok(new THREE.Vector3(0, 1, 0).applyQuaternion(v.root.quaternion).dot(normal) > .9999);
    system.dispose();
  }
});

test('tauntaun runs at its authored cadence, reverses its gait, and stops without sliding', {skip: !available}, () => {
  const distances = [];
  for (const rate of [30, 60, 144]) {
    const {system, v} = fixture(); v.driver = {};
    const clip = v.clips.get('walk_leftfoot_rightfoot');
    // The original game's pace: the stride over the clip's frames at 30 fps.
    assert.ok(Math.abs(v.speed - clip.stride * 30 / clip.frames) < .001);
    let cycles = 0, previous = v.walkPhase, planted = null, contactSamples = 0;
    for (let i = 0; i < rate * 3; i++) {
      system.update(1 / rate, v, {forward: 1});
      if (v.walkPhase < previous) cycles++;
      previous = v.walkPhase;
      // During the middle of a half-stride, the supporting pad must stay
      // still in world space while the body passes over it.
      const halfPhase = v.walkPhase % clip.duration;
      if (i > rate && halfPhase > .105 && halfPhase < .195) {
        const side = v.walkPhase < clip.duration ? 'l' : 'r';
        const point = v.byName[`bone_${side}_pad`].getWorldPosition(new THREE.Vector3());
        assert.ok(point.y > .025 && point.y < .09, `running pad y=${point.y}`);
        if (planted?.side === side) assert.ok(point.distanceTo(planted.point) < .06, 'supporting foot must not skate');
        planted = {side, point}; contactSamples++;
      } else planted = null;
    }
    assert.ok(cycles >= 3 && cycles <= 4, `cycles=${cycles}`);
    assert.ok(contactSamples > 5);
    assert.ok(v.position.z > 55, `travel=${v.position.z}`);
    distances.push(v.position.z);
    for (let i = 0; i < rate * 2; i++) system.update(1 / rate, v, {forward: -1});
    const phase = v.walkPhase;
    system.update(1 / rate, v, {forward: -1});
    const duration = clip.duration + v.clips.get('walk_rightfoot_leftfoot').duration;
    const backwards = (phase - v.walkPhase + duration) % duration;
    // Backward is the same gallop played in reverse: under a tick of clip time.
    assert.ok(backwards > 0 && backwards < 1 / rate, `backwards=${backwards}`);
    for (let i = 0; i < rate * 2; i++) system.update(1 / rate, v, {});
    assert.ok(v.velocity.length() < .001); assert.ok(v.walkBlend < .001);
    system.dispose();
  }
  assert.ok(Math.max(...distances) - Math.min(...distances) < 1e-7);
});
