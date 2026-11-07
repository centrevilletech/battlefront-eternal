import {TurretCamera, presentGunner} from './helpers/cameras.js';
import {presentTurret, pageProps, local} from './helpers/page-presentation.js';
import { contentPath } from './helpers/content.js';
import { stepTurretPreview } from './helpers/network-preview.js';
// The views and aim of a manned turret (src/props.js Turret, src/gunner.js
// Gunner and TurretCamera, src/seats.js TurretSeats): each class's camera
// section as the original game reads it, its aim limits, the drum posed along
// the aim, and the page's and the server's aims meeting under the
// crosshair. Synthetic turrets first (no retail content), then every placed
// turret class when an extract is at hand (BFE_CONTENT_DIR, or public/).
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { makeHit } from '../src/collision.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Turret, scanVector } from '../src/props.js';
import { TurretSeats, turretAimPoint } from '../src/seats.js';
import { Gunner } from '../src/gunner.js';
import {previewSteer} from './helpers/network-preview.js';
import { blankIntent } from '../src/units.js';

const DEG = Math.PI / 180;
const near = (a, b, eps, message) => assert.ok(Math.abs(a - b) <= eps, `${message}: ${a} vs ${b}`);
const nearV = (a, b, eps, message) => assert.ok(a.distanceTo(b) <= eps, `${message}: ${a.toArray().map((v) => v.toFixed(3))} vs ${b.toArray().map((v) => v.toFixed(3))}`);
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return [g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3)];
};

// A turret at `at` facing `facing`: a drum 0.6 m up (its bind turned by
// drumTurn about y), a gun 1 m above it and 0.7 m forward with a fire point
// 1.2 m along it, and the class keys given (over a minigun turret's).
function emplacement({ facing = 0.4, at = [5, 0, -3], drumTurn = 0, settings = {}, collision = null } = {}) {
  const root = new THREE.Group();
  const byName = {};
  for (const name of ['drum', 'gun', 'hp_fire', 'seat']) { byName[name] = new THREE.Object3D(); byName[name].name = name; }
  root.add(byName.drum); byName.drum.add(byName.gun); byName.gun.add(byName.hp_fire); byName.drum.add(byName.seat);
  byName.drum.position.set(0, 0.6, 0);
  byName.drum.quaternion.setFromAxisAngle(new THREE.Vector3(0, 1, 0), drumTurn);
  // The gun sits ahead of the drum's own +z, whichever way the bind turns it.
  byName.gun.position.set(0, 1, 0.7);
  byName.hp_fire.position.set(0, 0, 1.2);
  byName.seat.position.set(0, -0.3, -0.4);
  const matrix = new THREE.Matrix4().compose(new THREE.Vector3(...at), new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), facing), new THREE.Vector3(1, 1, 1));
  root.matrix.copy(matrix); root.matrix.decompose(root.position, root.quaternion, root.scale);
  root.updateMatrixWorld(true);
  const desc = { base: 'armedbuilding', class: 'test_turret', matrix: matrix.toArray(), weapon: null,
    settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', FirePointName: 'hp_fire', PilotPosition: 'seat', MaxTurnSpeed: 2, MaxHealth: 300,
      PitchLimits: '-45 45', PitchTurnFactor: 0, AimerPitchLimits: '-45 45', AimerYawLimits: '-5 5',
      EyePointOffset: '0 1.5 1', TrackCenter: '0 1.5 -2', TrackOffset: '0 0.5 1.5', TiltValue: 5, ...settings } };
  const bodies = {};
  const turret = new Turret(desc, { built: { root, byName, model: { box: [[-1, 0, -1], [1, 2, 1]] } } }, bodies, null);
  if (collision) {
    const g = new THREE.BoxGeometry(1.6, 1, 1.6).translate(0, 0.5, 0).applyMatrix4(matrix);
    bodies.built = collision.addBody(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3), turret);
  }
  turret.show(turret.state);
  return turret;
}

test('vector keys read as the original game\'s sscanf "%f %f %f" reads them', () => {
  assert.deepEqual(scanVector('0.0 1.5 -2.0'), [0, 1.5, -2]);
  // The Hoth dish turret's EyePointOffset: ".0" is the third float.
  assert.deepEqual(scanVector('0.0 1.5.0 -2.0'), [0, 1.5, 0]);
  assert.deepEqual(scanVector('1 2'), [1, 2, 0]);
  assert.deepEqual(scanVector(['9 9 9', '0 1.2 4']), [0, 1.2, 4], 'a repeated key keeps its last value');
  assert.equal(scanVector('x 1 2'), null);
  assert.equal(scanVector(undefined), null);
});

test('the camera section: EyePointOffset sets TrackCenter too, unset keys are 0, both views 48 degrees', () => {
  // The Hoth turret: an eye, a TrackOffset, no TrackCenter, no TiltValue.
  const hoth = emplacement({ settings: { EyePointOffset: '0.0 0.0 0.0', TrackCenter: undefined, TrackOffset: '0.0 1.2 4.0', TiltValue: undefined } });
  assert.deepEqual(hoth.camera.center.toArray(), [0, 0, 0], 'pivots on its eye');
  assert.deepEqual(hoth.camera.offset.toArray(), [0, 1.2, 4]);
  assert.equal(hoth.camera.tilt, 0, 'no tilt unless set');
  near(hoth.camera.thirdFov, 48 * DEG, 1e-12, 'third person FOV');
  near(hoth.camera.firstFov, 48 * DEG, 1e-12, 'first person FOV');
  // The Yavin tower: TrackCenter is its eye.
  const tower = emplacement({ settings: { EyePointOffset: '0.0 3.0 0.5', TrackCenter: undefined, TrackOffset: '0.0 -1.0 3.5' } });
  assert.deepEqual(tower.camera.center.toArray(), [0, 3, 0.5]);
  // The gun turret: a TrackCenter after the eye wins; one before it loses.
  const gun = emplacement();
  assert.deepEqual(gun.camera.eye.toArray(), [0, 1.5, 1]);
  assert.deepEqual(gun.camera.center.toArray(), [0, 1.5, -2]);
  near(gun.camera.tilt, 5 * DEG, 1e-12, 'its TiltValue');
  const before = emplacement();
  const { EyePointOffset, ...rest } = before.desc.settings;
  before.desc.settings = { ...rest, TrackCenter: '0 9 9', EyePointOffset: '0 2 0' };
  const reread = new Turret(before.desc, before.looks, before.bodies, null);
  assert.deepEqual(reread.camera.center.toArray(), [0, 2, 0], 'an eye after TrackCenter sets it again');
  // Without any camera keys the camera sits on the drum's pivot.
  const bare = emplacement({ settings: { EyePointOffset: undefined, TrackCenter: undefined, TrackOffset: undefined, TiltValue: undefined } });
  assert.deepEqual([bare.camera.eye, bare.camera.center, bare.camera.offset].map((v) => v.toArray()), [[0, 0, 0], [0, 0, 0], [0, 0, 0]]);
});

test('PitchLimits are down positive: the Yavin tower looks 10 degrees up and 35 down', () => {
  const t = emplacement({ settings: { PitchLimits: '-10.0 35.0' } });
  near(t.pitchLimits[0], -35 * DEG, 1e-9, 'lowest (up positive)');
  near(t.pitchLimits[1], 10 * DEG, 1e-9, 'highest');
  for (let i = 0; i < 120; i++) stepTurretPreview(t, 1 / 60, 0, -16);
  near(t.pitch, -35 * DEG, 1e-9, 'held 35 degrees down');
  for (let i = 0; i < 120; i++) stepTurretPreview(t, 1 / 60, 0, 16);
  near(t.pitch, 10 * DEG, 1e-9, 'held 10 degrees up');
  // The gun's own AimerPitchLimits (up positive: 10 down to 35 up) are
  // widened to the view's, so it reaches 35 degrees down too.
  const tower = emplacement({ settings: { PitchLimits: '-10.0 35.0', AimerPitchLimits: '-10.0 35.0' } });
  near(tower.gunPitchLimits[0], -35 * DEG, 1e-9, 'gun lowest');
  near(tower.gunPitchLimits[1], 35 * DEG, 1e-9, 'gun highest');
});



test('YawLimits hold the barge turret\'s aim to +-70 degrees of its heading; a full circle turns freely', () => {
  const barge = emplacement({ settings: { YawLimits: '-70.0 70.0' } });
  assert.deepEqual(barge.yawLimits.map((v) => +(v / DEG).toFixed(6)), [-70, 70]);
  for (let i = 0; i < 300; i++) stepTurretPreview(barge, 1 / 60, 16, 0);
  near(barge.yaw, 70 * DEG, 1e-9, 'held at its left limit');
  for (let i = 0; i < 600; i++) stepTurretPreview(barge, 1 / 60, -16, 0);
  near(barge.yaw, -70 * DEG, 1e-9, 'and its right');
  // The page's look preview holds the same arc.
  const state = previewSteer(barge, { yaw: barge.yaw, pitch: 0, yawVel: 0, pitchVel: 0 }, [{ look: [[400, 0]], mouseScale: 1 }, { look: [[400, 0]], mouseScale: 1 }], null, 1, false);
  near(state.yaw, -70 * DEG, 1e-9, 'previewed inside the arc');
  const free = emplacement({ settings: { YawLimits: '-180.0 180.0' } });
  assert.equal(free.yawLimits, null);
  for (let i = 0; i < 300; i++) stepTurretPreview(free, 1 / 60, 16, 0);
  assert.ok(Math.abs(free.yaw) <= Math.PI, 'wraps');
  // An arc round the back is kept in 0..360.
  const back = emplacement({ settings: { YawLimits: '120 240' } });
  assert.equal(back.wrapYaw, true);
  near(back.holdYaw(-170 * DEG), 190 * DEG, 1e-9, 'behind it, inside the arc');
  near(back.holdYaw(10 * DEG), 120 * DEG, 1e-9, 'held to the arc');
});

test('the drum is posed along the aim from its pivot, whatever its bind turn (the barge turret\'s half turn)', () => {
  for (const drumTurn of [0, Math.PI]) {
    const t = emplacement({ drumTurn });
    t.yaw = 0.3;
    t.gunYaw = t.gunPitch = 0;
    presentTurret(t);
    t.rig.root.updateMatrixWorld(true);
    const along = new THREE.Vector3(0, 0, 1).transformDirection(t.gun.matrixWorld);
    const aim = t.viewDirection(new THREE.Vector3());
    assert.ok(along.angleTo(aim) < 1e-6, `bind turn ${drumTurn}: the gun points along the aim`);
    const muzzle = t.muzzle(new THREE.Vector3());
    const pivot = t.drum.getWorldPosition(new THREE.Vector3());
    assert.ok(muzzle.clone().sub(pivot).dot(aim) > 1, 'its fire point ahead of the drum');
  }
});

test('the aim frame: eye, chase camera rest and where the aim starts', () => {
  // The Hoth turret's section: the eye on the drum's pivot, the camera 1.2 m
  // above it and 4 m back.
  const t = emplacement({ facing: 0.4, settings: { EyePointOffset: '0 0 0', TrackCenter: undefined, TrackOffset: '0 1.2 4', TiltValue: undefined } });
  t.yaw = -0.2; t.pitch = 0;
  const pivot = new THREE.Vector3(5, 0.6, -3);
  const aim = t.viewDirection(new THREE.Vector3());
  nearV(t.eyePoint(new THREE.Vector3()), pivot, 1e-9, 'eye on the pivot');
  const rest = t.cameraRest(new THREE.Vector3());
  nearV(rest, pivot.clone().addScaledVector(aim, -4).add(new THREE.Vector3(0, 1.2, 0)), 1e-9, 'rest');
  nearV(t.aimOrigin(new THREE.Vector3(), false), pivot.clone().add(new THREE.Vector3(0, 1.2, 0)), 1e-9, 'brought forward to the eye\'s plane');
  nearV(t.aimOrigin(new THREE.Vector3(), true), pivot, 1e-9, 'first person: the eye');
  // TrackCenter turns and tips with the aim.
  const gun = emplacement({ facing: 0 });
  gun.yaw = 0; gun.pitch = 30 * DEG;
  const up = new THREE.Vector3(0, Math.cos(gun.pitch), -Math.sin(gun.pitch));
  const fwd = gun.viewDirection(new THREE.Vector3());
  const centre = new THREE.Vector3(5, 0.6, -3).addScaledVector(up, 1.5).addScaledVector(fwd, -2);
  const pivotOut = new THREE.Vector3();
  gun.cameraRest(new THREE.Vector3(), gun.yaw, gun.pitch, pivotOut);
  nearV(pivotOut, centre, 1e-9, 'TrackCenter in the pitched aim frame');
  // The camera looks along the aim, tipped down by the tilt.
  const q = gun.cameraTurn(new THREE.Quaternion());
  const look = new THREE.Vector3(0, 0, -1).applyQuaternion(q);
  near(look.angleTo(fwd), 5 * DEG, 1e-9, 'looks 5 degrees under the aim');
  assert.ok(look.y < fwd.y, 'below it');
});

test('TurretCamera: rests where the section puts it, pulled in by a wall, and in first person at the eye', () => {
  const g = new THREE.BoxGeometry(400, 1, 400).translate(0, -0.5, 0);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const t = emplacement({ facing: 0, at: [0, 0, 0], collision });
  const camera = new THREE.PerspectiveCamera(); camera.aspect = 16 / 9;
  const view = new TurretCamera(camera, collision, t);
  view.update(1 / 60, t, 0, 0, false);
  nearV(camera.position, t.cameraRest(new THREE.Vector3()), 1e-6, 'at its rest');
  near(THREE.MathUtils.degToRad(camera.fov), 2 * Math.atan(Math.tan(24 * DEG) / (16 / 9)), 1e-9, 'vertical FOV from 48 across');
  // A wall 2 m behind the turret: the camera comes in in front of it.
  const wall = collision.addBody(...box(0, 2, -2.2, 10, 6, 0.2));
  view.update(1 / 60, t, 0, 0, false);
  assert.ok(camera.position.z > -2.1, `in front of the wall (${camera.position.z.toFixed(2)})`);
  collision.removeBody(wall);
  for (let i = 0; i < 120; i++) view.update(1 / 60, t, 0, 0, false);
  nearV(camera.position, t.cameraRest(new THREE.Vector3()), 1e-3, 'eased back out');
  view.update(1 / 60, t, 0.2, -0.1, true);
  nearV(camera.position, t.eyePoint(new THREE.Vector3(), 0.2, -0.1), 1e-9, 'first person at the eye');
  assert.ok(new THREE.Vector3(0, 0, -1).applyQuaternion(camera.quaternion).angleTo(t.viewDirection(new THREE.Vector3(), 0.2, -0.1)) < 1e-6, 'straight down the aim');
});

test('the reticle stays on the aim point while a wall pulls the chase camera in', () => {
  const g = new THREE.BoxGeometry(400, 1, 400).translate(0, -0.5, 0);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  collision.addBody(...box(0, 3, 8, 20, 8, 0.5)); // what it aims at, 8 m out
  collision.addBody(...box(0, 2, -2.2, 10, 6, 0.2)); // a wall behind it
  const t = emplacement({ facing: 0, at: [0, 0, 0], collision });
  t.pitch = 0.2;
  const camera = new THREE.PerspectiveCamera(); camera.aspect = 16 / 9;
  const gunner = new Gunner({ camera, collision, ordnance: null, combat: null, animator: { gunner() {}, clips: new Map() } });
  gunner.turret = t;
  presentGunner(gunner, 1 / 60, 1, false);
  assert.ok(camera.position.distanceTo(t.cameraRest(new THREE.Vector3())) > 0.3, 'pulled in off its rest');
  const point = turretAimPoint(t, false, t.viewDirection(new THREE.Vector3()), collision, null, null, new THREE.Vector3(), { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() });
  const dir = gunner.reticleDirection(new THREE.Vector3());
  const v = point.clone().sub(camera.position);
  assert.ok(v.clone().addScaledVector(dir, -v.dot(dir)).length() < 1e-4, 'the reticle\'s ray goes through the aim point');
  const along = gunner.viewDirection(new THREE.Vector3());
  assert.ok(v.clone().addScaledVector(along, -v.dot(along)).length() > 0.05, 'which the aim\'s own direction from the camera would miss');
});

// A network human at the gun, as the server's seats see him.
function person(firstPerson) {
  return { id: 7, generation: 1, team: 1, alive: true, dead: false, human: true, networkHuman: true, seat: null, walker: false, classDef: {},
    intent: blankIntent(), combat: { firstPerson, zoom: 1 },
    soldier: { position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0 } };
}



// Every placed turret class, imported (BFE_CONTENT_DIR, or public/), in the
// headless world the worker and the server run: its chase camera rests
// outside the turret, above its drum; and, level and at the lowest and
// highest pitch its view takes, in either view, its gun turns toward the
// point the server's aim met for a network human, which lies on the page's
// crosshair ray (the camera at rest), and his rounds go through it.
const CONTENT = contentPath();
const CLASS_MAPS = ['hot1', 'yav1', 'tat2', 'bes1', 'geo1'];
