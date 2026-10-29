// Vehicle speeds follow each class's ODF as the 2004 game reads it
// (docs/vehicles.md "Movement"). The synthetic cases always run; the last
// test pins every locally imported vehicle's top speed to its own data
// (BFE_CONTENT_DIR, or public/), which stays out of the repository.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { hullQuaternion } from '../src/vehicle-hover.js';
import { classSetting, flightSpeeds, gaitSpeed, groundMotion } from '../src/vehicle-motion.js';
import { throttleFor } from '../src/flight/config.js';
import { vehiclePacks } from '../tools/vehicle-packs.mjs';
import { pilotInput } from '../src/flight/ai.js';

const run = (f, seconds, input = {}) => { for (let i = 0; i < seconds * 60; i++) f.system.update(1 / 60, f.v, input); };
const airborne = (settings) => { const f = fixture(settings); f.system.flightEnvironment.bounds = null; f.v.position.y = 300; run(f, 1 / 60, { takeoff: true }); run(f, 3); return f; };
// The body sections' flight keys, some hashed as the import keeps them.
const SNOWSPEEDER = { MinSpeed: '20.0', MidSpeed: '40.0', MaxSpeed: '72.0', Acceleraton: '30.0', PitchRate: '0.8', PitchFilter: '2.75', TurnRate: '1.25', TurnFilter: '5.75',
  '6d7c9f7e': '0.8', '35de28f1': '4.5', f4f5f38f: '5.0', '61195c0a': '1.0', '8255424e': '3.0', '1283209d': '2.0', '6ae3a957': '5.0', '5107fafc': '2.5', '7642977a': '0.5' };
const GUNSHIP = { MinSpeed: '-15.0', MidSpeed: '0.0', MaxSpeed: '45.0', Acceleraton: '20.0', StrafeSpeed: '15.0', PitchRate: '0.8', PitchFilter: '1.75', TurnRate: '1.0', TurnFilter: '5.75',
  BankAngle: '0.55', BankFilter: '5.5', LevelFilter: '2.0', TakeoffTime: '1.5', TakeoffSpeed: '5.0', LandingTime: '1.5', LandingSpeed: '8.0', TakeoffHeight: '10.0' };

test('a flyer takes MinSpeed, MidSpeed, MaxSpeed and Acceleraton from its ODF, named or hashed, else the defaults', () => {
  assert.deepEqual(flightSpeeds({ settings: SNOWSPEEDER }), { minSpeed: 20, cruiseSpeed: 40, maxSpeed: 72, acceleration: 30 });
  // The extractor keeps unnamed keys as their FNV-1a hash.
  assert.deepEqual(flightSpeeds({ settings: { fbe2eb62: '-15.0', '838bda20': '0.0', MaxSpeed: '45.0', Acceleraton: '20.0' } }),
    { minSpeed: -15, cruiseSpeed: 0, maxSpeed: 45, acceleration: 20 });
  // The cloud car leaves MidSpeed unset: the class default, 20.
  assert.deepEqual(flightSpeeds({ settings: { MinSpeed: '20.0', MaxSpeed: '72.0', Acceleraton: '8.0' } }), { minSpeed: 20, cruiseSpeed: 20, maxSpeed: 72, acceleration: 8 });
  assert.deepEqual(flightSpeeds({ settings: {} }), { minSpeed: 10, cruiseSpeed: 20, maxSpeed: 30, acceleration: 5 });
  // A key after a later seat's section is still the class's.
  assert.equal(classSetting({ settings: {}, seats: [{ settings: {} }, { settings: { AnimationSpeed: '1.5' } }] }, 'AnimationSpeed'), '1.5');
});

test('the snowspeeder cruises at 40, tops out at 72 and brakes to 20, at 30 m/s^2; boost never passes MaxSpeed', () => {
  const f = airborne(SNOWSPEEDER), v = f.v;
  assert.equal(v.flightState, 'Flying');
  assert.equal(v.airspeed, 40);
  run(f, .5, { forward: 1 });
  assert.ok(Math.abs(v.airspeed - 55) < 1e-9, `linear at Acceleraton: ${v.airspeed}`);
  run(f, 2, { forward: 1 });
  assert.equal(v.airspeed, 72);
  run(f, 3, { forward: -1 });
  assert.equal(v.airspeed, 20);
  run(f, 2);
  assert.equal(v.airspeed, 40);
  // Boost holds MaxSpeed at twice the rate, and no more.
  run(f, .5, { forward: 1, boost: true });
  assert.ok(Math.abs(v.airspeed - 70) < 1e-9, `twice Acceleraton: ${v.airspeed}`);
  let fastest = 0;
  for (let i = 0; i < 120; i++) { run(f, 1 / 60, { forward: 1, boost: true }); fastest = Math.max(fastest, v.airspeed); }
  assert.equal(fastest, 72);
  assert.equal(v.speed, 72);
});

test('a gunship hovers at MidSpeed 0, backs up at its negative MinSpeed, and may land while hovering', () => {
  const f = airborne(GUNSHIP), v = f.v;
  assert.equal(v.airspeed, 0);
  const before = v.position.clone();
  run(f, 1);
  assert.ok(Math.hypot(v.position.x - before.x, v.position.z - before.z) < 1e-6, 'it hovers in place');
  run(f, 2, { forward: -1 });
  assert.equal(v.airspeed, -15);
  const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(v.quaternion);
  assert.ok(v.velocity.dot(forward) < -14.9, 'it flies backward');
  run(f, 2);
  assert.equal(v.airspeed, 0);
  assert.notEqual(v.landingReason, 'Too fast');
  // A pilot never asks this craft to back up: turning hard it keeps moving
  // (standing still reads as stuck), released it travels, landing it stops.
  assert.equal(throttleFor(v.flightConfig, 0), 0);
  const unit = { vehicle: v, steer: {} };
  const behind = v.position.clone().addScaledVector(forward, -100);
  const turning = pilotInput(unit, null, behind, 1 / 60, {}, {});
  assert.ok(Math.abs(turning.turn) > .5 && turning.forward > 0, `turning hard it keeps moving: ${turning.forward}`);
  const ahead = v.position.clone().addScaledVector(forward, 100).add(new THREE.Vector3(30, 0, 0));
  assert.ok(pilotInput(unit, null, ahead, 1 / 60, {}, {}).forward >= .5, 'it travels rather than hovering');
  unit.steer.land = true;
  assert.equal(pilotInput(unit, null, null, 1 / 60, {}, {}).forward, 0);
});

test('an ordinary fighter still brakes fully for the AI, and may land at speed', () => {
  const f = airborne(SNOWSPEEDER), c = f.v.flightConfig;
  assert.equal(throttleFor(c, c.minSpeed), -1);
  run(f, 1 / 60, { takeoff: true });
  // At 300 m, at full speed: down it goes, as in the 2004 game.
  assert.equal(f.v.flightState, 'Landing');
  assert.notEqual(f.v.landingReason, 'Too fast');
});

test('hovers take ForwardSpeed, ReverseSpeed, StrafeSpeed, Acceleration and Deceleration, with the class defaults', () => {
  assert.deepEqual(groundMotion({ base: 'hover', settings: { ForwardSpeed: '40.0', ReverseSpeed: '12.0', StrafeSpeed: '0.0', Acceleration: '30.0', Deceleration: '40.0', TurnRate: '2.0' } }),
    { speed: 40, reverseSpeed: 12, strafeSpeed: 0, acceleration: 30, deceleration: 40, turnRate: 2 });
  const d = groundMotion({ base: 'hover', settings: {} });
  assert.deepEqual([d.speed, d.reverseSpeed, d.strafeSpeed, d.acceleration, d.deceleration], [20, 10, 15, 10, 5]);
});

test('walkers walk at their clips pace, the same backward, and never strafe', () => {
  // Two 81-frame steps of 4 m each at AnimationSpeed 1.5 (an AT-AT's).
  const clips = new Map([['walk_leftfoot_rightfoot', { stride: 4, frames: 81 }], ['walk_rightfoot_leftfoot', { stride: 4, frames: 81 }]]);
  const pace = 8 * 30 / 162 * 1.5;
  assert.ok(Math.abs(gaitSpeed(clips, 1.5) - pace) < 1e-12);
  const m = groundMotion({ base: 'commandwalker', settings: { MaxSpeed: '2.0', MaxStrafeSpeed: '3.5', Acceleraton: '1.0', AnimationSpeed: '1.5' } }, clips);
  assert.ok(Math.abs(m.speed - pace) < 1e-12);
  assert.equal(m.reverseSpeed, m.speed);
  assert.equal(m.strafeSpeed, 0);
  assert.equal(m.acceleration, 1);
  // No walk clips: what the class tells the AI.
  assert.equal(groundMotion({ base: 'walker', settings: { MaxSpeed: '5.0' } }, new Map()).speed, 5);
  // An animal reaches its gallop in 1/Acceleraton s.
  const animal = groundMotion({ base: 'walker', settings: { HealthType: 'animal', Acceleraton: '2.4' } }, clips);
  assert.ok(Math.abs(animal.acceleration - 2.4 * animal.speed) < 1e-9 && animal.deceleration === animal.acceleration);
});

// Every imported class, driven through Vehicles.update as the world drives
// it: W to its top speed, S to its slowest (a flyer) or full reverse.
const content = process.env.BFE_CONTENT_DIR ? pathToFileURL(`${process.env.BFE_CONTENT_DIR}/`) : new URL('../public/', import.meta.url);
const maps = new URL('maps/', content);
const imported = vehiclePacks(maps);
const binary = (file) => { const b = fs.readFileSync(file); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const number = (def, name, fallback) => { const n = parseFloat(classSetting(def, name)); return Number.isFinite(n) ? n : fallback; };

test('every imported vehicle tops out at its own imported speed', { skip: !imported.length && 'no imported vehicles' }, () => {
  const seen = new Set();
  for (const file of imported) {
    const data = JSON.parse(fs.readFileSync(file));
    const names = Object.keys(data.classes).filter((n) => !seen.has(n));
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
      const axis = new THREE.Vector3(), orientation = new THREE.Quaternion();
      const speed = () => v.flying ? v.airspeed : v.walking ? Math.hypot(v.velocity.x, v.velocity.z) : Math.abs(v.velocity.dot(axis.set(0, 0, 1).applyQuaternion(hullQuaternion(v, orientation))));
      const drive = (seconds, input) => { let top = -Infinity, low = Infinity; for (let i = 0; i < seconds * 60; i++) { system.update(1 / 60, v, input); const s = speed(); top = Math.max(top, s); low = Math.min(low, s); } return { top, low, now: speed() }; };
      v.driver = {};
      if (v.flying) {
        v.position.y += 200; v.syncBody();
        system.update(1 / 60, v, { takeoff: true });
        // Its lift (the cloud car's TakeoffTime is 2.5 s), then on to cruise.
        drive(8, {});
        assert.equal(v.flightState, 'Flying', `${name}: airborne`);
        const want = [number(def, 'MinSpeed', 10), number(def, 'MidSpeed', 20), number(def, 'MaxSpeed', 30)];
        assert.equal(v.airspeed, want[1], `${name}: cruise at MidSpeed`);
        assert.equal(drive(10, { forward: 1 }).now, want[2], `${name}: top speed is MaxSpeed`);
        assert.equal(drive(10, { forward: -1 }).now, want[0], `${name}: brake to MinSpeed`);
        continue;
      }
      const left = v.clips?.get('walk_leftfoot_rightfoot'), right = v.clips?.get('walk_rightfoot_leftfoot');
      const walking = v.walking && left && right;
      const top = walking ? (left.stride + right.stride) * 30 / (left.frames + right.frames) * number(def, 'AnimationSpeed', 1) : number(def, 'ForwardSpeed', 20);
      const back = walking ? top : number(def, 'ReverseSpeed', 10);
      // Hover spring forces add a small horizontal component as the hull settles.
      const tolerance = v.walking ? 1e-6 : .05;
      const forward = drive(10, { forward: 1 });
      assert.ok(Math.abs(forward.now - top) < tolerance && forward.top < top + tolerance, `${name}: top speed ${forward.now} is not ${top}`);
      const reverse = drive(12, { forward: -1 });
      assert.ok(Math.abs(reverse.now - back) < tolerance, `${name}: reverse ${reverse.now} is not ${back}`);
      if (v.walking) {
        drive(4, {});
        const side = drive(4, { right: 1 });
        assert.ok(side.top < 1e-6, `${name}: a walker turns on A/D, it does not strafe`);
      }
    }
    system.dispose?.();
  }
  assert.ok(seen.size > 0);
});
