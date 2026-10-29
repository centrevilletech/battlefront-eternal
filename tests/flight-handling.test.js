// How a craft handles and how its pilot sees it, from its own ODF as the
// 2004 game reads it (docs/vehicles.md "Movement" and "Camera"): the
// turn and pitch rates and their filters, the flat turn and the drawn lean,
// the wings leveling, the lift off a pad, and the chase camera's place,
// turn, easing and field of view. The synthetic cases always run; the last
// test pins every locally imported flyer to its own data (BFE_CONTENT_DIR,
// or public/), which stays out of the repository.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { flightHandling } from '../src/vehicle-motion.js';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleCamera } from '../src/vehicles.js';
import { setting } from '../src/seats.js';
import { vehiclePacks } from '../tools/vehicle-packs.mjs';

const DT = 1 / 60;
const run = (f, ticks, input = {}) => { for (let i = 0; i < ticks; i++) f.system.update(DT, f.v, input); };
const near = (a, b, tolerance, what) => assert.ok(Math.abs(a - b) <= tolerance, `${what}: ${a} is not ${b}`);
const DEG = Math.PI / 180;
// The snowspeeder's body section, its hashed keys as the import keeps them
// (BankAngle 6d7c9f7e, BankFilter 35de28f1, LevelFilter f4f5f38f,
// TakeoffTime 61195c0a, TakeoffSpeed 8255424e, LandingTime 1283209d,
// LandingSpeed 6ae3a957, TakeoffHeight 5107fafc, LandedHeight 7642977a), and
// its pilot section's camera (AimTension 5e3171cc, MoveTensionX 88f1308a,
// MoveTensionY 89f1321d, MoveTensionZ 86f12d64).
const SNOWSPEEDER = { MinSpeed: '20.0', MidSpeed: '40.0', MaxSpeed: '72.0', Acceleraton: '30.0', PitchRate: '0.8', PitchFilter: '2.75', TurnRate: '1.25', TurnFilter: '5.75',
  '6d7c9f7e': '0.8', '35de28f1': '4.5', f4f5f38f: '5.0', '61195c0a': '1.0', '8255424e': '3.0', '1283209d': '2.0', '6ae3a957': '5.0', '5107fafc': '2.5', '7642977a': '0.5' };
const SNOWSPEEDER_CAMERA = { EyePointOffset: '0.0 1.0 -1.2', TrackCenter: '0.0 3.0 -8.0', TrackOffset: '0.0 0.0 0.0', TiltValue: '0.0', FirstPersonFOV: '60',
  '5e3171cc': '10.0', '88f1308a': '5.5 5.5', '89f1321d': '2.5 8.5', '86f12d64': '6.5' };
// A craft in level flight at altitude, its nose along +z, at cruise.
function cruising(settings = SNOWSPEEDER, camera = SNOWSPEEDER_CAMERA) {
  const f = fixture(settings, 'ship', {}, camera);
  f.system.flightEnvironment.bounds = null; f.system.flightEnvironment.ceiling = 5000;
  f.v.position.set(0, 300, 0); f.v.syncBody();
  run(f, 1, { takeoff: true }); run(f, 240);
  assert.equal(f.v.flightState, 'Flying');
  f.v.quaternion.identity(); f.v.velocity.set(0, 0, f.v.airspeed);
  return f;
}
const heading = (v) => { const h = new THREE.Vector3(0, 0, 1).applyQuaternion(v.quaternion); return Math.atan2(h.x, h.z); };

test('a flyer takes its handling from its body section, named or hashed, TurnRate at least 1.5, else the class defaults', () => {
  const h = flightHandling({ settings: SNOWSPEEDER });
  assert.deepEqual(h, { turnRate: 1.5, turnFilter: 5.75, pitchRate: .8, pitchFilter: 2.75, bankAngle: .8, bankFilter: 4.5, levelFilter: 5,
    takeoffHeight: 2.5, takeoffTime: 1, takeoffSpeed: 3, landedHeight: .5, landingTime: 2, landingSpeed: 5 });
  assert.equal(flightHandling({ settings: { TurnRate: '1.75' } }).turnRate, 1.75, 'the cloud car turns faster than the floor');
  // A gunner's section keeps its own turn keys: they are not the craft's.
  const gunner = flightHandling({ settings: {}, seats: [{ settings: {} }, { settings: { TurnRate: '3.0', TurnFilter: '2.75', PitchRate: '2.0', PitchFilter: '2.75', BankAngle: '0.4' } }] });
  assert.deepEqual([gunner.turnRate, gunner.turnFilter, gunner.pitchRate, gunner.pitchFilter], [2, 10, 1, 10]);
  // A class key authored after a gunner's section is still the class's.
  assert.equal(gunner.bankAngle, .4);
  assert.deepEqual(flightHandling({ settings: {} }), { turnRate: 2, turnFilter: 10, pitchRate: 1, pitchFilter: 10, bankAngle: .5, bankFilter: 2, levelFilter: 3,
    takeoffHeight: 5, takeoffTime: 5, takeoffSpeed: 3, landedHeight: 0, landingTime: 5, landingSpeed: 3 });
});

test('the snowspeeder turns flat at 1.5 rad/s, a 26.7 m circle at cruise and 13.3 m at MinSpeed, its drawn hull leaning 38 degrees', () => {
  const f = cruising(), v = f.v, y = v.position.y;
  // The rate follows the stick at TurnFilter (5.75/s): one update takes 5.75/60 of the way.
  run(f, 1, { stickX: 1 });
  near(v.yawRate, -1.5 * 5.75 / 60, 1e-12, 'the first update');
  run(f, 179, { stickX: 1 });
  near(v.yawRate, -1.5, 1e-6, 'the turn rate');
  near(new THREE.Vector3(1, 0, 0).applyQuaternion(v.quaternion).y, 0, 1e-9, 'the frame stays level');
  near(v.position.y, y, 1e-6, 'no height lost');
  // BankAngle x the turn's pull (1.5 rad/s x 40 m/s) over MaxSpeed (72).
  near(v.bank, .8 * 1.5 * 40 / 72, 1e-3, 'the lean');
  // One circuit: 2 pi / 1.5 s, back where it began.
  const start = v.position.clone(), h0 = heading(v);
  let ticks = 0, turned = 0, prev = h0;
  while (turned < 2 * Math.PI) { run(f, 1, { stickX: 1 }); ticks++; let d = heading(v) - prev; turned += Math.abs(Math.atan2(Math.sin(d), Math.cos(d))); prev = heading(v); }
  near(ticks * DT, 2 * Math.PI / 1.5, 2 * DT, 'a circuit');
  near(v.position.distanceTo(start), 0, 1.5, 'the circle closes');
  // Its radius: the speed over the rate.
  const center = start.clone().add(new THREE.Vector3(-1, 0, 0).applyAxisAngle(new THREE.Vector3(0, 1, 0), h0).multiplyScalar(40 / 1.5));
  for (let i = 0; i < 60; i++) { run(f, 1, { stickX: 1 }); near(Math.hypot(v.position.x - center.x, v.position.z - center.z), 40 / 1.5, .5, 'on the 26.7 m circle'); }
  // Holding S: MinSpeed, the same rate, half the circle.
  run(f, 120, { stickX: 1, forward: -1 });
  assert.equal(v.airspeed, 20);
  near(v.airspeed / -v.yawRate, 13.33, .01, 'the slow circle');
  // Let go: the rate and the lean fade.
  run(f, 240);
  near(v.yawRate, 0, 1e-4, 'the turn stops'); near(v.bank, 0, 1e-3, 'the lean goes');
});

test('the tow cable at BF1 pace: three circuits of an AT-AT leg in 12.6 s, well inside 1.2 x CableLength', () => {
  // The snowspeeder circles a leg with the stick hard over: three turns of
  // 2 pi at 1.5 rad/s, 26.7 m out (CableLength 60 x 1.2 = 72 m).
  const f = cruising(), v = f.v;
  run(f, 60, { stickX: 1 });
  const h0 = heading(v);
  const leg = v.position.clone().add(new THREE.Vector3(-1, 0, 0).applyAxisAngle(new THREE.Vector3(0, 1, 0), h0).multiplyScalar(v.airspeed / 1.5));
  let wound = 0, prev = Math.atan2(v.position.x - leg.x, v.position.z - leg.z), ticks = 0, far = 0;
  while (wound < 6 * Math.PI && ticks < 60 * 30) {
    run(f, 1, { stickX: 1 }); ticks++;
    const a = Math.atan2(v.position.x - leg.x, v.position.z - leg.z), d = a - prev; prev = a;
    wound += Math.abs(Math.atan2(Math.sin(d), Math.cos(d)));
    far = Math.max(far, Math.hypot(v.position.x - leg.x, v.position.z - leg.z));
  }
  near(ticks * DT, 6 * Math.PI / 1.5, .1, 'three circuits');
  assert.ok(far < 30 && far < 72, `${far} m from the leg`);
});

test('a player pitches at TurnRate, a bot at PitchRate, each following at PitchFilter', () => {
  const human = cruising(), bot = cruising();
  run(human, 300, { stickY: .5 }); run(bot, 1, {});
  bot.v.quaternion.identity();
  for (let i = 0; i < 300; i++) bot.system.update(DT, bot.v, { stickY: .5, bot: true });
  // A half stick (the response curve, exponent 1.35, softens it).
  const half = Math.pow(.5, 1.35);
  near(human.v.tiltRate, 1.5 * half, 1e-6, 'the player');
  near(bot.v.tiltRate, .8 * half, 1e-6, 'the bot');
});

test('a takeoff lets the pilot turn as it rises and leaves slowly; a roof over the pad holds it under it', () => {
  const f = fixture(SNOWSPEEDER, 'ship', {}, SNOWSPEEDER_CAMERA), v = f.v;
  f.system.flightEnvironment.bounds = null;
  const h0 = heading(v), y0 = v.position.y;
  run(f, 1, { takeoff: true });
  assert.equal(v.flightState, 'TakingOff');
  // Hard over through the lift: the turn grows with the height (full
  // TakeoffHeight up), so it can face a way out of a niche before it goes.
  let turned = 0, prev = h0, flying = null;
  for (let t = 1; t < 400 && !flying; t++) {
    run(f, 1, { stickX: 1 });
    let d = heading(v) - prev; turned += Math.abs(Math.atan2(Math.sin(d), Math.cos(d))); prev = heading(v);
    if (v.flightState === 'Flying') flying = { t, rise: v.position.y - y0, speed: v.airspeed };
  }
  assert.ok(turned > 2 * Math.PI / 3, `turned ${(turned / DEG).toFixed(0)} degrees before flying`);
  // From 0.5 m up to TakeoffHeight + LandedHeight (3 m), then TakeoffTime.
  assert.ok(flying.t > 120 && flying.t < 140, `flying after ${flying.t} ticks`);
  near(flying.rise, 2.5 + 1.5, .15, 'the rise');
  assert.ok(flying.speed < 6, `left at ${flying.speed} m/s`);
  near(new THREE.Vector3(1, 0, 0).applyQuaternion(v.quaternion).y, 0, .01, 'the wings level');
  // A deck 1.5 m over the parked hull: it cannot rise the 2.5 m it needs.
  // As in the 2004 game the lift starts anyway, and the deck holds it.
  const roofed = fixture(SNOWSPEEDER, 'ship', {}, SNOWSPEEDER_CAMERA);
  const deck = new THREE.BoxGeometry(40, 1, 40).translate(0, 4.5, 0);
  roofed.c.addBody(deck.attributes.position.array, deck.index.array, new Uint32Array(deck.index.count / 3));
  run(roofed, 1, { takeoff: true });
  assert.equal(roofed.v.flightState, 'TakingOff');
  run(roofed, 240);
  assert.equal(roofed.v.flightState, 'TakingOff'); assert.equal(roofed.v.flightWarning, 'Takeoff path blocked');
  assert.ok(roofed.v.position.y + roofed.v.bounds.max.y <= 4 + 1e-6, `held under the deck (${roofed.v.position.y})`);
});

test('the pilot\'s chase camera is his section\'s: TrackCenter, TrackOffset in its own frame pitched down by TiltValue, 48 degrees across', () => {
  // The X-wing's (the fixture's): TrackCenter (0, 2, -4), TrackOffset
  // (0, 2, 6), TiltValue 5, AimTension 20, MoveTension 4.25, no ThirdPersonFOV.
  const f = fixture(); f.v.flightState = 'Flying'; f.v.position.set(0, 300, 0); f.v.quaternion.identity(); f.v.bank = f.v.roll = 0;
  f.v.captureTick(); f.v.present(1);
  const cam = new THREE.PerspectiveCamera(70, 16 / 10, .1, 5000), view = new VehicleCamera(cam, f.c);
  view.update(f.v, DT, false);
  const t = 5 * DEG;
  near(cam.position.x, 0, 1e-9, 'x'); near(cam.position.y, 302 + 2 * Math.cos(t) + 6 * Math.sin(t), 1e-9, 'height');
  near(cam.position.z, -4 - 6 * Math.cos(t) + 2 * Math.sin(t), 1e-9, 'back');
  const look = cam.getWorldDirection(new THREE.Vector3());
  near(look.y, -Math.sin(t), 1e-9, 'pitched down by TiltValue'); near(look.z, Math.cos(t), 1e-9, 'along the nose');
  near(2 * Math.atan(Math.tan(cam.fov * DEG / 2) * cam.aspect) / DEG, 48, 1e-9, 'across');
  // Speed and boost change nothing; nothing blurs.
  f.v.airspeed = 82; f.v.boosting = true;
  for (let i = 0; i < 30; i++) view.update(f.v, DT, false);
  near(2 * Math.atan(Math.tan(cam.fov * DEG / 2) * cam.aspect) / DEG, 48, 1e-9, 'across at top speed');
  assert.equal(view.speedEffect, 0);
  // In the cockpit: FirstPersonFOV across (52).
  for (let i = 0; i < 30; i++) view.update(f.v, DT, true);
  near(2 * Math.atan(Math.tan(cam.fov * DEG / 2) * cam.aspect) / DEG, 52, 1e-9, 'cockpit across');
});

test('the snowspeeder\'s camera eases each axis at its MoveTension and turns at its AimTension, and does not lean with the hull', () => {
  const f = cruising(), v = f.v;
  const cam = new THREE.PerspectiveCamera(70, 16 / 10, .1, 5000), view = new VehicleCamera(cam, f.c);
  v.velocity.set(0, 0, 0); v.airspeed = 0; f.system.update(DT, v, { forward: 0 }); // hold still
  v.position.set(0, 300, 0); v.quaternion.identity(); v.captureTick(); v.present(1);
  view.update(v, DT, false);
  // At rest at TrackCenter (0, 3, -8) in the frame, looking along the nose.
  near(cam.position.distanceTo(new THREE.Vector3(0, 303, -8)), 0, 1e-9, 'rest');
  // The craft drops 1 m: the camera sinks toward it at 8.5/s; rises at 2.5/s.
  const step = (dy) => { v.position.y += dy; v.captureTick(); v.present(1); const before = cam.position.y; view.update(v, DT, false); return (cam.position.y - before) / dy; };
  near(step(-1), 1 - Math.exp(-8.5 * DT), 1e-9, 'sinking');
  for (let i = 0; i < 300; i++) view.update(v, DT, false);
  near(step(1), 1 - Math.exp(-2.5 * DT), 1e-9, 'rising');
  // A quarter turn of the frame: the camera turns toward it at 10/s.
  for (let i = 0; i < 300; i++) view.update(v, DT, false);
  const was = cam.quaternion.clone();
  v.quaternion.setFromAxisAngle(new THREE.Vector3(0, 1, 0), Math.PI / 2); v.captureTick(); v.present(1);
  view.update(v, DT, false);
  near(was.angleTo(cam.quaternion), (Math.PI / 2) * (1 - Math.exp(-10 * DT)), 1e-6, 'the turn eases');
  // The drawn hull leans; the frame the camera follows does not.
  for (let i = 0; i < 600; i++) view.update(v, DT, false);
  v.bank = v.roll = .6; v.captureTick(); v.present(1);
  const up = new THREE.Vector3(0, 1, 0);
  near(new THREE.Vector3(0, 1, 0).applyQuaternion(v.root.quaternion).angleTo(up), .6, 1e-9, 'the hull leans');
  view.update(v, DT, false);
  near(new THREE.Vector3(0, 1, 0).applyQuaternion(cam.quaternion).angleTo(up), 0, 1e-6, 'the camera does not');
});

// Every imported flyer as the world flies it: off the ground, at cruise, the
// stick hard over turns it flat at its body section's TurnRate (at least
// 1.5 rad/s), and its pilot's chase camera is ThirdPersonFOV across (48
// degrees unless the section sets one) at cruise and at top speed, nothing
// blurring.
const content = process.env.BFE_CONTENT_DIR ? pathToFileURL(`${process.env.BFE_CONTENT_DIR}/`) : new URL('../public/', import.meta.url);
const imported = vehiclePacks(new URL('maps/', content));
const binary = (file) => { const b = fs.readFileSync(file); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const across = (cam) => 2 * Math.atan(Math.tan(cam.fov * DEG / 2) * cam.aspect) / DEG;

test('every imported flyer turns flat at its own TurnRate and is seen at its own field of view', { skip: !imported.length && 'no imported vehicles' }, () => {
  const seen = new Set();
  for (const file of imported) {
    const data = JSON.parse(fs.readFileSync(file));
    const names = Object.keys(data.classes).filter((n) => !seen.has(n) && data.classes[n].base === 'flyer');
    if (!names.length) continue;
    names.forEach((n) => seen.add(n));
    const g = new THREE.BoxGeometry(40000, 2, 40000).translate(0, -1, 0);
    const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
    const spawns = names.map((name, i) => ({ name, matrix: new THREE.Matrix4().makeTranslation(i * 200 - 5000, 0, -15000).toArray(), team: 1, post: '', respawnTime: 45, classes: { 1: name } }));
    const dir = new URL('./', file);
    const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision, data: { ...data, spawns, blockers: null },
      buffer: binary(new URL('models.bin', dir)), indexBuffer: binary(new URL('models_index.bin', dir)), animationBuffer: binary(new URL('animations.bin', dir)),
      texture: () => null, effects: { play: () => null }, ordnance: { explosion() {} }, flightEnvironment: { bounds: null, ceiling: 1e4 } });
    system.cosmetic = false;
    for (const name of names) {
      const v = system.vehicles.find((x) => x.name === name), def = v.def;
      const fly = (ticks, input) => { for (let i = 0; i < ticks; i++) system.update(DT, v, input); };
      v.driver = {};
      v.position.y += 200; v.syncBody();
      fly(1, { takeoff: true }); fly(480, {});
      assert.equal(v.flightState, 'Flying', `${name}: airborne`);
      const turnRate = parseFloat(setting(def.settings, 'TurnRate'));
      const rate = Math.max(Number.isFinite(turnRate) ? turnRate : 2, 1.5);
      fly(180, { stickX: 1 });
      near(-v.yawRate / rate, 1, .01, `${name}: turns at ${rate} rad/s`);
      near(new THREE.Vector3(1, 0, 0).applyQuaternion(v.quaternion).y, 0, 1e-3, `${name}: wings level in the turn`);
      fly(240, {});
      const fov = parseFloat(setting(def.seats[0]?.settings, 'ThirdPersonFOV'));
      const want = Number.isFinite(fov) ? fov : 48;
      const cam = new THREE.PerspectiveCamera(70, 16 / 10, .1, 5000), view = new VehicleCamera(cam, collision);
      const look = () => { for (let i = 0; i < 30; i++) { fly(1, { forward: 1 }); v.captureTick(); v.present(1); view.update(v, DT, false); } };
      look();
      near(across(cam), want, 1e-9, `${name}: ${want} degrees across`);
      fly(600, { forward: 1 }); look();
      near(across(cam), want, 1e-9, `${name}: ${want} degrees across at top speed`);
      assert.equal(view.speedEffect, 0, `${name}: nothing blurs`);
    }
    system.dispose?.();
  }
  assert.ok(seen.size > 0);
});
