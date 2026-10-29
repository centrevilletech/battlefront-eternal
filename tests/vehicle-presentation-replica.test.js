import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import {captureEntityVehicle} from '../src/runtime/network-entity-capture.js';
import { VehicleEffects } from '../src/vehicle-effects.js';
import { VehicleSounds } from '../src/audio/vehicles.js';

// Semantic hull records contain gameplay state. The hull's
// effects and sounds are the page's alone: their handles live in the
// presentation modules, never on the hull, so a resync (even from a world
// that still carries the old smoke, engine and wake fields as null) can't
// orphan or restart them.

const PAGE_ONLY = ['smoke', 'engine', 'engineRetry', 'wake', 'audioFollow'];

test('a hull carries no presentation handles among its replicated fields', () => {
  const { v } = fixture({ MaxSpeed: '72' });
  const record=captureEntityVehicle(v,{tick:1,classId:1});
  for (const key of PAGE_ONLY) assert.ok(!(key in record), `${key} would travel`);
  for (const key of PAGE_ONLY) assert.ok(!(key in v), `${key} on the hull`);
});

test('a keyframe resync leaves the page\'s effect and engine handles running', () => {
  const page = fixture({ MaxSpeed: '72' }).v, world = fixture({ MaxSpeed: '72' }).v;
  for (const v of [page, world]) {
    v.def.fx = { thrust: [{ effect: 'exhaust', node: '', offset: [0, 0, -2], minScale: 0.5, maxScale: 1.4, scaleStart: 0.5 }], speed: [20, 72],
      damage: [{ start: 0.6, stop: 0, effect: 'smoke', scale: 1, inherit: 0, node: '', offset: [0, 0, 0] }] };
    v.health = v.maxHealth * 0.5; v.flightState = 'Flying';
  }
  const played = [];
  const effects = { play(name, p) { const h = { name, position: p.clone(), stopped: false, done: false, scale: 1, move() {}, stop() { this.stopped = true; } }; played.push(h); return h; } };
  const fx = new VehicleEffects({ effects });
  const engines = [];
  const audio = { game: { vehicles: { vehicles: [page] }, camera: { position: new THREE.Vector3() } }, data: { classSounds: () => ({}) },
    classSound: (cls, key) => (key === 'EngineSound' ? { name: 'engine' } : null), playClass: () => null,
    engine: { playParameterized() { const h = { stopped: false, done: false, layers: [], apply() {}, stop() { this.stopped = true; this.done = true; } }; engines.push(h); return h; } } };
  const sounds = new VehicleSounds(audio);
  page.driver = {}; page.seatIndex = 0;
  for (let i = 0; i < 10; i++) { fx.update([page], 1 / 60); sounds.update(1 / 60); }
  assert.deepEqual(played.map(h => h.name), ['exhaust', 'smoke']);
  assert.equal(engines.length, 1);
  // The world's hull, as an older world still has it, copied over the page's.
  const record=captureEntityVehicle(world,{tick:1,classId:1});
  page.position.fromArray(record.position);page.health=record.health;page.quaternion.fromArray(record.quaternion);
  for (let i = 0; i < 10; i++) { fx.update([page], 1 / 60); sounds.update(1 / 60); }
  assert.equal(played.length, 2, 'nothing restarted');
  assert.ok(played.every(h => !h.stopped), 'nothing orphaned or stopped');
  assert.equal(engines.length, 1);
  assert.equal(engines[0].stopped, false);
});
