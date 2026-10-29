// World objects collide as the original places them: a tree's trunk
// primitives stop a soldier, a raised platform mesh carries one. The
// exporter files a class's collision under the model it draws
// (tools/extract.py collision_model); Yavin's trees and Kashyyyk's dock
// platforms had none because theirs was looked up by the class name. The
// synthetic cases build the same polygons the exporter writes
// (tools/zero_world.py shape_polygons, ear-clipped coll faces) through the
// runtime's own CollisionWorld and Soldier; the retail cases run against an
// extract when one is present (BFE_CONTENT_DIR, or public/).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { CollisionWorld, makeGround, makeHit } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');

// Polygons (lists of points) into the exporter's layout: fan-triangulated
// convex faces, each triangle numbered with its polygon.
function polygonWorld(polygons, terrain = null) {
  const positions = [], indices = [], polygonOf = [];
  polygons.forEach((points, number) => {
    const base = positions.length / 3;
    for (const p of points) positions.push(...p);
    for (let i = 1; i < points.length - 1; i++) {
      indices.push(base, base + i, base + i + 1);
      polygonOf.push(number);
    }
  });
  const ground = terrain ? polygonWorld(terrain) : null;
  return new CollisionWorld(new Float32Array(positions), new Uint32Array(indices), new Uint32Array(polygonOf),
    ground ? ground.positions : new Float32Array(0), ground ? ground.indices : new Uint32Array(0));
}

// A cylinder primitive along Y as tools/zero_world.py writes it: 16 side
// quads and two caps, around (cx, cy, cz).
function cylinder(cx, cy, cz, r, height, segments = 16) {
  const h = height / 2, ring = [];
  for (let i = 0; i < segments; i++) ring.push([cx + r * Math.cos(2 * Math.PI * i / segments), cz + r * Math.sin(2 * Math.PI * i / segments)]);
  const polygons = [];
  for (let i = 0; i < segments; i++) {
    const [x0, z0] = ring[i], [x1, z1] = ring[(i + 1) % segments];
    polygons.push([[x0, cy - h, z0], [x1, cy - h, z1], [x1, cy + h, z1], [x0, cy + h, z0]]);
  }
  polygons.push(ring.map(([x, z]) => [x, cy + h, z]));
  polygons.push(ring.slice().reverse().map(([x, z]) => [x, cy - h, z]));
  return polygons;
}

// A slab's six faces, as a coll mesh of a platform deck: x and z half
// extents, from y0 up to y1.
function slab(cx, cz, hx, hz, y0, y1) {
  const c = [];
  for (const sx of [-1, 1]) for (const sy of [y0, y1]) for (const sz of [-1, 1]) c.push([cx + sx * hx, sy, cz + sz * hz]);
  return [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]].map((face) => face.map((i) => c[i]));
}

const FLOOR = [[[-60, 0, -60], [-60, 0, 60], [60, 0, 60], [60, 0, -60]]];

function walk(world, start, yaw, ticks, watch = () => {}) {
  const soldier = new Soldier(world, {});
  soldier.spawn(...start, yaw);
  for (let i = 0; i < ticks; i++) {
    soldier.tick({ forward: 1, right: 0 }, 1 / 60);
    assert(soldier.position.toArray().every(Number.isFinite), 'finite collision response');
    watch(soldier, i);
  }
  return soldier;
}

test('a tree trunk primitive stops a walking soldier and a ray', () => {
  // Yavin's main trunk: radius 0.9, 15.7 m tall, centred 7.1 m up, so it reaches into the ground.
  const world = polygonWorld(cylinder(0, 7.1, 6, 0.9, 15.7), FLOOR);
  const soldier = walk(world, [0, 0, 0], 0, 150);
  assert(soldier.position.z > 3, 'the soldier walked toward the trunk');
  assert(soldier.position.z < 6 - 0.9 - 0.3, `the trunk's side stops him (z=${soldier.position.z.toFixed(2)})`);
  assert(soldier.grounded, 'he stays on the ground against it');
  const hit = world.cast(new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1), 10, makeHit());
  assert(hit && Math.abs(hit.distance - 5.1) < 0.05, "a ray meets the trunk's near side");
  assert(!world.cast(new THREE.Vector3(3, 1, 0), new THREE.Vector3(0, 0, 1), 10, makeHit()), 'and passes beside it');
});

test('a raised platform mesh carries a soldier across and drops him at its edge', () => {
  // A deck 18 m up, like Kashyyyk's dock platforms, 0.3 m thick.
  const world = polygonWorld(slab(0, 0, 6, 6, 17.7, 18), FLOOR);
  let lowest = Infinity, left = null;
  const soldier = walk(world, [0, 18.2, -5], 0, 300, (s) => {
    if (s.position.z < 5) lowest = Math.min(lowest, s.position.y);
    else left ??= s.position.z;
  });
  assert(lowest > 17.95, `the deck holds him while he crosses it (lowest ${lowest.toFixed(3)})`);
  assert(left !== null, 'he reaches the far edge');
  assert(soldier.position.y < 0.1 && soldier.grounded, 'past the edge he falls to the ground below');
  const probe = world.cast(new THREE.Vector3(2, 30, 2), new THREE.Vector3(0, -1, 0), 40, makeHit());
  assert(probe && !probe.terrain && Math.abs(probe.point.y - 18) < 1e-6, 'a downward ray lands on the deck, not the ground');
});

test('the exporter files a class\'s collision under the model it draws', (t) => {
  const run = spawnSync('python3', ['tests/test_collision_export.py', '-k', 'GeometryName'], { cwd: ROOT, encoding: 'utf8' });
  if (run.error) return t.skip('python3 not available');
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stderr, /Ran 4 tests/);
});

function retailWorld(name) {
  const dir = path.join(CONTENT, 'maps', name);
  if (!fs.existsSync(path.join(dir, 'collision.bin'))) return null;
  const load = (file, Type) => {
    const bytes = fs.readFileSync(path.join(dir, file));
    return new Type(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  };
  return new CollisionWorld(load('collision.bin', Float32Array), load('collision_index.bin', Uint32Array),
    load('collision_poly.bin', Uint32Array), load('terrain_collision.bin', Float32Array), load('terrain_collision_index.bin', Uint32Array));
}

const docks = retailWorld('kas2');
test('Kashyyyk Docks: the placed dock platforms carry a soldier', { skip: !docks && 'no kas2 extract' }, () => {
  // Three kas2_bldg_platform_1.msh placements (their decks' centres).
  for (const [x, y, z] of [[9, 17.47, 16], [212, 18.582, 97], [7, 18.404, 97]]) {
    const probe = docks.cast(new THREE.Vector3(x, y + 20, z), new THREE.Vector3(0, -1, 0), 60, makeHit());
    assert(probe && !probe.terrain && Math.abs(probe.point.y - y) < 0.3, `a deck at (${x}, ${z}); re-import the game data if this fails`);
    for (const yaw of [0, Math.PI / 2]) {
      let lowest = Infinity;
      walk(docks, [x, y + 0.5, z], yaw, 90, (s) => { lowest = Math.min(lowest, s.position.y); });
      assert(lowest > y - 0.3, `the deck at (${x}, ${z}) holds a soldier walking on it (yaw ${yaw})`);
    }
  }
});

const temple = retailWorld('yav1');
test('Yavin Temple: the placed trees stop a soldier at their trunks', { skip: !temple && 'no yav1 extract' }, () => {
  // Three yav_prop_leaftree placements' main trunks (p_tree1, radius 0.9) as
  // the exporter places them: the axis, and the cylinder's top.
  for (const [x, top, z] of [[75.52, -56.56, -26.45], [62.69, -43.61, -157.42], [127.11, -41.9, -200.15]]) {
    const cap = temple.cast(new THREE.Vector3(x, top + 10, z), new THREE.Vector3(0, -1, 0), 30, makeHit());
    assert(cap && !cap.terrain && Math.abs(cap.point.y - top) < 0.1, `a downward ray lands on the trunk's top at (${x}, ${z}); re-import the game data if this fails`);
    for (const [dx, dz, yaw] of [[-5, 0, Math.PI / 2], [5, 0, -Math.PI / 2], [0, -5, 0], [0, 5, Math.PI]]) {
      const ground = temple.ground(x + dx, z + dz, 1000, makeGround());
      assert(ground, 'ground beside the tree');
      const soldier = walk(temple, [x + dx, ground.height, z + dz], yaw, 150);
      const distance = Math.hypot(soldier.position.x - x, soldier.position.z - z);
      assert(distance > 1.2 && distance < 2, `the trunk at (${x}, ${z}) stops a soldier coming from (${dx}, ${dz}): ${distance.toFixed(2)} m from its axis`);
      assert(soldier.grounded, 'he stays on the ground against it');
    }
  }
});
