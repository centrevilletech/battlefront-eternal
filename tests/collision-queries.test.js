import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';

function box(x, y, z, width, height, depth) {
  const g = new THREE.BoxGeometry(width, height, depth).translate(x, y, z);
  return { p: g.attributes.position.array, i: Uint32Array.from(g.index.array),
    polygons: Uint32Array.from({ length: g.index.count / 3 }, (_, i) => Math.floor(i / 2)) };
}
function triangles(data) {
  const result = [];
  for (let i = 0; i < data.i.length; i += 3) {
    result.push(new THREE.Triangle(...[0, 1, 2].map(j => new THREE.Vector3().fromArray(data.p, data.i[i + j] * 3))));
  }
  return result;
}
function fixture() {
  const map = box(0, -1, 0, 40, 2, 40), terrain = box(0, -3, 0, 120, 1, 120);
  const world = new CollisionWorld(map.p, map.i, map.polygons, terrain.p, terrain.i);
  const bodies = [];
  for (let i = 0; i < 36; i++) {
    const data = box((i % 6) * 7 - 18, (i % 3) + 0.5, Math.floor(i / 6) * 7 - 18, 2, 3, 2);
    const body = world.addBody(data.p, data.i, data.polygons, { i });
    bodies.push({ ...data, triangles: triangles(data), body });
  }
  return { world, solids: [{ ...map, triangles: triangles(map), body: null }, ...bodies],
    terrain: { ...terrain, triangles: triangles(terrain), terrain: true, body: null }, bodies };
}
function random() {
  let seed = 173;
  return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
}
function key(triangle) { return [...triangle.a, ...triangle.b, ...triangle.c].join(','); }

test('repeated terrain columns reuse only exact coordinates and ceiling, without aliasing caller output', () => {
  const { world } = fixture();
  const raycast = world.terrainGrid.raycastFirst.bind(world.terrainGrid);
  let queries = 0;
  world.terrainGrid.raycastFirst = (...args) => { queries++; return raycast(...args); };
  const out = { height: 0, normal: new THREE.Vector3() };
  assert.equal(world.ground(0, 0, 100, out), out);
  const height = out.height, normal = out.normal.clone();
  out.height = 123; out.normal.set(9, 9, 9);
  world.ground(0, 0, 100, out);
  assert.equal(queries, 1);
  assert.equal(out.height, height); assert.deepEqual(out.normal, normal);
  world.ground(.001, 0, 100, out);
  world.ground(.001, .001, 100, out);
  assert.equal(queries, 3, 'lateral movement never reuses a nearby column');
  assert.equal(world.ground(.001, .001, -100, out), null);
  assert.equal(world.ground(.001, .001, -100, out), null);
  assert.equal(queries, 4, 'exact misses are cached, but a different ceiling is not');
  assert.equal(world.ground(.001, .001, 100, out), out);
  assert.equal(queries, 5);
});

test('pooled polygon gathers and sphere clearance match brute-force triangles as live bodies toggle', () => {
  const f = fixture(), rand = random(), center = new THREE.Vector3(), closest = new THREE.Vector3();
  for (let pass = 0; pass < 250; pass++) {
    f.bodies[pass % f.bodies.length].body.enabled = pass % 4 !== 0;
    center.set(rand() * 48 - 24, rand() * 8 - 3, rand() * 48 - 24);
    const radius = 0.2 + rand() * 2;
    const bounds = new THREE.Box3(center.clone().addScalar(-radius), center.clone().addScalar(radius));
    const expected = new Map();
    let overlap = false;
    for (const solid of [...f.solids, f.terrain]) {
      if (solid.body && !solid.body.enabled) continue;
      for (const [i, triangle] of solid.triangles.entries()) {
        triangle.closestPointToPoint(center, closest);
        overlap ||= closest.distanceToSquared(center) < radius * radius;
        if (solid.terrain || !new THREE.Box3().setFromPoints([triangle.a, triangle.b, triangle.c]).intersectsBox(bounds)) continue;
        const id = `${solid.body?.owner.i ?? 'map'}:${solid.polygons[i]}`;
        if (!expected.has(id)) expected.set(id, []);
        expected.get(id).push(key(triangle));
      }
    }
    const actual = f.world.gather(center, radius);
    const groups = entries => entries.map(group => group.sort().join(';')).sort();
    assert.deepEqual(groups(actual.map(poly => poly.triangles.map(key))), groups([...expected.values()]));
    assert(actual.every(poly => !poly.done), 'reused contact records reset their consumed state');
    for (const poly of actual) poly.done = true;
    assert.equal(f.world.overlapsSphere(center, radius), overlap, `clearance probe ${pass}`);
  }
});

test('cached static triangles preserve uncached query order while nearby bodies change', () => {
  const {world,bodies} = fixture(), cache = world.gatherCache();
  const center = new THREE.Vector3();
  const serialize = polygons => polygons.map(polygon => polygon.triangles.map(key));
  let reused = null;
  for (let pass=0;pass<120;pass++) {
    center.set((pass % 30)*.05, .3, (pass % 20)*.025);
    bodies[pass % bodies.length].body.enabled = pass % 3 !== 0;
    const expected = serialize(world.gather(center,1));
    const actual = world.gather(center,1,false,cache);
    assert.deepEqual(serialize(actual),expected);
    assert.ok(actual.every(polygon=>!polygon.done));
    for (const polygon of actual) polygon.done = true;
    const materialized = world.staticTriangles.find(Boolean);
    if (materialized) {
      reused ||= materialized;
      assert.ok(world.staticTriangles.includes(reused), 'the immutable triangle survives repeated gathers');
    }
  }
  assert.ok(reused,'the test queried static building geometry');
});

test('bounded ray queries preserve nearest point, normal, live owner, ignored body, and terrain identity', () => {
  const f = fixture(), rand = random(), ray = new THREE.Ray(), point = new THREE.Vector3(), out = makeHit();
  for (let pass = 0; pass < 450; pass++) {
    f.bodies[pass % f.bodies.length].body.enabled = pass % 4 !== 0;
    ray.origin.set(rand() * 48 - 24, rand() * 8 - 3, rand() * 48 - 24);
    ray.direction.set(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1).normalize();
    const far = rand() * 40, ignore = pass % 3 ? null : f.bodies[pass % f.bodies.length].body;
    let expected = null;
    for (const solid of [...f.solids, f.terrain]) {
      if (solid.body && (!solid.body.enabled || solid.body === ignore)) continue;
      for (const triangle of solid.triangles) {
        if (!ray.intersectTriangle(triangle.a, triangle.b, triangle.c, false, point)) continue;
        const distance = ray.origin.distanceTo(point);
        if (distance > far || (expected && distance >= expected.distance)) continue;
        const normal = triangle.getNormal(new THREE.Vector3());
        if (normal.dot(ray.direction) > 0) normal.negate();
        expected = { distance, normal, point: point.clone(), body: solid.body, terrain: Boolean(solid.terrain) };
      }
    }
    const actual = f.world.cast(ray.origin, ray.direction, far, out, ignore);
    assert.equal(Boolean(actual), Boolean(expected), `ray ${pass}`);
    if (expected) {
      assert(Math.abs(actual.distance - expected.distance) < 1e-7);
      assert(actual.point.distanceTo(expected.point) < 1e-7);
      assert(actual.normal.distanceTo(expected.normal) < 1e-7);
      assert.equal(actual.body, expected.body); assert.equal(actual.terrain, expected.terrain);
    }
  }
});
