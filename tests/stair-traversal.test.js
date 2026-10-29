import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { ENGINE, Soldier } from '../src/soldier.js';

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

function walk(world, start, yaw, ticks) {
  const soldier = new Soldier(world, {});
  soldier.spawn(...start, yaw);
  let highest = soldier.position.y;
  for (let i = 0; i < ticks; i++) {
    soldier.tick({ forward: 1, right: 0 }, 1 / 60);
    highest = Math.max(highest, soldier.position.y);
    assert(soldier.position.toArray().every(Number.isFinite));
  }
  return { soldier, highest };
}

const yavin = retailWorld('yav2');
test('Yavin Arena: all four arch staircases climb at three lateral offsets', { skip: !yavin }, () => {
  for (const [x, z, sign] of [[-15.918, 82.206, 1], [15.969, 82, 1], [15.891, -52.343, -1], [-16.135, -52.406, -1]]) {
    for (const side of [-3, 0, 3]) {
      const { soldier, highest } = walk(yavin, [x + side, -53.7, z + sign * 12], sign > 0 ? Math.PI : 0, 190);
      assert((z - soldier.position.z) * sign > 10, `reach upper landing at ${x}, ${z}, offset ${side}`);
      assert(highest > -45.6 && soldier.position.y > -46.1, 'climb the eight metre staircase');
      assert(soldier.grounded && !soldier.air);
      const down = walk(yavin, [x + side, -46, z - sign * 10], sign > 0 ? 0 : Math.PI, 210).soldier;
      assert((down.position.z - z) * sign > 12, 'descend to ground level');
      assert(down.grounded && down.position.y < -53.5);
    }
  }
});

test('Yavin Arena: temple front stairs connect ground to the upper landing', { skip: !yavin }, () => {
  for (const x of [0, 2.5, 5]) {
    const { soldier } = walk(yavin, [x, -53.7, -75], Math.PI, 350);
    assert(soldier.position.z < -117 && soldier.position.y > -38.1, 'climb sixteen metres to the temple');
    assert(soldier.grounded && !soldier.air);
    const down = walk(yavin, [x, -38.04, -117], 0, 350).soldier;
    assert(down.position.z > -76 && down.position.y < -53.5, 'return down the temple stairs');
    assert(down.grounded && !down.air);
  }
});

// Retail stairs are usually authored as sloping collision faces. Check clear
// interior runs on those faces in every map, independent of asset naming.
// Keep both spheres away from edges/walls so a deliberately enclosed ramp or
// a roof meeting another building is not mistaken for a broken staircase.
for (const name of ['bes1', 'bes2', 'end1', 'geo1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab1', 'nab2', 'rhn1', 'rhn2', 'tat1', 'tat2', 'yav1', 'yav2']) {
  test(`${name}: clear inclined collision surfaces support walking in both directions`, (t) => {
    const world = retailWorld(name);
    if (!world) { t.skip('local retail assets are not installed'); return; }
    const tri = new THREE.Triangle(), normal = new THREE.Vector3(), center = new THREE.Vector3();
    const direction = new THREE.Vector3(), point = new THREE.Vector3(), closest = new THREE.Vector3();
    const sphere = new THREE.Vector3();
    let checked = 0;
    for (let i = 0; i < world.indices.length; i += 3) {
      tri.set(...[0, 1, 2].map(j => new THREE.Vector3().fromArray(world.positions, world.indices[i + j] * 3)));
      tri.getNormal(normal);
      if (normal.y < 0) normal.negate();
      if (normal.y <= ENGINE.steepNormal + 0.01 || normal.y > 0.98 || tri.getArea() < 8) continue;
      tri.getMidpoint(center);
      direction.set(-normal.x, 0, -normal.z).normalize();
      direction.y = -(normal.x * direction.x + normal.z * direction.z) / normal.y;
      direction.normalize();
      let clear = true;
      for (let along = -1.3; along <= 1.31; along += 0.1) {
        point.copy(center).addScaledVector(direction, along);
        tri.closestPointToPoint(point, closest);
        if (point.distanceTo(closest) > 0.001) { clear = false; break; }
        for (const [a, b] of [[tri.a, tri.b], [tri.b, tri.c], [tri.c, tri.a]]) {
          new THREE.Line3(a, b).closestPointToPoint(point, true, closest);
          if (point.distanceTo(closest) < 0.7) clear = false;
        }
        for (const height of [0.57, 1.48]) {
          sphere.copy(point); sphere.y += 0.57 / normal.y - 0.57 + height;
          if (world.overlapsSphere(sphere, 0.56)) clear = false;
        }
      }
      if (!clear) continue;
      for (const sign of [-1, 1]) {
        point.copy(center).addScaledVector(direction, -sign * 1.2);
        point.y += 0.57 / normal.y - 0.57;
        const start = point.clone();
        const yaw = Math.atan2(direction.x * sign, direction.z * sign);
        const { soldier } = walk(world, start.toArray(), yaw, 20);
        const progress = soldier.position.clone().sub(start).dot(direction) * sign;
        assert(progress > 0.9, `${name}: blocked incline at ${start.toArray()} (${progress.toFixed(2)}m)`);
        assert(soldier.grounded, `${name}: lost support at ${start.toArray()}`);
      }
      checked++;
    }
    console.log(`${name}: traversed ${checked} clear incline interiors uphill and downhill`);
  });
}
