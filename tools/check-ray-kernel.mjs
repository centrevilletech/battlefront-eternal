// The ray kernel (src/ray-kernel.js, ?rays=wasm) against collision.js's own
// JavaScript, on a real battle: every few hundred ticks of a headless battle
// (hulls moving, bodies switching on and off), thousands of random casts,
// castMovings and ground probes go through both on the same world, and every
// field of every answer must be the same number, bit for bit (Object.is: -0
// is not 0), the same body, the same miss.
//
//   BFE_CONTENT_DIR=<game data> node tools/check-ray-kernel.mjs \
//     [--map nab2] [--side rep] [--seed 1] [--ticks 1800] [--every 300] [--rays 4000]
import { headlessGame, idleCommand } from './headless-world.mjs';
import * as THREE from 'three';
import { makeHit } from '../src/collision.js';

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

const game = await headlessGame(`?map=${map}&side=${side}&era=${era}&ai=classic&seed=${seed}&rays=wasm`);
const world = game.collision;
if (!world.rays) throw Error('no ray kernel');
const bounds = world.bvh.getBoundingBox(new THREE.Box3());
if (world.terrain) bounds.union(world.terrain.getBoundingBox(new THREE.Box3()));
bounds.expandByScalar(20);
const size = bounds.getSize(new THREE.Vector3());

const same = (a, b) => Object.is(a, b);
const fields = (h) => [h.distance, h.point.x, h.point.y, h.point.z, h.normal.x, h.normal.y, h.normal.z, h.terrain];
const fill = (h) => { h.distance = 123.5; h.point.set(1, 2, 3); h.normal.set(4, 5, 6); h.terrain = 'unset'; h.body = 'unset'; return h; };
let checked = 0, hits = 0, bodyHits = 0, terrainHits = 0, movingHits = 0, groundHits = 0, failures = 0;
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
const js = (fn) => { const rays = world.rays; world.rays = null; try { return fn(); } finally { world.rays = rays; } };
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
  }
}
for (let t = 0; t <= ticks; t++) {
  if (t % every === 0) round();
  if (t === ticks) break;
  now += 1000 / 60;
  game.step(idleCommand(game));
}
const enabled = world.bodies.filter((b) => b.enabled).length, moving = world.bodies.filter((b) => b.moving).length;
console.log(`${map} ${side} seed ${seed}: ${checked} answers compared over ${ticks} ticks (${world.bodies.length} bodies, ${moving} moving, ${enabled} enabled at the end); `
  + `casts hit ${hits} (${bodyHits} bodies, ${terrainHits} terrain), castMoving ${movingHits}, ground ${groundHits}; ${failures} mismatches`);
process.exit(failures ? 1 : 0);
