import { VehicleCamera } from './helpers/cameras.js';
import './fixtures/native-anim.js';
import {bodyHost} from './helpers/presentation.js';
import { contentURL } from './helpers/content.js';
// Vehicle seats as the 2004 game reads them (docs/formats.md "Vehicle
// seats"): who shows as a rider, the turret arcs, where the guns aim and fire
// from, and the seat cameras. Optional: uses the extracted vehicle packs under
// public/maps when present; retail data stays local and ignored.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld, nativeVehicles } from './helpers/native-vehicles.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
import { packWith, vehiclePacks } from '../tools/vehicle-packs.mjs';
import { attachPage } from './helpers/page-presentation.js';

const DEG = Math.PI / 180;
const maps = contentURL('maps/');
const has = (...list) => list.every((map) => vehiclePacks(maps, map).length);
const binary = (file) => { const b = fs.readFileSync(file); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const near = (a, b, eps, message) => assert.ok(Math.abs(a - b) <= eps, `${message}: ${a} vs ${b}`);

// The classes of one map's pack on a flat test floor, 100 m apart, heading
// yaw.
function hulls(map, names, yaw = 0) {
  // Each battle's pack holds its own era's vehicles: the map's that has the
  // classes, else any other prepared pack with a copy of them.
  const file = packWith(maps, names, map) || packWith(maps, names);
  assert.ok(file, `Missing extracted classes: ${names.join(', ')}`);
  const data = JSON.parse(fs.readFileSync(file, 'utf8'));
  const g = new THREE.BoxGeometry(20000, 2, 20000).translate(0, -1, 0);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const fired = [];
  const ordnance = { fire: (def, origin, dir, owner, visual, ignoreBody, emitter) => { fired.push({ def, origin: origin.clone(), dir: dir.clone(), owner, emitter }); }, explosion() {}, lights: { flash() {} } };
  const spawns = names.map((name, i) => ({ name, matrix: new THREE.Matrix4().makeRotationY(yaw).setPosition(i * 100, 0, 0).toArray(), team: 1, post: '', respawnTime: 45, classes: { 1: name } }));
  const dir = new URL('./', file);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision, data: { ...data, spawns, blockers: null },
    buffer: binary(new URL('models.bin', dir)), indexBuffer: binary(new URL('models_index.bin', dir)), animationBuffer: binary(new URL('animations.bin', dir)),
    texture: () => null, ordnance, effects: { play: () => null } });
  const byName = Object.fromEntries(names.map((name) => [name, system.vehicles.find((v) => v.name === name)]));
  return { system, collision, fired, pack: { data: system.data, buffer: binary(new URL('models.bin', dir)), indexBuffer: binary(new URL('models_index.bin', dir)), animationBuffer: binary(new URL('animations.bin', dir)) }, ...byName };
}

test('a rider shows exactly where its seat has a pilot animation', { skip: !has('nab2', 'geo1', 'hot1') }, () => {
  const { rep_hover_fightertank: fightertank, cis_hover_aat: aat } = hulls('nab2', ['rep_hover_fightertank', 'cis_hover_aat']);
  assert.equal(fightertank.seats[0].view.rider, null, 'the fighter tank pilot is enclosed');
  const gunner = fightertank.seats[1].view.rider;
  assert.equal(gunner.node, fightertank.byName.hp_active, 'the roof gunner stands at hp_active');
  assert.equal(gunner.anim, 'minigun_9pose');
  assert.equal(gunner.ninePose, true);
  assert.ok(aat.seats.every((seat) => seat.view.rider === null), 'the AAT is enclosed');
  const { rep_walk_atte: atte } = hulls('geo1', ['rep_walk_atte']);
  assert.equal(atte.seats[1].view.rider.node, atte.byName.hp_main_gunner);
  assert.equal(atte.seats[0].view.rider, null);
  assert.equal(atte.seats[2].view.rider, null);
  const { all_fly_snowspeeder: speeder, imp_walk_atst_snow: atst, all_walk_tauntaun: tauntaun } = hulls('hot1', ['all_fly_snowspeeder', 'imp_walk_atst_snow', 'all_walk_tauntaun']);
  assert.ok(speeder.seats.every((seat) => seat.view.rider?.node), 'the snowspeeder crew shows through its canopy');
  assert.ok(atst.seats.every((seat) => seat.view.rider === null));
  assert.equal(tauntaun.seats[0].view.rider.node, tauntaun.byName.hp_active);
});

test('creature and skiff riders find their nodes', { skip: !has('nab1', 'tat1') }, () => {
  const { gun_walk_kaadu: kaadu } = hulls('nab1', ['gun_walk_kaadu']);
  assert.equal(kaadu.seats[0].view.rider.node, kaadu.byName.bone_rider, 'the kaadu has no hp_active; its rider node is bone_rider');
  const { tat_hover_skiff: skiff } = hulls('tat1', ['tat_hover_skiff']);
  const nodeName = (node) => Object.keys(skiff.byName).find((k) => skiff.byName[k] === node);
  assert.deepEqual(skiff.seats.map((seat) => nodeName(seat.view.rider?.node)), ['hp_pilot', 'hp_activel', 'hp_active', 'hp_activer']);
});

test('turret arcs keep the original game conventions: turret pitch down positive, yaw from the hull', { skip: !has('nab2', 'geo1', 'hot1', 'tat1') }, () => {
  const { rep_hover_fightertank: tank, cis_hover_aat: aat } = hulls('nab2', ['rep_hover_fightertank', 'cis_hover_aat']);
  // PitchLimits -40 15 on a turret: 40 degrees up to 15 down.
  near(tank.seats[1].view.pitchLimits[0], -15 * DEG, 1e-6, 'fighter tank turret, down');
  near(tank.seats[1].view.pitchLimits[1], 40 * DEG, 1e-6, 'fighter tank turret, up');
  assert.equal(tank.seats[1].view.yawLimits, null, 'a -180..180 arc turns freely');
  // PitchLimits -8 25 on the hull's own seat: up positive.
  near(tank.seats[0].view.pitchLimits[0], -8 * DEG, 1e-6, 'fighter tank pilot, down');
  near(tank.seats[0].view.pitchLimits[1], 25 * DEG, 1e-6, 'fighter tank pilot, up');
  near(aat.seats[1].view.pitchLimits[1], 40 * DEG, 1e-6, 'AAT turret, up');
  assert.equal(aat.seats[1].view.pitchFactor, 0, 'the AAT turret body only turns');
  const { tat_hover_skiff: skiff } = hulls('tat1', ['tat_hover_skiff']);
  skiff.seats[1].aimYaw = skiff.yaw; skiff.clampAim(1);
  near(skiff.seats[1].aimYaw - skiff.yaw, 65 * DEG, 1e-6, 'the left gun stays on its arc');
  skiff.seats[3].aimYaw = skiff.yaw - 150 * DEG; skiff.clampAim(3);
  near(skiff.seats[3].aimYaw - skiff.yaw, -115 * DEG, 1e-6, 'the right gun stays on its arc');
  // An arc round the back (120..240) is held in 0..360.
  const { rep_walk_atte: atte } = hulls('geo1', ['rep_walk_atte']);
  assert.equal(atte.seats[2].view.wrapYaw, true);
  atte.seats[2].aimYaw = atte.yaw + 10 * DEG; atte.clampAim(2);
  near(THREE.MathUtils.euclideanModulo(atte.seats[2].aimYaw - atte.yaw, 2 * Math.PI), 120 * DEG, 1e-6, 'from the front-left to the arc');
  atte.seats[2].aimYaw = atte.yaw - 10 * DEG; atte.clampAim(2);
  near(THREE.MathUtils.euclideanModulo(atte.seats[2].aimYaw - atte.yaw, 2 * Math.PI), 240 * DEG, 1e-6, 'from the front-right to the arc');
  const { all_fly_snowspeeder: speeder } = hulls('hot1', ['all_fly_snowspeeder']);
  speeder.seats[1].pitch = -1; speeder.clampAim(1);
  near(speeder.seats[1].pitch, -25 * DEG, 1e-6, 'the tow cable gun dips 25 degrees at most');
});

test('a turret seat without a node takes its pivot from its section; walker gunners see from the walker\'s head', { skip: !has('hot1', 'bes1') }, () => {
  const { imp_walk_atst_snow: atst, imp_walk_atat: atat } = hulls('hot1', ['imp_walk_atst_snow', 'imp_walk_atat']);
  // The AT-ST gunner's section places its pivot 8 m up (hash f2aa4a90)
  // and its eye on it: at the head, not at the feet.
  const eye = atst.seatEye(1, new THREE.Vector3()).sub(atst.position);
  near(eye.y, 8, 1e-6, 'the AT-ST gunner\'s eye height');
  const frame = atst.seatFrame(1, new THREE.Matrix4());
  const native = new VehicleCamera(new THREE.PerspectiveCamera(), atst.system.collision, true);
  atst.seatIndex=1;native.update(atst,1/60,false);
  const camera = native.camera.position.clone().sub(atst.position);
  assert.ok(camera.y > 8.5 && camera.y < 11, `the AT-ST gunner's chase camera above the head: ${camera.y}`);
  // The AT-AT gunner's section repeats its limits; the second pair holds:
  // 20 degrees up to 15 down, 25 left to 20 right.
  near(atat.seats[1].view.pitchLimits[0], -15 * DEG, 1e-6, 'AT-AT gunner, down');
  near(atat.seats[1].view.pitchLimits[1], 20 * DEG, 1e-6, 'AT-AT gunner, up');
  near(atat.seats[1].view.yawLimits[0], -25 * DEG, 1e-6, 'AT-AT gunner, right');
  near(atat.seats[1].view.yawLimits[1], 20 * DEG, 1e-6, 'AT-AT gunner, left');
  const { bes_fly_cloudcar: car } = hulls('bes1', ['bes_fly_cloudcar']);
  // The cloud car gunner's pivot is 15 m behind; its eye 14 m ahead of it
  // lands by the second pod.
  const carEye = car.seatEye(1, new THREE.Vector3()).sub(car.position);
  assert.ok(carEye.distanceTo(new THREE.Vector3(-1.75, 2.5, -1)) < 1e-6, `the cloud car gunner's eye: ${carEye.toArray()}`);
});











test('a full-circle aimer crosses behind the shorter way', { skip: !has('geo1') }, () => {
  const { system, cis_walk_spider: v } = hulls('geo1', ['cis_walk_spider']);
  const a = v.seats[0].weapons[0].aimers[0];
  assert.equal(a.node, 'turret_y_2');
  // Held, drawn by the page's hull phase at the tick, its aimers turning by
  // each frame's time; with no walk pose, so the aimer's frame stays put.
  attachPage(system);
  v.pose = null; v.driver = {}; v.seatIndex = 0; v.sleeping = false; v.resetPresentationHistory();
  v.present(1, 0);
  const pivot = a.bone.getWorldPosition(new THREE.Vector3());
  const aimAt = (deg) => { v.seats[0].target = pivot.clone().add(new THREE.Vector3(Math.sin(deg * DEG) * 50, 0, Math.cos(deg * DEG) * 50)); };
  const run = (n) => { for (let i = 0; i < n; i++) v.present(1, 1 / 60); };
  // Its angles are the phase's; where it points is the hull's (a.facing).
  aimAt(170); run(300);
  near(Math.atan2(a.facing.x, a.facing.z), 170 * DEG, 1e-6, 'on a target behind-left');
  // Twenty degrees on across the back in the time that turn takes (the
  // long way round would take nine times as long).
  aimAt(-170); run(30);
  near(Math.atan2(a.facing.x, a.facing.z), -170 * DEG, 1e-6, 'across the back');
  const facing = new THREE.Vector3(0, 0, 1).transformDirection(a.bone.matrixWorld);
  near(Math.atan2(facing.x, facing.z), -170 * DEG, 1e-6, 'pointing at it');
});





test('the player\'s own body stays out of sight in an enclosed cockpit (Bespin: Platforms)', { skip: !has('bes1') }, () => {
  const names = ['cis_fly_droidfighter', 'cis_fly_maf', 'rep_fly_gunship', 'rep_fly_jedifighter'];
  const { system, collision, ...hull } = hulls('bes1', names);
  for (const name of names) {
    const v = hull[name], root = new THREE.Group();
    const driver = new VehicleDriver({ vehicles: system, camera: system.camera, collision, actor: { root, byName: {} } });
    driver.vehicle = v; v.seatIndex = 0;
    // An own correction from the world (EntityGameState.correct) shows the
    // body; the frame's presentation decides what is drawn.
    root.visible = true;
    const dummy=new THREE.Bone();dummy.name='DummyRoot';root.add(dummy);
    const host=bodyHost({root,bones:[dummy],byName:{DummyRoot:dummy}});
    host.unit.seat={vehicle:v,index:0};host.begin(1/60);host.owner.run();
    const node = v.seats[0].view.rider?.node;
    assert.equal(root.visible, Boolean(node), `${name}: the pilot shows only where his seat has a rider`);
    if (node) assert.ok(root.position.distanceTo(node.getWorldPosition(new THREE.Vector3())) < 1e-6, `${name}: at its rider node`);
  }
  assert.equal(hull.rep_fly_jedifighter.seats[0].view.rider.node, hull.rep_fly_jedifighter.byName.hp_active, 'the Jedi starfighter\'s pilot sits in the open');
});

test('the seat camera hangs from the seat: chase at TrackCenter and TrackOffset, eye at EyePointOffset, the seat\'s field of view', { skip: !has('nab2') }, () => {
  const { system, collision, pack, rep_hover_fightertank: v } = hulls('nab2', ['rep_hover_fightertank']);
  const camera = new THREE.PerspectiveCamera(50, 16 / 9, .1, 1000);
  const view = new VehicleCamera(camera, collision, true);
  v.driver = {}; v.seatIndex = 1; v.aimYaw = v.yaw + .5; v.pitch = .1;
  nativeVehicles(system, pack);
  system.update(1 / 60);
  v.seedPose();
  view.update(v, 1 / 60, false);
  const frame = v.seatFrame(1, new THREE.Matrix4());
  // TrackCenter (0, 1, -1) in the aim frame and TrackOffset (0, 0, 2) behind.
  const aim = v.viewDirection(new THREE.Vector3());
  const pivot = new THREE.Vector3(0, 1, -1).applyMatrix4(frame);
  near(camera.position.distanceTo(pivot), 2, 1e-6, 'two metres back from the pivot');
  const look = camera.getWorldDirection(new THREE.Vector3());
  near(look.angleTo(aim), 3 * DEG, 1e-4, 'looking along the aim, tilted down by TiltValue');
  assert.ok(look.y < aim.y);
  near(camera.fov, THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(24 * DEG) / camera.aspect)), 1e-6, '48 degrees across');
  view.update(v, 1 / 60, true);
  const eye = new THREE.Vector3(0, .5, 3).applyMatrix4(frame);
  assert.ok(camera.position.distanceTo(eye) < 1e-6, 'the eye at EyePointOffset in the turret\'s aim frame');
  near(camera.getWorldDirection(new THREE.Vector3()).angleTo(aim), 0, 1e-5, 'first person looks straight along the aim');
  v.seatIndex = 0; v.aimYaw = v.yaw; v.pitch = 0;
  view.cameraSet = false;
  view.update(v, 1 / 60, true);
  near(camera.fov, THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(39 * DEG) / camera.aspect)), 1e-6, 'the ground driver cockpit FOV is 78');
  const hull=new THREE.Matrix4().compose(v.position,new THREE.Quaternion().setFromEuler(new THREE.Euler(-v.tilt,v.yaw,v.roll,"YXZ")),new THREE.Vector3(1,1,1));
  assert.ok(camera.position.distanceTo(new THREE.Vector3(0, 2, 0).applyMatrix4(hull)) < 1e-6);
});
