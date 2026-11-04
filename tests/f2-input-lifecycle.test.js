import test from 'node:test';
import assert from 'node:assert/strict';
import { Session } from 'node:inspector/promises';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, Vehicle } from '../src/vehicles.js';
import { attachF2 } from '../src/f2-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { Unit } from '../src/units.js';
import { Turret } from '../src/props.js';
import { TurretSeats } from '../src/seats.js';
import { Ordnance } from '../src/ordnance.js';
import { attachOrdnanceSim, ordnanceTests } from '../src/ordnance-sim.js';
import { Soldier } from '../src/soldier.js';
import { seatLook, leaveKilled } from '../src/boarding.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { predictionVehicle } from '../src/runtime/vehicle-prediction.js';
import { networkVehicleInput } from '../src/runtime/network-vehicle.js';
import { CommandControls } from '../src/runtime/commands.js';

function box(w, h, d, y) {
  const geometry = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
  return { positions: Array.from(geometry.attributes.position.array), indices: Array.from(geometry.index.array),
    polygons: Array(geometry.index.count / 3).fill(0) };
}

async function fleet(native, cage = false, nativeCollision = false) {
  const instance = native ? await loadSim() : null;
  const rays = nativeCollision ? await loadRayKernel(instance) : null;
  const floor = box(2000, 2, 2000, -1);
  const collision = new CollisionWorld(new Float32Array(floor.positions), new Uint32Array(floor.indices), new Uint32Array(floor.polygons), undefined, undefined, rays);
  if (cage) for (const axis of ['x', 'z']) for (const side of [-1, 1]) {
    const wall = box(axis === 'x' ? .1 : 16, 10, axis === 'z' ? .1 : 16, 4);
    for (let i = axis === 'x' ? 0 : 2; i < wall.positions.length; i += 3) wall.positions[i] += side * 2.6;
    collision.addBody(new Float32Array(wall.positions), new Uint32Array(wall.indices), new Uint32Array(wall.polygons));
  }
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
    attachF2(system, bridgeFor(instance, { classes: { Unit, Vehicle } }),
      { killY: -300, deathRegions: [], scripted: [] }, { data, buffer, indexBuffer }, Unit, true);
  }
  return { system, post };
}

function observe(v) {
  return { name: v.name, alive: v.alive, health: v.health, position: v.position.toArray(), velocity: v.velocity.toArray(),
    yaw: v.yaw, yawRate: v.yawRate, seats: v.seats.map(s => [s.aimYaw, s.pitch, s.lookYaw, s.lookPitch]) };
}

// Sample allocation requests in the real boundary after both its buffers and
// V8's hot loops are warm. Native requests have their separate exact counter.
test('warm live F2 boundary creates no sampled JavaScript objects', async () => {
  const { system } = await fleet(true, false, true);
  system.f2.debug = false;
  const session = new Session(); session.connect();
  try {
    const u = rider(system, 42); assert.equal(system.board(u, system.vehicles[0]), true);
    u.intent.forward = .5;
    for (let i = 0; i < 1600; i++) system.update(1 / 60);
    // Warm the profiler's own allocation-stack metadata before the measured
    // interval, as well as warming the game and V8's phase loops above.
    await session.post('HeapProfiler.startSampling', { samplingInterval: 1,
      includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
    for (let i = 0; i < 120; i++) system.update(1 / 60);
    await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.startSampling', { samplingInterval: 1,
      includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
    for (let i = 0; i < 120; i++) system.update(1 / 60);
    const { profile } = await session.post('HeapProfiler.stopSampling');
    const allocations = [];
    function visit(node) {
      if (node.selfSize && /\/src\/f2-[^/]+\.js$/.test(node.callFrame.url)) allocations.push({ function: node.callFrame.functionName, bytes: node.selfSize });
      for (const child of node.children) visit(child);
    }
    visit(profile.head); assert.deepEqual(allocations, []);
  } finally { session.disconnect(); system.dispose(); }
});

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
        assert.equal(other.alive, false, 'an empty hull publishes death before the command returns');
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
  unit.combat.damage = () => { unit.alive = false; unit.dead = unit.combat.dead = true; leaveKilled(unit); };
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
    lease.input.forward = 2; assert.throws(() => system.f2.check(), /F2 lease mirror write/); lease.input.forward = 1;
    v.seats[0].lookYaw = .7; assert.throws(() => system.f2.check(), /F2 mirror write: seats/); v.seats[0].lookYaw = null;
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

test('live F2 safe exits refuse blocked geometry and retain the same lease', async () => {
  const run = async native => {
    const { system } = await fleet(native, true);
    try {
      const v = system.vehicles[0], u = rider(system, 15); u.human = u.networkHuman = true;
      assert.equal(system.board(u, v), true); const lease = u.seat;
      assert.equal(system.unboard(u), false); assert.equal(u.seat, lease); assert.equal(lease.released, false);
      assert.equal(system.unboard(u, { force: true }), true); assert.equal(u.seat, null);
      return { position: u.position.toArray(), grounded: u.soldier.grounded };
    } finally { system.dispose(); }
  };
  assert.deepEqual(await run(true), await run(false));
});

test('live F2 private hull replay leaves the native authority input untouched', async () => {
  const { system } = await fleet(true);
  try {
    const v = system.vehicles[0], u = rider(system, 14);
    u.human = u.networkHuman = true;
    assert.equal(system.board(u, v), true);
    const clone = predictionVehicle(v), before = observe(v), controls = new CommandControls();
    const command = { tick: 1, sequence: 1, epoch: 1, generation: 1, down: [], actions: [], look: [[400, 100]], mouseScale: 1 };
    controls.accept(command); networkVehicleInput(clone, 0, controls, command);
    assert.notEqual(clone.seats[0].aimYaw, v.seats[0].aimYaw);
    assert.deepEqual(observe(v), before);
    system.update(1 / 60);
    assert.equal(v.seats[0].aimYaw, before.seats[0][0]);
  } finally { system.dispose(); }
});

test('live F2 damage preserves the latest foreign hull rider body at kill entry', async () => {
  const run = async native => {
    const { system } = await fleet(native);
    try {
      const v = system.vehicles[0], u = rider(system, 13);
      assert.equal(system.board(u, v), true);
      u.intent.forward = 1;
      for (let i = 0; i < 8; i++) system.update(1 / 60);
      const before = { position: u.position.toArray(), yaw: u.soldier.aimYaw, ticks: u.tickCount };
      let entry;
      u.combat.damage = () => {
        entry = { position: u.position.toArray(), yaw: u.soldier.aimYaw, ticks: u.tickCount };
        u.alive = false; u.dead = u.combat.dead = true; leaveKilled(u);
      };
      v.damage(Infinity);
      assert.deepEqual(entry, before, 'destroyedCrew sees the body already moved by syncCrews');
      assert.equal(u.seat, null);
      return entry;
    } finally { system.dispose(); }
  };
  assert.deepEqual(await run(true), await run(false));
});

function turret(x, base = 'bolt', height = 3) {
  const root = new THREE.Group(), drum = new THREE.Bone(), gun = new THREE.Bone(), seat = new THREE.Bone(), fire = new THREE.Bone();
  root.matrixAutoUpdate = false; root.matrix.makeTranslation(x, height, 50);
  root.add(drum); drum.add(gun, seat); gun.add(fire); fire.position.z = 1; seat.position.z = -.5;
  const rig = { root, bones: [drum, gun, seat, fire], byName: { drum, gun, seat, fire }, model: { box: [[-1, 0, -1], [1, 2, 1]] } };
  const def = { class: 'fixture_turret', base: 'cannon', channel: 0, ShotDelay: .1, ShotsPerSalvo: 1, SalvoCount: 1, RoundsPerClip: 0,
    ordnance: { class: `fixture_${base}`, base, Velocity: 40, LifeSpan: 1, Damage: 0 } };
  return new Turret({ base: 'armedbuilding', matrix: root.matrix.toArray(), weapon: def,
    settings: { MaxHealth: 100, TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'fire' } },
    { built: rig }, {}, null);
}

test('live F2 Classic turret exits exhaust unsupported candidates before fallback', async () => {
  const run = async native => {
    const { system } = await fleet(native);
    try {
      const t = turret(20, 'bolt', 50); t.weapon = null;
      const crews = new TurretSeats({ live: { turrets: [t] }, collision: system.collision });
      if (native) system.f2.seats.attachTurrets(crews);
      const u = rider(system, 35); u.human = u.networkHuman = true;
      assert.equal(crews.man(u, t), true); assert.equal(crews.leave(u), true);
      assert.deepEqual(u.position.toArray(), t.position.toArray());
      return { position: u.position.toArray(), grounded: u.soldier.grounded, air: u.soldier.air };
    } finally { system.dispose(); }
  };
  assert.deepEqual(await run(true), await run(false));
});

test('live F2 turret exits use built bounds after construction and rebuild', async () => {
  const run = async native => {
    const { system } = await fleet(native);
    try {
      const t = turret(20); t.weapon = null;
      t.looks.unbuilt = { root: new THREE.Object3D(), model: { sphere: [0, 1, 0, 8] } };
      t.show('unbuilt');
      const crews = new TurretSeats({ live: { turrets: [t] }, collision: system.collision });
      if (native) system.f2.seats.attachTurrets(crews);
      const results = [];
      for (let i = 0; i < 2; i++) {
        t.setHealth(t.maxHealth);
        const u = rider(system, 30 + i); u.human = u.networkHuman = true;
        assert.equal(crews.man(u, t), true);
        assert.equal(crews.leave(u), true);
        results.push(u.position.toArray());
        t.setHealth(0);
      }
      return results;
    } finally { system.dispose(); }
  };
  assert.deepEqual(await run(true), await run(false));
});

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
    const old = b.seat; turrets[1].setHealth(0);
    assert.equal(b.alive, false); assert.equal(b.seat, null); assert.equal(old.released, true);
    turrets[1].setHealth(turrets[1].maxHealth);
    const c = rider(system, 22); c.human = c.networkHuman = true;
    assert.equal(crews.man(c, turrets[1]), true);
    assert.notEqual(c.seat, old); assert.equal(old.unit, b); assert.equal(turrets[1].gunner, c.soldier);
  } finally { system.dispose(); }
});

test('live F2 beams follow the original turret lease through departure and reuse', async () => {
  async function run(native) {
    const { system } = await fleet(native);
    try {
      const bridge = system.f2?.bridge ?? bridgeFor(await loadSim(), { classes: { Unit, Vehicle }, tests: ordnanceTests });
      const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: system.collision, effects: { play() {} }, lights: { add() {} }, texture: () => null });
      attachOrdnanceSim(ordnance, bridge);
      const t = turret(30, 'beam'), crews = new TurretSeats({ live: { turrets: [t] }, ordnance, collision: system.collision });
      if (native) system.f2.seats.attachTurrets(crews);
      const a = rider(system, 30), b = rider(system, 31);
      a.human = b.human = a.networkHuman = b.networkHuman = true;
      assert.equal(crews.man(a, t), true);
      const trace = [];
      for (let tick = 0; tick < 6; tick++) {
        a.intent.turn = .4; a.intent.buttons[0] = tick === 0 ? 1 : 0;
        if (tick === 3) { assert.equal(crews.leave(a, { placement: false }), true); assert.equal(crews.man(b, t), true); }
        system.update(1 / 60); if (!native) crews.update(1 / 60);
        ordnance.update(1 / 60);
        trace.push([t.yaw, t.pitch, t.barrel, ordnance.beams.map(beam => [beam.position.toArray(), beam.direction.toArray(), beam.life])]);
      }
      return trace;
    } finally { system.dispose(); }
  }
  assert.deepEqual(await run(true), await run(false));
});

test('live F2 stale leases do not reset a reused soldier body', async () => {
  const { system } = await fleet(true);
  try {
    const v = system.vehicles[0], u = rider(system, 40);
    assert.equal(system.board(u, v), true); const lease = u.seat;
    u.presentation = { position: new THREE.Vector3(), reset() {} }; u.animator = { reset() {} }; u.combat.reset = () => {};
    u.spawn(60, 2, 20, .4); const position = u.soldier.position.clone(), ticks = u.tickCount;
    system.update(1 / 60);
    assert.equal(u.seat, null); assert.equal(lease.released, true); assert.equal(v.seats[0].lease, null);
    assert.deepEqual(u.soldier.position, position); assert.equal(u.tickCount, ticks);
  } finally { system.dispose(); }
});

test('live F2 nested shot deaths keep the remaining crew snapshot valid', async () => {
  const { system } = await fleet(true);
  try {
    const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: system.collision, effects: { play() {} }, lights: { add() {} }, texture: () => null });
    attachOrdnanceSim(ordnance, system.f2.bridge);
    const turrets = [turret(20), turret(40)], crews = new TurretSeats({ live: { turrets }, ordnance, collision: system.collision });
    system.f2.seats.attachTurrets(crews);
    const a = rider(system, 50), b = rider(system, 51);
    a.human = b.human = a.networkHuman = b.networkHuman = true;
    assert.ok(crews.man(a, turrets[0])); assert.ok(crews.man(b, turrets[1]));
    const fired = [], old = b.seat;
    ordnance.hitHistory = { shotTick(owner) { fired.push(owner.id); if (owner === a) turrets[1].setHealth(0); return null; } };
    a.intent.buttons[0] = b.intent.buttons[0] = 1;
    system.update(1 / 60);
    assert.deepEqual(fired, [50]); assert.equal(b.alive, false); assert.equal(b.seat, null);
    assert.equal(old.released, true); assert.equal(a.seat.unit, a);
  } finally { system.dispose(); }
});

test('warm live F2 hull and crew phases reuse their native storage', async () => {
  const { system } = await fleet(true);
  try {
    const u = rider(system, 40); assert.equal(system.board(u, system.vehicles[0]), true);
    u.intent.forward = .5;
    for (let i = 0; i < 60; i++) system.update(1 / 60);
    for (let i = 0; i < 1200; i++) {
      system.update(1 / 60);
      assert.equal(system.f2.x.f2_allocations(), 0, `native allocation at warm tick ${i}`);
    }
  } finally { system.dispose(); }
});

test('live F2 generated collision records match world bodies without game files', async () => {
  async function run(native) {
    const { system } = await fleet(native, false, native);
    try {
      const v = system.vehicles[0], u = rider(system, 51); assert.equal(system.board(u, v), true);
      u.intent.forward = .5;
      for (let i = 0; i < 120; i++) system.update(1 / 60);
      return { hull: observe(v), bounds: [v.body.bounds.min.toArray(), v.body.bounds.max.toArray()],
        matrix: v.body.matrix.toArray(), inverse: v.body.inverse.toArray() };
    } finally { system.dispose(); }
  }
  assert.deepEqual(await run(true), await run(false));
});
// Loaded geometry bounds each query's scratch, including polygons first met
// after the hull starts moving. The phase itself must not grow those buffers.
test('warm F2 native collision reuses storage at a later wall contact', async () => {
  const { system } = await fleet(true, true, true);
  try {
    const v = system.vehicles[0], u = rider(system, 46);
    assert.equal(system.board(u, v), true);
    for (let i = 0; i < 30; i++) system.update(1 / 60);
    u.intent.forward = 1;
    for (let i = 0; i < 240; i++) {
      system.update(1 / 60);
      assert.equal(system.f2.x.f2_allocations(), 0, `collision allocation at ${i}`);
    }
    assert.ok(v.position.z > 0, 'the hull reaches the blocking geometry');
    assert.ok(v.position.z < 2.6, 'the wall still stops the hull');
  } finally { system.dispose(); }
});
// A normal lifecycle hook may synchronously board a new occupant. Its scalar
// command has its own foreign facts; it must not replace another crew's batch.
test('a pre-latch birth command preserves another crew\'s pending intent', async () => {
  const { system } = await fleet(true);
  try {
    const [pilot, other] = system.vehicles, a = rider(system, 61), b = rider(system, 62);
    assert.equal(system.board(a, pilot), true); system.update(1 / 60);
    other.setHealth(0); a.intent.forward = .7;
    const adopt = system.f2.firing.adopt;
    let boarded = false;
    system.f2.firing.adopt = function(v) {
      adopt.call(this, v);
      if (v === other && !boarded) { boarded = true; assert.equal(system.board(b, v), true); }
    };
    system.update(1 / 60);
    assert.ok(boarded); assert.equal(b.seat.vehicle, other);
    assert.equal(a.seat.input.forward, .7, 'the pending batch survives the birth command');
  } finally { system.dispose(); }
});
