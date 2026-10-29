import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';

function boxes(items) {
  const positions = [], indices = [];
  for (const [x, y, z, w, h, d] of items) {
    const geometry = new THREE.BoxGeometry(w, h, d).translate(x, y, z);
    const offset = positions.length / 3;
    positions.push(...geometry.attributes.position.array);
    indices.push(...Array.from(geometry.index.array, (i) => i + offset));
  }
  return new CollisionWorld(new Float32Array(positions), new Uint32Array(indices),
    Uint32Array.from({ length: indices.length / 3 }, (_, i) => i));
}
function walk(world, start, yaw, ticks = 70) {
  const soldier = new Soldier(world, {});
  soldier.spawn(...start, yaw);
  let highest = soldier.position.y;
  for (let i = 0; i < ticks; i++) {
    soldier.tick({ forward: 1, right: 0 }, 1 / 60);
    highest = Math.max(highest, soldier.position.y);
    assert(soldier.position.toArray().every(Number.isFinite), 'finite collision response');
  }
  return { soldier, highest };
}
const floor = [0, -0.5, 0, 30, 1, 30];
for (const height of [0.2, 0.4]) {
  const { soldier, highest } = walk(boxes([floor, [0, height / 2, 4, 5, height, 4]]), [0, 0, 0], 0);
  assert(soldier.position.z > 8, `cross ${height}m raised platform`);
  assert(highest >= height - 0.03, 'stand on the platform');
  assert(soldier.grounded && Math.abs(soldier.position.y) < 0.03, 'descend onto the floor');
}
{
  const { soldier } = walk(boxes([floor, [0, 0.4, 4, 5, 0.8, 4]]), [0, 0, 0], 0);
  assert(soldier.position.z < 1.5, 'do not climb tall walls');
}
{
  const { soldier } = walk(boxes([floor, [0, 0.2, 4, 5, 0.4, 4], [0, 2.5, 4, 5, 0.4, 8]]), [0, 0, 0], 0);
  assert(soldier.position.z < 1.5, 'head clearance prevents stepping into a ceiling');
}
{
  const world = boxes([floor, [0, 1.5, 3, 8, 3, 0.04]]);
  const soldier = new Soldier(world, {});
  soldier.spawn(0, 0, 0, 0);
  soldier.velocity.z = 150;
  soldier.position.z = 5;
  soldier.collide();
  assert(soldier.position.z < 2.5, 'fast movement cannot tunnel through a thin wall');
}
{
  const world = boxes([floor]);
  const body = world.addBody(new Float32Array([-2, 0, 2, 2, 0, 2, 0, 3, 2]), new Uint32Array([0, 1, 2]), new Uint32Array([0]));
  assert(world.overlapsSphere(new THREE.Vector3(0, 1, 2), 0.5), 'enabled live body blocks clearance');
  body.enabled = false;
  assert(!world.overlapsSphere(new THREE.Vector3(0, 1, 2), 0.5), 'disabled live body permits clearance');
}
function retailWorld(name) {
  const dir = new URL(`../public/maps/${name}/`, import.meta.url);
  if (!fs.existsSync(new URL('collision.bin', dir))) return null;
  const load = (file, Type) => {
    const bytes = fs.readFileSync(new URL(file, dir));
    return new Type(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  };
  return new CollisionWorld(load('collision.bin', Float32Array), load('collision_index.bin', Uint32Array),
    load('collision_poly.bin', Uint32Array), load('terrain_collision.bin', Float32Array), load('terrain_collision_index.bin', Uint32Array));
}
const theed = retailWorld('nab2');
if (theed) {
  // The screenshot's arcade is canal_home, placed at three rotations around
  // the rotunda. VehicleCollisionOnly boxes used to seal these openings.
  for (const yaw of [-2 * Math.PI / 3, Math.PI, 2 * Math.PI / 3]) {
    const sin = Math.sin(yaw), cos = Math.cos(yaw);
    for (const side of [-2, 0, 2]) for (const posture of ['stand', 'prone']) {
      const start = new THREE.Vector3(-168 + cos * side - sin * 71, -49, -96 - sin * side - cos * 71);
      const soldier = new Soldier(theed, {});
      soldier.spawn(...start.toArray(), yaw);
      soldier.posture = posture;
      for (let i = 0; i < (posture === 'stand' ? 250 : 700); i++) soldier.tick({ forward: 1, right: 0 }, 1 / 60);
      const progress = soldier.position.clone().sub(start).dot(new THREE.Vector3(sin, 0, cos));
      assert(progress > 25, `arcade passage and bridge stairs: yaw=${yaw}, side=${side}, ${posture}`);
      assert(soldier.position.y > -46, 'retain detailed bridge floor support');
    }
    for (const side of [-5, 5]) {
      const origin = new THREE.Vector3(-168 + cos * side - sin * 71, -47, -96 - sin * side - cos * 71);
      assert(theed.cast(origin, new THREE.Vector3(sin, 0, cos), 5, makeHit()), 'arcade pillars remain solid');
    }
  }
  console.log('Retail Theed: all three screenshot arcade variants crossed standing/prone at three offsets; pillars and bridge floors retained.');
  for (const x of [-173, -168, -163]) for (const direction of [-1, 1]) {
    const startZ = 14.56 - direction * 10;
    const { soldier } = walk(theed, [x, -50, startZ], direction > 0 ? 0 : Math.PI, 150);
    assert((soldier.position.z - 14.56) * direction > 9, `open Theed arch x=${x}, direction=${direction}`);
    assert(soldier.grounded, 'arch walkway keeps ground support');
  }
  const { soldier } = walk(theed, [-168, -50, 90], 0, 150);
  assert(soldier.position.z < 99, 'visibly closed north gate remains solid');
  console.log('Retail Theed: open arch crossed in both directions at three lateral offsets; closed gate preserved.');
}
const bespin = retailWorld('bes2');
if (bespin) {
  // The three placed switchback ramps, using their source instance transforms.
  for (const [x, z, sign] of [[-133.068, 81.088, -1], [-95.568, 121.123, -1], [-239.965, 77.883, 1]]) {
    for (const offset of [-2, 0, 2]) {
      const { soldier, highest } = walk(bespin, [x + sign * 30, -41, z + offset], sign < 0 ? Math.PI / 2 : -Math.PI / 2, 220);
      assert(Math.abs(soldier.position.x - (x + sign * 30)) > 19, 'cross raised Bespin ramp and its upper seam');
      assert(highest > -33.3 && soldier.grounded, 'gain eight metres while maintaining support');
      const down = walk(bespin, [x + sign * 10, -33, z + offset], sign > 0 ? Math.PI / 2 : -Math.PI / 2, 170).soldier;
      assert(Math.abs(down.position.x - (x + sign * 10)) > 20, 'descend the raised walkway');
      assert(down.grounded && !down.air, 'ramp descent does not flicker into falling');
    }
  }
  console.log('Retail Bespin: three raised ramps crossed uphill/downhill at three lateral offsets.');
}
console.log('Collision checks passed.');

const yavin = retailWorld('yav1');
if (yavin) {
  // Temple's viaduct crosses the water at x=40, from z=-15 to z=49.
  // Keep the full route and its floor support: foliage is decorative here.
  for (const x of [39, 40, 41]) for (const sign of [-1, 1]) {
    const start = new THREE.Vector3(x, -63.33, sign > 0 ? -18 : 52);
    const soldier = new Soldier(yavin, {});
    soldier.spawn(...start.toArray(), sign > 0 ? 0 : Math.PI);
    let lowest = Infinity;
    for (let i = 0; i < 520; i++) {
      soldier.tick({forward:1,right:0},1/60);
      lowest = Math.min(lowest, soldier.position.y);
    }
    assert((soldier.position.z-start.z)*sign>70, `Yavin Temple bridge remains traversable at x=${x}, direction=${sign}`);
    assert(lowest>-70, 'Yavin bridge floor holds the soldier above the water');
  }
  console.log('Retail Yavin Temple: bridge crossed in both directions at three offsets with continuous floor support.');
}
