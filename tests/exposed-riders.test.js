// Riders in the open (docs/formats.md "Riders"): rounds meet a soldier shown
// on an exposed seat where his body is drawn, he dies there as on foot, and
// his seat is let go with the hull left for anyone. Synthetic fixtures (a
// box hull, a made-up skeleton and clip, no retail content), then the
// imported battles when an extract is present.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles, VehicleDriver } from '../src/vehicles.js';
import { Units, makeUnitHit, segmentCapsule, strikeable } from '../src/registry.js';
import { Ordnance } from '../src/ordnance.js';
import { HitHistory } from '../src/runtime/hit-history.js';
import { Unit } from '../src/units.js';
import { leaveKilled } from '../src/boarding.js';

const same = (actual, expected, message) => assert.ok(actual === expected, message ?? `${typeof actual} !== ${typeof expected}`);
const near = (a, b, eps, message) => assert.ok(Math.abs(a - b) <= eps, `${message}: ${a} vs ${b}`);
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};

// A soldier skeleton with the joints a rider's body hangs from, at rest
// along y: feet at the origin, pelvis 1 m up, neck 1.55, head 1.67.
const JOINTS = [
  ['DummyRoot', null, [0, 0, 0]], ['bone_root', 'DummyRoot', [0, 1, 0]], ['bone_pelvis', 'bone_root', [0, 0, 0]],
  ['bone_neck', 'bone_root', [0, 0.55, 0]], ['bone_head', 'bone_neck', [0, 0.12, 0]],
  ['bone_l_thigh', 'bone_pelvis', [0.1, -0.1, 0]], ['bone_l_calf', 'bone_l_thigh', [0, -0.42, 0]], ['bone_l_foot', 'bone_l_calf', [0, -0.35, 0]],
  ['bone_r_thigh', 'bone_pelvis', [-0.1, -0.1, 0]], ['bone_r_calf', 'bone_r_thigh', [0, -0.42, 0]], ['bone_r_foot', 'bone_r_calf', [0, -0.35, 0]],
];
// The seat's clip: one frame, every joint at rest but the root, which the
// clip carries 0.2 m forward (bone_root takes its clip's translation).
function rideClip() {
  const data = new Float32Array(JOINTS.length * 7);
  JOINTS.forEach(([name, , at], i) => {
    const t = name === 'bone_root' ? [0, 1, 0.2] : at;
    data.set([0, 0, 0, 1, ...t], i * 7);
  });
  return { name: 'human_ride', fps: 30, frames: 1, duration: 0, rootMotion: false, tracks: JOINTS.map((_, i) => i), data };
}

let nextId = 500;
// A bot (a real Unit) of `team` with that skeleton.
function rider(team = 1) {
  const root = new THREE.Group();
  const bones = JOINTS.map(([name, , at]) => { const b = new THREE.Bone(); b.name = name; b.position.fromArray(at); return b; });
  JOINTS.forEach(([, parent], i) => (parent ? bones[JOINTS.findIndex(([n]) => n === parent)] : root).add(bones[i]));
  const byName = Object.fromEntries(bones.map((b) => [b.name, b]));
  const calls = { die: 0, reset: 0, gunner: [] };
  const animator = {
    bones, clips: new Map([['human_ride', rideClip()]]), rootBind: new THREE.Vector3(),
    bindLengths: bones.filter((b) => !['DummyRoot', 'bone_root'].includes(b.name)).map((b) => [b, b.position.clone()]),
    gunner(clip, pitch, turn, time) { calls.gunner.push({ clip: clip?.name ?? null, time }); },
    reset() { calls.reset++; }, die() { calls.die++; },
  };
  const soldier = {
    position: new THREE.Vector3(30, 0, 0), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, posture: 'stand', air: null, dive: 0, grounded: true, passThrough: null,
    spawn(x, y, z, yaw) { this.position.set(x, y, z); this.aimYaw = yaw; this.velocity.set(0, 0, 0); },
    pitchRange: () => [-1.4, 1.4],
  };
  const combat = { mounted: false, dead: false, team, health: 300, lastAttacker: null,
    damage(amount, onShield, point, attacker) { if (this.dead) return; this.health -= amount; this.lastAttacker = attacker;
      if (this.health <= 0) { this.dead = true; animator.die(); unit.alive = false; unit.dead = true; leaveKilled(unit); } } };
  const unit = new Unit({ side: 'test', className: 'test_trooper', def: {}, actor: { root, bones, byName }, soldier, animator, combat, presentation: { reset() {} } });
  unit.id = nextId++; unit.generation = 1; unit.team = team; unit.alive = true; unit.removed = false;
  unit.calls = calls;
  return unit;
}

// A hull with riders' seats on a floor: seat 0 shows its rider at hp_seat,
// 1.5 m up over a box 1 m tall; seat 1 is a gunner's seat that shows him
// too; settings for each seat's section. healthType: 'animal' for a riding
// animal. enclose: the hull's collision box swallows the riders.
function mount({ seats = [{}], healthType = null, enclose = false } = {}) {
  const floor = box(0, -1, 0, 4000, 2, 4000);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const bones = [{ name: 'dummyroot', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'hp_seat', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 1.5, 0).toArray() },
    { name: 'hp_gunner', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 1.5, -1.5).toArray() }];
  const seatDefs = seats.map((settings, i) => ({ name: i ? `TURRET${i}` : 'BODY', weapons: [],
    settings: { PilotPosition: i ? 'hp_gunner' : 'hp_seat', PilotAnimation: 'ride', ...settings } }));
  const settings = { MaxHealth: 1000, ForwardSpeed: 20, Deceleration: 10, ...(healthType ? { HealthType: healthType } : {}) };
  const def = { model: 'mount', base: 'hover', label: 'Test mount', settings, seats: seatDefs, explosion: null };
  const hullBox = enclose ? box(0, 2, 0, 3, 4, 5) : box(0, 0.5, 0, 2, 1, 4);
  const data = { models: { mount: { bones, parts: [] } }, collision: { mount: hullBox }, classes: { mount: def },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', respawnTime: 2, classes: { 1: 'mount' } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision,
    texture: () => null, camera: new THREE.PerspectiveCamera(), posts: { posts: [] }, ordnance: { fire() {}, explosion() {}, lights: { flash() {} } },
    effects: { play() { return null; } } });
  const v = system.vehicles[0];
  if (healthType) v.healthType = healthType;
  const units = new Units();
  system.attachUnits(units);
  return { collision, system, v, units };
}

// Rounds through the shared Ordnance, struck units and hulls recorded.
function rounds(world) {
  const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: world.collision, effects: { play() {} }, lights: { add() {}, flash() {} }, texture: () => null });
  const struck = { units: [], hulls: [] };
  ordnance.units = world.units;
  ordnance.onHitUnit = (unit, def, point, owner, part) => { struck.units.push({ unit, part, point: point.clone() }); unit.combat.damage((def.Damage || 0) * (part === 'head' ? 3 : 1), 0, point, owner); };
  ordnance.onHit = (body) => struck.hulls.push(body);
  return { ordnance, struck };
}
const rifle = { base: 'bolt', Velocity: 300, LifeSpan: 2, Damage: 50 };
// Steps the rounds until none fly.
const fly = (ordnance) => { for (let i = 0; i < 120 && (ordnance.bolts.length || ordnance.rounds.length); i++) ordnance.update(1 / 60, false); };

const nodeOf = (world, index = 0) => world.v.seats[index].view.rider.node.getWorldPosition(new THREE.Vector3());

function seat(world, unit, index = 0) {
  world.units.add(unit);
  assert.ok(world.system.board(unit, world.v, index), 'boards');
  unit.tickCount++;
  return unit;
}

test('capsules meet rays on their sides and rounded ends, from outside and from within', () => {
  const center = new THREE.Vector3(0, 1, 0), half = new THREE.Vector3(0, 0.5, 0);
  const at = (o, d, far = 100) => segmentCapsule(new THREE.Vector3(...o), new THREE.Vector3(...d).normalize(), far, center, half, 0.25);
  near(at([-5, 1, 0], [1, 0, 0]), 4.75, 1e-9, 'side');
  near(at([0, 5, 0], [0, -1, 0]), 3.25, 1e-9, 'top end');
  near(at([0, -5, 0], [0, 1, 0]), 5.25, 1e-9, 'bottom end');
  near(at([-5, 1.6, 0], [1, 0, 0]), 5 - Math.sqrt(0.25 ** 2 - 0.1 ** 2), 1e-9, 'past the axis, the rounded end');
  assert.equal(at([-5, 1.8, 0], [1, 0, 0]), null, 'clear above');
  assert.equal(at([-5, 1, 0], [1, 0, 0], 4), null, 'out of reach');
  assert.equal(at([0.1, 1.2, 0], [1, 0, 0]), 0, 'inside');
});

test('a rider shown in the open is struck where his body is drawn; sight tests and enclosed or shielded seats pass him by', () => {
  const world = mount({ seats: [{}, { '2b60e217': '0' }, { PilotAnimation: undefined }] });
  const unit = seat(world, rider());
  same(unit.seat.vehicle, world.v);
  assert.equal(unit.seat.exposed, true);
  assert.equal(strikeable(unit), true);
  const shapes = unit.hitShapes();
  assert.equal(shapes.length, 6);
  const head = shapes.find((s) => s.part === 'head');
  // On the seat's node, the clip's root 0.2 m forward, the head bone 1.67 m
  // above the feet, its middle (0.05, 0.04, 0) along it. (The hull bobs on
  // its repulsors: within a few centimetres of the node's own turn.)
  const node = nodeOf(world);
  near(head.center.x - node.x, 0.05, 0.03, 'head x'); near(head.center.y - node.y, 1.67 + 0.04, 0.03, 'head y'); near(head.center.z - node.z, 0.2, 0.03, 'head z');
  assert.equal(shapes[1].type, 'capsule');
  // From the pelvis (1 m) up to 0.25 m under the neck (1.55 m): its
  // rounded top ends at the neck, below the head.
  near(shapes[1].center.y - node.y, (1 + 1.3) / 2, 0.03, 'torso from pelvis to neck');
  // The seat's own clip, drawn and hit alike.
  assert.equal(unit.calls.gunner.at(-1).clip, 'human_ride');
  const from = new THREE.Vector3(-10, head.center.y, head.center.z), dir = new THREE.Vector3(1, 0, 0);
  const hit = world.units.raycast(from, dir, 20, null, makeUnitHit(), null, true);
  same(hit?.unit, unit, 'a round meets him');
  assert.equal(hit.part, 'head');
  assert.equal(world.units.raycast(from, dir, 20, null, makeUnitHit()), null, 'sight leaves him out, as anyone seated');

  // Seat 1's section shields its rider (hash 2b60e217 0); seat 2 shows none.
  for (const index of [1, 2]) {
    const other = mount({ seats: [{}, { '2b60e217': '0' }, { PilotAnimation: undefined }] });
    const sitter = seat(other, rider(), index);
    assert.equal(sitter.seat.exposed, false, `seat ${index}`);
    assert.equal(strikeable(sitter), false);
    const c = sitter.sphere.center;
    assert.equal(other.units.raycast(new THREE.Vector3(c.x - 10, c.y, c.z), dir, 20, null, makeUnitHit(), null, true), null, `seat ${index} is out of reach`);
  }
});

test('a rifle kills a rider in the open as on foot: his hull takes nothing, his seat is let go, the hull stays for anyone', () => {
  const world = mount();
  const { ordnance, struck } = rounds(world);
  const unit = seat(world, rider(1));
  const shooter = { id: 1, generation: 1, team: 2, alive: true };
  const torso = unit.hitShapes()[1].center.clone();
  let shots = 0;
  while (unit.alive && shots < 20) {
    ordnance.fire(rifle, new THREE.Vector3(torso.x - 20, torso.y, torso.z), new THREE.Vector3(1, 0, 0), shooter);
    fly(ordnance);
    shots++;
  }
  assert.equal(shots, 6, 'six 50-point bolts for 300 health');
  assert.equal(struck.hulls.length, 0, 'the hull takes none of them');
  assert.ok(struck.units.every((s) => s.unit === unit && s.part === 'body'));
  assert.equal(unit.seat, null);
  assert.equal(world.v.seatFree(0), true);
  assert.equal(world.v.alive, true);
  assert.equal(world.v.occupied, false);
  // His body falls from the seat: under his pelvis, his soles where his
  // feet were drawn (the ankles 0.87 m below the pelvis, 0.13 above the
  // soles: here at the node), at rest, his death clip under way.
  const node = nodeOf(world);
  near(unit.soldier.position.y - node.y, 0, 0.03, 'drop height');
  near(unit.soldier.position.z - node.z, 0.2, 0.03, 'under the pelvis');
  assert.equal(unit.soldier.velocity.length(), 0);
  assert.equal(unit.soldier.grounded, false);
  assert.equal(unit.calls.die, 1);
  assert.equal(unit.calls.gunner.at(-1).clip, null, 'out of the seat pose');
  assert.equal(strikeable(unit), true, 'a corpse still stops rounds');
});

test('a headshot does three times the damage on a rider too', () => {
  const world = mount();
  const { ordnance, struck } = rounds(world);
  const unit = seat(world, rider(1));
  const head = unit.hitShapes()[0].center.clone();
  ordnance.fire(rifle, new THREE.Vector3(head.x - 20, head.y, head.z), new THREE.Vector3(1, 0, 0), { id: 1, generation: 1, team: 2, alive: true });
  fly(ordnance);
  assert.equal(struck.units[0]?.part, 'head');
  assert.equal(unit.combat.health, 150);
});

test("a hull's own guns never strike the riders on it", () => {
  const world = mount();
  const { ordnance, struck } = rounds(world);
  const unit = seat(world, rider(1));
  const torso = unit.hitShapes()[1].center.clone();
  // A round passing through him, fired from the hull (it passes through the
  // hull's own body as every mounted round does).
  ordnance.fire(rifle, new THREE.Vector3(torso.x - 3, torso.y, torso.z), new THREE.Vector3(1, 0, 0), { id: 2, generation: 1, team: 1, alive: true }, null, world.v.body);
  fly(ordnance);
  assert.equal(struck.units.length, 0);
  assert.equal(unit.combat.health, 300);
});

test('killed on his seat, the last bot aboard takes it over, as after an exit', () => {
  const world = mount({ seats: [{}, {}] });
  const pilot = seat(world, rider(1), 0);
  const gunner = seat(world, rider(1), 1);
  pilot.combat.damage(Infinity, 0, null, null);
  same(world.v.seats[0].lease?.unit, gunner, 'the gunner moves to the controls');
  assert.equal(world.v.seats[1].lease, null);
  assert.equal(pilot.seat, null);
});

test("a riding animal's collision doesn't keep rounds off its rider; any other hull's does", () => {
  for (const healthType of ['animal', null]) {
    const world = mount({ healthType, enclose: true });
    const { ordnance, struck } = rounds(world);
    const unit = seat(world, rider(1));
    const torso = unit.hitShapes()[1].center.clone();
    ordnance.fire(rifle, new THREE.Vector3(torso.x - 20, torso.y, torso.z), new THREE.Vector3(1, 0, 0), { id: 1, generation: 1, team: 2, alive: true });
    fly(ordnance);
    if (healthType) {
      assert.equal(struck.units.length, 1, 'the animal lets the round on to its rider');
      assert.equal(struck.hulls.length, 0);
    } else {
      assert.equal(struck.units.length, 0, 'a hull that swallows its rider shields him');
      assert.equal(struck.hulls.length, 1);
    }
  }
  // Killed on the animal, his body falls through it to the ground.
  const world = mount({ healthType: 'animal', enclose: true });
  const unit = seat(world, rider(1));
  unit.combat.damage(Infinity, 0, null, null);
  same(unit.soldier.passThrough, world.v.body);
});

test('the player killed on an exposed seat leaves it where he rode, his death clip kept', () => {
  const world = mount({ seats: [{}, {}] });
  const player = rider(1);
  player.human = true;
  world.units.add(player);
  const bot = seat(world, rider(1), 1);
  const driver = new VehicleDriver({ vehicles: world.system, soldier: player.soldier, combat: { ...player.combat, owner: player, team: 1 }, actor: player.actor,
    animator: player.animator, clips: player.animator.clips, camera: new THREE.PerspectiveCamera(), collision: world.collision,
    hud: { message() {} }, presentation: { reset() {} } });
  assert.ok(driver.enter(world.v));
  same(player.seat?.vehicle, world.v);
  player.tickCount++;
  assert.equal(player.seat.exposed, true);
  assert.equal(player.hitShapes().length, 6, 'his body is struck as a bot\'s is');
  const resets = player.calls.reset;
  assert.ok(leaveKilled(player));
  assert.equal(driver.active, false);
  assert.equal(player.seat, null);
  assert.equal(player.calls.reset, resets, 'his death clip is not cut short');
  near(player.soldier.position.y - nodeOf(world).y, 0, 0.03, 'his body falls from the seat');
  same(world.v.seats[0].lease?.unit, bot, 'the bot aboard takes the controls');
});

test("the server's hit history keeps riders in the open", () => {
  const world = mount();
  const unit = seat(world, rider(1));
  const history = new HitHistory(world.units);
  history.capture(0);
  const head = unit.hitShapes()[0].center.clone();
  const from = new THREE.Vector3(head.x - 10, head.y, head.z), dir = new THREE.Vector3(1, 0, 0);
  same(history.raycast(0, from, dir, 20, null, makeUnitHit(), null, true)?.unit, unit);
  assert.equal(history.raycast(0, from, dir, 20, null, makeUnitHit(), null), null, 'only rounds that ask for riders');
});

// The imported battles (BFE_CONTENT_DIR, or public/): rifle bolts at the
// riders the 2004 game shows, in the headless world the worker and the
// server run.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
const has = (...maps) => maps.every((m) => fs.existsSync(path.join(CONTENT, `maps/${m}/vehicles/vehicles.json`)));
test('imported riders: speeder bikes, skiffs and a tauntaun die to rifle bolts; fighters\' pilots and the AT-ST\'s crew are out of reach', { skip: !has('tat1', 'hot1') && 'no tat1/hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  for (const [map, side] of [['tat1', 'rep'], ['hot1', 'all']]) {
    const game = await headlessGame(`map=${map}&side=${side}&ai=off`);
    await game.ready;
    const T = game.THREE;
    const step = (n) => { for (let i = 0; i < n; i++) { const tick = game.worldClock.tick + 1; game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] }); } };
    const plain = (o) => o && !o.ExplosionImpact && o.base === 'bolt';
    let bolt = null;
    for (const s of Object.values(game.data.sides)) for (const d of Object.values(s.units || {})) for (const w of d.weapons || []) if (!bolt && plain(w.ordnance) && w.ordnance.Damage === 50) bolt = w.ordnance;
    let className = null;
    for (const s of Object.values(game.data.sides)) for (const [n, d] of Object.entries(s.units || {})) if (!className && d.base === 'soldier' && !d.hero && !Number(d.NoEnterVehicles ?? 0)) className = n;
    await game.factory.prepare(className);
    const shooter = { id: 9999, generation: 1, team: 3, alive: true };
    const checked = new Set();
    for (const v of game.vehicles.vehicles) {
      if (!v.alive || checked.has(v.name)) continue;
      checked.add(v.name);
      for (let index = 0; index < v.seats.length; index++) {
        const unit = game.factory.acquire(className);
        unit.team = unit.combat.team = v.team || 1;
        unit.spawn(v.position.x + 3, v.position.y + 1, v.position.z, 0);
        if (!game.vehicles.board(unit, v, index)) { unit.remove(); continue; }
        step(2);
        const exposed = Boolean(unit.seat.exposed), hull = v.health;
        let shots = 0;
        // Along a line clear of anything but the rider's own mount.
        const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3() };
        while (unit.alive && shots < 12) {
          const target = unit.hitShapes()[exposed ? 1 : 0].center.clone();
          let from = null;
          for (let k = 0; k < 32 && !from; k++) {
            const out = new T.Vector3(Math.sin(k * 0.7), 0.3 + (k % 4) * 0.3, Math.cos(k * 0.7)).normalize();
            const start = target.clone().addScaledVector(out, 20);
            const back = game.collision.cast(start, out.clone().negate(), 20, hit, v.body);
            if (!back || hit.distance > 19.5) from = start;
          }
          if (!from) break;
          game.ordnance.fire(bolt, from, target.clone().sub(from).normalize(), shooter);
          step(8);
          shots++;
        }
        const expect = exposed ? 6 : null;
        if (exposed) {
          assert.equal(shots, expect, `${v.name} seat ${index}: shots to kill`);
          assert.equal(unit.alive, false, `${v.name} seat ${index}`);
          assert.equal(v.seatFree(index), true);
          assert.equal(v.alive, true);
          assert.equal(v.health, hull, `${v.name} takes none of it`);
        } else assert.equal(unit.combat.health, 300, `${v.name} seat ${index} is out of reach`);
        if (unit.seat) game.vehicles.unboard(unit, { force: true, placement: false });
        unit.remove();
      }
    }
    assert.ok([...checked].some((name) => /speederbike|tauntaun/.test(name)), 'a mount was checked');
  }
});

test('a crewman hidden inside a hull that is destroyed stays out of sight in that life; one in the open is drawn dying', () => {
  // Seat 0 hides its rider (no pilot animation); seat 1 shows him.
  const world = mount({ seats: [{ PilotAnimation: undefined }, {}] });
  const inside = seat(world, rider(), 0);
  const open = seat(world, rider(), 1);
  world.system.presentRiders(1 / 60);
  assert.equal(inside.actor.root.visible, false, 'hidden while he rides inside');
  assert.equal(open.actor.root.visible, true);
  // The hull is destroyed: its crew die with it, and the page draws their bodies again.
  world.v.setHealth(0);
  assert.equal(inside.dead, true);
  inside.actor.root.visible = open.actor.root.visible = true;
  world.system.presentRiders(1 / 60);
  assert.equal(inside.actor.root.visible, false, 'he dies with the hull, out of sight');
  assert.equal(open.actor.root.visible, true, 'the open gunner\'s body is his own');
  inside.actor.root.visible = true;
  world.system.presentRiders(1 / 60);
  assert.equal(inside.actor.root.visible, false, 'kept hidden whatever drew it');
  // His next life shows him.
  inside.generation += 1;
  inside.actor.root.visible = true;
  world.system.presentRiders(1 / 60);
  assert.equal(inside.actor.root.visible, true);
  assert.equal(world.system.vanished.size, 0);
});
