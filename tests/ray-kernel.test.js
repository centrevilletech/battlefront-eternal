import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { loadRayKernel, RayKernel } from '../src/ray-kernel.js';

test('query input views stay within reserved memory at a page boundary and after growth', () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  let pointer = memory.buffer.byteLength - 130 * 8, capacity = 0;
  const rays = Object.assign(Object.create(RayKernel.prototype), {
    memory, inputView: null,
    x: { input_reserve(count) {
      // A conforming allocator may leave no spare room beyond the request.
      if (count > capacity) {
        capacity = count;
        const missing = pointer + count * 8 - memory.buffer.byteLength;
        if (missing > 0) memory.grow(Math.ceil(missing / 65536));
      }
      return pointer;
    } },
  });
  const first = rays.input(106);
  assert.ok(first.length >= 106);
  assert.ok(first.byteLength <= capacity * 8, 'the whole writable view belongs to the input allocation');
  first[105] = -31.5;
  assert.equal(rays.input(16), first, 'a smaller request reuses the live view');
  memory.grow(1);
  assert.equal(first.byteLength, 0);
  const grown = rays.input(106);
  assert.equal(grown[105], -31.5, 'growth retains the input values');
  assert.equal(grown.buffer, memory.buffer);
  pointer = memory.buffer.byteLength - 513 * 8;
  capacity = 513;
  const moved = rays.input(513);
  assert.equal(moved.byteOffset, pointer);
  assert.equal(moved.length, 513);
  moved[512] = 7;
  assert.equal(rays.input(1), moved, 'a moved allocation is cached at its new address');
  assert.equal(rays.input(513)[512], 7);
});

// The ray kernel (src/ray-kernel.js, rust/sim/src/collision) must answer cast, castMoving
// and ground exactly as collision.js's JavaScript does: the same numbers to
// the last bit, the same body, the same misses, as bodies switch on and off,
// move, come and go.

function box(x, y, z, width, height, depth) {
  const g = new THREE.BoxGeometry(width, height, depth).translate(x, y, z);
  return { p: g.attributes.position.array, i: Uint32Array.from(g.index.array),
    polygons: Uint32Array.from({ length: g.index.count / 3 }, (_, i) => Math.floor(i / 2)) };
}
// Rolling ground, a grid of 64 x 64 quads 2.5 m across, steep in places.
function hills() {
  const n = 64, size = 2.5, p = new Float32Array((n + 1) * (n + 1) * 3), i = [];
  for (let z = 0; z <= n; z++) for (let x = 0; x <= n; x++) {
    const o = (z * (n + 1) + x) * 3;
    p[o] = (x - n / 2) * size; p[o + 2] = (z - n / 2) * size;
    p[o + 1] = -3 + 2 * Math.sin(x * 0.4) * Math.cos(z * 0.3) + (x % 7 === 0 ? 1.5 : 0);
  }
  for (let z = 0; z < n; z++) for (let x = 0; x < n; x++) {
    const a = z * (n + 1) + x, b = a + 1, c = a + n + 1, d = c + 1;
    i.push(a, c, b, b, c, d);
  }
  return { p, i: Uint32Array.from(i) };
}
function random(seed) {
  return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
}
function world(rays) {
  const map = box(0, -1, 0, 40, 2, 40), ground = hills();
  const w = new CollisionWorld(map.p, map.i, map.polygons, ground.p, ground.i, rays);
  const bodies = [];
  for (let i = 0; i < 24; i++) {
    const data = box((i % 6) * 7 - 18, (i % 3) + 0.5, Math.floor(i / 6) * 7 - 18, 2, 3, 2);
    bodies.push(w.addBody(data.p, data.i, data.polygons, { i }));
  }
  const hull = box(0, 0, 0, 4, 2, 6), shape = w.createBodyShape(hull.p, hull.i, hull.polygons), matrix = new THREE.Matrix4();
  for (let i = 0; i < 6; i++) bodies.push(w.addMovingBody(shape, matrix.makeRotationY(i).setPosition(i * 9 - 25, 2, 12), { hull: i }));
  return { w, bodies, shape };
}
const fields = (h) => [h.distance, h.point.x, h.point.y, h.point.z, h.normal.x, h.normal.y, h.normal.z, h.terrain];
const fresh = () => { const h = makeHit(); h.distance = 9.5; h.point.set(1, 2, 3); h.normal.set(4, 5, 6); h.terrain = 'unset'; h.body = 'unset'; return h; };
function same(a, b, ra, rb, what) {
  assert.equal(ra === null, rb === null, `${what}: hit or miss`);
  fields(a).forEach((v, i) => assert.ok(Object.is(v, fields(b)[i]), `${what}: field ${i} ${v} != ${fields(b)[i]}`));
  assert.equal(a.body, b.body, `${what}: body`);
}

test('the ray kernel answers casts, moving casts and ground probes as the JavaScript does', async () => {
  const rays = await loadRayKernel();
  const { w, bodies, shape } = world(rays);
  const rand = random(91), matrix = new THREE.Matrix4(), js = (fn) => { w.rays = null; try { return fn(); } finally { w.rays = rays; } };
  let hits = 0, bodyHits = 0, terrainHits = 0, movingHits = 0, grounds = 0;
  for (let pass = 0; pass < 60; pass++) {
    // Bodies switch on and off; hulls move and turn; one comes and one goes.
    for (const body of bodies) if (rand() < 0.2) body.enabled = !body.enabled;
    for (const body of bodies) if (body.moving) w.moveBody(body, matrix.makeRotationY(rand() * 6.3).premultiply(new THREE.Matrix4().makeRotationX((rand() - 0.5) * 0.4)).setPosition((rand() - 0.5) * 60, rand() * 4, (rand() - 0.5) * 60));
    if (pass % 10 === 5) w.removeBody(bodies.splice(Math.floor(rand() * bodies.length), 1)[0]);
    if (pass % 10 === 7) bodies.push(w.addMovingBody(shape, matrix.makeRotationY(rand()).setPosition(rand() * 20, 1, rand() * 20), { late: pass }));
    for (let i = 0; i < 300; i++) {
      let origin = new THREE.Vector3((rand() - 0.5) * 170, rand() * 12 - 4, (rand() - 0.5) * 170);
      const k = rand();
      let dir = k < 0.25 ? new THREE.Vector3(0, -1, 0) : k < 0.35 ? new THREE.Vector3(rand() < 0.5 ? -0 : 1, 0, rand() < 0.5 ? 0 : -1).normalize()
        : new THREE.Vector3(rand() - 0.5, rand() - 0.5, rand() - 0.5).normalize();
      // Many aimed at a body, from up to 20 m away.
      if (rand() < 0.4) {
        const center = bodies[Math.floor(rand() * bodies.length)].bounds.getCenter(new THREE.Vector3());
        origin = center.clone().add(new THREE.Vector3(rand() - 0.5, rand() * 0.5, rand() - 0.5).multiplyScalar(40));
        dir = center.clone().sub(origin).normalize();
      }
      const far = [512, 60, 6, 1][i % 4], ignore = rand() < 0.3 ? bodies[Math.floor(rand() * bodies.length)] : null, staticOnly = rand() < 0.3;
      const a = fresh(), b = fresh();
      const ra = js(() => w.cast(origin, dir, far, a, ignore, staticOnly)), rb = w.cast(origin, dir, far, b, ignore, staticOnly);
      same(a, b, ra, rb, `cast ${pass}/${i}`);
      if (ra) { hits++; if (a.terrain) terrainHits++; else if (a.body) bodyHits++; }
      const c = fresh(), d = fresh();
      const rc = js(() => w.castMoving(origin, dir, far, c, ignore)), rd = w.castMoving(origin, dir, far, d, ignore);
      same(c, d, rc, rd, `castMoving ${pass}/${i}`);
      if (rc) movingHits++;
      const g1 = { height: 7, normal: new THREE.Vector3(1, 1, 1) }, g2 = { height: 7, normal: new THREE.Vector3(1, 1, 1) }, top = rand() < 0.5 ? 1e4 : origin.y;
      w.lastGround.x = NaN; const r1 = js(() => w.ground(origin.x, origin.z, top, g1));
      w.lastGround.x = NaN; const r2 = w.ground(origin.x, origin.z, top, g2);
      assert.equal(r1 === null, r2 === null);
      assert.ok(Object.is(g1.height, g2.height) && g1.normal.toArray().every((v, j) => Object.is(v, g2.normal.getComponent(j))), `ground ${pass}/${i}`);
      if (r1) grounds++;
    }
  }
  // Every kind of answer came up, many times.
  assert.ok(bodyHits > 1000 && terrainHits > 1000 && hits - bodyHits - terrainHits > 200 && movingHits > 200 && grounds > 5000, JSON.stringify({ hits, bodyHits, terrainHits, movingHits, grounds }));
});

test('a body\'s enabled keeps its value and its copy in the kernel', async () => {
  const rays = await loadRayKernel();
  const { w, bodies } = world(rays);
  const body = bodies[0], hit = makeHit(), down = new THREE.Vector3(0, -1, 0);
  const above = body.bounds.getCenter(new THREE.Vector3()).setY(body.bounds.max.y + 1);
  assert.equal(w.cast(above, down, 5, hit)?.body, body);
  body.enabled = 0;
  assert.equal(body.enabled, 0);
  assert.notEqual(w.cast(above, down, 5, hit)?.body, body);
  body.enabled = true;
  assert.equal(w.cast(above, down, 5, hit)?.body, body);
  assert.equal(w.cast(above, down, 5, hit, body)?.body ?? null, null, 'an ignored body is passed through');
  w.removeBody(body);
  assert.equal(body.enabled, false);
  assert.notEqual(w.cast(above, down, 5, hit)?.body, body);
});

// A gather's list as plain values: each polygon's id, moving and done, and
// each triangle's nine corners.
const listed = (list) => list.map((p) => [p.id, p.moving, p.done, p.triangles.map((t) => [...t.a.toArray(), ...t.b.toArray(), ...t.c.toArray()])]);
function sameList(a, b, what) {
  assert.equal(a.length, b.length, `${what}: polygons`);
  for (let i = 0; i < a.length; i++) {
    const [ida, ma, da, ta] = a[i], [idb, mb, db, tb] = b[i];
    assert.ok(Object.is(ida, idb) && ma === mb && da === db && ta.length === tb.length, `${what}: polygon ${i} ${ida}/${idb} ${ta.length}/${tb.length}`);
    ta.forEach((t, j) => t.forEach((v, k) => assert.ok(Object.is(v, tb[j][k]), `${what}: polygon ${i} triangle ${j} corner ${k} ${v} != ${tb[j][k]}`)));
  }
}

test('the kernel gathers, overlaps and finds the terrain near a sphere as the JavaScript does', async () => {
  const rays = await loadRayKernel();
  const { w, bodies, shape } = world(rays);
  // The vehicle-only barriers: a low wall across the middle.
  const wall = box(3, 0.5, 0, 30, 1, 0.4);
  w.vehicleBlockers = w.createBodyShape(wall.p, wall.i, wall.polygons);
  const rand = random(37), matrix = new THREE.Matrix4(), js = (fn) => { w.rays = null; try { return fn(); } finally { w.rays = rays; } };
  // Callers' caches, each reused at nearby spots as a soldier's are.
  const caches = Array.from({ length: 6 }, (_, i) => w.gatherCache(i % 2 ? 0.75 : 0.3));
  const spots = caches.map(() => new THREE.Vector3((rand() - 0.5) * 50, rand() * 5 - 2, (rand() - 0.5) * 50));
  let polygons = 0, triangles = 0, movingPolygons = 0, blockerPolygons = 0, overlaps = 0, near = 0;
  for (let pass = 0; pass < 80; pass++) {
    for (const body of bodies) if (rand() < 0.15) body.enabled = !body.enabled;
    for (const body of bodies) if (body.moving && rand() < 0.5) w.moveBody(body, matrix.makeRotationY(rand() * 6.3).setPosition((rand() - 0.5) * 40, rand() * 3 - 1, (rand() - 0.5) * 40));
    if (pass % 10 === 5) w.removeBody(bodies.splice(Math.floor(rand() * bodies.length), 1)[0]);
    if (pass % 10 === 7) bodies.push(w.addMovingBody(shape, matrix.makeRotationY(rand()).setPosition(rand() * 20, 1, rand() * 20), { late: pass }));
    for (let i = 0; i < 120; i++) {
      const k = i % caches.length, cache = rand() < 0.7 ? caches[k] : null;
      // Cached callers drift; others query anywhere, some far out.
      const center = cache ? spots[k].add(new THREE.Vector3(rand() - 0.5, (rand() - 0.5) * 0.3, rand() - 0.5).multiplyScalar(0.8)).clone()
        : new THREE.Vector3((rand() - 0.5) * 70, rand() * 8 - 3, (rand() - 0.5) * 70);
      // Now and then the whole map: the module's output and memory grow.
      const radius = i === 60 && pass % 20 === 0 ? 90 + pass : [0.3, 0.6, 1.2, 3, 9][Math.floor(rand() * 5)], vehicle = rand() < 0.3;
      const ignore = rand() < 0.2 ? bodies[Math.floor(rand() * bodies.length)] : null;
      const a = listed(js(() => w.gather(center, radius, vehicle, cache, ignore)));
      const b = listed(w.gather(center, radius, vehicle, cache, ignore));
      sameList(a, b, `gather ${pass}/${i}`);
      polygons += a.length; triangles += a.reduce((n, p) => n + p[3].length, 0);
      movingPolygons += a.filter((p) => p[1]).length; blockerPolygons += a.filter((p) => p[0] >= 0x70000000 && p[0] < 0x71000000).length;
      const o1 = js(() => w.overlapsSphere(center, radius * 0.5, ignore)), o2 = w.overlapsSphere(center, radius * 0.5, ignore);
      assert.equal(o1, o2, `overlapsSphere ${pass}/${i}`);
      if (o1) overlaps++;
      const n1 = js(() => w.terrainNear(center, radius)).map((t) => [...t.a.toArray(), ...t.b.toArray(), ...t.c.toArray()]);
      const n2 = w.terrainNear(center, radius).map((t) => [...t.a.toArray(), ...t.b.toArray(), ...t.c.toArray()]);
      assert.deepEqual(n1, n2, `terrainNear ${pass}/${i}`);
      near += n1.length;
    }
  }
  // Every kind of answer came up.
  assert.ok(polygons > 3000 && triangles > 6000 && movingPolygons > 200 && blockerPolygons > 50 && overlaps > 1000 && near > 10000,
    JSON.stringify({ polygons, triangles, movingPolygons, blockerPolygons, overlaps, near }));
});

test('a query the kernel fails is answered by the JavaScript, and so is every later one', async () => {
  const rays = await loadRayKernel();
  const { w } = world(rays), reference = world(null).w;
  const center = new THREE.Vector3(-18, 1.5, -18), down = new THREE.Vector3(0, -1, 0), above = new THREE.Vector3(-18, 9, -18);
  const before = listed(w.gather(center, 2));
  // The module traps on its next call (a fault in it, or no memory left).
  rays.x = { ...rays.x, gather_begin() { throw new WebAssembly.RuntimeError('unreachable'); } };
  const warn = console.warn, warnings = [];
  console.warn = (message) => warnings.push(message);
  try {
    sameList(listed(w.gather(center, 2)), before, 'the failed gather');
    assert.equal(w.rays, null, 'the kernel is dropped');
    assert.equal(warnings.length, 1);
    sameList(listed(w.gather(center, 2)), listed(reference.gather(center, 2)), 'a later gather');
    const a = fresh(), b = fresh(), ra = w.cast(above, down, 20, a), rb = reference.cast(above, down, 20, b);
    assert.ok(ra && rb && fields(a).every((v, i) => Object.is(v, fields(b)[i])), 'a later cast');
    assert.deepEqual(a.body.owner, b.body.owner, 'the same body of the other world');
    assert.equal(w.overlapsSphere(center, 1), reference.overlapsSphere(center, 1));
  } finally { console.warn = warn; }
});

test('every answer is read through views made after the call, however the module\'s memory grows', async () => {
  const rays = await loadRayKernel();
  const { w, bodies } = world(rays), reference = world(null).w;
  reference.vehicleBlockers = reference.createBodyShape(box(3, 0.5, 0, 30, 1, 0.4).p, box(3, 0.5, 0, 30, 1, 0.4).i, box(3, 0.5, 0, 30, 1, 0.4).polygons);
  w.vehicleBlockers = w.createBodyShape(box(3, 0.5, 0, 30, 1, 0.4).p, box(3, 0.5, 0, 30, 1, 0.4).i, box(3, 0.5, 0, 30, 1, 0.4).polygons);
  // Every call into the module grows its memory, as any call that
  // allocates may: views made before it are empty after it.
  const exports = rays.x, grown = {};
  for (const [name, value] of Object.entries(exports)) grown[name] = typeof value === 'function' ? (...args) => { const out = value(...args); rays.memory.grow(1); return out; } : value;
  rays.x = grown;
  const rand = random(5), caches = [w.gatherCache(), reference.gatherCache()];
  for (let i = 0; i < 120; i++) {
    if (i % 40 === 20) { for (const which of [w, reference]) which.removeBody(which.bodies[3]); }
    const origin = new THREE.Vector3((rand() - 0.5) * 60, rand() * 6, (rand() - 0.5) * 60), dir = new THREE.Vector3(rand() - 0.5, -rand(), rand() - 0.5).normalize();
    const far = [60, 6, 1][i % 3], a = fresh(), b = fresh();
    const ra = w.cast(origin, dir, far, a), rb = reference.cast(origin, dir, far, b);
    assert.ok((ra === null) === (rb === null) && fields(a).every((v, k) => Object.is(v, fields(b)[k])) && a.body?.owner === b.body?.owner || a.body?.owner?.i === b.body?.owner?.i, `cast ${i}`);
    const c = fresh(), d = fresh(), rc = w.castMoving(origin, dir, 60, c), rd = reference.castMoving(origin, dir, 60, d);
    assert.ok((rc === null) === (rd === null) && fields(c).every((v, k) => Object.is(v, fields(d)[k])), `castMoving ${i}`);
    const g1 = { height: 0, normal: new THREE.Vector3() }, g2 = { height: 0, normal: new THREE.Vector3() };
    assert.equal(w.ground(origin.x, origin.z, 1e4, g1) === null, reference.ground(origin.x, origin.z, 1e4, g2) === null);
    assert.ok(Object.is(g1.height, g2.height), `ground ${i}`);
    sameList(listed(w.gather(origin, 1.5, i % 2 === 0, caches[0])), listed(reference.gather(origin, 1.5, i % 2 === 0, caches[1])), `gather ${i}`);
    assert.equal(w.overlapsSphere(origin, 1), reference.overlapsSphere(origin, 1), `overlap ${i}`);
    assert.deepEqual(w.terrainNear(origin, 2).map((t) => t.a.toArray()), reference.terrainNear(origin, 2).map((t) => t.a.toArray()), `terrainNear ${i}`);
  }
  assert.ok(w.rays, 'the kernel answered throughout');
  assert.equal(bodies.length > 0, true);
});

test('a hull\'s wall probes push it in the kernel as in the JavaScript', async () => {
  const { Vehicle } = await import('../src/vehicles.js');
  const { VehiclesWorld } = await import('../src/vehicles-world.js');
  const rays = await loadRayKernel();
  const { w, bodies, shape } = world(rays);
  const wall = box(3, 0.5, 0, 30, 1, 0.4);
  w.vehicleBlockers = w.createBodyShape(wall.p, wall.i, wall.polygons);
  const rand = random(23), matrix = new THREE.Matrix4(), js = (fn) => { w.rays = null; try { return fn(); } finally { w.rays = rays; } };
  // A stand-in hull: what pushWalls reads and writes, its battle the
  // collision world alone (src/vehicles-world.js pushes its probes).
  const system = { collision: w };
  system.world = new VehiclesWorld(system);
  const hull = (walking) => {
    const h = { system, probeRadius: 0.5 + rand() * 1.5, walking, previousYaw: rand() * 6, yaw: 1,
      position: new THREE.Vector3(), velocity: new THREE.Vector3(), previous: new THREE.Vector3(), ownStart: new THREE.Quaternion(),
      probes: Array.from({ length: 6 }, (_, i) => new THREE.Vector3((i % 2 - 0.5) * 3, 0.4 + (i % 3) * 0.6, (Math.floor(i / 2) - 1) * 2.5)) };
    h.wallCaches = h.probes.map(() => w.gatherCache(0.75));
    h.pushWalls = Vehicle.prototype.pushWalls;
    return h;
  };
  const hulls = [hull(false), hull(true), hull(false), hull(true)];
  let moved = 0, undone = 0;
  for (let pass = 0; pass < 150; pass++) {
    for (const body of bodies) if (rand() < 0.1) body.enabled = !body.enabled;
    for (const body of bodies) if (body.moving && rand() < 0.3) w.moveBody(body, matrix.makeRotationY(rand() * 6.3).setPosition((rand() - 0.5) * 50, rand() * 2 - 1, (rand() - 0.5) * 50));
    if (pass === 70) bodies.push(w.addMovingBody(shape, matrix.makeRotationY(1).setPosition(5, 0.5, 5), { late: true }));
    for (const h of hulls) {
      // Near a body or the barrier, sometimes inside them; a walker's last step behind it.
      const near = rand() < 0.5 ? bodies[Math.floor(rand() * bodies.length)].bounds.getCenter(new THREE.Vector3()) : new THREE.Vector3(3 + (rand() - 0.5) * 30, 0, 0);
      const position = near.add(new THREE.Vector3(rand() - 0.5, rand() * 0.5, rand() - 0.5).multiplyScalar(5));
      const velocity = new THREE.Vector3(rand() - 0.5, rand() - 0.5, rand() - 0.5).multiplyScalar(12), previous = position.clone().add(new THREE.Vector3(rand() - 0.5, 0, rand() - 0.5));
      const q = new THREE.Quaternion().setFromEuler(new THREE.Euler((rand() - 0.5) * 0.3, rand() * 6.3, (rand() - 0.5) * 0.3, 'YXZ'));
      const terrain = rand() < 0.5, oldYaw = rand();
      const run = (kernel) => {
        h.position.copy(position); h.velocity.copy(velocity); h.previous.copy(previous); h.yaw = 7;
        if (kernel) h.pushWalls(oldYaw, q, terrain); else js(() => h.pushWalls(oldYaw, q, terrain));
        return [...h.position.toArray(), ...h.velocity.toArray(), h.yaw];
      };
      const a = run(false), b = run(true);
      a.forEach((v, i) => assert.ok(Object.is(v, b[i]), `pass ${pass} hull ${hulls.indexOf(h)} value ${i}: ${v} != ${b[i]}`));
      if (!position.equals(h.position)) moved++;
      if (a[6] === oldYaw) undone++;
    }
  }
  assert.ok(w.rays, 'the kernel answered throughout');
  assert.ok(moved > 100 && undone > 20, JSON.stringify({ moved, undone }));
});
