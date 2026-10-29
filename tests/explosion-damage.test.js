import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Soldier } from '../src/soldier.js';
import { PlayerCombat } from '../src/combat.js';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { LiveObjects } from '../src/props.js';
import { Items } from '../src/items.js';
import { RemoteDroids } from '../src/remote.js';
import { blastDistance, blastDamage, blastBlocked, healthScale } from '../src/ordnance.js';

// One area damage rule for everything a blast reaches (src/ordnance.js,
// "What an explosion does"), as the 2004 game's ExplosionClass::Explode
// measures it: the distance to the target's bounding sphere's centre less
// half its radius (a soldier's sphere by stance), line of sight to the
// target's sight point through terrain and buildings only, and the target's
// HealthType scale. The numbers are the retail rocket launcher explosion's.
const ROCKET = { Damage: 320, DamageRadiusInner: 0.75, DamageRadiusOuter: 2, Push: 10, PushRadiusInner: 0.75, PushRadiusOuter: 0.75,
  PersonScale: 1, DroidScale: 1, VehicleScale: 0.25, BuildingScale: 0.25, AnimalScale: 1 };
const near = (a, b, tolerance = 1e-6) => assert.ok(Math.abs(a - b) <= tolerance, `${a} is not within ${tolerance} of ${b}`);

// A flat floor, and with `wall` an upright wall `height` high across x = wall.
function world(wall = null, height = 5) {
  const positions = [-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100];
  const indices = [0, 2, 1, 0, 3, 2];
  const polygons = [0, 1];
  if (wall !== null) {
    positions.push(wall, 0, -50, wall, height, -50, wall, height, 50, wall, 0, 50);
    indices.push(4, 5, 6, 4, 6, 7);
    polygons.push(2, 3);
  }
  return new CollisionWorld(new Float32Array(positions), new Uint32Array(indices), new Uint32Array(polygons));
}
function soldier(w = world(), posture = 'stand') {
  const s = new Soldier(w, {});
  s.spawn(0, 0, 0, 0);
  s.posture = posture;
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, {
    soldier: s, def: { base: 'soldier' }, chunks: 0, random: () => 1, scale: 'PersonScale', health: 300, maxHealth: 300, dead: false, deathTime: 0, zoomed: false,
    collision: w, hit: { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() }, worldPoint: new THREE.Vector3(), from: new THREE.Vector3(),
    shake: { amount: 0, left: 0, length: 0 }, hud: null, player: false, blowApart: false, lastAttacker: null,
    onDamaged() {}, onDeath() {}, onBlowApart() {}, updateHeld() {}, animator: { die() {} },
  });
  return combat;
}

test('a rocket bursting 2 m from a soldier does the original game\'s damage, by stance', () => {
  // On the ground 2 m from his feet, 0.2 m up. Standing: the sphere's
  // centre 1.0 m up, radius 1.4626; 2.154 m away less 0.731 is 1.423 m,
  // 0.462 of the way from 2 m in to 0.75: 148 damage (85 before, with a
  // centre 0.9 m up and 0.45 off).
  const expected = { stand: [1, 1.4626], crouch: [0.75, 1.1023], prone: [0.35, 1.622] };
  for (const [posture, [middle, radius]] of Object.entries(expected)) {
    const combat = soldier(world(), posture);
    combat.explode(ROCKET, new THREE.Vector3(2, 0.2, 0), null, 1);
    const d = Math.hypot(2, middle - 0.2) - radius / 2;
    near(300 - combat.health, 320 * (2 - d) / 1.25, 1e-6);
  }
  const standing = soldier();
  standing.explode(ROCKET, new THREE.Vector3(2, 0.2, 0), null, 1);
  near(300 - standing.health, 147.8, 0.1);
});

test('terrain shields a soldier only below his sight point; a vehicle never shields', () => {
  // A wall 0.7 m high halfway: the line to his sight point, 1.4 m up, clears
  // it (the old line to his centre, 0.9 m up, did not); 1.2 m high, it
  // shields him.
  const blast = { Damage: 320, DamageRadiusInner: 4.5, DamageRadiusOuter: 6.5 };
  const low = soldier(world(1.5, 0.7));
  low.explode(blast, new THREE.Vector3(3, 0.1, 0), null, 1);
  assert.equal(low.health, 0);
  const high = soldier(world(1.5, 1.2));
  high.explode(blast, new THREE.Vector3(3, 0.1, 0), null, 1);
  assert.equal(high.health, 300);
  // A hull (a moving body) between them shields nothing.
  const w = world();
  const box = new THREE.BoxGeometry(1, 4, 4).translate(1.5, 2, 0);
  const shape = w.createBodyShape(box.attributes.position.array, box.index.array, new Uint32Array(box.index.count / 3));
  w.addMovingBody(shape, new THREE.Matrix4());
  assert.equal(blastBlocked(w, new THREE.Vector3(3, 0.1, 0), new THREE.Vector3(0, 1.4, 0)), false);
  const behind = soldier(w);
  behind.explode(blast, new THREE.Vector3(3, 0.1, 0), null, 1);
  assert.equal(behind.health, 0);
});

test('the shared helpers: distance less half the radius, damage by health type', () => {
  near(blastDistance(new THREE.Vector3(5, 0, 0), new THREE.Vector3(0, 0, 0), 4), 3);
  near(blastDamage(ROCKET, 1.375, 'vehicle'), 320 * 0.5 * 0.25);
  near(blastDamage(ROCKET, 0, 'droid'), 320);
  assert.equal(blastDamage(ROCKET, 2, 'person'), 0);
  assert.equal(healthScale({ VehicleScale: 0.5 }, 'Vehicle'), 0.5);
  assert.equal(healthScale({}, undefined), 1, 'unset scales and types count as 1 and person');
});

test('a hull is measured from its model\'s bounding sphere, takes friendly fire\'s factor, and shields nothing of itself', () => {
  const w = world(), hull = new THREE.BoxGeometry(2, 2, 4).translate(0, 1, 0);
  const d = { model: 'tank', label: 'Tank', settings: { MaxHealth: 1000 }, seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
  const data = { models: { tank: { bones: [], parts: [], sphere: [0, 1, 0, 3] } }, collision: { tank: { positions: hull.attributes.position.array, indices: hull.index.array, polygons: new Uint32Array(hull.index.count / 3) } },
    classes: { tank: d }, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: 'tank', 2: 'tank' } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision: w, texture: () => null,
    camera: new THREE.PerspectiveCamera(), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() { return null; } } });
  const v = system.vehicles[0];
  // 4 m from the sphere's centre less 1.5: 2.5 m, halfway from 1 to 4.
  const blast = { Damage: 400, DamageRadiusInner: 1, DamageRadiusOuter: 4, VehicleScale: 0.5 };
  const center = new THREE.Vector3();
  v.blastSphere(center);
  system.blast(blast, center.clone().add(new THREE.Vector3(4, 0, 0)), null);
  near(1000 - v.health, 400 * 0.5 * 0.5);
  // Set off inside the hull itself: its own body is no shield.
  system.blast(blast, center.clone(), null);
  near(1000 - v.health, 100 + 200);
  // Friendly fire off: nothing.
  system.blast(blast, center.clone(), null, () => 0);
  near(1000 - v.health, 300);
  // A round that struck it: its own Damage by the hull's scale.
  system.struck(v.body, { Damage: 100, VehicleScale: 2 }, null);
  near(1000 - v.health, 500);
});

test('a building is measured from its model\'s bounding sphere and shielded by a wall', () => {
  const taken = [];
  const turret = { alive: true, state: 'built', bodies: { built: null }, blastSphere(out) { out.set(0, 1.65, 0.32); return 2.25; }, damage(amount) { taken.push(amount); } };
  const live = (w) => ({ buildings: [turret], collision: w, center: new THREE.Vector3() });
  // 2 m from the sphere's centre less 1.125: 0.875 m, inside 0.75..2 by 0.9.
  LiveObjects.prototype.blast.call(live(world()), ROCKET, new THREE.Vector3(2, 1.65, 0.32), null);
  near(taken[0], 320 * (2 - 0.875) / 1.25 * 0.25);
  LiveObjects.prototype.blast.call(live(world(1, 5)), ROCKET, new THREE.Vector3(2, 1.65, 0.32), null);
  assert.equal(taken.length, 1, 'the wall shields it');
});

test('a mine is reached only once it has landed; a recon droid takes DroidScale and the push', () => {
  const items = new Items({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, model: () => null, explode() {} });
  const mine = items.spawn({ base: 'mine', MaxHealth: 200 }, new THREE.Vector3(1, 0.1, 0), new THREE.Vector3(), 0);
  items.blast(ROCKET, new THREE.Vector3(0, 0.1, 0));
  assert.equal(mine.health, 200, 'in flight');
  mine.landed = true;
  items.blast(ROCKET, new THREE.Vector3(0, 0.1, 0));
  assert.ok(mine.health < 200);
  const remotes = new RemoteDroids({ scene: new THREE.Scene(), camera: null, collision: world(), lights: null, model: () => null, explode() {}, fire() {} });
  const droid = { dead: false, health: 1000, position: new THREE.Vector3(0, 1, 0), velocity: new THREE.Vector3() };
  remotes.droids.push(droid);
  remotes.blast({ ...ROCKET, DroidScale: 0.5 }, new THREE.Vector3(1, 1.155, 0));
  // 1 m from its 0.271 m collision sphere's centre: 0.865 m away.
  near(1000 - droid.health, 320 * (2 - (1 - 0.271 / 2)) / 1.25 * 0.5);
  assert.equal(droid.velocity.length(), 0, 'outside the 0.75 m push radius');
  remotes.blast(ROCKET, new THREE.Vector3(0.5, 1.155, 0));
  near(droid.velocity.x, -10);
  near(droid.velocity.y, 6);
});
