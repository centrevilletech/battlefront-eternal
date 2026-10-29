import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { VehicleEffects, thrustScale, damageBurns, bodyCarrier } from '../src/vehicle-effects.js';

// The hull's effects come from its state alone (alive, health, flight
// state, velocity, pose), so a hull the page simulates, a worker's replica
// and a server's record draw alike. These hulls are plain objects with just
// those fields, as a replica's are.

const THRUST = { effect: 'exhaust', node: 'dummyroot', offset: [0.7, -0.3, -2.4], minScale: 0.5, maxScale: 1.4, scaleStart: 0.55 };
const fakeEffects = () => {
  const played = [];
  return {
    played,
    play(name, position, normal, velocity, quaternion) {
      const h = { name, position: position.clone(), quaternion: quaternion?.clone() ?? null, velocity: velocity?.clone() ?? null, scale: 1, inherit: 0, stopped: false, done: false,
        move(p, q, v) { this.position.copy(p); if (q) this.quaternion = q.clone(); if (v) this.velocity = v.clone(); },
        stop() { this.stopped = true; } };
      played.push(h);
      return h;
    },
  };
};
function hull(fx, fields = {}) {
  const root = new THREE.Object3D();
  root.position.set(10, 0, 0);
  const dummy = new THREE.Object3D();
  const damage = new THREE.Object3D(); damage.position.set(0, 1, 0);
  root.add(dummy, damage);
  root.updateMatrixWorld(true);
  return { name: 'test_fly', def: { fx }, alive: true, health: 100, maxHealth: 100, flying: true, flightState: 'Landed',
    velocity: new THREE.Vector3(), position: root.position, root, byName: { dummyroot: dummy, hp_damage_1: damage }, aiTarget: { generation: 1 }, ...fields };
}

test('thrust scale rests at its minimum, ramps from ScaleStart of the way to MaxSpeed and stops at its maximum', () => {
  const speeds = [20, 72];
  const start = 20 + (72 - 20) * 0.55;
  assert.equal(thrustScale(THRUST, 0, speeds), 0.5);
  assert.equal(thrustScale(THRUST, start, speeds), 0.5);
  assert.ok(Math.abs(thrustScale(THRUST, (start + 72) / 2, speeds) - 0.95) < 1e-9);
  assert.equal(thrustScale(THRUST, 72, speeds), 1.4);
  assert.equal(thrustScale(THRUST, 150, speeds), 1.4);
  // A ramp that starts at MaxSpeed jumps there.
  assert.equal(thrustScale({ ...THRUST, scaleStart: 1 }, 80, speeds), 1.4);
});

test('a damage effect burns at most its start share of MaxHealth and above its stop share', () => {
  const entry = { start: 0.6, stop: 0.2 };
  assert.equal(damageBurns(entry, 100, 100), false);
  assert.equal(damageBurns(entry, 60, 100), true);
  assert.equal(damageBurns(entry, 21, 100), true);
  assert.equal(damageBurns(entry, 20, 100), false);
  assert.equal(damageBurns({ start: 0.3, stop: 0 }, 0, 100), false);
});

test('thrust burns from takeoff to touchdown, at its attach point, scaled by speed', () => {
  const effects = fakeEffects();
  const fx = new VehicleEffects({ effects });
  const v = hull({ thrust: [THRUST, { ...THRUST, offset: [-0.7, -0.3, -2.4] }], speed: [20, 72] });
  fx.update([v], 1 / 60);
  assert.equal(effects.played.length, 0, 'nothing on the pad');
  v.flightState = 'TakingOff';
  fx.update([v], 1 / 60);
  assert.equal(effects.played.length, 2);
  assert.deepEqual(effects.played[0].position.toArray().map(n => +n.toFixed(4)), [10.7, -0.3, -2.4]);
  assert.equal(effects.played[0].scale, 0.5);
  v.flightState = 'Flying'; v.velocity.set(0, 0, 72);
  fx.update([v], 1 / 60);
  assert.equal(effects.played.length, 2, 'the same two keep burning');
  assert.equal(effects.played[0].scale, 1.4);
  v.flightState = 'Landing';
  fx.update([v], 1 / 60);
  assert.ok(effects.played.every(h => !h.stopped));
  v.flightState = 'Landed';
  fx.update([v], 1 / 60);
  assert.ok(effects.played.every(h => h.stopped), 'out once landed');
});

test('damage effects follow the health bands at their nodes, with their scale and inherited velocity', () => {
  const effects = fakeEffects();
  const fx = new VehicleEffects({ effects });
  const damage = [
    { start: 0.6, stop: 0.2, effect: 'vehiclespark', scale: 1, inherit: 0.75, node: 'hp_damage_1', offset: [0, 0, 0] },
    { start: 0.3, stop: 0, effect: 'vehiclesmoke', scale: 0.4, inherit: 0, node: 'hp_missing', offset: [0, 0, 1] },
  ];
  const v = hull({ damage }, { flying: false, flightState: null });
  fx.update([v], 1 / 60);
  assert.equal(effects.played.length, 0);
  v.health = 50;
  fx.update([v], 1 / 60);
  assert.deepEqual(effects.played.map(h => h.name), ['vehiclespark']);
  assert.deepEqual(effects.played[0].position.toArray(), [10, 1, 0]);
  assert.equal(effects.played[0].inherit, 0.75);
  v.health = 25;
  fx.update([v], 1 / 60);
  assert.deepEqual(effects.played.map(h => h.name), ['vehiclespark', 'vehiclesmoke']);
  // A node the model lacks: the hull's root, as the original game falls back.
  assert.deepEqual(effects.played[1].position.toArray(), [10, 0, 1]);
  assert.equal(effects.played[1].scale, 0.4);
  v.health = 10;
  fx.update([v], 1 / 60);
  assert.equal(effects.played[0].stopped, true, 'sparks stop under their stop share');
  assert.equal(effects.played[1].stopped, false);
});

test('a destroyed hull stops its effects, throws its chunks and raises its death dust after the delay', () => {
  const effects = fakeEffects();
  const calls = [];
  const debris = { pieces: [], blowApart: (...args) => calls.push(args) };
  const fx = new VehicleEffects({ effects, debris });
  const chunks = [{ ChunkGeometryName: 'piece', ChunkSmokeEffect: 'smokeplume' }];
  const v = hull({ damage: [{ start: 1, stop: 0, effect: 'vehiclesmoke', scale: 1, inherit: 0, node: '', offset: [0, 0, 0] }], chunks,
    deathDust: { effect: 'deathdust', offset: [0, 2, 0], delay: 0.5 } }, { flying: false, health: 50 });
  fx.update([v], 1 / 60);
  v.alive = false; v.health = 0;
  fx.update([v], 0.25);
  assert.equal(effects.played[0].stopped, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], chunks);
  assert.equal(calls[0][1][''], v.root, 'a chunk without a node starts at the hull');
  assert.ok(!effects.played.some(h => h.name === 'deathdust'));
  fx.update([v], 0.3);
  const dust = effects.played.find(h => h.name === 'deathdust');
  assert.deepEqual(dust.position.toArray(), [10, 2, 0]);
  fx.update([v], 1);
  assert.equal(effects.played.filter(h => h.name === 'deathdust').length, 1, 'once');
  // Its next life starts clean.
  v.alive = true; v.health = 100; v.aiTarget.generation = 2;
  fx.update([v], 1 / 60);
  assert.equal(calls.length, 1);
});

test('a walker stomps where a foot comes down; a hover sprays over water only', () => {
  const effects = fakeEffects();
  const water = { level: 0, over: (x) => x < 50 };
  const fx = new VehicleEffects({ effects, water });
  const walker = hull({ feet: [{ name: 'p_l_foot', bone: 'dummyroot', at: [0, 0, 0] }], stomp: { effect: 'walkerstomp' } }, { flying: false, flightState: null });
  const foot = walker.byName.dummyroot;
  const at = (y) => { foot.position.y = y; walker.root.updateMatrixWorld(true); fx.update([walker], 0.1); };
  at(3); at(2); at(1);
  assert.equal(effects.played.length, 0, 'still coming down');
  at(1);
  assert.deepEqual(effects.played.map(h => h.name), ['walkerstomp']);
  at(1); at(1.5); at(1.5);
  assert.equal(effects.played.length, 1, 'once a footfall');
  const hover = hull({ water: 'waterwake_lg' }, { flying: false, flightState: null });
  hover.root.position.set(10, 1, 0); hover.root.updateMatrixWorld(true);
  fx.update([hover], 1 / 60);
  const spray = effects.played.find(h => h.name === 'waterwake_lg');
  assert.deepEqual(spray.position.toArray(), [10, 0, 0], 'on the surface under it');
  hover.root.position.x = 60;
  fx.update([hover], 1 / 60);
  assert.equal(spray.stopped, true);
});

test('an effect scale multiplies drawn sizes on both paths; inherit adds a share of the emitter velocity', async () => {
  const { Effects } = await import('../src/effects.js');
  const { emitter, seeded } = await import('./fixtures/effects.js');
  const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
  const sizes = (scale, gpu) => seeded(() => {
    const fx = new Effects(new THREE.Scene(), { p: [emitter()] }, { texture: () => texture, gpu });
    const pushed = [];
    if (!gpu) fx.renderer.batch = () => ({ push: (...args) => pushed.push(args[7]) });
    const e = fx.play('p', new THREE.Vector3());
    e.scale = scale;
    fx.update(0.1);
    return { pushed, row: gpu ? fx.gpu.data[9] : null };
  });
  const one = sizes(1, false), two = sizes(2, false);
  assert.ok(one.pushed.length > 0);
  assert.deepEqual(two.pushed, one.pushed.map(s => s * 2));
  assert.equal(sizes(1.5, true).row, 1.5, 'the GPU emitter row carries it');
  const velocity = (inherit) => seeded(() => {
    const fx = new Effects(new THREE.Scene(), { p: [emitter()] }, { texture: () => texture, gpu: false });
    const e = fx.play('p', new THREE.Vector3(), null, new THREE.Vector3(10, 0, 0));
    e.inherit = inherit;
    fx.update(0.01);
    return e.emitters[0].particles.map(p => p.velocity.x);
  });
  const base = velocity(0), extra = velocity(0.75);
  assert.deepEqual(extra.map(x => +x.toFixed(6)), base.map(x => +(x + 7.5).toFixed(6)));
});

test('a destroyed walker\'s blast goes off at its ExplosionOffset, turned with the hull', async () => {
  const { CollisionWorld } = await import('../src/collision.js');
  const { Vehicles } = await import('../src/vehicles.js');
  const box = (x, y, z, w, h, d) => {
    const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
    return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
  };
  const ground = box(0, -1, 0, 200, 2, 200);
  const blasts = [];
  const def = (offset) => ({ model: 'walker', label: 'AT-ST', base: 'walker', settings: { MaxHealth: 1000, ForwardSpeed: 5 }, seats: [{ name: 'BODY', settings: {}, weapons: [] }],
    explosion: { class: 'imp_walk_atst_exp' }, fx: offset ? { explosionOffset: offset } : {} });
  const blastAt = (offset) => {
    const data = { models: { walker: { bones: [], parts: [] } }, collision: { walker: box(0, 1, 0, 2, 2, 2) }, classes: { walker: def(offset) },
      spawns: [{ name: 'pad', matrix: new THREE.Matrix4().makeTranslation(5, 0, 5).toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: 'walker' } }] };
    const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0),
      collision: new CollisionWorld(ground.positions, ground.indices, ground.polygons), texture: () => null, camera: new THREE.PerspectiveCamera(),
      posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion: (d, at) => blasts.push(at.clone()) }, effects: { play() { return null; } } });
    const v = system.vehicles[0];
    v.yaw = Math.PI / 2;
    const from = v.position.clone();
    v.setHealth(0);
    return blasts.at(-1).sub(from);
  };
  const plain = blastAt(null);
  assert.ok(plain.length() < 1e-6, 'no offset: at the hull');
  const head = blastAt([0, 5.9, 1]);
  // A quarter turn left takes the hull's forward (+z) to +x.
  assert.ok(head.distanceTo(new THREE.Vector3(1, 5.9, 0)) < 1e-6, `at the head: ${head.toArray()}`);
});

test('a dying walker pops its dying explosions at random nodes, raises its dust only while dying, and flies apart when it is gone', () => {
  const effects = fakeEffects();
  const calls = [];
  const debris = { pieces: [], blowApart: (...args) => calls.push(args) };
  // The page's picks, in turn: the explosion, then the node.
  const picks = [0.9, 0.99, 0.1, 0.1];
  const fx = new VehicleEffects({ effects, debris, random: () => picks.shift() ?? 0 });
  // The spider walker's pieces name the model itself, a node it lacks.
  const chunks = [{ ChunkGeometryName: 'piece', ChunkNodeName: 'CIS_walk_spider' }];
  const v = hull({ chunks, deathDust: { effect: 'deathdust', offset: [0, 2, 0], delay: 0.7 },
    dying: { effects: ['atat_explosion', 'small_explosion'], interval: 0.4, finalOffset: [0, 0, 0], span: 1 } }, { flying: false, flightState: null });
  v.bones = [v.byName.dummyroot, v.byName.hp_damage_1];
  fx.update([v], 1 / 60);
  // Its dying clock, as the simulation (or a worker's or server's record) has it.
  v.alive = false; v.health = 0; v.dying = 1;
  fx.update([v], 0.25);
  assert.equal(calls.length, 0, 'whole while it is dying');
  assert.equal(effects.played.length, 0);
  v.dying = 0.7;
  fx.update([v], 5);
  assert.equal(effects.played.length, 0, 'the clock says when, not the page\'s frame time');
  v.dying = 0.55;
  fx.update([v], 0.01);
  assert.deepEqual(effects.played.map(h => h.name), ['small_explosion'], 'once DeathAnimationExplosionTime has passed');
  assert.deepEqual(effects.played[0].position.toArray(), [10, 1, 0], 'at the node picked');
  v.dying = 0.45;
  fx.update([v], 0.01);
  assert.equal(effects.played.length, 1, 'one an interval');
  v.dying = 0.2;
  fx.update([v], 0.01);
  assert.deepEqual(effects.played.map(h => h.name), ['small_explosion', 'deathdust', 'atat_explosion'], 'the dust rises at its delay, still dying');
  assert.deepEqual(effects.played[2].position.toArray(), [10, 0, 0]);
  v.dying = 0;
  fx.update([v], 0.1);
  assert.equal(calls.length, 1, 'its pieces fly as it goes');
  assert.equal(calls[0][0], chunks);
  assert.equal(calls[0][1].cis_walk_spider, v.root, 'from the hull\'s root, where the node is missing');
  fx.update([v], 1);
  assert.equal(calls.length, 1);
  assert.equal(effects.played.length, 3, 'nothing more once gone');
  // Dust later than the dying lasts never rises.
  const late = hull({ deathDust: { effect: 'deathdust', offset: [0, 0, 0], delay: 2 }, dying: { effects: [], interval: 0.4, finalOffset: [0, 0, 0], span: 1 } });
  fx.update([late], 1 / 60);
  late.alive = false; late.dying = 1;
  for (let i = 1; i <= 10; i++) { late.dying = (10 - i) / 10; fx.update([late], 0.1); }
  fx.update([late], 3);
  assert.ok(!effects.played.slice(3).some(h => h.name === 'deathdust'));
});

test('a placed turret burns its damage effects while built and flies apart when destroyed; built again it starts afresh', () => {
  const effects = fakeEffects();
  const calls = [];
  const debris = { pieces: [], blowApart: (...args) => calls.push(args) };
  const fx = new VehicleEffects({ effects, debris });
  // A built rig as src/props.js makes it: the root placed by its matrix.
  const root = new THREE.Group();
  root.matrixAutoUpdate = false;
  root.matrix.makeTranslation(20, 0, 5);
  const smoke = new THREE.Object3D(); smoke.position.set(0, 3, 0);
  root.add(smoke);
  const chunks = [{ ChunkGeometryName: 'antiaircraft_chunk2', ChunkSpeed: 8 }];
  const building = { state: 'built', health: 500, maxHealth: 500, get alive() { return this.state === 'built'; },
    desc: { class: 'bes_bldg_AAA_turret', fx: { damage: [
      { start: 0.5, stop: 0.3, effect: 'vehiclespark', scale: 1, inherit: 0, node: '', offset: [0, 0, 0] },
      { start: 0.3, stop: 0, effect: 'smokeplume', scale: 1, inherit: 0, node: 'hp_smoke_1', offset: [0, 0, 0] }], chunks } },
    looks: { built: { root, byName: { hp_smoke_1: smoke } } }, bodies: { built: { name: 'turret' }, destroyed: { name: 'wreck' } } };
  const unarmed = { state: 'built', health: 80, maxHealth: 80, get alive() { return true; }, desc: { class: 'com_item_healthrecharge' }, looks: {} };
  fx.updateBuildings([building, unarmed], 1 / 60);
  assert.equal(effects.played.length, 0);
  building.health = 200;
  fx.updateBuildings([building, unarmed], 1 / 60);
  assert.deepEqual(effects.played.map(h => h.name), ['vehiclespark'], 'at the turret itself: the record names no node');
  assert.deepEqual(effects.played[0].position.toArray(), [20, 0, 5]);
  building.health = 100;
  fx.updateBuildings([building], 1 / 60);
  assert.deepEqual(effects.played.map(h => h.name), ['vehiclespark', 'smokeplume']);
  assert.equal(effects.played[0].stopped, true);
  assert.deepEqual(effects.played[1].position.toArray(), [20, 3, 5], 'at hp_smoke_1');
  // Destroyed: its wreck shows (Building.show), its effects stop, its pieces fly from where it stands.
  building.state = 'destroyed'; building.health = 0;
  fx.updateBuildings([building], 1 / 60);
  assert.equal(effects.played[1].stopped, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], chunks);
  assert.equal(calls[0][1][''], root, 'a piece without a node starts at the turret');
  assert.deepEqual(calls[0][2].toArray(), [20, 0, 5]);
  assert.equal(calls[0][3].length(), 0, 'standing still');
  assert.equal(calls[0][5], building.bodies.destroyed, 'the pieces pass through the wreck they start inside');
  // Coming back by itself (its health rises in the wreck), then built again: a new life.
  building.health = 400;
  fx.updateBuildings([building], 1);
  building.state = 'built'; building.health = 500;
  fx.updateBuildings([building], 1 / 60);
  building.health = 100;
  fx.updateBuildings([building], 1 / 60);
  assert.deepEqual(effects.played.map(h => h.name), ['vehiclespark', 'smokeplume', 'smokeplume']);
  building.state = 'destroyed';
  fx.updateBuildings([building], 1 / 60);
  assert.equal(calls.length, 2, 'again at its next destruction');
});

test('a piece passes through the wreck it starts inside, and lands on the ground', async () => {
  const { CollisionWorld } = await import('../src/collision.js');
  const { Debris } = await import('../src/debris.js');
  const box = (x, y, z, w, h, d) => {
    const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
    return [g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3)];
  };
  const fly = (ignoreWreck) => {
    const collision = new CollisionWorld(...box(0, -1, 0, 200, 2, 200));
    const wreck = collision.addBody(...box(0, 2, 0, 4, 4, 4));
    const effects = fakeEffects();
    const debris = new Debris({ scene: new THREE.Scene(), collision, effects, model: () => () => new THREE.Group() });
    const root = new THREE.Object3D(); root.position.set(0, 0.5, 0); root.updateMatrixWorld(true);
    debris.blowApart([{ ChunkGeometryName: 'piece', ChunkSpeed: 8, ChunkTerrainCollisions: 3, ChunkTerrainEffect: 'explosion' }], { '': root },
      new THREE.Vector3(0, 0.5, 0), new THREE.Vector3(), -50, ignoreWreck ? wreck : null);
    const piece = debris.pieces[0];
    let peak = 0;
    for (let i = 0; i < 240; i++) { debris.update(1 / 60); peak = Math.max(peak, piece.position.y); }
    return { peak, first: effects.played[0]?.position.y ?? null };
  };
  const through = fly(true);
  assert.ok(through.peak > 3, 'it rises clear of the wreck');
  assert.ok(through.first !== null && through.first < 0.2, 'its first hit is the ground');
  const stuck = fly(false);
  assert.ok(stuck.peak < 4, 'without it the piece is caught inside');
});

// A hull's piece as the extras pack has it: modelled in the hull's frame,
// here a 4 x 1 x 2 m plate whose middle is 4 m out and 3 m up.
const plate = () => () => {
  const group = new THREE.Group();
  group.add(new THREE.Mesh(new THREE.BoxGeometry(4, 1, 2).translate(4, 3, 0)));
  return group;
};
const ground = async () => {
  const { CollisionWorld } = await import('../src/collision.js');
  const g = new THREE.BoxGeometry(400, 2, 400).translate(0, -1, 0);
  return new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
};

test('a hull\'s piece flies out from where it is modelled, spins about its middle and comes to lie on the ground', async () => {
  const { Debris } = await import('../src/debris.js');
  const debris = new Debris({ scene: new THREE.Scene(), collision: await ground(), effects: fakeEffects(), model: plate });
  // Carried by the hull's root, on the ground, as every node-less chunk is.
  const root = new THREE.Object3D(); root.updateMatrixWorld(true);
  debris.blowApart([{ ChunkGeometryName: 'piece', ChunkSpeed: 8, ChunkOmega: [2, 2, 2], ChunkTerrainCollisions: 3 }], { '': root },
    new THREE.Vector3(), new THREE.Vector3(), -50, null, { centre: true, floor: 0 });
  const piece = debris.pieces[0];
  assert.deepEqual(piece.position.toArray(), [4, 3, 0], 'at its middle');
  assert.ok(piece.velocity.x > 5 && piece.velocity.y > 0, `out and up: ${piece.velocity.toArray()}`);
  const box = new THREE.Box3().setFromObject(piece.mesh);
  assert.ok(box.min.y > 2.4 && box.max.y < 3.6, 'drawn where it was modelled');
  for (let i = 0; i < 600; i++) debris.update(1 / 60);
  assert.equal(piece.resting, true);
  assert.ok(piece.position.x > 8, `it went somewhere: ${piece.position.toArray()}`);
  piece.mesh.updateMatrixWorld(true);
  box.setFromObject(piece.mesh);
  assert.ok(box.min.y > -0.3 && box.min.y < 0.05, `lying on the floor: ${box.min.y}`);
  assert.ok(box.max.y - box.min.y < 1.2, `on its broad side: ${box.max.y - box.min.y} m tall`);
  // Before, carried by the root, it went straight up from the ground and
  // lay where it started, drawn 2.5 m in the air.
  const old = new Debris({ scene: new THREE.Scene(), collision: await ground(), effects: fakeEffects(), model: plate });
  old.blowApart([{ ChunkGeometryName: 'piece', ChunkSpeed: 8, ChunkTerrainCollisions: 3 }], { '': root }, new THREE.Vector3(), new THREE.Vector3(), -50);
  assert.deepEqual(old.pieces[0].velocity.toArray(), [0, 8, 0], 'a droid\'s pieces are as they were');
});

test('a dead walker\'s pieces start where its fallen body lies, never under the ground', () => {
  // A walker whose body bone stood 4 m up and lies 2.5 m lower, tipped.
  const root = new THREE.Object3D(); root.position.set(10, 0, 0);
  const dummy = new THREE.Bone(); const body = new THREE.Bone(); body.name = 'bone_root';
  body.position.set(0, 4, 0);
  root.add(dummy); dummy.add(body);
  const bones = [dummy, body];
  const v = { root, bones, byName: { dummyroot: dummy, bone_root: body },
    bindPositions: bones.map(b => b.position.clone()), bindQuaternions: bones.map(b => b.quaternion.clone()) };
  root.updateMatrixWorld(true);
  const standing = bodyCarrier(v).matrixWorld.clone();
  assert.ok(standing.equals(root.matrixWorld), 'standing, its pieces start where they are modelled');
  body.position.set(0, 1.5, 0); body.quaternion.setFromAxisAngle(new THREE.Vector3(0, 0, 1), 0.2);
  root.updateMatrixWorld(true);
  const at = new THREE.Vector3(0, 4, 0).applyMatrix4(bodyCarrier(v).matrixWorld);
  assert.ok(at.distanceTo(new THREE.Vector3(10, 1.5, 0)) < 1e-6, `a piece at the body goes down with it: ${at.toArray()}`);
  assert.equal(bodyCarrier({ ...v, byName: { dummyroot: dummy } }), null, 'without a body bone the root carries them');
  // Through VehicleEffects: its pieces are carried so, floored at its ground.
  const calls = [];
  const fx = new VehicleEffects({ effects: fakeEffects(), debris: { pieces: [], blowApart: (...args) => calls.push(args) } });
  const walker = { ...v, name: 'walker', alive: true, health: 1, maxHealth: 1, flying: false, velocity: new THREE.Vector3(), position: root.position,
    aiTarget: { generation: 1 }, def: { fx: { chunks: [{ ChunkGeometryName: 'piece' }], dying: { effects: [], interval: 1, finalOffset: [0, 0, 0], span: 1 } } } };
  fx.update([walker], 1 / 60);
  walker.alive = false; walker.dying = 1;
  fx.update([walker], 1 / 60);
  walker.dying = 0;
  fx.update([walker], 1 / 60);
  assert.equal(calls.length, 1);
  const carried = new THREE.Vector3(0, 4, 0).applyMatrix4(calls[0][1][''].matrixWorld);
  assert.ok(carried.distanceTo(new THREE.Vector3(10, 1.5, 0)) < 1e-6);
  assert.deepEqual(calls[0][6], { centre: true, floor: 0 });
});
