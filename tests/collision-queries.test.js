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
  const raycast = world.rayGroundHit.bind(world);
  let queries = 0;
  world.rayGroundHit = (...args) => { queries++; return raycast(...args); };
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

// The tree's own visit order, grouped by polygon on first sight: what an
// uncached gather files, written out independently of it.
function treeOrder(world, center, radius) {
  const box = new THREE.Box3(center.clone().addScalar(-radius), center.clone().addScalar(radius));
  const groups = new Map(), triBox = new THREE.Box3();
  world.bvh.shapecast({
    intersectsBounds: (bounds) => bounds.intersectsBox(box),
    intersectsTriangle: (tri, index) => {
      if (!triBox.setFromPoints([tri.a, tri.b, tri.c]).intersectsBox(box)) return false;
      const id = world.polygonOf[index];
      if (!groups.has(id)) groups.set(id, []);
      groups.get(id).push(key(tri));
      return false;
    },
  });
  return [...groups.values()];
}

test('a gather meeting many polygons files them in the tree order, cached or not', () => {
  // A floor of 40 x 40 cells whose triangles are dealt round 97 polygons (or
  // 1,201, past the 128 the polygon index holds before it grows): a sphere
  // over it meets far more polygons than the gather scans for before it
  // takes the index, each polygon's triangles scattered through the tree.
  for (const [polygons, many] of [[97, 32], [1201, 128]]) {
    const floor = new THREE.PlaneGeometry(40, 40, 40, 40).rotateX(-Math.PI / 2);
    const p = floor.attributes.position.array, i = Uint32Array.from(floor.index.array);
    const world = new CollisionWorld(p, i, Uint32Array.from({ length: i.length / 3 }, (_, k) => (k * 7) % polygons));
    const cache = world.gatherCache(), rand = random(), center = new THREE.Vector3();
    const serialize = (list) => list.map((polygon) => polygon.triangles.map(key));
    let most = 0;
    for (let pass = 0; pass < 60; pass++) {
      center.set(rand() * 30 - 15, rand() * 2 - 1, rand() * 30 - 15);
      const radius = pass % 3 ? 0.4 + rand() : 3 + rand() * 3;
      const expected = treeOrder(world, center, radius);
      most = Math.max(most, expected.length);
      assert.deepEqual(serialize(world.gather(center, radius)), expected);
      assert.deepEqual(serialize(world.gather(center, radius, false, cache)), expected);
    }
    assert.ok(most > many, `some gathers met over ${many} polygons (${most})`);
  }
});

test('a hull probe\'s cached gather matches its uncached one, barriers and moving hulls included', () => {
  const { world } = fixture();
  // Vehicle-only barriers: a ring of walls only hulls meet.
  const walls = [];
  for (let k = 0; k < 12; k++) {
    const a = k / 12 * 2 * Math.PI;
    walls.push(box(Math.cos(a) * 15, 1, Math.sin(a) * 15, 4, 2, 0.5));
  }
  const positions = new Float32Array(walls.flatMap((w) => [...w.p]));
  let base = 0;
  const indices = Uint32Array.from(walls.flatMap((w) => { const out = [...w.i].map((v) => v + base); base += w.p.length / 3; return out; }));
  world.vehicleBlockers = world.createBodyShape(positions, indices, Uint32Array.from({ length: indices.length / 3 }, (_, k) => k >> 1));
  // Two hulls driving circles, turning as they go.
  const hull = box(0, 0, 0, 3, 2, 6), shape = world.createBodyShape(hull.p, hull.i, hull.polygons);
  const matrix = new THREE.Matrix4(), q = new THREE.Quaternion(), up = new THREE.Vector3(0, 1, 0);
  const hulls = [0, 1].map((k) => world.addMovingBody(shape, matrix.identity(), { k }));
  const caches = [0, 1, 2].map(() => world.gatherCache(1.5));
  const serialize = (polygons) => polygons.map((polygon) => [polygon.moving, ...polygon.triangles.map(key)]);
  const center = new THREE.Vector3();
  let blocked = 0, moving = 0;
  for (let pass = 0; pass < 400; pass++) {
    hulls.forEach((body, k) => {
      const a = pass * 0.02 + k * Math.PI;
      matrix.compose(new THREE.Vector3(Math.cos(a) * 14, 1, Math.sin(a) * 14), q.setFromAxisAngle(up, -a), new THREE.Vector3(1, 1, 1));
      world.moveBody(body, matrix);
    });
    // The probes of a third hull following the first, a little behind it.
    const a = pass * 0.02 - 0.3;
    for (let k = 0; k < caches.length; k++) {
      center.set(Math.cos(a) * (13 + k * 0.8), 0.9, Math.sin(a) * (13 + k * 0.8));
      const expected = serialize(world.gather(center, 2.6, true));
      const actual = serialize(world.gather(center, 2.6, true, caches[k]));
      assert.deepEqual(actual, expected, `pass ${pass}, probe ${k}`);
      blocked += world.list.some((polygon) => polygon.id >= 0x70000000);
      moving += world.list.some((polygon) => polygon.moving);
    }
  }
  assert.ok(blocked > 0 && moving > 0, 'the probes met barriers and hulls');
  assert.ok(caches.every((cache) => cache.fill < 200), 'the probes reused their caches');
});
