// Turret gunners (docs/formats.md "Riders" and "Turrets"): a turret whose
// class names a pilot animation shows its gunner, and rounds, swings and
// blasts meet him where he is drawn, whoever he is (a bot, a network human,
// the player); one that names none hides him. Killed at the gun he leaves it
// at once, his body falling from where he stood, and the turret waits for
// the next gunner. Synthetic fixtures (a made-up turret, skeleton and clip,
// no retail content), then the imported Hoth turrets when an extract is
// present.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Turret } from '../src/props.js';
import { SeatLease, TurretSeats } from '../src/seats.js';
import { Gunner } from '../src/gunner.js';
import { Units, makeUnitHit, strikeable } from '../src/registry.js';
import { Ordnance } from '../src/ordnance.js';
import { Unit } from '../src/units.js';
import { leaveKilled } from '../src/boarding.js';
import { PlayerCombat } from '../src/combat.js';
import { Soldier } from '../src/soldier.js';

const same = (actual, expected, message) => assert.ok(actual === expected, message ?? `${typeof actual} !== ${typeof expected}`);
const near = (a, b, eps, message) => assert.ok(Math.abs(a - b) <= eps, `${message}: ${a} vs ${b}`);
const box = (x, y, z, w, h, d) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};

// A soldier skeleton with the joints a gunner's body hangs from, at rest
// along y: feet at the origin, pelvis 1 m up, neck 1.55, head 1.67.
const JOINTS = [
  ['DummyRoot', null, [0, 0, 0]], ['bone_root', 'DummyRoot', [0, 1, 0]], ['bone_pelvis', 'bone_root', [0, 0, 0]],
  ['bone_neck', 'bone_root', [0, 0.55, 0]], ['bone_head', 'bone_neck', [0, 0.12, 0]],
  ['bone_l_thigh', 'bone_pelvis', [0.1, -0.1, 0]], ['bone_l_calf', 'bone_l_thigh', [0, -0.42, 0]], ['bone_l_foot', 'bone_l_calf', [0, -0.35, 0]],
  ['bone_r_thigh', 'bone_pelvis', [-0.1, -0.1, 0]], ['bone_r_calf', 'bone_r_thigh', [0, -0.42, 0]], ['bone_r_foot', 'bone_r_calf', [0, -0.35, 0]],
];
// The gunner's 9-pose: nine frames of the rest pose with the root carried
// 0.2 m forward (bone_root takes its clip's translation); frame 1, the gun
// raised, carries it 0.1 m higher too.
function ninePose() {
  const frames = 9;
  const data = new Float32Array(JOINTS.length * frames * 7);
  JOINTS.forEach(([name, , at], k) => {
    for (let f = 0; f < frames; f++) data.set([0, 0, 0, 1, ...(name === 'bone_root' ? [0, f === 1 ? 1.1 : 1, 0.2] : at)], (k * frames + f) * 7);
  });
  return { name: 'human_minigun_9pose', fps: 30, frames, duration: (frames - 1) / 30, rootMotion: false, tracks: JOINTS.map((_, i) => i), data };
}

let nextId = 700;
// Arms, for a skeleton that has them: the upper arms out to the sides from
// the neck, then straight down (x runs down every limb bone): shoulders
// 1.5 m up, elbows 1.25, wrists 1.0.
const ARM_JOINTS = ['l', 'r'].flatMap((side, i) => {
  const out = i ? -0.2 : 0.2;
  return [[`bone_${side}_upperarm`, 'bone_neck', [out, -0.05, 0]], [`bone_${side}_forearm`, `bone_${side}_upperarm`, [0.25, 0, 0]], [`bone_${side}_hand`, `bone_${side}_forearm`, [0.25, 0, 0]]];
});
const DOWN = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), -Math.PI / 2); // bone x pointing down

// A soldier (a real Unit) of `team` with that skeleton (with arms if asked).
// Killed, he leaves his seat as game.js has every death do (src/boarding.js
// leaveKilled).
function soldierUnit(team = 1, { arms = false } = {}) {
  const root = new THREE.Group();
  const joints = arms ? [...JOINTS, ...ARM_JOINTS] : JOINTS;
  const bones = joints.map(([name, , at]) => { const b = new THREE.Bone(); b.name = name; b.position.fromArray(at); return b; });
  joints.forEach(([, parent], i) => (parent ? bones[joints.findIndex(([n]) => n === parent)] : root).add(bones[i]));
  // An arm's first joint turns its bone's x down; the rest keep it.
  if (arms) for (const side of ['l', 'r']) bones.find((b) => b.name === `bone_${side}_upperarm`).quaternion.copy(DOWN);
  const byName = Object.fromEntries(bones.map((b) => [b.name, b]));
  const actor = { root, bones, byName };
  const calls = { die: 0, reset: 0, gunner: [], spawns: 0 };
  const animator = {
    actor, bones, clips: new Map([['human_minigun_9pose', ninePose()]]), rootBind: new THREE.Vector3(),
    bindLengths: bones.filter((b) => !['DummyRoot', 'bone_root'].includes(b.name)).map((b) => [b, b.position.clone()]),
    gunner(clip, pitch, turn, time, seat = null) { calls.gunner.push({ clip: clip?.name ?? null, pitch, turn, time, seat: seat?.clone() ?? null }); },
    reset() { calls.reset++; }, die() { calls.die++; },
  };
  const soldier = {
    position: new THREE.Vector3(0, 0, 8), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(),
    aimYaw: 0, aimPitch: 0, posture: 'stand', air: null, dive: 0, grounded: true, passThrough: null,
    spawn(x, y, z, yaw) { calls.spawns++; this.position.set(x, y, z); this.aimYaw = yaw; this.velocity.set(0, 0, 0); },
    pitchRange: () => [-1.4, 1.4],
  };
  const combat = { mounted: false, dead: false, team, health: 300, lastAttacker: null, blowApart: false,
    damage(amount, onShield, point, attacker) { if (this.dead) return; this.health -= amount; this.lastAttacker = attacker;
      if (this.health <= 0) { this.dead = true; animator.die(); unit.alive = false; unit.dead = true; leaveKilled(unit); } } };
  const unit = new Unit({ side: 'test', className: 'test_trooper', def: {}, actor, soldier, animator, combat, presentation: { reset() {} } });
  unit.id = nextId++; unit.generation = 1; unit.team = team; unit.alive = true; unit.removed = false;
  combat.owner = unit;
  unit.calls = calls;
  return unit;
}

// A gun turret on a floor at the origin, facing +z: the drum turns about
// its middle, the gunner's seat 0.5 m behind it, 0.2 m up, in a tub 1 m
// tall that the turret's collision fills, the gun 1.3 m up. settings: its
// class's, over a minigun turret's. drumTurn: a turn about the vertical the
// drum's bind pose has (the barge turret's is half round).
function emplacement(settings = {}, { drumTurn = 0 } = {}) {
  const floor = box(0, -1, 0, 400, 2, 400);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const rig = { root: new THREE.Group(), byName: {} };
  for (const name of ['drum', 'gun', 'seat', 'hp_fire']) { const o = new THREE.Object3D(); o.name = name; rig.byName[name] = o; }
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.gun.add(rig.byName.hp_fire); rig.byName.drum.add(rig.byName.seat);
  rig.byName.gun.position.set(0, 1.3, 0.4); rig.byName.hp_fire.position.z = 1; rig.byName.seat.position.set(0, 0.2, -0.5);
  rig.byName.drum.quaternion.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, drumTurn);
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  rig.root.updateMatrixWorld(true);
  const desc = { base: 'armedbuilding', class: 'test_gunturret', matrix: new THREE.Matrix4().toArray(),
    settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'hp_fire', MaxTurnSpeed: 2, MaxHealth: 300, Pilot9Pose: 'minigun_9pose', ...settings }, weapon: null };
  const bodies = {};
  const turret = new Turret(desc, { built: rig }, bodies, null);
  const tub = box(0, 0.5, -0.5, 1.4, 1, 1.4);
  bodies.built = collision.addBody(tub.positions, tub.indices, tub.polygons, turret);
  turret.show(turret.state);
  const units = new Units();
  const unboarded = [];
  const crews = new TurretSeats({ live: { turrets: [turret] }, collision, units, onUnboard: (unit, object, seat, post, reason) => unboarded.push({ unit, reason }) });
  return { collision, rig, turret, units, crews, unboarded };
}

// Rounds through the shared Ordnance, struck units and bodies recorded.
function rounds(world) {
  const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: world.collision, effects: { play() {} }, lights: { add() {}, flash() {} }, texture: () => null });
  const struck = { units: [], bodies: [] };
  ordnance.units = world.units;
  ordnance.onHitUnit = (unit, def, point, owner, part) => { struck.units.push({ unit, part }); unit.combat.damage((def.Damage || 0) * (part === 'head' ? 3 : 1), 0, point, owner); };
  ordnance.onHit = (body) => struck.bodies.push(body);
  return { ordnance, struck };
}
const rifle = { base: 'bolt', Velocity: 300, LifeSpan: 2, Damage: 50 };
const fly = (ordnance) => { for (let i = 0; i < 120 && (ordnance.bolts.length || ordnance.rounds.length); i++) ordnance.update(1 / 60, false); };
const enemy = { id: 1, generation: 1, team: 2, alive: true };

function man(world, unit = soldierUnit(1)) {
  world.units.add(unit);
  assert.ok(world.crews.man(unit, world.turret), 'mans it');
  return unit;
}
const seatPoint = (world) => world.rig.byName.seat.getWorldPosition(new THREE.Vector3());
// A point given in the gunner's frame (x left, y up, z ahead) in the world:
// at the seat, turned with the drum.
const fromSeat = (world, x, y, z) => new THREE.Vector3(x, y, z).applyAxisAngle(THREE.Object3D.DEFAULT_UP, world.turret.facing + world.turret.yaw).add(seatPoint(world));

test("a network human takes a bot-manned turret over; another bot cannot", () => {
  const world = emplacement();
  const bot = man(world);
  const otherBot = soldierUnit(1); world.units.add(otherBot);
  assert.equal(world.crews.man(otherBot, world.turret), false, "a bot cannot take a manned turret");
  const human = soldierUnit(1); human.networkHuman = true; world.units.add(human);
  assert.ok(world.crews.man(human, world.turret), "the human mans it");
  assert.equal(bot.seat, null, "the bot was set down");
  assert.equal(world.turret.gunner, human.soldier);
  assert.equal(world.unboarded.at(-1)?.unit, bot, "the bot's exit was reported");
  const secondHuman = soldierUnit(1); secondHuman.networkHuman = true; world.units.add(secondHuman);
  assert.equal(world.crews.man(secondHuman, world.turret), false, "a person never takes another person's turret");
});

test("a turret's shown gunner is struck where he is drawn, turning and tipping with the gun; sight tests leave him out", () => {
  const world = emplacement();
  const unit = man(world);
  assert.equal(unit.seat.exposed, true);
  assert.equal(unit.seat.blastable, true);
  assert.equal(strikeable(unit), true);
  assert.equal(unit.actor.root.visible, true);
  // The head's middle: the head bone 1.67 m above his feet with the clip's
  // root 0.2 m forward, (0.05, 0.04, 0) along the bone.
  const headAt = () => unit.hitShapes().find((s) => s.part === 'head').center;
  assert.equal(unit.hitShapes().length, 6);
  assert.ok(headAt().distanceTo(fromSeat(world, 0.05, 1.71, 0.2)) < 1e-5, 'over the seat');
  // Drawn and struck in one pose: the turret's sample, the 9-pose.
  assert.equal(unit.calls.gunner.at(-1).clip, 'human_minigun_9pose');
  // The drum a quarter turn left: the seat and the gunner go round with it.
  world.turret.yaw = Math.PI / 2;
  world.turret.present();
  world.crews.pose(unit.seat, 0);
  assert.ok(headAt().distanceTo(fromSeat(world, 0.05, 1.71, 0.2)) < 1e-5, 'turned with the drum');
  near(seatPoint(world).x, -0.5, 1e-9, 'the seat went round');
  // The gun raised all the way: the 9-pose's top frame, 0.1 m higher here.
  world.turret.pitch = Math.PI / 4;
  world.crews.pose(unit.seat, 0);
  assert.ok(headAt().distanceTo(fromSeat(world, 0.05, 1.81, 0.2)) < 1e-5, 'up with the gun');
  near(unit.calls.gunner.at(-1).pitch, -1, 1e-9, 'drawn in the same frame');
  // Rays: a round's meets him; a sight test passes him by, as anyone seated.
  const head = headAt().clone();
  const from = head.clone().add(new THREE.Vector3(10, 0, 0)), dir = new THREE.Vector3(-1, 0, 0);
  same(world.units.raycast(from, dir, 20, null, makeUnitHit(), null, true)?.unit, unit);
  assert.equal(world.units.raycast(from, dir, 20, null, makeUnitHit()), null);
});

test("a drum bound turned (the barge turret's) still points its gun along the facing, its gunner behind it facing the same way", () => {
  const world = emplacement({}, { drumTurn: Math.PI });
  const muzzle = new THREE.Vector3(), direction = new THREE.Vector3();
  world.turret.present();
  world.turret.muzzle(muzzle, direction);
  assert.ok(direction.distanceTo(new THREE.Vector3(0, 0, 1)) < 1e-6, 'the gun along the facing, as the view is');
  near(seatPoint(world).z, -0.5, 1e-6, 'the seat behind the gun');
  near(world.turret.seatYaw(), 0, 1e-6, 'facing along the gun');
  const unit = man(world);
  near(unit.soldier.aimYaw, 0, 1e-6, 'the body faces the gun');
  assert.ok(unit.calls.gunner.at(-1).seat?.angleTo(new THREE.Quaternion()) < 1e-6, 'drawn turned as the seat node is');
  assert.ok(unit.hitShapes()[0].center.distanceTo(fromSeat(world, 0.05, 1.71, 0.2)) < 1e-5, 'struck there');
  // Turned, he goes round with the drum, still facing along the gun.
  world.turret.yaw = Math.PI / 2;
  world.turret.present();
  world.crews.pose(unit.seat, 0);
  world.turret.muzzle(muzzle, direction);
  near(unit.soldier.aimYaw, Math.PI / 2, 1e-6, 'turned with the gun');
  near(Math.atan2(direction.x, direction.z), Math.PI / 2, 1e-6, 'the gun turned the same way');
  assert.ok(unit.hitShapes()[0].center.distanceTo(fromSeat(world, 0.05, 1.71, 0.2)) < 1e-5, 'struck where he is drawn');
});

test("a gunner's arms are struck too, from the shoulder to the middle of the hand", () => {
  const world = emplacement();
  const unit = man(world, soldierUnit(1, { arms: true }));
  const shapes = unit.hitShapes();
  assert.equal(shapes.length, 10, 'head, torso, legs and arms');
  // The left forearm: elbow 1.25 m up, 0.2 m out to his left, to the
  // hand's middle 0.1 m below the wrist and 0.02 m in; the clip's root
  // carries him 0.2 m ahead.
  const forearm = shapes[7];
  assert.equal(forearm.type, 'capsule');
  assert.equal(forearm.part, 'body');
  const elbow = fromSeat(world, 0.2, 1.25, 0.2), hand = fromSeat(world, 0.18, 0.9, 0.2);
  assert.ok(forearm.center.distanceTo(elbow.clone().add(hand).multiplyScalar(0.5)) < 1e-5, 'between elbow and hand');
  near(forearm.half.length(), elbow.distanceTo(hand) / 2, 1e-5, 'elbow to hand');
  // A round past his torso's capsule, through the hand, meets him.
  const from = hand.clone().add(new THREE.Vector3(10, 0, 0));
  const met = world.units.raycast(from, new THREE.Vector3(-1, 0, 0), 20, null, makeUnitHit(), null, true);
  same(met?.unit, unit);
  near(met.point.x, hand.x + 0.07, 1e-3, 'at the glove');
});

test("a page draws its own player's gunner at the seat in the gunner's pose, at the aim it previews, and lets him go with the turret", () => {
  // The drum turns but doesn't tip, as the gun turret's (PitchTurnFactor 0).
  const world = emplacement({ PitchTurnFactor: 0 }, { drumTurn: Math.PI });
  const player = soldierUnit(1);
  const gunner = new Gunner({ camera: new THREE.PerspectiveCamera(), collision: world.collision, ordnance: null, combat: player.combat, animator: player.animator });
  // A page showing a worker's or a server's battle: the seat packet sets
  // the turret; update() never runs there.
  gunner.turret = world.turret;
  world.turret.yaw = 0.3; world.turret.pitch = 0.1; world.turret.yawVel = 0;
  gunner.previewYaw = 0.2; gunner.previewPitch = Math.PI / 4 - 0.1;
  gunner.previewState.yawVel = 1;
  const state = { aimYaw: 0, aimPitch: 0 };
  const root = player.actor.root;
  assert.equal(gunner.poseBody(state, root), true);
  // The drum is drawn at the previewed yaw; he stands at its seat, turned
  // with it, in the 9-pose at the previewed pitch (45 degrees up: -1) and
  // turn speed (1 rad/s of MaxTurnSpeed 2).
  near(state.aimYaw, 0.5, 1e-6, 'facing the previewed drum');
  assert.ok(root.position.distanceTo(seatPoint(world)) < 1e-9, 'at the seat as drawn');
  near(root.position.x, -0.5 * Math.sin(0.5), 1e-6, 'the seat went round with the preview');
  const pose = player.calls.gunner.at(-1);
  assert.equal(pose.clip, 'human_minigun_9pose');
  near(pose.pitch, -1, 1e-9, 'the previewed pitch');
  near(pose.turn, 0.5, 1e-9, 'the previewed turn speed');
  assert.ok(pose.seat.angleTo(new THREE.Quaternion().setFromAxisAngle(THREE.Object3D.DEFAULT_UP, 0.5)) < 1e-6, 'turned as the seat node');
  assert.equal(root.visible, true);
  // The seat packet lets the turret go: out of the gunner's pose.
  gunner.turret = null;
  assert.equal(player.calls.gunner.at(-1).clip, null);
  assert.equal(gunner.poseBody(state, root), false);
  // A turret that hides its gunner hides him on the page too.
  const hidden = emplacement({ Pilot9Pose: undefined });
  gunner.turret = hidden.turret;
  gunner.poseBody(state, root);
  assert.equal(root.visible, false);
});

test("a page's copy of a turret turns its gunner as the world does: the drum's turn speed from its records", () => {
  const world = emplacement();
  const turret = world.turret;
  const record = (tick, yaw) => ({ tick, yaw, pitch: 0.1, gunYaw: 0, gunPitch: 0.1 });
  turret.aimFrom(record(100, 0));
  assert.equal(turret.yawVel, 0, 'one record: no speed yet');
  // Two ticks later the yaw is 0.04 rad on: 1.2 rad/s, as the world's steer
  // turned it (yaw += yawVel x dt), across the wrap too.
  turret.aimFrom(record(102, 0.04));
  near(turret.yawVel, 1.2, 1e-9, 'the speed');
  near(turret.pitch, 0.1, 1e-9);
  turret.aimFrom(record(104, Math.PI - 0.01));
  turret.aimFrom(record(106, -Math.PI + 0.03));
  near(turret.yawVel, 1.2, 1e-9, 'across the wrap');
  // The world drew and struck its gunner turning: the page draws him so.
  const sample = turret.riderSample(new Map([['human_minigun_9pose', { name: 'human_minigun_9pose' }]]), {});
  near(sample.turn, 0.6, 1e-9, 'turn frames by 1.2 of MaxTurnSpeed 2');
  // No record since: its aim stopped changing, the drum stands still.
  turret.settleTurn(110);
  near(turret.yawVel, 1.2, 1e-9, 'still turning a few ticks on');
  turret.settleTurn(115);
  assert.equal(turret.yawVel, 0, 'standing still');
  // After a long gap, the next record's change is no speed.
  turret.aimFrom(record(200, 1));
  assert.equal(turret.yawVel, 0);
});

test('rifle bolts kill a gunner over the rim, not through the turret; its own gun never strikes him', () => {
  const world = emplacement();
  const { ordnance, struck } = rounds(world);
  const unit = man(world);
  // His shins are inside the tub: the turret takes the round.
  const shin = unit.hitShapes()[3].center.clone();
  ordnance.fire(rifle, shin.clone().add(new THREE.Vector3(0, 0, -15)), new THREE.Vector3(0, 0, 1), enemy);
  fly(ordnance);
  assert.equal(struck.units.length, 0);
  same(struck.bodies[0], world.turret.bodies.built, 'the turret takes it');
  // A round of the turret's own (it passes through the turret's body, as
  // every mounted round does) leaves him alone.
  const head = unit.hitShapes()[0].center.clone();
  ordnance.fire(rifle, head.clone().add(new THREE.Vector3(0, 0, -3)), new THREE.Vector3(0, 0, 1), unit, null, world.turret.bodies.built);
  fly(ordnance);
  assert.equal(struck.units.length, 0);
  // From behind, over the rim: a headshot (three times the damage), then
  // one more. 300 health.
  let shots = 0;
  while (unit.alive && shots < 10) {
    const at = unit.hitShapes()[0].center.clone();
    ordnance.fire(rifle, at.clone().add(new THREE.Vector3(0, 0.5, -15)), new THREE.Vector3(0, -0.5, 15).normalize(), enemy);
    fly(ordnance);
    shots++;
  }
  assert.equal(shots, 2);
  assert.ok(struck.units.every((s) => s.unit === unit && s.part === 'head'));
  same(unit.combat.lastAttacker, enemy, 'credited to the shooter');
});

test('killed at the gun, a bot or a network human leaves it at once, his body falling from where he stood', () => {
  for (const networkHuman of [false, true]) {
    const world = emplacement();
    const unit = soldierUnit(1);
    unit.networkHuman = networkHuman;
    man(world, unit);
    world.turret.yaw = Math.PI / 2;
    world.turret.present();
    world.crews.pose(unit.seat, 0);
    const lease = unit.seat;
    unit.combat.damage(Infinity, 0, null, enemy);
    assert.equal(unit.seat, null);
    assert.equal(lease.released, true);
    assert.equal(world.turret.lease, null);
    assert.equal(world.turret.gunner, null);
    assert.equal(world.crews.leases.length, 0);
    assert.deepEqual(world.unboarded.map((u) => u.reason), ['killed']);
    // Under his pelvis (the clip's root 0.2 m ahead), his soles where his
    // feet were drawn (here the seat's height), at rest; never set down
    // beside the turret, his death clip kept.
    assert.ok(unit.soldier.position.distanceTo(fromSeat(world, 0, 0, 0.2)) < 1e-5, 'falls from where he stood');
    assert.equal(unit.soldier.velocity.length(), 0);
    assert.equal(unit.soldier.grounded, false);
    assert.equal(unit.calls.spawns, 0);
    assert.equal(unit.calls.reset, 0);
    assert.equal(unit.calls.die, 1);
    assert.equal(unit.calls.gunner.at(-1).clip, null, 'out of the gunner pose');
    assert.equal(strikeable(unit), true, 'a corpse still stops rounds');
    // The turret waits for the next gunner.
    assert.ok(world.crews.man(soldierUnit(1), world.turret));
  }
});

test('a turret destroyed under its gunner kills him there, credited to its destroyer', () => {
  const world = emplacement();
  const unit = man(world);
  const killer = { id: 2, generation: 1, team: 2, alive: true };
  world.turret.damage(1000, killer);
  assert.equal(unit.alive, false);
  same(unit.combat.lastAttacker, killer);
  assert.equal(unit.seat, null);
  assert.ok(unit.soldier.position.distanceTo(fromSeat(world, 0, 0, 0.2)) < 1e-5, 'falls from where he stood');
});

test('the player killed at the gun leaves it the same way, his death clip kept', () => {
  const world = emplacement();
  const player = soldierUnit(1);
  player.human = true;
  world.units.add(player);
  const gunner = new Gunner({ camera: new THREE.PerspectiveCamera(), collision: world.collision, ordnance: null, combat: player.combat, animator: player.animator });
  gunner.enter(world.turret, player.soldier);
  same(player.seat, world.turret.lease);
  same(world.turret.humanGunner, gunner);
  gunner.update(1 / 60, player.soldier, { take: () => false, held: () => false });
  assert.equal(player.seat.exposed, true);
  assert.equal(player.hitShapes().length, 6, "his body is struck as a bot's is");
  assert.ok(player.hitShapes()[0].center.distanceTo(fromSeat(world, 0.05, 1.71, 0.2)) < 1e-5);
  assert.equal(player.calls.gunner.at(-1).clip, 'human_minigun_9pose');
  const { ordnance, struck } = rounds(world);
  const head = player.hitShapes()[0].center.clone();
  ordnance.fire(rifle, head.clone().add(new THREE.Vector3(0, 0.5, -15)), new THREE.Vector3(0, -0.5, 15).normalize(), enemy);
  fly(ordnance);
  same(struck.units[0]?.unit, player);
  assert.equal(player.combat.health, 150);
  player.combat.damage(Infinity, 0, null, enemy);
  assert.equal(gunner.active, false);
  assert.equal(player.seat, null);
  assert.equal(world.turret.lease, null);
  assert.equal(world.turret.gunner, null);
  assert.equal(world.turret.humanGunner, null);
  assert.equal(player.combat.mounted, false);
  assert.equal(player.calls.spawns, 0, 'not set down beside it');
  assert.equal(player.calls.reset, 0, 'his death clip is not cut short');
  assert.ok(player.soldier.position.distanceTo(fromSeat(world, 0, 0, 0.2)) < 1e-5, 'falls from where he stood');
  assert.ok(world.crews.man(soldierUnit(1), world.turret), 'the turret waits for the next gunner');
});

test('a turret that names no pilot animation hides its gunner: no rounds, no blasts, seen again once out', () => {
  const world = emplacement({ Pilot9Pose: undefined });
  same(world.turret.rider, null);
  const unit = man(world);
  assert.equal(unit.seat.exposed, false);
  assert.equal(unit.seat.blastable, false);
  assert.equal(strikeable(unit), false);
  assert.equal(unit.actor.root.visible, false);
  const c = unit.sphere.center;
  assert.equal(world.units.raycast(new THREE.Vector3(c.x + 10, c.y, c.z), new THREE.Vector3(-1, 0, 0), 20, null, makeUnitHit(), null, true), null);
  world.crews.leave(unit, { placement: false });
  assert.equal(unit.actor.root.visible, true);
  // The player too.
  const player = soldierUnit(1);
  player.human = true;
  const gunner = new Gunner({ camera: new THREE.PerspectiveCamera(), collision: world.collision, ordnance: null, combat: player.combat, animator: player.animator });
  gunner.enter(world.turret, player.soldier);
  gunner.update(1 / 60, player.soldier, { take: () => false, held: () => false });
  assert.equal(player.seat.exposed, false);
  assert.equal(player.actor.root.visible, false);
  gunner.exit(player.soldier);
  assert.equal(player.actor.root.visible, true);
  assert.equal(player.calls.spawns, 1, 'set down beside it');
  // A shootable class can still keep its gunner out of reach (hash 2b60e217 0).
  assert.equal(emplacement({ '2b60e217': '0' }).turret.exposedSeat(), false);
});

test("blasts hurt only a turret's shown gunner among the seated, and move him only once they have killed him", () => {
  const world = emplacement();
  const unit = man(world);
  assert.equal(unit.seat.blastable, true);
  assert.equal(new SeatLease({ vehicle: { exposedSeat: () => true }, index: 1 }).blastable, false, "a vehicle's rider: the hull takes it");
  unit.seat.released = true;
  assert.equal(unit.seat.blastable, false);
  // Combat.explode on a soldier at a gun: a thermal detonator's numbers.
  const flat = box(0, -1, 0, 200, 2, 200);
  const floor = new CollisionWorld(flat.positions, flat.indices, flat.polygons);
  const blasted = (blast, mounted) => {
    const soldier = new Soldier(floor, {});
    soldier.spawn(0, 0, 0, 0);
    const combat = Object.create(PlayerCombat.prototype);
    Object.assign(combat, {
      soldier, def: { base: 'soldier' }, chunks: 0, random: () => 1, scale: 'PersonScale', health: 300, maxHealth: 300, dead: false, deathTime: 0, zoomed: false, mounted,
      collision: floor, worldPoint: new THREE.Vector3(), from: new THREE.Vector3(), shake: { amount: 0, left: 0, length: 0 },
      hud: null, player: false, blowApart: false, lastAttacker: null, onDamaged() {}, onBlowApart() {}, updateHeld() {},
      // Killed at the gun, he has left it (leaveKilled) before the push.
      onDeath() { this.mounted = false; }, animator: { die() {} },
    });
    combat.explode(blast, new THREE.Vector3(2, 1, 0), enemy);
    return { health: combat.health, speed: soldier.velocity.length(), air: soldier.air };
  };
  const shove = { Damage: 100, DamageRadiusInner: 4.5, DamageRadiusOuter: 6.5, Push: 10, PushRadiusInner: 4.5, PushRadiusOuter: 4.5 };
  const grenade = { ...shove, Damage: 320 };
  const atGun = blasted(shove, true);
  assert.equal(atGun.health, 200, 'hurt');
  assert.equal(atGun.speed, 0, 'not moved while he mans it');
  assert.ok(blasted(shove, false).speed > 0, 'on foot the same blast throws him');
  const killed = blasted(grenade, true);
  assert.equal(killed.health, 0);
  assert.ok(killed.speed > 0, 'killed, his body is thrown from the gun');
  assert.equal(killed.air, 'thrown');
});

// The imported Hoth turrets (BFE_CONTENT_DIR, or public/), in the headless
// world the worker and the server run: the dish turret shows its gunner,
// and rifle bolts kill him; the big turret hides its own.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
test('imported: the Hoth dish turret\'s gunner dies to rifle bolts and leaves it; the Hoth turret\'s is out of reach', { skip: !fs.existsSync(path.join(CONTENT, 'maps/hot1/props.json')) && 'no hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=hot1&side=all&ai=off');
  await game.ready;
  const T = game.THREE;
  const step = (n) => { for (let i = 0; i < n; i++) { const tick = game.worldClock.tick + 1; game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] }); } };
  let bolt = null, className = null;
  for (const s of Object.values(game.data.sides)) for (const [n, d] of Object.entries(s.units || {})) {
    if (!className && d.base === 'soldier' && !d.hero && !Number(d.NoEnterVehicles ?? 0)) className = n;
    for (const w of d.weapons || []) if (!bolt && w.ordnance?.base === 'bolt' && !w.ordnance.ExplosionImpact && w.ordnance.Damage === 50) bolt = w.ordnance;
  }
  await game.factory.prepare(className);
  const shooter = { id: 9999, generation: 1, team: 3, alive: true };
  const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3() };
  for (const [cls, shown] of [['hoth_bldg_dish_turret', true], ['hoth_bldg_hoth_turret', false]]) {
    const turret = game.live.turrets.find((t) => t.desc.class === cls);
    if (!turret.alive) turret.setHealth(turret.maxHealth);
    const unit = game.factory.acquire(className);
    unit.team = unit.combat.team = 1;
    unit.spawn(turret.position.x + 2, turret.position.y + 1, turret.position.z, 0);
    assert.ok(game.turretSeats.man(unit, turret), cls);
    step(3);
    assert.equal(unit.seat.exposed, shown, cls);
    assert.equal(unit.actor.root.visible, shown, `${cls} drawn`);
    if (!shown) { game.turretSeats.leave(unit, { placement: false }); unit.remove(); continue; }
    // From a direction where nothing static stands between: bolts at his head.
    let shots = 0;
    while (unit.alive && shots < 10) {
      const head = unit.hitShapes()[0].center.clone();
      let from = null;
      for (let k = 0; k < 24 && !from; k++) {
        const a = turret.facing + turret.yaw + Math.PI + k * Math.PI / 12;
        const start = head.clone().add(new T.Vector3(Math.sin(a) * 20, 3, Math.cos(a) * 20));
        const dir = head.clone().sub(start).normalize();
        const met = game.units.raycast(start, dir, 30, null, makeUnitHit(), null, true);
        const wall = game.collision.cast(start, dir, 30, hit) ? hit.distance : Infinity;
        if (met?.unit === unit && met.distance < wall) from = start;
      }
      assert.ok(from, 'a line in to him');
      game.ordnance.fire(bolt, from, head.clone().sub(from).normalize(), shooter);
      shots++;
      step(30);
    }
    assert.equal(shots, 2, 'two headshots for 300 health');
    assert.equal(unit.seat, null);
    assert.equal(turret.lease, null);
    assert.equal(turret.alive, true);
    unit.remove();
  }
});

// Every retail class that shows its gunner, in the headless world: a bot at
// the gun is drawn (his animator run as a page runs it) where he is struck.
// His body takes the seat node's turn (the barge turret's gunner faces
// along its gun), the Hoth dish turret's gunner plays his own clip where
// the bank has it, and his drawn head, hands, elbows, pelvis, knees and
// feet lie inside his hit shapes at any pitch and turn.
const SHOWN = { hot1: ['hoth_bldg_dish_turret'], tat2: ['com_weap_gunturret', 'tat_bldg_barge_turret'], geo1: ['geo_bldg_geoturret'], yav1: ['yav_bldg_tower_turret'] };
for (const [map, classes] of Object.entries(SHOWN)) {
  test(`imported: ${map}'s shown gunners are drawn where they are struck`, { skip: !fs.existsSync(path.join(CONTENT, `maps/${map}/props.json`)) && `no ${map} extract` }, async () => {
    process.env.BFE_CONTENT_DIR = CONTENT;
    const { headlessGame } = await import('../tools/headless-world.mjs');
    const game = await headlessGame(`map=${map}&side=all&ai=off`);
    await game.ready;
    const T = game.THREE;
    let className = null;
    for (const s of Object.values(game.data.sides)) for (const [n, d] of Object.entries(s.units || {})) if (!className && d.base === 'soldier' && !d.hero && !Number(d.NoEnterVehicles ?? 0)) className = n;
    await game.factory.prepare(className);
    const away = (p, shapes) => Math.min(...shapes.map((s) => {
      if (s.type === 'sphere') return p.distanceTo(s.center) - s.radius;
      const l2 = s.half.lengthSq(), k = l2 > 1e-12 ? Math.max(-1, Math.min(1, p.clone().sub(s.center).dot(s.half) / l2)) : 0;
      return p.distanceTo(s.center.clone().addScaledVector(s.half, k)) - s.radius;
    }));
    for (const cls of classes) {
      // The most tilted of the class (one Hoth dish turret stands 9 degrees
      // off level): he leans with it.
      const lean = (t) => { t.rig.root.updateMatrixWorld(true); return new T.Vector3(0, 1, 0).transformDirection((t.seat || t.rig.root).matrixWorld).angleTo(new T.Vector3(0, 1, 0)); };
      const turret = game.live.turrets.filter((t) => t.desc.class === cls).sort((a, b) => lean(b) - lean(a))[0];
      if (!turret.alive) turret.setHealth(turret.maxHealth);
      const unit = game.factory.acquire(className);
      unit.team = unit.combat.team = 1;
      unit.spawn(turret.position.x + 2, turret.position.y + 1, turret.position.z, 0);
      assert.ok(game.turretSeats.man(unit, turret), cls);
      if (cls === 'hoth_bldg_dish_turret' && unit.animator.clips.has('human_dishcannon_9pose')) assert.equal(unit.animator.gunning.clip.name, 'human_dishcannon_9pose');
      for (const [yaw, pitch, yawVel] of [[0, 0, 0], [1.2, turret.pitchLimits[1], turret.turnRate], [-2.5, turret.pitchLimits[0], -turret.turnRate / 2]]) {
        turret.yaw = yaw; turret.pitch = pitch; turret.yawVel = yawVel; turret.present();
        game.turretSeats.pose(unit.seat, 0);
        unit.animator.update(1 / 60, unit.soldier);
        unit.actor.root.updateMatrixWorld(true);
        const seat = (turret.seat || turret.rig.root);
        assert.ok(unit.actor.root.quaternion.angleTo(seat.getWorldQuaternion(new T.Quaternion())) < 1e-4, `${cls}: turned as the seat`);
        // Facing along the gun.
        const gun = new T.Vector3(0, 0, 1).transformDirection(turret.drum.matrixWorld);
        const body = new T.Vector3(0, 0, 1).applyQuaternion(unit.actor.root.quaternion);
        assert.ok(gun.angleTo(body) < 0.01, `${cls}: facing the gun`);
        const shapes = unit.hitShapes();
        assert.equal(shapes.length, 10, `${cls}: with arms`);
        for (const name of ['bone_head', 'bone_pelvis', 'bone_l_hand', 'bone_r_hand', 'bone_l_forearm', 'bone_r_forearm', 'bone_l_calf', 'bone_r_calf', 'bone_l_foot', 'bone_r_foot']) {
          const at = unit.actor.byName[name].getWorldPosition(new T.Vector3());
          assert.ok(away(at, shapes) < 0.01, `${cls} ${name} drawn ${away(at, shapes).toFixed(3)} m outside its shapes at yaw ${yaw}`);
        }
      }
      game.turretSeats.leave(unit, { placement: false });
      unit.animator.update(1 / 60, unit.soldier);
      assert.ok(Math.abs(unit.actor.root.rotation.x) + Math.abs(unit.actor.root.rotation.z) < 1e-9, `${cls}: upright again off the gun`);
      unit.remove();
    }
  });
}
