import './fixtures/native-anim.js';
// The shared boarding rules (src/boarding.js) as every path uses them: the
// network human through Vehicles.board and the
// bots. Synthetic fixtures only: a flat floor, box hulls, made-up weapons.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Vehicles } from '../src/vehicles.js';
import { blankIntent } from '../src/units.js';
import { mountInReach, seatControl, seatScheme, turretTakeover } from '../src/boarding.js';

// Identity checks that never print their operands (a unit's object graph).
const same = (actual, expected, message) => assert.ok(actual === expected, message ?? `${typeof actual} !== ${typeof expected}`);

const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};
const cannon = () => ({ class: 'test_cannon', ammo: 0, aimers: [], def: { class: 'test_cannon', base: 'cannon', ShotDelay: 0.2, RoundsPerClip: 0, ReloadTime: 0, ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 2, Damage: 50 } } });

// One hull of `base` with `seats` seats on its pad at the origin, team 1.
function hull({ base = 'hover', seats = 2, name = 'tank' } = {}) {
  const floor = box(0, -1, 0, 4000, 2, 4000);
  const c = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const settings = { MaxHealth: 1000, ForwardSpeed: 20, ReverseSpeed: 8, Acceleration: 10, Deceleration: 10, SetAltitude: 0.5, TurnRate: 1.7, PitchRate: 0.5 };
  const seatDefs = [{ name: 'BODY', settings: {}, weapons: [cannon()] }];
  for (let i = 1; i < seats; i++) seatDefs.push({ name: `TURRET${i}`, settings: { TurnRate: 1.5, PitchRate: 0.5 }, weapons: [cannon()] });
  const def = { model: 'hull', base, label: 'Test hull', settings, seats: seatDefs, explosion: null };
  const data = { models: { hull: { bones: [], parts: [] } }, collision: { hull: box(0, 1, 0, 2, 2, 4) }, classes: { [name]: def },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', respawnTime: 2, classes: { 1: name } }] };
  const ordnance = { fire() {}, explosion() {}, lights: { flash() {} } };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision: c,
    texture: () => null, camera: new THREE.PerspectiveCamera(), posts: { posts: [] }, ordnance, effects: { play() { return null; } } });
  return { c, system, v: system.vehicles[0] };
}

let nextId = 100;
// A bot, or with person a network human, of `team`, standing at (x, 0, z).
function soldier(team = 1, { person = false, x = 4, z = 0 } = {}) {
  const body = {
    position: new THREE.Vector3(x, 0, z), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, air: null, fuel: 0.3, grounded: true,
    spawn(px, py, pz, yaw) { this.position.set(px, py, pz); this.aimYaw = yaw; this.velocity.set(0, 0, 0); this.air = null; this.grounded = py < 0.5; },
    setAir(kind) { this.air = kind; },
  };
  const unit = {
    id: nextId++, generation: 1, team, alive: true, dead: false, human: person, networkHuman: person, walker: false, seat: null, controller: null,
    classDef: {}, intent: blankIntent(), soldier: body,
    actor: { root: new THREE.Group(), byName: {} },
    animator: { clips: new Map(), gunner() {}, reset() {} },
    presentation: { reset() {} },
    combat: { mounted: false, dead: false, team, damage() { unit.alive = false; unit.dead = true; this.dead = true; } },
  };
  return unit;
}

test('E reaches a bot-manned turret for a person, never another person\'s', () => {
  const turret = { gunner: {}, lease: { released: false, human: false, unit: {} } };
  same(turretTakeover(turret), true, "a bot's turret can be taken over");
  turret.lease = { released: false, human: true, unit: {} };
  same(turretTakeover(turret), false, "a person's turret cannot");
  turret.lease = null;
  same(turretTakeover(turret), false, 'a gunner with no lease (the local player) cannot be displaced');
  turret.gunner = null;
  same(turretTakeover(turret), true, 'an unmanned turret is free to take');
  const looker = soldier(1, { person: true, x: 0, z: -5 });
  looker.soldier.aimYaw = 0; looker.soldier.aimPitch = 0;
  const live = { enterable: (eye, aim, may) => (may?.(turret) ? turret : null) };
  turret.gunner = {}; turret.lease = { released: false, human: false, unit: {} };
  same(mountInReach({ live }, looker, true)?.turret, turret, "a person's E is offered the bot's turret");
  same(mountInReach({ live }, looker, false), null, "a bot's E is not");
});


test('seat controls: only a craft\'s pilot flies it, and a craft reads the flight key map', () => {
  const { v: craft } = hull({ base: 'flyer', seats: 2, name: 'ship' });
  const { v: tank } = hull();
  same(seatControl(craft, 0), 'flight');
  same(seatControl(craft, 1), 'turret');
  same(seatControl(tank, 0), 'drive');
  same(seatControl(tank, 1), 'turret');
  same(seatScheme(tank, undefined), null, 'on foot or driving, the soldier\'s keys');
  same(seatScheme(craft, undefined).fireSecondary[0], 'Mouse2', 'in a craft the flight bindings');
});

import { playersWorld, playerCommand } from './fixtures/players-world.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { KIND_SEAT_LEASE } from '../src/sim/gen/kinds.js';

for (const base of ['hover', 'flyer']) test(`native ${base} controls read the occupied seat with default flight bindings`, async t => {
  t.mock.method(performance, 'now', () => 1000);
  const { game, world } = await playersWorld({ fleet: fleetFixture({ base, seats: 2, positions: [20] }) });
  await world.addPlayer('pilot', { team: 1, separateActions: true });
  const p = world.players.get('pilot'), v = game.vehicles.vehicles[0];
  const command = tick => playerCommand(p, tick, { down: ['KeyW', 'AltLeft'] });
  assert.equal(game.vehicles.board(p.unit, v), true);
  assert.equal(world.input('pilot', command(1)), true);
  world.step(1000);
  assert.equal(p.unit.intent.forward, 1);
  assert.equal(p.unit.intent.buttons[1], base === 'flyer' ? 0 : 1,
    'Alt is a flight trick in a craft and secondary fire in a ground hull');
  assert.equal(world.actions('pilot', { epoch: 1, id: 1, tick: 2, generation: p.unit.generation, actions: ['nextPrimary'] }), true);
  assert.equal(world.input('pilot', command(2)), true);
  world.step(1000);
  const nativeLease = game.simBridge.read(KIND_SEAT_LEASE).find(row => row.object.unit === p.unit);
  assert.equal(nativeLease.object.index, 1, 'the native command selected the gunner seat');
  assert.equal(p.unit.seat.index, 1, 'the host must publish that same seat after the step');
  assert.equal(p.unit.intent.forward, 0, 'the gunner never drives or flies');
  assert.equal(p.unit.intent.nextPrimary, false, 'the seat switch consumes its press');
  assert.equal(world.actions('pilot', { epoch: 1, id: 2, tick: 3, generation: p.unit.generation, actions: ['previousPrimary'] }), true);
  world.step(1000); assert.equal(p.unit.seat.index, 0);
  assert.equal(p.unit.intent.forward, 1, 'held movement follows the seat after the switch');
  world.removePlayer('pilot');
  assert.equal(p.unit.seat, null); assert.equal(v.seats[0].lease, null);
  game.vehicles.dispose();
});
