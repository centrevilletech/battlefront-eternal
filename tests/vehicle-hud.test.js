import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { VehicleDriver } from '../src/vehicles.js';
import { EntitySeats } from '../src/runtime/entity-seats.js';

// The player at a craft's stick, as the page holds him (tests/flight-lifecycle.test.js).
function mounted() {
  const f = fixture(); f.v.driver = null;
  const soldier = { position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, spawn(x, y, z, yaw) { this.position.set(x, y, z); this.velocity.set(0, 0, 0); this.aimYaw = yaw; } };
  const actor = { root: new THREE.Group() }, animator = { gunner() {}, reset() {} }, combat = { dead: false, mounted: false, team: 1 };
  const unit = { id: 7, generation: 1, alive: true, soldier, actor, animator, combat }; combat.owner = unit;
  const driver = new VehicleDriver({ vehicles: f.system, soldier, combat, actor, animator, camera: f.system.camera, collision: f.c, hud: { message() {} }, presentation: { reset() {} } });
  const reader = new EntitySeats({ units: { byId: () => unit }, driver });
  reader.setLease(null, f.v, [unit.id, 1, 'pilot', unit.generation], value => { f.v.seats[0].lease = value; });
  driver.vehicle = f.v; f.v.driver = soldier; f.v.humanDriver = driver;
  return { ...f, driver, unit, soldier, combat };
}

test('a seat switch on the page leaves his body seated, as the world has it', () => {
  // His old seat empties before the new one is taken (the seat keys go in
  // seat order), which unseats the body on the way.
  const f = mounted();
  const seats = [{ lease: null, weapons: [] }, { lease: null, weapons: [] }];
  const v = { seats, alive: true }, game = { vehicles: { vehicles: [v] }, units: { byId: () => f.unit }, driver: {}, playerUnit: f.unit };
  f.unit.seat = null;
  const reader = new EntitySeats(game);
  reader.setLease(null,v,[7,1,'pilot',1],value=>{seats[0].lease=value;},0);
  assert.equal(f.unit.seat.index, 0);
  assert.ok(f.combat.mounted); assert.equal(f.unit.targetable, false);
  reader.setLease(null,v,null,value=>{seats[0].lease=value;},0);
  reader.setLease(null,v,[7,1,'gunner',1],value=>{seats[1].lease=value;},1);
  assert.equal(f.unit.seat, seats[1].lease);
  assert.equal(seats[0].lease, null);
  assert.ok(f.combat.mounted, 'still mounted: his own weapon stays out of his hands');
  assert.equal(f.unit.targetable, false, 'still no target of his own while he sits');
  assert.equal(game.driver.lease, seats[1].lease);
  // Off the hull altogether, he is on his feet.
  reader.setLease(null,v,null,value=>{seats[1].lease=value;},1);
  assert.equal(f.unit.seat, null); assert.equal(f.combat.mounted, false); assert.equal(f.unit.targetable, true);
});

test('a pilot\'s reticle follows the nose as drawn; any other seat its view', () => {
  const f = mounted();
  f.v.flightFrameQ = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), .5);
  f.v.quaternion.identity();
  const nose = f.driver.reticleDirection(new THREE.Vector3());
  assert.ok(nose.distanceTo(new THREE.Vector3(Math.sin(.5), 0, Math.cos(.5))) < 1e-9, 'the drawn frame, which the chase camera follows');
  f.v.flying = false;
  const view = f.driver.reticleDirection(new THREE.Vector3()), expected = f.driver.viewDirection(new THREE.Vector3());
  assert.ok(view.distanceTo(expected) < 1e-9);
});
