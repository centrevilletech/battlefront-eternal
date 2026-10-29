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
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Turret, scanVector } from '../src/props.js';
import { TurretSeats, turretAimPoint } from '../src/seats.js';
import { Gunner, TurretCamera, previewSteer } from '../src/gunner.js';
import { blankIntent } from '../src/units.js';
import { VehicleAI } from '../src/ai/classic/vehicles/index.js';

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
  for (let i = 0; i < 120; i++) t.steer(1 / 60, 0, -16);
  near(t.pitch, -35 * DEG, 1e-9, 'held 35 degrees down');
  for (let i = 0; i < 120; i++) t.steer(1 / 60, 0, 16);
  near(t.pitch, 10 * DEG, 1e-9, 'held 10 degrees up');
  // The gun's own AimerPitchLimits (up positive: 10 down to 35 up) are
  // widened to the view's, so it reaches 35 degrees down too.
  const tower = emplacement({ settings: { PitchLimits: '-10.0 35.0', AimerPitchLimits: '-10.0 35.0' } });
  near(tower.gunPitchLimits[0], -35 * DEG, 1e-9, 'gun lowest');
  near(tower.gunPitchLimits[1], 35 * DEG, 1e-9, 'gun highest');
});

test('a gun held at its limits short of the aim point still sends its rounds there', () => {
  // A gun that never yaws (the AAA's and dish turret's AimerYawLimits 0 0)
  // and pitches only 5 degrees, aimed at a point well off to the side and
  // above: it stands at its limits, and the round goes from the fire point
  // straight at the point; while it is still turning, along the gun.
  const t = emplacement({ facing: 0, at: [0, 0, 0], settings: { AimerYawLimits: '0 0', AimerPitchLimits: '-5 5', PitchLimits: '-5 5' } });
  t.present();
  const target = new THREE.Vector3(3, 6, 10);
  for (let i = 0; i < 120; i++) t.pose(1 / 60, target);
  near(t.gunYaw, 0, 1e-12, 'held at its yaw');
  near(t.gunPitch, 5 * DEG, 1e-12, 'held at its pitch');
  const from = t.muzzle(new THREE.Vector3()), along = t.muzzle(new THREE.Vector3(), new THREE.Vector3()) && new THREE.Vector3(0, 0, 1).transformDirection(t.gun.matrixWorld);
  const dir = t.fireDirection(from, target, along, new THREE.Vector3());
  assert.ok(dir.angleTo(target.clone().sub(from)) < 1e-9, 'straight at the aim point');
  t.gunPitch = -5 * DEG; t.present();
  const turning = new THREE.Vector3(0, 0, 1).transformDirection(t.gun.matrixWorld);
  assert.ok(t.fireDirection(from, target, turning, new THREE.Vector3()).angleTo(turning) < 1e-9, 'still turning: along the gun');
});

test('YawLimits hold the barge turret\'s aim to +-70 degrees of its heading; a full circle turns freely', () => {
  const barge = emplacement({ settings: { YawLimits: '-70.0 70.0' } });
  assert.deepEqual(barge.yawLimits.map((v) => +(v / DEG).toFixed(6)), [-70, 70]);
  for (let i = 0; i < 300; i++) barge.steer(1 / 60, 16, 0);
  near(barge.yaw, 70 * DEG, 1e-9, 'held at its left limit');
  for (let i = 0; i < 600; i++) barge.steer(1 / 60, -16, 0);
  near(barge.yaw, -70 * DEG, 1e-9, 'and its right');
  // The page's look preview holds the same arc.
  const state = previewSteer(barge, { yaw: barge.yaw, pitch: 0, yawVel: 0, pitchVel: 0 }, [{ look: [[400, 0]], mouseScale: 1 }, { look: [[400, 0]], mouseScale: 1 }], null, 1, false);
  near(state.yaw, -70 * DEG, 1e-9, 'previewed inside the arc');
  const free = emplacement({ settings: { YawLimits: '-180.0 180.0' } });
  assert.equal(free.yawLimits, null);
  for (let i = 0; i < 300; i++) free.steer(1 / 60, 16, 0);
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
    t.present();
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

test('a bot engages what its placed turret can aim at: the Yavin tower 35 degrees down, not up; the barge within its arc', () => {
  const ai = new VehicleAI({ world: {} });
  const tower = emplacement({ facing: 0, at: [0, 0, 0], settings: { PitchLimits: '-10.0 35.0', AimerPitchLimits: '-10.0 35.0' } });
  const pivot = tower.gun.getWorldPosition(new THREE.Vector3());
  const at = (yawDeg, pitchDeg) => pivot.clone().add(new THREE.Vector3(Math.sin(yawDeg * DEG) * Math.cos(pitchDeg * DEG), Math.sin(pitchDeg * DEG), Math.cos(yawDeg * DEG) * Math.cos(pitchDeg * DEG)).multiplyScalar(40));
  assert.equal(ai.bodyReach(tower, at(0, -30)), true, '30 degrees down');
  assert.equal(ai.bodyReach(tower, at(0, 30)), false, '30 degrees up');
  assert.equal(ai.bodyReach(tower, at(0, 8)), true, '8 degrees up');
  const barge = emplacement({ facing: 0, at: [0, 0, 0], settings: { YawLimits: '-70.0 70.0' } });
  assert.equal(ai.bodyReach(barge, at(60, 0)), true, 'inside the arc');
  assert.equal(ai.bodyReach(barge, at(100, 0)), false, 'outside it');
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
  gunner.updateCamera(1 / 60, 1, false);
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

test('the page\'s Gunner and the server\'s seats turn the gun to the same point under the crosshair, and rounds meet it', () => {
  for (const firstPerson of [false, true]) {
    const g = new THREE.BoxGeometry(400, 1, 400).translate(0, -0.5, 0);
    const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
    // A wall 12 m out along the turret's heading.
    collision.addBody(...box(0, 3, 12, 20, 8, 0.5));
    const fired = [];
    const ordnance = { units: null, fire: (def, origin, dir) => { fired.push({ origin: origin.clone(), dir: dir.clone() }); return null; } };
    const weapon = { def: { ordnance: { base: 'bolt' } }, update: (dt, a, trigger) => (trigger.held ? [{ yaw: 0, pitch: 0 }] : []) };
    // The server: TurretSeats driving a network human.
    const served = emplacement({ facing: 0, at: [0, 0, 0], collision });
    served.weapon = weapon;
    const seats = new TurretSeats({ live: { turrets: [served] }, collision, ordnance });
    const unit = person(firstPerson);
    assert.ok(seats.man(unit, served));
    unit.intent.turn = 0.05; unit.intent.pitch = -0.02;
    for (let i = 0; i < 60; i++) seats.update(1 / 60);
    // Let go, the aim coasts to a stop and the gun settles on it; then fire.
    unit.intent.turn = 0; unit.intent.pitch = 0;
    for (let i = 0; i < 60; i++) seats.update(1 / 60);
    unit.intent.buttons = [1, 0];
    for (let i = 0; i < 30; i++) seats.update(1 / 60);
    const serverTarget = seats.target.clone();
    // The page: a Gunner on a turret in the same state.
    const local = emplacement({ facing: 0, at: [0, 0, 0], collision });
    local.weapon = weapon;
    Object.assign(local, { yaw: served.yaw, pitch: served.pitch, gunYaw: served.gunYaw, gunPitch: served.gunPitch });
    const camera = new THREE.PerspectiveCamera(); camera.aspect = 16 / 9;
    const gunner = new Gunner({ camera, collision, ordnance, combat: null, animator: { gunner() {}, clips: new Map() } });
    gunner.aimCamera = new THREE.PerspectiveCamera();
    gunner.turret = local;
    gunner.updateCamera(1 / 60, 1, firstPerson, true);
    const soldier = person(firstPerson).soldier;
    gunner.update(1 / 60, soldier, { take: () => false, held: () => false });
    nearV(gunner.target, serverTarget, 1e-4, `${firstPerson ? 'first' : 'third'} person: the same aim point`);
    assert.ok(Math.abs(serverTarget.z - 11.75) < 1e-3 && served.yaw < -1e-3, `turned, and on the wall (${serverTarget.z.toFixed(3)})`);
    // The crosshair's ray: from the page's drawn camera along the aim.
    gunner.updateCamera(1 / 60, 1, firstPerson);
    const aim = gunner.viewDirection(new THREE.Vector3());
    const v = serverTarget.clone().sub(camera.position);
    const off = v.clone().addScaledVector(aim, -v.dot(aim)).length();
    assert.ok(off < 1e-4, `${firstPerson ? 'first' : 'third'} person: under the crosshair (${off})`);
    // Every round from the fire point goes through it.
    assert.ok(fired.length > 0, 'fired');
    for (const round of fired) {
      const w = serverTarget.clone().sub(round.origin);
      assert.ok(w.clone().addScaledVector(round.dir, -w.dot(round.dir)).length() < 1e-4, 'the round meets the aim point');
    }
  }
});

// Every placed turret class, imported (BFE_CONTENT_DIR, or public/), in the
// headless world the worker and the server run: its chase camera rests
// outside the turret, above its drum; and, level and at the lowest and
// highest pitch its view takes, in either view, its gun turns toward the
// point the server's aim met for a network human, which lies on the page's
// crosshair ray (the camera at rest), and his rounds go through it.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
const CLASS_MAPS = ['hot1', 'yav1', 'tat2', 'bes1', 'geo1'];
test('imported: every turret class frames its gun and puts its rounds under the crosshair', { skip: !CLASS_MAPS.every((m) => fs.existsSync(path.join(CONTENT, `maps/${m}/props.json`))) && 'no extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const seen = new Set();
  for (const map of CLASS_MAPS) {
    const game = await headlessGame(`map=${map}&side=imp&ai=off`);
    await game.ready;
    const T = game.THREE;
    for (const turret of game.live.turrets) {
      const cls = turret.desc.class;
      if (seen.has(cls)) continue;
      seen.add(cls);
      if (!turret.alive) turret.setHealth(turret.maxHealth);
      turret.yaw = turret.pitch = turret.yawVel = turret.pitchVel = 0;
      // The chase camera at rest: outside the turret's box, no lower than
      // the drum's pivot less a metre, looking at the turret's side of it.
      const rest = turret.cameraRest(new T.Vector3());
      const pivot = turret.pivot.clone().applyMatrix4(turret.rig.root.matrixWorld);
      assert.ok(!turret.encloses(rest), `${cls}: camera outside the turret`);
      assert.ok(rest.y > pivot.y - 1, `${cls}: camera ${rest.y.toFixed(2)} not below the drum ${pivot.y.toFixed(2)}`);
      // Manned by a network human (the server's seat path), level, in each
      // view: the gun settles along the aim and every round meets the aim
      // point, which the crosshair's ray from the resting camera crosses.
      const fired = [];
      const ordnance = game.turretSeats.ordnance;
      const fire = ordnance.fire;
      ordnance.fire = (def, origin, dir, ...rest) => { fired.push({ origin: origin.clone(), dir: dir.clone() }); return fire.call(ordnance, def, origin, dir, ...rest); };
      try {
        for (const firstPerson of [false, true]) for (const extreme of ['level', 'low', 'high']) {
          const view = `${cls} ${firstPerson ? 'first' : 'third'} ${extreme}`;
          const pitch = extreme === 'low' ? turret.pitchLimits[0] : extreme === 'high' ? turret.pitchLimits[1] : 0;
          const unit = { id: 90000 + seen.size, generation: 1, team: 1, alive: true, dead: false, human: true, networkHuman: true, seat: null, walker: false, classDef: {},
            intent: blankIntent(), combat: { firstPerson, zoom: 1 },
            soldier: { position: new T.Vector3(), previous: new T.Vector3(), settled: new T.Vector3(), velocity: new T.Vector3(), aimYaw: 0, aimPitch: 0 } };
          assert.ok(game.turretSeats.man(unit, turret), cls);
          turret.yaw = turret.yawVel = turret.pitchVel = 0;
          turret.pitch = pitch;
          for (let i = 0; i < 120; i++) game.turretSeats.update(1 / 60);
          fired.length = 0;
          unit.intent.buttons = [1, 0];
          for (let i = 0; i < 90; i++) { game.turretSeats.update(1 / 60); if (i === 30) unit.intent.buttons = [0, 0]; if (i === 50) unit.intent.buttons = [1, 0]; }
          const target = game.turretSeats.target.clone();
          const aim = turret.viewDirection(new T.Vector3());
          const from = firstPerson ? turret.eyePoint(new T.Vector3()) : turret.cameraRest(new T.Vector3());
          const v = target.clone().sub(from);
          assert.ok(v.clone().addScaledVector(aim, -v.dot(aim)).length() < 1e-3, `${view}: aim point on the crosshair ray`);
          const gunPivot = turret.gun.getWorldPosition(new T.Vector3());
          const along = new T.Vector3(0, 0, 1).transformDirection(turret.gun.matrixWorld);
          // The gun turned toward it (widened to the view's pitch, it reaches
          // every extreme; the guns that never yaw stand at their limit).
          assert.ok(along.angleTo(target.clone().sub(gunPivot)) < 0.35, `${view}: the gun points at the aim point (${(along.angleTo(target.clone().sub(gunPivot)) / (Math.PI / 180)).toFixed(1)} degrees off)`);
          assert.ok(fired.length > 0, `${view}: fired`);
          // Within the weapon's own spread (the minigun's 0.1 degree).
          const spread = (Math.max(Number(turret.weapon?.def?.YawSpread) || 0, Number(turret.weapon?.def?.PitchSpread) || 0) * Math.SQRT2 + 1e-3) * Math.PI / 180;
          for (const round of fired.slice(-1)) {
            const miss = round.dir.angleTo(target.clone().sub(round.origin));
            assert.ok(miss <= spread, `${view}: the round meets the aim point (${(miss * 180 / Math.PI).toFixed(4)} degrees off)`);
          }
          game.turretSeats.leave(unit, { placement: false });
        }
      } finally {
        ordnance.fire = fire;
      }
    }
    game.dispose?.();
  }
  for (const cls of ['com_weap_gunturret', 'bes_bldg_AAA_turret', 'geo_bldg_geoturret', 'hoth_bldg_hoth_turret', 'hoth_bldg_dish_turret', 'tat_bldg_barge_turret', 'yav_bldg_tower_turret']) assert.ok(seen.has(cls), `${cls} checked`);
});
