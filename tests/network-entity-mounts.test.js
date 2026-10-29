import {decodeOwnPlayer,OWN_PLAYER_SCHEMA} from '../src/runtime/network-entity-player.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Object3D } from 'three';
import { Soldier } from '../src/soldier.js';
import { SoldierPresentation } from '../src/soldier-presentation.js';
import { PlayerCombat } from '../src/combat.js';
import { Arsenal } from '../src/weapons.js';
import { Unit } from '../src/units.js';
import { CollisionWorld } from '../src/collision.js';
import { EntitySeats } from '../src/runtime/entity-seats.js';
import { EntityGameState } from '../src/runtime/network-entity-game.js';
import { EntityWorldCapture, captureEntityVehicle, captureEntityMount, captureEntityTurret, captureEntitySoldier, tryCapturePlayerCorrection, EntityCaptureUnsupported, ENTITY_VEHICLE_STATES } from '../src/runtime/network-entity-capture.js';
import { encodeEntityPacket, decodeEntityPacket, encodeEntityRecord } from '../src/runtime/network-entity-codec.js';
import { mountFixture } from './mount-fixture.js';
import { createSnapshotSender } from './snapshot-send.js';

// Real Soldier, PlayerCombat, Unit and SeatLease objects; real Vehicle hulls
// from the shared Vehicles system (tests/mount-fixture.js). No stubs stand in
// for simulation state.
const registry = { contentEpoch: 1, classes: new Map([[1, 'trooper'], [2, 'tank'], [3, 'tank2']]), weapons: new Map([[1, 'rifle']]), inputCodes: [], bindingProfiles: new Map([[0, {}]]) };
const ANGLE = Math.PI / 32767;
function body(id, generation = 1) {
  const world = new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
  const soldier = new Soldier(world, {}); soldier.spawn(0, 0, 0, 0);
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { soldier, arsenal: new Arsenal([{ class: 'rifle', channel: 0, RoundsPerClip: 30, ammo: 3, base: 'cannon' }]), health: 100, dead: false, deathTime: 0, zoomStep: 0, firstPerson: true, kick: { rate: 0, hold: 0, offset: 0 }, updateHeld() {} });
  const unit = new Unit({ className: 'trooper', def: {}, soldier, combat, presentation: new SoldierPresentation(soldier), actor: { root: new Object3D() }, animator: { reset() {}, gunner() {} } });
  Object.assign(unit, { id, generation, team: 1, removed: false, alive: true });
  return unit;
}
function registryUnits(...units) {
  const byIdMap = new Map(units.map(u => [u.id, u]));
  return { byIdMap, byId(id) { return byIdMap.get(id); }, all() { return [...byIdMap.values()]; } };
}
function client(classes = ['tank']) {
  const { system, v } = mountFixture({ classes });
  const own = body(1), remote = body(2), units = registryUnits(own, remote);
  const driver = { vehicle: null, lease: null, soldier: own.soldier, released: 0, synced: 0, release() { this.released++; this.vehicle = null; this.lease = null; }, sync() { this.synced++; } };
  const game = { ordnance: { clearProjectilePresentation() {} }, playerUnit: own, playerTeam: 1, units, vehicles: system, turretSeats: { turrets: [] }, live: { buildings: [] }, driver, gunner: null,
    factory: { build() { throw Error('unexpected body build'); } }, bodies: new Map(), adopt() {}, setHumanTeam() {} };
  game.client = { seats: new EntitySeats(game) };
  const adapter = new EntityGameState(game, registry); adapter.baseline();
  return { system, v, own, remote, game, driver, adapter, sender: createSnapshotSender(adapter) };
}
const packet = (packetId, tick, records) => encodeEntityPacket({ epoch: 1, packetId, tick, records });
const send = (c, tick, records) => c.sender.send(tick, records);
const seatRecord = (unit, tick, mount, seatIndex, leaseVersion) => ({ kind: 'seat', id: unit.id, generation: unit.generation, tick, vehicleId: mount?.aiTarget.id ?? 0, vehicleGeneration: mount?.aiTarget.generation ?? 0, seatIndex, leaseVersion });

test('remote pose packets cannot replace exact own pilot or passenger aim baselines', () => {
  const c = client(), server = mountFixture(), hull = server.v; hull.aiTarget.id = 40;
  send(c, 10, [captureEntityVehicle(hull, { tick: 10, classId: 2 }), captureEntityMount(hull, { tick: 10, slot: 0 }), captureEntitySoldier(c.own, { tick: 10, classId: 1, weaponId: 1 }), seatRecord(c.own, 10, hull, 0, 1)]);
  c.adapter.previous = { id: c.own.id, generation: c.own.generation, vehicleId: 40, vehicleGeneration: hull.aiTarget.generation };
  c.v.position.set(7, 8, 9); c.v.seats[0].aimYaw = .123456789; c.v.tilt = .23456789;
  hull.position.set(20, 30, 40); hull.seats[0].aimYaw = .8; hull.seats[1].aimYaw = .5;
  send(c, 20, [captureEntityVehicle(hull, { tick: 20, classId: 2 }), captureEntityMount(hull, { tick: 20, slot: 0 })]);
  assert.deepEqual(c.v.position.toArray(), [7, 8, 9]);
  assert.equal(c.v.seats[0].aimYaw, .123456789);
  assert.equal(c.v.tilt, .23456789);
  assert.ok(Math.abs(c.v.seats[1].aimYaw - .5) <= ANGLE, 'other seat still receives remote aim');
  send(c, 30, [captureEntitySoldier(c.own, { tick: 30, classId: 1, weaponId: 1 }), seatRecord(c.own, 30, hull, 1, 2)]);
  c.adapter.previous = { id: c.own.id, generation: c.own.generation, mountId: 40, mountGeneration: hull.aiTarget.generation, mountSeat: 1 };
  c.v.seats[1].aimYaw = .23456789;
  // A driven hull moves, so its next snapshot carries the position and aim
  // again (an unchanged snapshot entry is not resent under delta encoding).
  hull.position.set(21, 30, 40); hull.seats[0].aimYaw = .81;
  send(c, 40, [captureEntityVehicle(hull, { tick: 40, classId: 2 }), captureEntityMount(hull, { tick: 40, slot: 0 })]);
  assert.ok(Math.abs(c.v.position.x-21)<.04&&Math.abs(c.v.position.y-30)<.04&&Math.abs(c.v.position.z-40)<.04, 'passenger hull still receives remote motion');
  assert.equal(c.v.seats[1].aimYaw, .23456789);
  assert.ok(Math.abs(c.v.seats[0].aimYaw - .81) <= ANGLE);
  hull.seats[0].aimYaw = .82;
  send(c, 50, [captureEntitySoldier(c.own, { tick: 50, classId: 1, weaponId: 1 }), seatRecord(c.own, 50, null, 0, 3), captureEntityMount(hull, { tick: 50, slot: 0 })]);
  assert.ok(Math.abs(c.v.seats[1].aimYaw - .5) <= ANGLE, 'exit resumes remote aim despite old correction context');
});

test('exact own turret aim survives remote pose but destruction still releases its lease', () => {
  const c = client();
  const turret = { aiTarget: { id: 50, generation: 3 }, rig: { root: new Object3D() },
    state: 'built', alive: true, health: 100, team: 1, yaw: .123456789, pitch: .2,
    gunYaw: .03, gunPitch: .04, present() {}, show(state) { this.state = state; this.alive = state === 'built'; } };
  const lease = { turret, unit: c.own, index: 0, released: false };
  c.own.seat = turret.lease = lease;
  c.game.live.buildings.push(turret); c.game.turretSeats.turrets.push(turret);
  c.adapter.previous = { id: c.own.id, generation: c.own.generation, mountId: 50, mountGeneration: 3, mountSeat: 0 };
  const record = { kind: 'turret', id: 50, generation: 3, tick: 20, building: 0,
    state: 'built', team: 1, health: 50, yaw: .8, pitch: .7, gunYaw: .6, gunPitch: .5 };
  c.adapter.turret(record);
  assert.equal(turret.yaw, .123456789); assert.equal(turret.gunPitch, .04); assert.equal(turret.health, 50);
  c.adapter.turret({ ...record, tick: 30, state: 'destroyed', health: 0 });
  assert.equal(turret.alive, false); assert.equal(c.own.seat, null); assert.equal(turret.lease, null);
});

test('capture: a real hull yields a crew pose with every seat aim, lean and slot; seats name hulls or turrets; a mounted player yields no correction', () => {
  const { system, v } = mountFixture();
  v.aiTarget.id = 40;
  const gunner = body(2);
  assert.ok(system.board(gunner, v, 1), 'the shared boarding rule seats the unit');
  v.seats[0].aimYaw = -1.1; v.seats[1].aimYaw = .7; v.seats[1].pitch = .2; v.tilt = .1; v.roll = -.05;
  const game = { units: registryUnits(gunner), vehicles: system, turretSeats: { turrets: [] }, live: { buildings: [] } };
  const records = new EntityWorldCapture(game, registry).capture(10);
  const seat = records.find(r => r.kind === 'seat' && r.id === 2), mount = records.find(r => r.kind === 'mount');
  assert.equal(seat.vehicleId, 40); assert.equal(seat.seatIndex, 1);
  assert.equal(mount.id, 40); assert.equal(mount.slot, 0); assert.equal(mount.seats.length, 2);
  const wire = decodeEntityPacket(packet(1, 10, records)).records.find(r => r.kind === 'mount');
  assert.ok(Math.abs(wire.seats[1].yaw - .7) <= ANGLE && Math.abs(wire.seats[1].pitch - .2) <= ANGLE && Math.abs(wire.seats[0].yaw + 1.1) <= ANGLE);
  assert.ok(Math.abs(wire.tilt - .1) <= ANGLE && Math.abs(wire.roll + .05) <= ANGLE);
  assert.equal(encodeEntityRecord(mount).length, 30, 'two seats: 13 header + 9 pose + 8 aim bytes');
  // Passenger own aim now has an exact schema9 checkpoint alongside remote hull state.
  const world = { players: new Map([[2, { unit: gunner, spectator: false, mailbox: { down: [], view: {}, discarded: 0 } }]]) };
  const own=tryCapturePlayerCorrection(game, world, 2, { tick: 10, registry, epoch: 1, bindingProfileId: 0 });
  assert.equal(own.schema,OWN_PLAYER_SCHEMA);assert.equal(decodeOwnPlayer(own).controlMode,2);
  assert.throws(() => tryCapturePlayerCorrection(game, world, 99, { tick: 10, registry, epoch: 1 }), /spectator correction/, 'other errors still surface');
  // A turret as its registry target; a turret off the registry is skipped.
  const turret = { aiTarget: { id: 9, generation: 2, team: 1 }, state: 'built', health: Infinity, yaw: .3, pitch: -.1, gunYaw: .02, gunPitch: .05 };
  const record = captureEntityTurret(turret, { tick: 10, building: 4 });
  assert.equal(record.health, 65535); assert.equal(record.building, 4); assert.equal(record.id, 9);
  assert.equal(captureEntityTurret({ ...turret, aiTarget: null }, { tick: 10, building: 4 }), null);
  const turretSeat = new EntityWorldCapture({ units: registryUnits(body(3)), vehicles: { vehicles: [] }, turretSeats: { turrets: [] } }, registry);
  const occupant = turretSeat.game.units.byId(3); occupant.seat = { turret: { aiTarget: turret.aiTarget }, index: 0 };
  assert.equal(turretSeat.capture(11).find(r => r.kind === 'seat').vehicleId, 9);
  assert.throws(() => encodeEntityRecord({ kind: 'turret', id: 1, generation: 1, tick: 1, building: 1, state: 'melted', team: 1, health: 1, yaw: 0, pitch: 0, gunYaw: 0, gunPitch: 0 }), /turret state/);
});

test('client: hull creation, seat ownership for both humans, replicated aim and lean, exit, destruction and respawn without stale leases', () => {
  const c = client(['tank', 'tank2']), server = mountFixture();
  const hull = server.v; hull.aiTarget.id = 40; hull.position.set(12, 0, -3); hull.yaw = .4; hull.syncBody();
  const remote = body(2); server.system.board(remote, hull, 1);
  hull.seats[1].aimYaw = .7; hull.seats[1].pitch = .2; hull.tilt = .1; hull.roll = -.05;
  const vehicle = tick => captureEntityVehicle(hull, { tick, classId: 2 }), mount = tick => captureEntityMount(hull, { tick, slot: 0 });
  const soldier = (unit, tick) => captureEntitySoldier(unit, { tick, classId: 1, weaponId: 1 });
  // 1. The gunner arrives seated on a hull the client knows only by class.
  send(c, 10, [vehicle(10), mount(10), soldier(remote, 10), seatRecord(remote, 10, hull, 1, 1)]);
  assert.equal(c.v.aiTarget.id, 40, 'the client hull takes the server identity');
  assert.ok(c.v.alive); assert.ok(Math.abs(c.v.position.x - 12) < .04); assert.ok(Math.abs(c.v.yaw - .4) < .01, 'a ground hull heading follows its quaternion');
  assert.ok(Math.abs(c.v.seats[1].aimYaw - .7) <= ANGLE && Math.abs(c.v.seats[1].pitch - .2) <= ANGLE, 'the gunner seat aims where the server says');
  assert.ok(Math.abs(c.v.tilt - .1) <= ANGLE && Math.abs(c.v.roll + .05) <= ANGLE);
  assert.equal(c.remote.seat?.vehicle, c.v); assert.equal(c.v.seats[1].lease.unit, c.remote); assert.equal(c.v.seats[1].lease.human, false);
  assert.equal(c.remote.combat.mounted, true);
  // Presentation: the sampled pose places the root with its lean and poses the crew.
  c.adapter.present(10); c.system.present(1);
  assert.ok(c.v.networkPose, 'the hull draws from its replicated sample');
  assert.ok(Math.abs(c.v.root.rotation.x + .1) <= ANGLE + 1e-6, 'ground lean is drawn from tilt');
  assert.ok(Math.abs(c.v.root.rotation.y - .4) < 1e-3);
  const bind = c.v.bind.get(c.v.seats[1].view.turret);
  assert.ok(bind && c.v.seats[1].view.turret.quaternion.angleTo(bind) > .1, 'the gunner turret node swings to the replicated aim');
  // 2. The local player takes the pilot seat; its life fields arrive as a soldier record, no correction.
  c.own.combat.health = 100;
  const ownState = soldier(c.own, 20); ownState.health = 77;
  send(c, 20, [vehicle(20), mount(20), ownState, seatRecord(c.own, 20, hull, 0, 2)]);
  assert.equal(c.driver.vehicle, c.v, 'the shared driver adapter owns the hull'); assert.equal(c.v.seatIndex, 0);
  assert.equal(c.v.seats[0].lease?.human, true); assert.equal(c.own.seat?.vehicle, c.v); assert.equal(c.driver.lease, c.v.seats[0].lease);
  assert.equal(c.own.combat.health, 77, 'own life fields apply without a correction');
  assert.ok(c.driver.synced > 0);
  // 3. The gunner exits: its lease is let go on this client too.
  send(c, 30, [vehicle(30), mount(30), soldier(remote, 30), seatRecord(remote, 30, null, 0, 3)]);
  assert.equal(c.remote.seat, null); assert.equal(c.v.seats[1].lease, null); assert.equal(c.remote.combat.mounted, false);
  assert.equal(c.v.seats[0].lease?.unit, c.own, 'the pilot keeps the hull');
  // 4. A second hull of a class this client never built appears from its slot.
  const other = mountFixture({ classes: ['tank', 'tank2'] }).v; other.aiTarget.id = 41; other.position.set(50, 0, 50); other.syncBody();
  send(c, 40, [captureEntityVehicle(other, { tick: 40, classId: 3 }), captureEntityMount(other, { tick: 40, slot: 0 })]);
  const built = c.system.vehicles.find(v => v.aiTarget.id === 41);
  assert.ok(built && built !== c.v && built.name === 'tank2' && built.alive, 'Vehicles.variant builds the unseen class');
  // 5. The hull is destroyed: ownership and collision end at once, the driver is released, nobody is damaged here.
  const dead = vehicle(50); dead.health = 0;
  send(c, 50, [dead]);
  assert.equal(c.v.alive, false); assert.equal(c.v.root.visible, false); assert.equal(c.v.body.enabled, false);
  assert.equal(c.driver.released, 1); assert.equal(c.own.seat, null); assert.equal(c.v.seats[0].lease, null); assert.equal(c.own.alive, true);
  // 6. A new life of the hull respawns through the shared Vehicle.spawn and starts empty.
  hull.aiTarget.generation = 2; hull.alive = true; hull.health = hull.maxHealth;
  send(c, 60, [vehicle(60), mount(60)]);
  assert.equal(c.v.alive, true); assert.equal(c.v.aiTarget.generation, 2); assert.ok(c.v.seats.every(s => s.lease === null));
  assert.equal(c.adapter.deferred, 0);
  // A vehicle record for an unknown id with no slot named is deferred, never a fault.
  const stray = captureEntityVehicle(other, { tick: 70, classId: 3 }); stray.id = 99;
  send(c, 70, [stray]);
  assert.equal(c.adapter.deferred, 1);
});

test('a dying walker stays in the roster, and on a client it is dead but drawn, posed by the time it has left, until its tombstone', () => {
  const c = client(), server = mountFixture(); const hull = server.v; hull.aiTarget.id = 40;
  const remote = body(2); server.system.board(remote, hull, 0);
  const game = { units: registryUnits(remote), vehicles: server.system, turretSeats: { turrets: [] }, live: { buildings: [] } };
  const capture = new EntityWorldCapture(game, registry);
  send(c, 10, capture.capture(10));
  assert.equal(c.v.aiTarget.id, 40); assert.equal(c.v.alive, true);
  // Destroyed on the server: a walker plays out its death animation (Vehicle.setHealth leaves it so).
  hull.alive = false; hull.health = 0; hull.dying = 2.5;
  const records = capture.capture(20);
  const dying = records.find(r => r.kind === 'vehicle' && r.id === 40);
  assert.equal(ENTITY_VEHICLE_STATES[dying.state], 'Dying'); assert.equal(dying.power, 2500, 'the time it has left, in ms');
  assert.ok(!records.some(r => r.kind === 'tombstone' || r.kind === 'mount' && r.id === 40), 'no tombstone, no crew pose');
  send(c, 20, records);
  assert.equal(c.v.alive, false, 'dead: not a target, nothing to board');
  assert.equal(c.v.body.enabled, false); assert.equal(c.v.seats[0].lease, null);
  assert.equal(c.v.dying, 2.5); assert.equal(c.v.root.visible, true, 'still drawn');
  assert.equal(c.v.present(1), true, 'Vehicle.present poses it');
  hull.dying = 1.25;
  send(c, 30, capture.capture(30));
  assert.equal(c.v.dying, 1.25);
  // Gone on the server: the tombstone hides it.
  hull.dying = 0;
  const gone = capture.capture(40);
  assert.ok(gone.some(r => r.kind === 'tombstone' && r.id === 40));
  send(c, 40, gone);
  assert.equal(c.v.root.visible, false); assert.equal(c.v.dying, 0); assert.equal(c.v.present(1), false);
});

test('client: hull, mount and seat from one snapshot install together', () => {
  const c = client(), server = mountFixture(); const hull = server.v; hull.aiTarget.id = 40;
  const remote = body(2); server.system.board(remote, hull, 0);
  send(c, 10, [captureEntitySoldier(remote, { tick: 10, classId: 1, weaponId: 1 }), seatRecord(remote, 10, hull, 0, 1),
    captureEntityVehicle(hull, { tick: 10, classId: 2 }), captureEntityMount(hull, { tick: 10, slot: 0 })]);
  assert.equal(c.remote.seat?.vehicle, c.v, 'the vehicle, its mount group and its seat record travel in one snapshot');
});

test('a hull record without slot metadata is deferred, then built when a snapshot names its slot',()=>{
  const c=client(['tank','tank2']), server=mountFixture({classes:['tank2']}), hull=server.v;
  hull.aiTarget.id=70;hull.position.set(12,0,8);
  send(c,10,[captureEntityVehicle(hull,{tick:10,classId:3})]);
  assert.equal(c.system.vehicles.find(v=>v.aiTarget.id===70),undefined);
  send(c,11,[captureEntityMount(hull,{tick:11,slot:0})]);
  const remote=c.system.vehicles.find(v=>v.aiTarget.id===70);
  assert.ok(remote);assert.equal(remote.name,'tank2');
  assert.ok(Math.abs(remote.position.x-12)<.05&&Math.abs(remote.position.z-8)<.05);
  assert.ok(remote.tickFrom);
});

test('slot metadata cannot resurrect a tombstoned hull, and a new generation respawns it',()=>{
  const c=client(['tank','tank2']),server=mountFixture({classes:['tank2']}),hull=server.v;hull.aiTarget.id=70;
  send(c,10,[captureEntityVehicle(hull,{tick:10,classId:3}),captureEntityMount(hull,{tick:10,slot:0})]);
  assert.ok(c.system.vehicles.find(v=>v.aiTarget.id===70));
  send(c,11,[{kind:'tombstone',id:70,generation:hull.aiTarget.generation,tick:11}]);
  assert.equal(c.system.vehicles.find(v=>v.aiTarget.id===70).alive,false);
  assert.equal(c.system.vehicles.find(v=>v.aiTarget.id===70).root.visible,false);
  hull.aiTarget.generation++;
  send(c,12,[captureEntityVehicle(hull,{tick:12,classId:3}),captureEntityMount(hull,{tick:12,slot:0})]);
  const remote=c.system.vehicles.find(v=>v.aiTarget.id===70);
  assert.equal(remote.aiTarget.generation,hull.aiTarget.generation);assert.equal(remote.alive,true);
  c.adapter.baseline();assert.equal(c.adapter.slots.size,0);assert.equal(c.adapter.pendingSeats.size,0);
});

test('a mount captured before its hull supplies the crew pose when the hull snapshot arrives',()=>{
  const c=client(['tank','tank2']),server=mountFixture({classes:['tank2']}),hull=server.v;
  hull.aiTarget.id=70;hull.tilt=.2;hull.roll=-.1;hull.walkPhase=.35;hull.walkBlend=.6;
  hull.seats[0].aimYaw=.5;hull.seats[0].pitch=-.25;
  const mount=captureEntityMount(hull,{tick:10,slot:0});
  send(c,10,[mount]);
  assert.equal(c.system.vehicles.find(v=>v.aiTarget.id===70),undefined);
  send(c,20,[captureEntityVehicle(hull,{tick:20,classId:3})]);
  const remote=c.system.vehicles.find(v=>v.aiTarget.id===70);assert.ok(remote);
  assert.ok(Math.abs(remote.tilt-.2)<.01);assert.ok(Math.abs(remote.roll+.1)<.01);
  assert.ok(Math.abs(remote.seats[0].aimYaw-.5)<.01);assert.ok(Math.abs(remote.seats[0].pitch+.25)<.01);
  assert.ok(Math.abs(remote.walkPhase-.35)<.01);assert.ok(Math.abs(remote.walkBlend-.6)<.01);
});
