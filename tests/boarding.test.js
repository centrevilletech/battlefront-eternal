// The shared boarding rules (src/boarding.js) as every path uses them: the
// player's VehicleDriver, a network human through Vehicles.board, and the
// bots. Synthetic fixtures only: a flat floor, box hulls, made-up weapons.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
import { Units } from '../src/registry.js';
import { blankIntent } from '../src/units.js';
import { CommandControls } from '../src/runtime/commands.js';
import { cycleNetworkSeat } from '../src/runtime/network-seat.js';
import { boardingSeat, mayLeave, mountInReach, seatControl, seatInput, seatScheme, turretTakeover } from '../src/boarding.js';

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

// The player: his VehicleDriver and body.
function player(system, c, team = 1) {
  const body = { position: new THREE.Vector3(4, 0, 0), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, air: null, spawn(x, y, z, yaw) { this.position.set(x, y, z); this.aimYaw = yaw; this.velocity.set(0, 0, 0); } };
  const unit = { team, generation: 1, alive: true, seat: null, human: true };
  const combat = { dead: false, mounted: false, team, owner: unit };
  const driver = new VehicleDriver({ vehicles: system, soldier: body, combat, actor: { root: new THREE.Group() }, animator: {}, clips: new Map(),
    camera: system.camera, collision: c, hud: { message() {} } });
  return { driver, unit, body, combat };
}

test('a hull an enemy holds in any seat takes nobody; an empty one of any team takes a person; bots keep to their own team', () => {
  const { system, v } = hull();
  same(boardingSeat(v, { team: 2, person: true, relation: system.relation }), 0, 'an empty enemy hull takes a person');
  same(boardingSeat(v, { team: 2, relation: system.relation }), -1, 'but not an enemy bot');
  // An enemy gunner aboard, the pilot's seat empty (a playtest report: the player got in anyway).
  const gunner = soldier(1);
  assert.ok(system.board(gunner, v, 1));
  same(boardingSeat(v, { team: 2, person: true, relation: system.relation }), -1, 'a gunner of the other team keeps a person out');
  same(boardingSeat(v, { team: 2, relation: system.relation }), -1);
  same(boardingSeat(v, { team: 1, person: true, relation: system.relation }), 0, 'his friends may take the free seat');
  same(boardingSeat(v, { team: 1, relation: system.relation }), 0);
  // The player's own path, with the same rule.
  const enemy = player(system, system.collision, 2);
  same(enemy.driver.enter(v), false, 'the player cannot get into it');
  same(v.seats[1].lease.unit, gunner, 'and nobody aboard is moved');
  same(v.seats[0].lease, null);
  const friend = player(system, system.collision, 1);
  assert.ok(friend.driver.enter(v), 'a friend of the crew gets in');
  same(v.seatIndex, 0);
  friend.driver.release();
  // A network human: the same rules through Vehicles.board.
  const netEnemy = soldier(2, { person: true });
  same(system.eligibleSeat(netEnemy, v), -1);
  same(system.board(netEnemy, v), false);
  const netFriend = soldier(1, { person: true });
  same(system.board(netFriend, v), true);
  same(netFriend.seat.index, 0);
  // Allies count as friends (the registry's relation).
  const { system: allied, v: w } = hull();
  const units = new Units();
  units.setRelation(1, 3, 1);
  allied.attachUnits(units);
  assert.ok(allied.board(soldier(1), w, 1));
  same(boardingSeat(w, { team: 3, person: true, relation: allied.relation }), 0, 'an ally takes the free pilot seat');
  same(boardingSeat(w, { team: 2, person: true, relation: allied.relation }), -1, 'an enemy does not');
});

test('a person takes a seat a friendly bot holds and the bot moves on; never a seat another person holds', () => {
  const { system, v, c } = hull({ seats: 2 });
  const pilot = soldier(1);
  assert.ok(system.board(pilot, v, 0));
  const net = soldier(1, { person: true });
  assert.ok(system.board(net, v), 'a network human boards as the player does');
  same(net.seat.index, 0, 'into the bot\'s pilot seat');
  same(pilot.seat.index, 1, 'the bot moved on to the free seat');
  const other = soldier(1, { person: true });
  assert.ok(system.board(other, v), 'another person takes the bot\'s seat');
  same(other.seat.index, 1);
  same(pilot.seat, null, 'with no seat left the bot gets out');
  same(system.board(soldier(1, { person: true }), v), false, 'two persons fill it: neither is pushed out');
  const p = player(system, c, 1);
  same(p.driver.enter(v), false);
  // E reaches past a hull an enemy holds.
  const { system: s2, v: held } = hull();
  assert.ok(s2.board(soldier(1), held, 1));
  const looker = soldier(2, { person: true, x: 0, z: -5 });
  looker.soldier.aimYaw = 0; looker.soldier.aimPitch = 0;
  same(mountInReach({ vehicles: s2 }, looker, true), null, 'an enemy-held hull is not offered');
  const ally = soldier(1, { person: true, x: 0, z: -5 });
  same(mountInReach({ vehicles: s2 }, ally, true)?.vehicle, held);
  ally.soldier.air = 'jump';
  same(mountInReach({ vehicles: s2 }, ally, true), null, 'a person in the air waits for his feet');
});

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

test('bailing out: a person leaves a craft in flight beside it, at rest; a bot stays aboard', () => {
  const { system, v } = hull({ base: 'flyer', seats: 2, name: 'ship' });
  v.flightState = 'Flying'; v.position.set(0, 120, 0); v.velocity.set(0, 0, 60); v.syncBody();
  const bot = soldier(1), net = soldier(1, { person: true });
  assert.ok(system.board(bot, v, 1));
  assert.ok(system.board(net, v, 0));
  same(mayLeave(v, bot.seat), false);
  same(mayLeave(v, net.seat), true);
  same(system.unboard(bot), false, 'a bot does not bail out');
  bot.seat.exitRequest = null;
  assert.ok(system.unboard(net), 'the network human bails out');
  same(net.seat, null);
  assert.ok(Math.abs(net.soldier.position.y - 120) < 1e-9, 'left in the air at the craft\'s height');
  assert.ok(Math.hypot(net.soldier.position.x, net.soldier.position.z) > v.radius, 'clear of the hull');
  same(net.soldier.velocity.length(), 0, 'at rest, as the original game leaves him');
  same(net.soldier.air, 'fall');
  same(bot.seat.index, 0, 'the bot aboard takes the stick (promotion)');
  // Landed, both may leave.
  v.flightState = 'Landed';
  same(mayLeave(v, bot.seat), true);
});

test('an empty craft in flight comes down and is destroyed; one with someone aboard holds its course', () => {
  const empty = hull({ base: 'flyer', seats: 2, name: 'ship' });
  const v = empty.v;
  v.flightState = 'Flying'; v.airspeed = 60; v.position.set(0, 80, 0); v.velocity.set(0, 0, 60); v.syncBody();
  let t = 0;
  for (; t < 1200 && v.alive; t++) empty.system.update(1 / 60);
  same(v.alive, false, 'it crashed');
  assert.ok(t > 60 && t < 600, `down in ${t} ticks`);
  const crewed = hull({ base: 'flyer', seats: 2, name: 'ship' });
  const w = crewed.v;
  w.flightState = 'Flying'; w.airspeed = 60; w.position.set(0, 80, 0); w.velocity.set(0, 0, 60); w.syncBody();
  assert.ok(crewed.system.board(soldier(1, { person: true }), w, 1), 'a network human in the gunner seat');
  for (let i = 0; i < 240; i++) crewed.system.update(1 / 60);
  assert.ok(w.alive && w.flightState === 'Flying', 'flying on');
  assert.ok(w.position.y > 70, `level, at ${w.position.y.toFixed(1)} m`);
});

test('seat controls: only a craft\'s pilot flies it, and its keys are read for the seat held when read', () => {
  const { v: craft } = hull({ base: 'flyer', seats: 2, name: 'ship' });
  const { v: tank } = hull();
  same(seatControl(craft, 0), 'flight');
  same(seatControl(craft, 1), 'turret');
  same(seatControl(tank, 0), 'drive');
  same(seatControl(tank, 1), 'turret');
  const controls = new CommandControls();
  controls.accept({ epoch: 1, sequence: 1, generation: 1, down: ['KeyW', 'AltLeft'], actions: ['takeoff'] });
  // A command without flight settings (a playtest crash: no bindings yet).
  const flight = seatInput(craft, 0, controls, {});
  same(flight.forward, 1);
  same(flight.takeoff, true);
  same(seatInput(craft, 1, controls, {}).forward, 0, 'the gunner does not fly the craft');
  same(seatInput(tank, 0, controls, {}).forward, 1);
  same(seatInput(tank, 1, controls, {}).forward, 0);
  // The fire keys follow the seat at the moment they are read.
  let seated = null;
  controls.flightScheme = () => seatScheme(seated, undefined);
  same(controls.held('fireSecondary'), true, 'on foot Alt fires the secondary');
  seated = craft;
  same(controls.held('fireSecondary'), false, 'in a craft Alt is a trick, read at once after boarding');
});

test('seat switching: a bot trades places, persons are skipped, and the stick is let go', () => {
  const { system, v, c } = hull({ seats: 3 });
  const p = player(system, c, 1);
  assert.ok(p.driver.enter(v));
  const bot = soldier(1);
  assert.ok(system.board(bot, v, 1));
  v.stickX = v.stickY = .5;
  p.driver.nextSeat(v);
  same(v.seatIndex, 1);
  same(bot.seat.index, 0, 'the bot took the seat he left');
  same(v.stickX, 0, 'leaving the pilot seat lets go of the stick');
  const net = soldier(1, { person: true });
  assert.ok(system.board(net, v, 2));
  p.driver.nextSeat(v);
  same(v.seatIndex, 0, 'the network human\'s seat is skipped');
  same(bot.seat.index, 1);
  same(cycleNetworkSeat(system, net, 1), true);
  same(net.seat.index, 1, 'the network human trades with the bot, not the player');
  same(bot.seat.index, 2);
  p.driver.nextSeat(v, -1);
  same(v.seatIndex, 2, 'back one seat, past the person');
});
