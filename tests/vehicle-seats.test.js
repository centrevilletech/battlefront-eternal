// Vehicle seats as the 2004 game reads them (docs/formats.md "Vehicle
// seats"): who shows as a rider, the turret arcs, where the guns aim and fire
// from, and the seat cameras. Optional: uses the extracted vehicle packs under
// public/maps when present; retail data stays local and ignored.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleCamera, VehicleDriver } from '../src/vehicles.js';
import { blankIntent } from '../src/units.js';
import { packWith, vehiclePacks } from '../tools/vehicle-packs.mjs';

const DEG = Math.PI / 180;
const maps = new URL('../public/maps/', import.meta.url);
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
  return { system, collision, fired, ...byName };
}

let nextId = 1;
function bot() {
  const calls = [];
  const unit = {
    id: nextId++, generation: 1, team: 1, alive: true, dead: false, human: false, walker: false, seat: null, controller: null,
    classDef: {}, intent: blankIntent(),
    soldier: { position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, air: null, spawn() {} },
    actor: { root: new THREE.Group(), byName: {} },
    animator: { clips: new Map([['human_minigun_9pose', { fps: 30 }], ['human_drive', { fps: 30 }]]), gunner(clip, pitch, turn, time) { calls.push({ clip, pitch, turn, time }); }, reset() {} },
    presentation: { reset() {} },
    combat: { mounted: false, dead: false, team: 1, damage() {} },
  };
  unit.calls = calls;
  return unit;
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
  const camera = atst.cameraRest(1, frame, new THREE.Vector3()).sub(atst.position);
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

test('a bot gunner turns the turret, rides its hatch in the 9-pose, and fires from the barrel through its aim point', { skip: !has('nab2') }, () => {
  const { system, fired, rep_hover_fightertank: v } = hulls('nab2', ['rep_hover_fightertank']);
  const unit = bot();
  assert.ok(system.board(unit, v, 1));
  // Where the barrel's fire point is as each round leaves.
  const fire = system.ordnance.fire;
  system.ordnance.fire = (...args) => { fire(...args); fired.at(-1).muzzle = v.byName.hp_gun_1.getWorldPosition(new THREE.Vector3()); fired.at(-1).target = v.seats[1].target.clone(); };
  const yaw = v.yaw + 1, pitch = -.06;
  const aim = new THREE.Vector3(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
  unit.intent.aim = aim;
  for (let i = 0; i < 150; i++) {
    const seat = v.seats[1];
    const error = Math.atan2(Math.sin(yaw - seat.aimYaw), Math.cos(yaw - seat.aimYaw));
    unit.intent.turn = THREE.MathUtils.clamp(-error * 4, -1, 1);
    unit.intent.pitch = THREE.MathUtils.clamp((pitch - seat.pitch) * 4, -1, 1);
    unit.intent.buttons[0] = i > 90 ? 5 : 0;
    system.update(1 / 60);
  }
  const seat = v.seats[1];
  near(seat.aimYaw, yaw, .01, 'the seat comes round to the bot\'s aim');
  const turret = new THREE.Vector3(0, 0, 1).transformDirection(v.byName.turret_y.matrixWorld);
  near(Math.atan2(turret.x, turret.z), Math.atan2(aim.x, aim.z), .02, 'the turret node turns with the seat');
  // The rider: shown at hp_active, posed by the original game's 9-pose rule.
  assert.equal(unit.actor.root.visible, true);
  assert.ok(unit.actor.root.position.distanceTo(v.byName.hp_active.getWorldPosition(new THREE.Vector3())) < 1e-6);
  const pose = unit.calls.at(-1);
  near(pose.pitch, 2 * ((40 * DEG - pitch) / (55 * DEG) - .5), 1e-3, 'pitch spread over -15..40 degrees');
  assert.equal(pose.time, undefined);
  // Its rounds: the laser from hp_gun_1 through the point under its aim.
  const shots = fired.filter((f) => f.owner === unit);
  assert.ok(shots.length >= 1, 'the bot fires');
  const shot = shots.at(-1);
  assert.ok(shot.origin.distanceTo(shot.muzzle) < 1e-6, 'from the barrel\'s fire point');
  const rel = shot.target.clone().sub(shot.origin);
  assert.ok(rel.clone().addScaledVector(shot.dir, -rel.dot(shot.dir)).length() < .01, 'through the seat\'s target');
  assert.equal(shot.emitter, seat.weapons[0].muzzles[0].emitter, 'the beam follows its barrel');
  assert.equal(shot.emitter.matrix, v.byName.hp_gun_1.matrixWorld);
  const beam = new THREE.Vector3();
  shot.emitter.viewDirection(beam);
  assert.ok(beam.dot(shot.dir) > .9999, 'a beam keeps pointing where its barrel shoots');
});

test('each barrel\'s beam stays on its own barrel', { skip: !has('bes1', 'geo1') }, () => {
  const controls = (fire) => ({ take: (k) => fire.includes(k), held: (k) => fire.includes(k) });
  const at = (m) => new THREE.Vector3().setFromMatrixPosition(m);
  // The MAF gunner's two lasers fire together, one per barrel.
  const { system, fired, cis_fly_maf: maf } = hulls('bes1', ['cis_fly_maf']);
  maf.driver = {}; maf.seatIndex = 1; maf.flightState='Flying'; maf.position.y=100;
  system.update(1 / 60);
  maf.fire(1 / 60, controls(['firePrimary']));
  const beams = fired.filter((f) => f.def.base === 'beam');
  assert.equal(beams.length, 2);
  assert.notEqual(beams[0].emitter, beams[1].emitter);
  for (const b of beams) assert.ok(at(b.emitter.matrix).distanceTo(b.origin) < 1e-6, 'each beam starts at its own barrel, after the other has fired');
  // The spider walker's beam stays on hp_cannon_2 when the bolt fires from hp_cannon_1.
  const { system: geo, fired: shots, cis_walk_spider: spider } = hulls('geo1', ['cis_walk_spider']);
  spider.driver = {}; spider.seatIndex = 0;
  geo.update(1 / 60);
  spider.fire(1 / 60, controls(['firePrimary', 'fireSecondary']));
  const beam = shots.find((f) => f.def.base === 'beam');
  assert.ok(beam && shots.some((f) => f.def.base !== 'beam'), 'both weapons fired');
  assert.equal(beam.emitter.matrix, spider.byName.hp_cannon_2.matrixWorld);
});

test('a bot let out of a destroyed hull stands upright, facing its aim', { skip: !has('end1') }, () => {
  const { system, imp_hover_speederbike: bike } = hulls('end1', ['imp_hover_speederbike'], 150 * DEG);
  const unit = bot();
  assert.ok(system.board(unit, bike, 0));
  system.update(1 / 60);
  system.presentRiders(1 / 60);
  bike.damage(Infinity);
  assert.equal(unit.seat, null);
  near(unit.soldier.aimYaw, 150 * DEG, 1e-6, 'the bike\'s heading');
  near(unit.actor.root.rotation.y, unit.soldier.aimYaw, 1e-9, 'facing its aim');
  // The animator turns a body on foot by its yaw alone (here facing 0, as
  // after a respawn): nothing of the seat's turn may be left under it.
  unit.actor.root.rotation.y = 0;
  const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(unit.actor.root.quaternion);
  const up = new THREE.Vector3(0, 1, 0).applyQuaternion(unit.actor.root.quaternion);
  near(Math.atan2(forward.x, forward.z), 0, 1e-9, 'it faces where the animator turns it');
  near(up.y, 1, 1e-9, 'upright');
});

test('a bot takes a hover\'s aim level, not the player view\'s little-down rest', { skip: !has('end1') }, () => {
  const { system, imp_hover_speederbike: bike } = hulls('end1', ['imp_hover_speederbike']);
  const unit = bot();
  assert.ok(system.board(unit, bike, 0));
  assert.equal(bike.seats[0].pitch, 0);
});

test('switching seats never swings the hull, and a rear arc\'s aim eases round the back', { skip: !has('hot1', 'geo1', 'tat1') }, () => {
  const { system, all_fly_snowspeeder: speeder } = hulls('hot1', ['all_fly_snowspeeder']);
  const driver = Object.create(VehicleDriver.prototype);
  driver.lease = null;
  speeder.driver = {}; speeder.seatIndex = 0; speeder.restAim(0);
  const heading = speeder.yaw;
  driver.nextSeat(speeder);
  near(speeder.seats[1].aimYaw - heading, 80 * DEG, 1e-6, 'the gunner\'s view starts at the edge of its arc');
  driver.nextSeat(speeder);
  near(speeder.seats[0].aimYaw, speeder.yaw, 1e-9, 'back at the controls, along the hull');
  for (let i = 0; i < 120; i++) system.update(1 / 60, speeder, { forward: 0, right: 0, turn: 0, brake: false });
  near(speeder.yaw, heading, 1e-9, 'no input, no turn');
  // The same for each gunner seat that looks away from the nose.
  for (const [map, name] of [['geo1', 'rep_walk_atte'], ['tat1', 'tat_hover_skiff']]) {
    const { system: world, [name]: v } = hulls(map, [name]);
    v.driver = {}; v.seatIndex = 0; v.restAim(0);
    const start = v.yaw;
    for (let k = 0; k < v.seats.length; k++) {
      driver.nextSeat(v);
      if (v.seatIndex) { v.aimYaw = v.yaw + 2; v.clampAim(v.seatIndex); }
    }
    assert.equal(v.seatIndex, 0);
    for (let i = 0; i < 60; i++) world.update(1 / 60, v, { forward: 0, right: 0, turn: 0, brake: false });
    near(v.yaw, start, 1e-9, `${name}: round the seats and back`);
  }
  // A mouse flick from the arc's left end across the back to its right end
  // (80..280 degrees): the aim follows round the back, not stuck at 80.
  speeder.seatIndex = 1; speeder.aimYaw = speeder.yaw + 85 * DEG; speeder.pitch = 0; speeder.lookYaw = speeder.lookPitch = null;
  for (let k = 0; k < 10; k++) driver.look.call({ vehicle: speeder }, -19 * DEG * 2048, 0, 1, false);
  for (let i = 0; i < 120; i++) system.update(1 / 60, speeder, { forward: 0, right: 0, turn: 0, brake: false });
  near(THREE.MathUtils.euclideanModulo(speeder.aimYaw - speeder.yaw, 2 * Math.PI), 275 * DEG, 1e-3, 'the aim reaches the look');
});

test('a full-circle aimer crosses behind the shorter way', { skip: !has('geo1') }, () => {
  const { cis_walk_spider: v } = hulls('geo1', ['cis_walk_spider']);
  const a = v.seats[0].weapons[0].aimers[0];
  assert.equal(a.node, 'turret_y_2');
  const pivot = a.bone.getWorldPosition(new THREE.Vector3());
  const aimAt = (deg) => { v.seats[0].target = pivot.clone().add(new THREE.Vector3(Math.sin(deg * DEG) * 50, 0, Math.cos(deg * DEG) * 50)); };
  const run = (n) => { for (let i = 0; i < n; i++) { v.root.updateMatrixWorld(true); v.poseSeat(0, 1 / 60); } };
  aimAt(170); run(300);
  near(a.yaw, 170 * DEG, 1e-6, 'on a target behind-left');
  aimAt(-170); run(30);
  near(a.yaw, 190 * DEG, 1e-6, 'twenty degrees on, across the back');
  const facing = new THREE.Vector3(0, 0, 1).transformDirection(a.bone.matrixWorld);
  near(Math.atan2(facing.x, facing.z), -170 * DEG, 1e-6, 'pointing at it');
});

test('the AT-AT\'s head is the gunner\'s while he holds it; the pilot\'s rounds follow his own aim', { skip: !has('hot1') }, () => {
  const { system, fired, imp_walk_atat: v } = hulls('hot1', ['imp_walk_atat']);
  const node = v.byName.aimer_head;
  const facing = () => new THREE.Vector3(0, 0, 1).transformDirection(node.matrixWorld);
  const toTarget = () => v.seats[0].target.clone().sub(node.getWorldPosition(new THREE.Vector3())).normalize();
  const controls = { take: (k) => k === 'firePrimary', held: (k) => k === 'firePrimary' };
  const idle = { forward: 0, right: 0, turn: 0, brake: false };
  // Alone, the pilot's aimer turns the head to his target.
  v.driver = {}; v.seatIndex = 0; v.restAim(0); v.pitch = -10 * DEG;
  for (let i = 0; i < 120; i++) system.update(1 / 60, v, idle);
  assert.ok(facing().dot(toTarget()) > Math.cos(2 * DEG), 'the head looks where the pilot aims');
  // A gunner aboard turns the head with his own aim.
  const gunner = bot();
  assert.ok(system.board(gunner, v, 1));
  for (let i = 0; i < 90; i++) {
    v.seats[1].aimYaw = v.yaw - 20 * DEG; v.seats[1].pitch = 0;
    system.update(1 / 60, v, idle);
  }
  near(Math.atan2(facing().x, facing().z) - v.yaw, -20 * DEG, 1e-3, 'the head follows the gunner');
  fired.length = 0;
  v.fire(1 / 60, controls);
  const shot = fired.find((f) => f.owner === v);
  assert.ok(shot, 'the pilot fires');
  // At his target (within the weapon's spread), not along the head.
  const at = v.seats[0].target.clone().sub(shot.origin).normalize();
  assert.ok(shot.dir.angleTo(at) < .5 * DEG, `the pilot's round: ${shot.dir.angleTo(at) / DEG} degrees off his target`);
  assert.ok(facing().angleTo(at) > 15 * DEG, 'while the head looks away');
});

test('enclosed seats hide their bot, and a bot pilot drives', { skip: !has('nab2') }, () => {
  const { system, cis_hover_aat: v } = hulls('nab2', ['cis_hover_aat']);
  const pilot = bot(), gunner = bot();
  assert.ok(system.board(pilot, v, 0));
  assert.ok(system.board(gunner, v, 1));
  system.update(1 / 60);
  assert.equal(pilot.actor.root.visible, false);
  assert.equal(gunner.actor.root.visible, false);
  // Per-frame presentation must keep them hidden even if something else
  // showed the body in between (the remote-client race where unit
  // presentation kept drawing an enclosed rider outside the hull).
  pilot.actor.root.visible = gunner.actor.root.visible = true;
  system.presentRiders(1 / 60);
  assert.equal(pilot.actor.root.visible, false, 'enclosed pilot re-hidden at present time');
  assert.equal(gunner.actor.root.visible, false, 'enclosed gunner re-hidden at present time');
});

test('the player\'s own body stays out of sight in an enclosed cockpit (Bespin: Platforms)', { skip: !has('bes1') }, () => {
  const names = ['cis_fly_droidfighter', 'cis_fly_maf', 'rep_fly_gunship', 'rep_fly_jedifighter'];
  const { system, collision, ...hull } = hulls('bes1', names);
  system.update(1 / 60);
  for (const name of names) {
    const v = hull[name], root = new THREE.Group();
    const driver = new VehicleDriver({ vehicles: system, camera: system.camera, collision, actor: { root, byName: {} } });
    driver.vehicle = v; v.seatIndex = 0;
    // An own correction from the world (EntityGameState.correct) shows the
    // body; the frame's presentation decides what is drawn.
    root.visible = true;
    driver.presentRider(1 / 60);
    const node = v.seats[0].view.rider?.node;
    assert.equal(root.visible, Boolean(node), `${name}: the pilot shows only where his seat has a rider`);
    if (node) assert.ok(root.position.distanceTo(node.getWorldPosition(new THREE.Vector3())) < 1e-6, `${name}: at its rider node`);
  }
  assert.equal(hull.rep_fly_jedifighter.seats[0].view.rider.node, hull.rep_fly_jedifighter.byName.hp_active, 'the Jedi starfighter\'s pilot sits in the open');
});

test('the seat camera hangs from the seat: chase at TrackCenter and TrackOffset, eye at EyePointOffset, the seat\'s field of view', { skip: !has('nab2') }, () => {
  const { system, collision, rep_hover_fightertank: v } = hulls('nab2', ['rep_hover_fightertank']);
  const camera = new THREE.PerspectiveCamera(50, 16 / 9, .1, 1000);
  const view = new VehicleCamera(camera, collision, true);
  v.driver = {}; v.seatIndex = 1; v.aimYaw = v.yaw + .5; v.pitch = .1;
  system.update(1 / 60);
  view.update(v, 1 / 60, false);
  const frame = v.seatFrame(1, new THREE.Matrix4());
  const rest = v.cameraRest(1, frame, new THREE.Vector3());
  assert.ok(camera.position.distanceTo(rest) < 1e-6, 'nothing in the way: the camera rests where the section puts it');
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
  assert.ok(camera.position.distanceTo(new THREE.Vector3(0, 2, 0).applyMatrix4(view.hullFrame(v, new THREE.Matrix4()))) < 1e-6);
});
