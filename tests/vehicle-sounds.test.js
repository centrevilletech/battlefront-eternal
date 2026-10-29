import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { VehicleSounds } from '../src/audio/vehicles.js';
import { AudioEngine } from '../src/audio/engine.js';
import { hashKey } from '../src/audio/hash.js';

// A hull's sounds come from its state alone (alive, health, flight state,
// velocity, who holds the pilot's seat), as every host gives it: the same
// code sounds a hull simulated here, a worker's replica and a server's.

const TURN_OFF = hashKey('TurnOffTime');
function rig(sounds, fields = {}) {
  const played = [];
  const engines = [];
  const voices = [];
  const v = { name: 'rep_fly_test', alive: true, health: 1000, maxHealth: 1000, flying: true, flightState: 'Landed', velocity: new THREE.Vector3(),
    position: new THREE.Vector3(), root: { position: new THREE.Vector3() }, aiTarget: { generation: 1 }, def: { settings: { MaxSpeed: '72' } },
    pilot: false, held(i) { return i === 0 && this.pilot; }, ...fields };
  const audio = {
    game: { vehicles: { vehicles: [v] }, camera: { position: new THREE.Vector3() } },
    data: { classSounds: () => sounds, sound: (name) => (name.startsWith('whir') ? { Gain: 0.5, Pitch: 1.2 } : null) },
    classSound: (cls, key) => (sounds[key] ? { name: sounds[key] } : null),
    playClass(cls, key, options) { played.push(key); return { stop() {} }; },
    engine: {
      playParameterized(name) {
        const h = { name, stopped: false, inputs: [], done: false, layers: [{ handle: { reach: 70 } }],
          apply(inputs) { this.inputs.push(JSON.parse(JSON.stringify(inputs))); }, stop() { this.stopped = true; this.done = true; } };
        engines.push(h);
        return h;
      },
      play(name, options) {
        const h = { name, options, gain: 1, pitch: null, done: false, setGain(g) { this.gain = g; }, setAbsolutePitch(p) { this.pitch = p; }, stop() { this.done = true; } };
        voices.push(h);
        return h;
      },
    },
  };
  const sounds_ = new VehicleSounds(audio);
  const run = (seconds, dt = 1 / 60) => { for (let t = 0; t < seconds - 1e-9; t += dt) sounds_.update(dt); };
  return { v, played, engines, voices, run, audio };
}

test('the pilot turns a hull on; leaving a landed flyer turns it off after TurnOffTime', () => {
  const { v, played, engines, run } = rig({ EngineSound: 'engine_parameterized', TurnOnSound: 'r2_turn_on', '7d2c2fa2': 'r2_turn_off', TurnOffSound: 'off', TurnOffTime: '2.0' });
  run(0.5);
  assert.deepEqual(played, []);
  v.pilot = true;
  run(0.1);
  assert.deepEqual(played, ['TurnOnSound']);
  assert.equal(engines.length, 1);
  assert.equal(engines[0].inputs.at(-1)[TURN_OFF].raw, true, 'an idle input leaves the layers alone');
  v.pilot = false;
  run(0.1);
  assert.deepEqual(played, ['TurnOnSound', '7d2c2fa2'], 'the turning-off sound, by its hash where the export has no name');
  assert.equal(engines[0].stopped, false);
  const fed = engines[0].inputs.at(-1)[TURN_OFF];
  assert.equal(fed.raw, false);
  assert.ok(fed.value > 0 && fed.value < 0.2, 'the engine hears the turn-off counting up');
  run(2);
  assert.deepEqual(played, ['TurnOnSound', '7d2c2fa2', 'TurnOffSound']);
  assert.equal(engines[0].stopped, true);
  run(1);
  assert.equal(engines.length, 1, 'no engine once off');
});

test('a flyer keeps its engine while airborne without a pilot, and sounds its takeoff and landing only when on', () => {
  const { v, played, engines, run } = rig({ EngineSound: 'engine', TakeoffSound: 'r2_take_off', LandSound: 'r2_land' });
  v.flightState = 'TakingOff';
  run(0.1);
  assert.deepEqual(played, [], 'off: no takeoff sound');
  v.flightState = 'Landed';
  run(0.1);
  v.pilot = true;
  run(0.1);
  v.flightState = 'TakingOff';
  run(0.1);
  assert.deepEqual(played, ['TakeoffSound']);
  v.flightState = 'Flying';
  v.pilot = false;
  run(5);
  assert.equal(engines[0].stopped, false, 'still flying');
  v.flightState = 'Landing';
  run(0.1);
  assert.deepEqual(played, ['TakeoffSound', 'LandSound']);
  v.flightState = 'Landed';
  run(3.5);
  assert.equal(engines[0].stopped, true, 'down and empty: it turns off');
});

test('hits worth more than 2% of what is left hurt; destruction plays DeathSound and stops the engine at once', () => {
  const { v, played, engines, run } = rig({ EngineSound: 'engine', HurtSound: 'r2_damage', DeathSound: 'boom' }, { flying: false, flightState: null });
  v.pilot = true;
  run(0.1);
  v.health -= 10;
  run(0.1);
  assert.deepEqual(played, []);
  v.health -= 100;
  run(0.1);
  assert.deepEqual(played, ['HurtSound']);
  v.alive = false; v.health = 0;
  run(0.1);
  assert.deepEqual(played, ['HurtSound', 'DeathSound']);
  assert.equal(engines[0].stopped, true);
  // A new hull in the slot starts silent and fresh.
  v.alive = true; v.health = 1000; v.aiTarget.generation = 2; v.pilot = false;
  run(0.5);
  assert.deepEqual(played, ['HurtSound', 'DeathSound']);
});

test('an engine out of its reach is let go and comes back without a turn-on sound', () => {
  const { v, played, engines, run } = rig({ EngineSound: 'engine', TurnOnSound: 'on' }, { flying: false, flightState: null });
  v.pilot = true;
  run(0.1);
  assert.equal(engines.length, 1);
  v.root.position.set(500, 0, 0);
  run(0.1);
  assert.equal(engines[0].stopped, true);
  v.root.position.set(5, 0, 0);
  run(1.2);
  assert.equal(engines.length, 2);
  assert.deepEqual(played, ['TurnOnSound']);
});

test('parameterized layers multiply every fed input through its graph, or raw', () => {
  const engine = new AudioEngine();
  engine.ctx = {};
  const objects = {
    SoundParameterized: { engine: { SoundLayered: 'layers', 'Layer.children': [
      { Name: ['a'], ParameterGraph: [hashKey('TurnOffTime'), 'fade', hashKey('Gain')] },
      { Name: ['a'], ParameterGraph: [hashKey('Health'), 'hurt', hashKey('Gain')] },
      { Name: ['b'], ParameterGraph: [hashKey('TurnOffTime'), 'drop', hashKey('Pitch')] }] } },
    SoundLayered: { layers: { Layer: [['a', 'sound_a'], ['b', 'sound_b']] } },
    ParameterGraph: { fade: { ControlPoint: [[0, 1], [3, 0]] }, hurt: { ControlPoint: [[0, 2], [1, 1]] }, drop: { ControlPoint: [[0, 1], [2, 0.6]] } },
  };
  engine.data = { object: (kind, name) => objects[kind]?.[name] ?? null };
  const voices = {};
  engine.play = (key) => (voices[key] = { gain: 1, pitch: 1, setGain(g) { this.gain = g; }, setPitch(p) { this.pitch = p; }, stop() {} });
  const handle = engine.playParameterized('engine');
  handle.apply({ [hashKey('TurnOffTime')]: { value: 1.5, raw: false }, [hashKey('Health')]: { value: 0.5, raw: false } });
  assert.ok(Math.abs(voices.sound_a.gain - 0.75) < 1e-9, '0.5 x 1.5, held within 1');
  assert.ok(Math.abs(voices.sound_b.pitch - 0.7) < 1e-9, 'three quarters of the way from 1 to 0.6');
  handle.apply({ [hashKey('TurnOffTime')]: { value: 1, raw: true }, [hashKey('Health')]: { value: 1, raw: false } });
  assert.equal(voices.sound_a.gain, 1);
  assert.equal(voices.sound_b.pitch, 1);
});

test('a walker steps with its foot\'s FootstepSound and lifts with HydraulicSound', () => {
  const { v, played, run } = rig({ '97157af6': 'at_step', FootstepSound1: 'at_step', HydraulicSound: 'leg_up' }, { flying: false, flightState: null });
  const foot = new THREE.Object3D();
  v.root = new THREE.Object3D(); v.root.add(foot);
  v.byName = { bone_l_foot: foot };
  v.def.fx = { feet: [{ name: 'p_l_foot', bone: 'bone_l_foot', at: [0, 0, 0] }, { name: 'p_r_foot', bone: '', at: [0, 0, 0] }] };
  const at = (y) => { foot.position.y = y; v.root.updateMatrixWorld(true); run(0.1, 0.1); };
  at(3); at(2); at(1); at(1);
  assert.deepEqual(played, ['97157af6'], 'foot 0 down: FootstepSound0, by its hash where the export has no name');
  at(1.5);
  assert.deepEqual(played, ['97157af6', 'HydraulicSound']);
});

test('a crash (speed lost faster than CollisionThreshold) sounds; braking and a flyer settling to land do not', () => {
  const { v, played, run } = rig({ VehicleCollisionSound: 'crash' }, { flying: false, flightState: null });
  v.def.settings.CollisionThreshold = '5.0';
  v.velocity.set(0, 0, 20); run(0.1);
  v.velocity.set(0, 0, 17); run(1 / 60);
  assert.deepEqual(played, [], 'a brake');
  v.velocity.set(0, 0, 2); run(1 / 60);
  assert.deepEqual(played, ['VehicleCollisionSound']);
  const flyer = rig({ VehicleCollisionSound: 'crash' }, { flightState: 'Flying' });
  flyer.v.velocity.set(0, 0, 30); flyer.run(0.1);
  flyer.v.flightState = 'Landing'; flyer.v.velocity.set(0, 0, 1); flyer.run(1 / 60);
  assert.deepEqual(flyer.played, [], 'the landing path takes over the velocity');
  flyer.v.flightState = 'Flying'; flyer.v.velocity.set(0, 0, 30); flyer.run(0.5);
  flyer.v.velocity.set(0, 0, 5); flyer.run(1 / 60);
  assert.deepEqual(flyer.played, ['VehicleCollisionSound']);
  // A page drawing at 5 frames a second sees a tenth of a second's braking at once: no crash.
  const slow = rig({ VehicleCollisionSound: 'crash' }, { flying: false, flightState: null });
  slow.v.velocity.set(0, 0, 20); slow.run(0.4, 0.2);
  slow.v.velocity.set(0, 0, 12); slow.run(0.2, 0.2);
  assert.deepEqual(slow.played, [], 'a brake over 0.2 s');
  slow.v.velocity.set(0, 0, 12); slow.run(0.4, 0.2);
  slow.v.velocity.set(0, 0, -50); slow.run(0.2, 0.2);
  assert.deepEqual(slow.played, [], 'speed is a magnitude: a reversal at the same speed is no drop');
  slow.v.velocity.set(0, 0, 90); slow.run(0.4, 0.2);
  slow.v.velocity.set(0, 0, 0); slow.run(0.2, 0.2);
  assert.deepEqual(slow.played, ['VehicleCollisionSound'], 'a stop from 90 m/s');
});

test('a turret seat sounds as it is taken, whirs while its aim turns, and sounds as it empties', () => {
  const sounds = { TurretActivateSound: 'vehicle_equip', TurretDeactivateSound: 'vehicle_equip', TurretYawSound: 'whir_yaw', TurretPitchSound: 'whir_pitch',
    TurretYawSoundPitch: '0.7', TurretPitchSoundPitch: '0.7' };
  const turret = new THREE.Object3D();
  turret.position.set(0, 3, 0);
  const root = new THREE.Object3D();
  root.add(turret);
  root.updateMatrixWorld(true);
  const seats = [{ settings: {}, aimYaw: 0, pitch: 0, lease: null }, { settings: { PitchRate: '1' }, view: { turret, turnRate: 2 }, aimYaw: 0, pitch: 0, lease: null }];
  const { played, voices, run } = rig(sounds, { flying: false, flightState: null, seats, root, held(i) { return Boolean(this.seats[i].lease); } });
  run(0.1);
  assert.deepEqual(played, [], 'as first seen: no sound');
  seats[1].lease = {};
  run(1 / 60);
  assert.deepEqual(played, ['TurretActivateSound']);
  // At the seat's top rate (TurnRate 2 rad/s): the whir's full gain, and the sound's own pitch.
  for (let i = 0; i < 30; i++) { seats[1].aimYaw += 2 / 60; run(1 / 60); }
  const yaw = voices.find((h) => h.name === 'whir_yaw');
  assert.ok(yaw && !yaw.done);
  assert.ok(Math.abs(yaw.gain - 1) < 0.02, `full gain, not ${yaw.gain}`);
  assert.ok(Math.abs(yaw.pitch - 1.2) < 0.01);
  assert.equal(voices.some((h) => h.name === 'whir_pitch'), false, 'a resting pitch does not whir');
  assert.deepEqual(yaw.options.follow(), { x: 0, y: 3, z: 0 }, 'at the turret node');
  // Half the rate: half the gain, the pitch halfway from 0.7 of it.
  for (let i = 0; i < 30; i++) { seats[1].aimYaw += 1 / 60; run(1 / 60); }
  assert.ok(Math.abs(yaw.gain - 0.5) < 0.03);
  assert.ok(Math.abs(yaw.pitch - 0.85 * 1.2) < 0.02);
  // Aim updates a frame apart (a worker's ticks): the rate over the time between them, no fading in the gaps.
  for (let i = 0; i < 30; i++) { if (i % 2 === 0) seats[1].aimYaw += 2 / 60; run(1 / 60); }
  assert.ok(Math.abs(yaw.gain - 0.5) < 0.03, `held between ticks, not ${yaw.gain}`);
  // Stopped: it fades out.
  run(1);
  assert.equal(yaw.done, true);
  seats[1].lease = null;
  run(1 / 60);
  assert.deepEqual(played, ['TurretActivateSound', 'TurretDeactivateSound']);
});

test('a placed turret sounds for whoever mans it: taken, whirring as its drum turns, emptied; destroyed, silently', () => {
  const sounds = { TurretActivateSound: 'vehicle_equip', TurretDeactivateSound: 'vehicle_equip', TurretYawSound: 'whir_yaw', TurretPitchSound: 'whir_pitch',
    TurretYawSoundPitch: '0.7', TurretPitchSoundPitch: '0.7' };
  // A turret as src/props.js has it on any host: its state, lease, drum and angles.
  const root = new THREE.Group();
  root.matrixAutoUpdate = false;
  root.matrix.makeTranslation(30, 0, 0);
  const drum = new THREE.Object3D(); drum.position.set(0, 2, 0);
  root.add(drum);
  const turret = { desc: { class: 'com_weap_gunturret' }, state: 'built', get alive() { return this.state === 'built'; }, lease: null,
    yaw: 0, pitch: 0, turnRate: 2.85, drum, position: new THREE.Vector3(30, 0, 0) };
  const r = rig(sounds, { flying: false, flightState: null });
  const played = [];
  r.audio.playClass = (cls, key, options) => { played.push([cls, key, options.position && { ...options.position }]); return { stop() {} }; };
  r.audio.game.live = { turrets: [turret] };
  r.run(0.1);
  assert.deepEqual(played, [], 'as first seen: no sound');
  // A bot (or another player online) takes it: its lease arrives with the seat.
  turret.lease = { human: false };
  r.run(1 / 60);
  assert.deepEqual(played, [['com_weap_gunturret', 'TurretActivateSound', { x: 30, y: 2, z: 0 }]], 'at the drum');
  for (let i = 0; i < 30; i++) { turret.yaw += 2.85 / 60; r.run(1 / 60); }
  const yaw = r.voices.find((h) => h.name === 'whir_yaw');
  assert.ok(yaw && !yaw.done, 'the drum turning whirs');
  assert.ok(Math.abs(yaw.gain - 1) < 0.02, `at MaxTurnSpeed, full gain, not ${yaw.gain}`);
  for (let i = 0; i < 30; i++) { turret.pitch -= 1 / 60; r.run(1 / 60); }
  assert.ok(r.voices.some((h) => h.name === 'whir_pitch' && !h.done));
  turret.lease = null;
  r.run(1 / 60);
  assert.deepEqual(played.map(([, key]) => key), ['TurretActivateSound', 'TurretDeactivateSound']);
  assert.ok(r.voices.every((h) => h.done), 'the whirs stop with it');
  turret.lease = { human: true };
  r.run(1 / 60);
  for (let i = 0; i < 10; i++) { turret.yaw += 2 / 60; r.run(1 / 60); }
  turret.state = 'destroyed';
  r.run(1 / 60);
  assert.deepEqual(played.map(([, key]) => key), ['TurretActivateSound', 'TurretDeactivateSound', 'TurretActivateSound'], 'its blast is all it sounds');
  assert.ok(r.voices.every((h) => h.done));
  turret.lease = null; turret.state = 'built';
  r.run(1 / 60);
  assert.equal(played.length, 3, 'built again, empty: nothing');
});

test('a multi-turret hull repeats its whir pitches per turret: the pitch stays finite', () => {
  // rep_fly_gunship, rep_walk_atte and tat_hover_skiff set TurretYawSoundPitch
  // once per turret, so the export gives an array. Number(['0.7','0.7']) is
  // NaN, which reached an AudioParam and threw every frame (2026-10-14).
  const sounds = { TurretActivateSound: 'vehicle_equip', TurretDeactivateSound: 'vehicle_equip', TurretYawSound: 'whir_yaw', TurretPitchSound: 'whir_pitch',
    TurretYawSoundPitch: ['0.7', '0.7'], TurretPitchSoundPitch: ['0.7', '0.7'] };
  const seats = [{ settings: {}, aimYaw: 0, pitch: 0, lease: null }, { settings: { PitchRate: '1' }, view: { turnRate: 2 }, aimYaw: 0, pitch: 0, lease: null }];
  const { voices, run } = rig(sounds, { flying: false, flightState: null, seats, held(i) { return Boolean(this.seats[i].lease); } });
  run(0.1);
  seats[1].lease = {};
  for (let i = 0; i < 30; i++) { seats[1].aimYaw += 1 / 60; run(1 / 60); }
  const yaw = voices.find((h) => h.name === 'whir_yaw');
  assert.ok(yaw && !yaw.done, 'the turret whirs');
  assert.ok(Number.isFinite(yaw.pitch), `whir pitch is finite, not ${yaw.pitch}`);
  assert.ok(Math.abs(yaw.pitch - 0.85 * 1.2) < 0.02, 'halfway from 0.7 of the pitch at half rate');
  assert.ok(Number.isFinite(yaw.gain), 'whir gain is finite');
});
