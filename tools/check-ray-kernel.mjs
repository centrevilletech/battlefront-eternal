// The ray kernel (src/ray-kernel.js, ?rays=wasm) against collision.js's own
// JavaScript, on a real battle: every few hundred ticks of a headless battle
// (hulls moving, bodies switching on and off), thousands of random casts,
// castMovings and ground probes, and sphere gathers (cached as a soldier's
// are, and not; with the vehicle-only barriers and without), overlap tests
// and terrainNear lists go through both on the same world, and every field
// of every answer must be the same number, bit for bit (Object.is: -0 is not
// 0), the same body, the same miss, the same polygons in the same order with
// the same triangles. Every query the battle itself makes is answered both
// ways too (the module's answer is the one the battle gets), so a fault
// that shows only in the middle of a battle, after its bodies change, is
// caught where it happens.
//
//   BFE_CONTENT_DIR=<game data> node tools/check-ray-kernel.mjs \
//     [--map nab2] [--side rep] [--seed 1] [--ticks 1800] [--every 300] [--rays 4000]
import { headlessGame, idleCommand } from './headless-world.mjs';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Vehicle } from '../src/vehicles.js';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const map = opt('map', 'nab2'), side = opt('side', 'rep'), seed = Number(opt('seed', 1));
const ticks = Number(opt('ticks', 1800)), every = Number(opt('every', 300)), count = Number(opt('rays', 4000));
const era = ['rep', 'cis'].includes(side) ? 'cw' : 'gcw';
let now = 1000;
performance.now = () => now;
let draw = 991;
Math.random = () => { draw = (Math.imul(draw, 1664525) + 1013904223) >>> 0; return draw / 4294967296; };
let state = seed * 7919 + 17;
const rnd = () => { state = (Math.imul(state, 1103515245) + 12345) >>> 0; return state / 4294967296; };

const same = (a, b) => Object.is(a, b);
const fields = (h) => [h.distance, h.point.x, h.point.y, h.point.z, h.normal.x, h.normal.y, h.normal.z, h.terrain];
const fill = (h) => { h.distance = 123.5; h.point.set(1, 2, 3); h.normal.set(4, 5, 6); h.terrain = 'unset'; h.body = 'unset'; return h; };
let checked = 0, hits = 0, bodyHits = 0, terrainHits = 0, movingHits = 0, groundHits = 0, failures = 0;
let polygons = 0, triangles = 0, movingPolygons = 0, overlaps = 0, nearTriangles = 0;
const corners = (t) => [t.a.x, t.a.y, t.a.z, t.b.x, t.b.y, t.b.z, t.c.x, t.c.y, t.c.z];
const listed = (list) => list.map((p) => [p.id, p.moving, p.done, p.triangles.map(corners)]);
function sameLists(a, b) {
  return a.length === b.length && a.every((p, i) => Object.is(p[0], b[i][0]) && p[1] === b[i][1] && p[2] === b[i][2] && p[3].length === b[i][3].length
    && p[3].every((t, j) => t.every((v, k) => Object.is(v, b[i][3][j][k]))));
}
function fail(kind, args, a, b) {
  failures++;
  if (failures <= 10) console.error(`MISMATCH ${kind}`, JSON.stringify(args), '\n  js  ', JSON.stringify(a).slice(0, 400), '\n  wasm', JSON.stringify(b).slice(0, 400));
}
// The battle's own queries from its loading on, each answered by both (the
// check's rounds pass straight through: `checking`).
let checking = false, battleQueries = 0;
const jsOf = (w, fn) => { const rays = w.rays; w.rays = null; try { return fn(); } finally { w.rays = rays; } };
function battle(name, run) {
  const own = CollisionWorld.prototype[name];
  CollisionWorld.prototype[name] = function (...args) {
    if (checking || !this.rays) return own.apply(this, args);
    battleQueries++;
    return run(this, (...a) => own.apply(this, a), args);
  };
}
// A hull's wall probes (pushWalls), run whole in the module: the
// JavaScript's pushes first, then the module's, which the battle keeps.
let pushes = 0;
const pushWalls = Vehicle.prototype.pushWalls;
Vehicle.prototype.pushWalls = function (oldYaw, q, terrain) {
  const c = this.system.collision;
  if (checking || !c.rays) return pushWalls.call(this, oldYaw, q, terrain);
  pushes++;
  const position = this.position.clone(), velocity = this.velocity.clone(), yaw = this.yaw;
  jsOf(c, () => pushWalls.call(this, oldYaw, q, terrain));
  const a = [...this.position.toArray(), ...this.velocity.toArray(), this.yaw];
  this.position.copy(position); this.velocity.copy(velocity); this.yaw = yaw;
  pushWalls.call(this, oldYaw, q, terrain);
  const b = [...this.position.toArray(), ...this.velocity.toArray(), this.yaw];
  checked++;
  if (!a.every((v, i) => Object.is(v, b[i]))) fail('battle pushWalls', { hull: this.name, position, velocity, terrain }, a, b);
};
// A reference record holding what the caller's held before the query (a
// miss leaves some fields as they were).
const like = (out) => { const h = makeHit(); h.distance = out.distance; h.point.copy(out.point); h.normal.copy(out.normal); h.terrain = out.terrain; h.body = out.body; return h; };
battle('cast', (w, own, [origin, direction, far, out, ...rest]) => {
  const ref = like(out), r = own(origin, direction, far, out, ...rest), rr = jsOf(w, () => own(origin, direction, far, ref, ...rest));
  compare('battle cast', { origin, direction, far, rest: rest.map((x) => x?.raySlot ?? x) }, ref, out, rr, r);
  return r;
});
battle('castMoving', (w, own, [origin, direction, far, out, ignore]) => {
  const ref = like(out), r = own(origin, direction, far, out, ignore), rr = jsOf(w, () => own(origin, direction, far, ref, ignore));
  compare('battle castMoving', { origin, direction, far, ignore: ignore?.raySlot ?? null }, ref, out, rr, r);
  return r;
});
battle('ground', (w, own, [x, z, top, out]) => {
  const ref = { height: 7, normal: new THREE.Vector3(8, 9, 10) };
  w.lastGround.x = NaN; const r = own(x, z, top, out);
  const height = out.height, normal = out.normal.clone();
  w.lastGround.x = NaN; const rr = jsOf(w, () => own(x, z, top, ref));
  checked++;
  if ((r === null) !== (rr === null) || (r && (!same(height, ref.height) || !normal.toArray().every((v, i) => same(v, ref.normal.getComponent(i)))))) fail('battle ground', { x, z, top }, rr && [ref.height, ...ref.normal.toArray()], r && [height, ...normal.toArray()]);
  if (r) { out.height = height; out.normal.copy(normal); }
  return r;
});
battle('gather', (w, own, args) => {
  const b = listed(own(...args)), a = listed(jsOf(w, () => own(...args)));
  checked++;
  if (!sameLists(a, b)) fail('battle gather', { center: args[0], radius: args[1], vehicle: args[2], cached: Boolean(args[3]), ignore: args[4]?.raySlot ?? null }, a, b);
  return own(...args);
});
battle('overlapsSphere', (w, own, args) => {
  const r = own(...args), rr = jsOf(w, () => own(...args));
  checked++;
  if (r !== rr) fail('battle overlapsSphere', { center: args[0], radius: args[1] }, rr, r);
  return r;
});
battle('terrainNear', (w, own, args) => {
  const b = own(...args).map(corners), a = jsOf(w, () => own(...args)).map(corners);
  checked++;
  if (a.length !== b.length || !a.every((t, j) => t.every((v, m) => Object.is(v, b[j][m])))) fail('battle terrainNear', { center: args[0], radius: args[1] }, a, b);
  return own(...args);
});
function compare(kind, args, a, b, ra, rb) {
  checked++;
  const ok = (ra === null) === (rb === null) && fields(a).every((v, i) => same(v, fields(b)[i])) && a.body === b.body;
  if (!ok) {
    failures++;
    if (failures <= 10) console.error(`MISMATCH ${kind}`, JSON.stringify(args), '\n  js  ', ra === null ? 'null' : '', fields(a), a.body?.owner?.name ?? a.body,
      '\n  wasm', rb === null ? 'null' : '', fields(b), b.body?.owner?.name ?? b.body);
  }
  return ok;
}
const game = await headlessGame(`?map=${map}&side=${side}&era=${era}&ai=classic&seed=${seed}&rays=wasm`);
const world = game.collision;
if (!world.rays) throw Error('no ray kernel');
const bounds = world.bvh.getBoundingBox(new THREE.Box3());
if (world.terrain) bounds.union(world.terrain.getBoundingBox(new THREE.Box3()));
bounds.expandByScalar(20);
const size = bounds.getSize(new THREE.Vector3());

// Callers' caches, as a soldier's body spheres and a hull's probes keep them,
// each drifting about its own spot.
const caches = Array.from({ length: 24 }, (_, i) => ({ cache: world.gatherCache(i % 3 ? 0.75 : 0.4), at: null }));
const js = (fn) => jsOf(world, fn);
const point = () => new THREE.Vector3(bounds.min.x + rnd() * size.x, bounds.min.y + rnd() * size.y, bounds.min.z + rnd() * size.z);
function direction() {
  const k = rnd();
  if (k < 0.25) return new THREE.Vector3(0, -1, 0);
  if (k < 0.3) return new THREE.Vector3([1, -1, 0, -0][Math.floor(rnd() * 4)], 0, [0, -0, 1, -1][Math.floor(rnd() * 4)]).normalize();
  if (k < 0.55) return new THREE.Vector3(rnd() - 0.5, (rnd() - 0.5) * 0.2, rnd() - 0.5).normalize();
  return new THREE.Vector3(rnd() - 0.5, rnd() - 0.5, rnd() - 0.5).normalize();
}
function round() {
  const bodies = world.bodies;
  for (let i = 0; i < count; i++) {
    // Mostly random rays through the map; some aimed at a body's middle.
    let origin = point(), dir = direction();
    if (bodies.length && rnd() < 0.3) {
      const body = bodies[Math.floor(rnd() * bodies.length)], center = body.bounds.getCenter(new THREE.Vector3());
      origin = center.clone().add(new THREE.Vector3(rnd() - 0.5, rnd() * 0.5, rnd() - 0.5).multiplyScalar(60));
      dir = center.clone().sub(origin).normalize();
    }
    const far = [512, 50, 5, 1.3, 0.5][Math.floor(rnd() * 5)] * (0.5 + rnd());
    const ignore = rnd() < 0.3 && bodies.length ? bodies[Math.floor(rnd() * bodies.length)] : null;
    const staticOnly = rnd() < 0.3;
    const a = fill(makeHit()), b = fill(makeHit());
    const ra = js(() => world.cast(origin, dir, far, a, ignore, staticOnly)), rb = world.cast(origin, dir, far, b, ignore, staticOnly);
    if (compare('cast', { origin, dir, far, staticOnly, ignore: ignore?.raySlot ?? null }, a, b, ra, rb) && ra) { hits++; if (a.terrain) terrainHits++; else if (a.body) bodyHits++; }
    const c = fill(makeHit()), d = fill(makeHit());
    const rc = js(() => world.castMoving(origin, dir, far, c, ignore)), rd = world.castMoving(origin, dir, far, d, ignore);
    if (compare('castMoving', { origin, dir, far, ignore: ignore?.raySlot ?? null }, c, d, rc, rd) && rc) movingHits++;
    // Ground under a random spot, from a random height (a fresh column each time).
    const x = origin.x, z = origin.z, top = rnd() < 0.5 ? 1e4 : origin.y;
    const g1 = { height: 7, normal: new THREE.Vector3(8, 9, 10) }, g2 = { height: 7, normal: new THREE.Vector3(8, 9, 10) };
    world.lastGround.x = NaN; const r1 = js(() => world.ground(x, z, top, g1));
    world.lastGround.x = NaN; const r2 = world.ground(x, z, top, g2);
    checked++;
    if ((r1 === null) !== (r2 === null) || !same(g1.height, g2.height) || !same(g1.normal.x, g2.normal.x) || !same(g1.normal.y, g2.normal.y) || !same(g1.normal.z, g2.normal.z)) {
      failures++; if (failures <= 10) console.error('MISMATCH ground', x, z, top, r1 && [g1.height, ...g1.normal.toArray()], r2 && [g2.height, ...g2.normal.toArray()]);
    } else if (r1) groundHits++;
    // A sphere: around a cached caller's spot, or anywhere, at a body or not.
    const k = i % caches.length, caller = rnd() < 0.6 ? caches[k] : null;
    let center = point();
    if (caller) {
      if (!caller.at || rnd() < 0.02) caller.at = point();
      caller.at.add(new THREE.Vector3(rnd() - 0.5, (rnd() - 0.5) * 0.4, rnd() - 0.5).multiplyScalar(0.6));
      center = caller.at.clone();
    } else if (bodies.length && rnd() < 0.4) center = bodies[Math.floor(rnd() * bodies.length)].bounds.getCenter(new THREE.Vector3()).add(new THREE.Vector3(rnd() - 0.5, rnd() - 0.5, rnd() - 0.5).multiplyScalar(8));
    const radius = [0.35, 0.5, 0.8, 1.5, 4][Math.floor(rnd() * 5)], vehicle = rnd() < 0.3;
    const l1 = listed(js(() => world.gather(center, radius, vehicle, caller?.cache ?? null, ignore))), l2 = listed(world.gather(center, radius, vehicle, caller?.cache ?? null, ignore));
    checked++;
    if (!sameLists(l1, l2)) fail('gather', { center, radius, vehicle, cached: Boolean(caller), ignore: ignore?.raySlot ?? null }, l1, l2);
    else { polygons += l1.length; triangles += l1.reduce((n, p) => n + p[3].length, 0); movingPolygons += l1.filter((p) => p[1]).length; }
    const o1 = js(() => world.overlapsSphere(center, radius, ignore)), o2 = world.overlapsSphere(center, radius, ignore);
    checked++;
    if (o1 !== o2) fail('overlapsSphere', { center, radius, ignore: ignore?.raySlot ?? null }, o1, o2);
    else if (o1) overlaps++;
    const n1 = js(() => world.terrainNear(center, radius)).map(corners), n2 = world.terrainNear(center, radius).map(corners);
    checked++;
    if (n1.length !== n2.length || !n1.every((t, j) => t.every((v, m) => Object.is(v, n2[j][m])))) fail('terrainNear', { center, radius }, n1, n2);
    else nearTriangles += n1.length;
  }
}
for (let t = 0; t <= ticks; t++) {
  if (t % every === 0) { checking = true; round(); checking = false; }
  if (t === ticks) break;
  now += 1000 / 60;
  game.step(idleCommand(game));
}
const enabled = world.bodies.filter((b) => b.enabled).length, moving = world.bodies.filter((b) => b.moving).length;
console.log(`${map} ${side} seed ${seed}: ${checked} answers compared over ${ticks} ticks (${world.bodies.length} bodies, ${moving} moving, ${enabled} enabled at the end); `
  + `casts hit ${hits} (${bodyHits} bodies, ${terrainHits} terrain), castMoving ${movingHits}, ground ${groundHits}; `
  + `gathers ${polygons} polygons (${movingPolygons} moving) of ${triangles} triangles, overlaps ${overlaps}, terrainNear ${nearTriangles} triangles; `
  + `the battle's own ${battleQueries} queries and ${pushes} hulls' wall probes; ${failures} mismatches`);
process.exit(failures ? 1 : 0);
