import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { loadRayKernel } from '../src/ray-kernel.js';

// The ray kernel (src/ray-kernel.js, rust/rays) must answer cast, castMoving
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
