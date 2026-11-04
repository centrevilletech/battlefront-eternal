import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { attachF2 } from '../src/f2-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { Unit } from '../src/units.js';
import { Turret } from '../src/props.js';
import { TurretSeats } from '../src/seats.js';
import { Ordnance } from '../src/ordnance.js';
import { attachOrdnanceSim } from '../src/ordnance-sim.js';
import { Soldier } from '../src/soldier.js';
import { seatLook } from '../src/boarding.js';
import { loadSim } from '../src/sim/module.js';

function box(w, h, d, y) {
  const geometry = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
  return { positions: Array.from(geometry.attributes.position.array), indices: Array.from(geometry.index.array),
    polygons: Array(geometry.index.count / 3).fill(0) };
}

async function fleet(native) {
  const floor = box(2000, 2, 2000, -1);
  const collision = new CollisionWorld(new Float32Array(floor.positions), new Uint32Array(floor.indices), new Uint32Array(floor.polygons));
  const def = { model: 'tank', base: 'hover', settings: { MaxHealth: 1000, ForwardSpeed: 12, TurnRate: 1 },
    seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
  const post = { desc: { name: 'cp' }, team: 1 };
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: box(2, 2, 4, 1) },
    classes: { tank: def, replacement: def }, spawns: [0, 100].map((x, i) => ({ name: `pad${i}`,
      matrix: new THREE.Matrix4().makeTranslation(x, 0, 0).toArray(), team: 1, post: 'cp', respawnTime: .01,
      classes: { 1: 'tank', 2: 'replacement' } })) };
  const buffer = new ArrayBuffer(0), indexBuffer = new ArrayBuffer(0);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), data, buffer, indexBuffer,
    collision, posts: { posts: [post] }, texture: () => null, ordnance: { explosion() {} }, effects: { play() {} } });
  system.clock = () => system.time;
  if (native) {
    attachF2(system, bridgeFor(await loadSim(), { classes: { Unit, Vehicle } }),
      { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Unit, true);
  }
  return { system, post };
}

function observe(v) {
  return { name: v.name, alive: v.alive, health: v.health, position: v.position.toArray(), velocity: v.velocity.toArray(),
    yaw: v.yaw, yawRate: v.yawRate, seats: v.seats.map(s => [s.aimYaw, s.pitch, s.lookYaw, s.lookPitch]) };
}

for (const replacement of [false, true]) {
  test(`F2 preserves another hull's pending look during ${replacement ? 'variant construction' : 'respawn'}`, async () => {
    const run = async native => {
      const { system, post } = await fleet(native);
      try {
        const [pilot, other] = system.vehicles;
        // Give the old life a distinct aim so preserving it across respawn
        // would also fail, even when the other hull's input survives.
        seatLook(other, 0, -.7 * 2048, -.2 * 2048);
        system.update(1 / 60);
        other.setHealth(0);
        if (replacement) post.team = 2;
        seatLook(pilot, 0, -.4 * 2048, .1 * 2048);
        system.update(1 / 60);
        const spawned = system.slots[1].vehicle;
        assert.ok(spawned.alive);
        assert.equal(spawned.name, replacement ? 'replacement' : 'tank');
        assert.equal(pilot.seats[0].lookYaw, .4, 'the pending yaw reaches the ground substeps');
        assert.equal(pilot.seats[0].lookPitch, -.1, 'the pending pitch reaches the ground substeps');
        assert.equal(spawned.seats[0].lookYaw, null, 'the new life resets its own look');
        assert.equal(spawned.seats[0].pitch, 0, 'the new life resets its own pitch');
        return system.vehicles.map(observe);
      } finally { system.dispose(); }
    };
    assert.deepEqual(await run(true), await run(false), 'the whole phase matches JavaScript');
  });
}

function rider(system, id) {
  const soldier = new Soldier(system.collision, {});
  const unit = new Unit({ side: 'all', className: 'test', def: {}, actor: { root: new THREE.Object3D() }, soldier,
    animator: null, presentation: null, combat: { dead: false, zoomStep: 0, zoom: 1, firstPerson: false, blowApart: false } });
  unit.id = id; unit.generation = 1; unit.alive = true; unit.removed = false; unit.team = 1;
  return unit;
}

test('live F2 scalar commands publish leases before hooks and latch in f2_step', async () => {
  const { system } = await fleet(true);
  try {
    const v = system.vehicles[0], u = rider(system, 7);
    let boards = 0;
    system.onBoard = (unit, hull, index) => {
      boards++; assert.equal(unit, u); assert.equal(hull, v); assert.equal(index, 0);
      assert.equal(unit.seat, hull.seats[0].lease); assert.equal(unit.seat.live, true);
    };
    assert.equal(system.board(u, v), true); const lease = u.seat;
    assert.equal(boards, 1); assert.equal(lease.unit, u);
    u.intent.forward = 1; u.intent.reload = true;
    system.update(1 / 60);
    assert.equal(lease.input.forward, 1); assert.equal(lease.input.reload, true);
    assert.equal(u.intent.reload, false); assert.equal(u.seat, lease);
    assert.equal(system.requestExit(u, { placement: false }), true);
    system.update(1 / 60);
    assert.equal(u.seat, null); assert.equal(lease.released, true); assert.equal(v.seats[0].lease, null);
  } finally { system.dispose(); }
});

test('live F2 board hooks can synchronously board another hull', async () => {
  const { system } = await fleet(true);
  try {
    const a = rider(system, 10), b = rider(system, 11), first = system.vehicles[0], second = system.vehicles[1];
    system.onBoard = unit => { if (unit === a) assert.equal(system.board(b, second), true); };
    assert.equal(system.board(a, first), true);
    assert.equal(a.seat.vehicle, first); assert.equal(b.seat.vehicle, second);
    system.update(1 / 60);
    assert.equal(a.seat.unit, a); assert.equal(b.seat.unit, b);
  } finally { system.dispose(); }
});

function turret(x) {
  const root = new THREE.Group(), drum = new THREE.Bone(), gun = new THREE.Bone(), seat = new THREE.Bone(), fire = new THREE.Bone();
  root.matrixAutoUpdate = false; root.matrix.makeTranslation(x, 3, 50);
  root.add(drum); drum.add(gun, seat); gun.add(fire); fire.position.z = 1; seat.position.z = -.5;
  const rig = { root, bones: [drum, gun, seat, fire], byName: { drum, gun, seat, fire }, model: { box: [[-1, 0, -1], [1, 2, 1]] } };
  const def = { class: 'fixture_turret', base: 'cannon', channel: 0, ShotDelay: .1, ShotsPerSalvo: 1, SalvoCount: 1, RoundsPerClip: 0,
    ordnance: { class: 'fixture_bolt', base: 'bolt', Velocity: 40, LifeSpan: 1, Damage: 0 } };
  return new Turret({ base: 'armedbuilding', matrix: root.matrix.toArray(), weapon: def,
    settings: { MaxHealth: 100, TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'fire' } },
    { built: rig }, {}, null);
}

test('live F2 turret crews place each body before its direct native ordnance call', async () => {
  const { system } = await fleet(true);
  try {
    const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: system.collision, effects: { play() {} }, lights: { add() {} }, texture: () => null });
    attachOrdnanceSim(ordnance, system.f2.bridge);
    const turrets = [turret(20), turret(40)], crews = new TurretSeats({ live: { turrets }, ordnance, collision: system.collision });
    system.f2.seats.attachTurrets(crews);
    const a = rider(system, 20), b = rider(system, 21);
    a.human = b.human = a.networkHuman = b.networkHuman = true;
    assert.equal(crews.man(a, turrets[0]), true); assert.equal(crews.man(b, turrets[1]), true);
    const beforeA = a.tickCount, beforeB = b.tickCount, shots = [];
    ordnance.hitHistory = { shotTick(owner) { shots.push([owner.id, a.tickCount - beforeA, b.tickCount - beforeB]); return null; } };
    a.intent.buttons[0] = b.intent.buttons[0] = 1;
    system.update(1 / 60);
    assert.deepEqual(shots, [[20, 1, 0], [21, 1, 1]], 'aim, body and fire stay interleaved per crew');
    assert.equal(turrets[0].lease, a.seat); assert.equal(turrets[1].lease, b.seat);
    assert.equal(crews.leases.length, 2); assert.equal(crews.ticks, 1);
    assert.equal(ordnance.bolts.length, 2);
    assert.equal(crews.leave(a, { placement: false }), true);
    assert.equal(a.seat, null); assert.equal(turrets[0].gunner, null);
  } finally { system.dispose(); }
});
