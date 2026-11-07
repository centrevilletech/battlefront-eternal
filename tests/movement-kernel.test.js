import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { loadRayKernel } from '../src/ray-kernel.js';
import { MovementKernel } from '../src/movement-kernel.js';
import { loadSim } from '../src/sim/module.js';
import { Soldier } from './helpers/body.js';

// Hosts can read memory.buffer before discovering fixed movement addresses.
// Lazy pool construction belongs to mover creation, never pointer discovery.
test('movement pointer discovery keeps a captured memory buffer attached', async () => {
  const { exports: x } = await loadSim();
  const buffer = x.memory.buffer;
  const io = x.movement_io_ptr(), flags = x.movement_flags_ptr();
  assert.equal(x.memory.buffer, buffer);
  assert.equal(new Float64Array(buffer, io, 80).length, 80);
  assert.equal(new Uint8Array(buffer, flags, 1 << 16).length, 1 << 16);
});

// Discovery can precede SimBridge construction. All inline owner addresses
// must remain valid while the exchange core is admitted by its first export.
test('cold world pointer discovery preserves memory and later warm addresses', async () => {
  const { exports: x } = await loadSim();
  const buffer = x.memory.buffer;
  const discover = () => [x.movement_io_ptr(), x.movement_flags_ptr(),
    x.io_ptr(), x.enabled_ptr(), x.candidates_ptr(), x.camera_flags_ptr(),
    x.sim_import_args()];
  const before = discover();
  assert.equal(x.memory.buffer, buffer);
  assert.ok(before.every(Number.isInteger));
  assert.equal(x.f2_allocations(), 0);
  x.sim_str_count();
  assert.deepEqual(discover(), before);
});

// Recorded synthetic movement includes terrain, bodies and every soldier posture.
let polygonBase = 0;
function box(x, y, z, width, height, depth, turn = null) {
  const g = new THREE.BoxGeometry(width, height, depth);
  if (turn) g.applyMatrix4(turn);
  g.translate(x, y, z);
  const base = polygonBase; polygonBase += 6;
  return { p: Float32Array.from(g.attributes.position.array), i: Uint32Array.from(g.index.array),
    polygons: Uint32Array.from({ length: g.index.count / 3 }, (_, i) => base + Math.floor(i / 2)) };
}
function merge(parts) {
  const p = [], i = [], polygons = [];
  for (const part of parts) {
    const offset = p.length / 3;
    p.push(...part.p); i.push(...[...part.i].map((v) => v + offset)); polygons.push(...part.polygons);
  }
  return { p: Float32Array.from(p), i: Uint32Array.from(i), polygons: Uint32Array.from(polygons) };
}
// Rolling ground, 64 x 64 quads 2.5 m across, poking through the floor in places.
function hills() {
  const n = 64, size = 2.5, p = new Float32Array((n + 1) * (n + 1) * 3), i = [];
  for (let z = 0; z <= n; z++) for (let x = 0; x <= n; x++) {
    const o = (z * (n + 1) + x) * 3;
    p[o] = (x - n / 2) * size; p[o + 2] = (z - n / 2) * size;
    p[o + 1] = -2.6 + 2 * Math.sin(x * 0.4) * Math.cos(z * 0.3) + (x % 7 === 0 ? 1.5 : 0);
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

// A course: a floor with walls, a staircase, two ramps (walkable, and too
// steep to brake on), a low slab to crawl under, a block and a tower to
// fall from; terrain around and under it; props and two hulls as bodies.
function course(rays) {
  polygonBase = 0;
  const parts = [box(0, -0.5, 0, 40, 1, 40), box(10, 1.5, 0, 0.5, 3, 20), box(-10, 1.5, 5, 0.5, 3, 10), box(0, 1.5, -19.75, 40, 3, 0.5)];
  for (let s = 0; s < 6; s++) { const h = (s + 1) * 0.3; parts.push(box(0, h / 2, 10 + s * 0.6 + 0.3, 4, h, 0.6)); }
  parts.push(box(-15, 1.4, -8, 4, 0.2, 8, new THREE.Matrix4().makeRotationX(-0.6)));
  parts.push(box(-4, 1.4, -10, 4, 0.2, 8, new THREE.Matrix4().makeRotationX(-0.85)));
  parts.push(box(15, 1.3, -12, 6, 0.2, 6));
  parts.push(box(-15, 1.5, 12, 6, 3, 6));
  parts.push(box(16, 5, 14, 5, 10, 5));
  const map = merge(parts), ground = hills();
  const w = new CollisionWorld(map.p, map.i, map.polygons, ground.p, ground.i, rays);
  const props = [];
  for (let i = 0; i < 10; i++) {
    const data = box((i % 5) * 8 - 16, 0.6 + (i % 3) * 0.3, Math.floor(i / 5) * 9 - 4, 1.5, 1.2 + (i % 3) * 0.6, 1.5);
    props.push(w.addBody(data.p, data.i, data.polygons, { prop: i }));
  }
  const hull = box(0, 0, 0, 3, 1.6, 5), shape = w.createBodyShape(hull.p, hull.i, hull.polygons);
  const hulls = [0, 1].map((k) => w.addMovingBody(shape, new THREE.Matrix4().makeRotationY(k).setPosition(k * 12 - 6, 0.8, 4), { hull: k }));
  return { w, props, hulls };
}

const VECTORS = ['position', 'settled', 'previous', 'velocity', 'normal', 'avoidance'];
const NUMBERS = ['aimYaw', 'aimPitch', 'stateTime', 'lock', 'fuel', 'dive', 'groundAdjustment', 'collisionDamage', 'tickLength', 'lookX', 'lookY'];
const OTHERS = ['posture', 'air', 'diveFrom', 'grounded'];
let digest;
function same(a,b) {
 digest.update(JSON.stringify([VECTORS.map(k=>b[k].toArray()),NUMBERS.map(k=>b[k]),OTHERS.map(k=>b[k])]));
}
function twins(w,def,spawn) {
 const s=new Soldier(w,def);s.getupTime=.9;s.diveTime=.6;s.maxFlyHeight=14;
 s.spawn(...spawn);return [s,s];
}
function tickPair(w,kernel,pair,input,dt) { pair[1].tick(input,dt); }

const COVERAGE = () => ({ ticks: 0, prone: 0, crouch: 0, jet: 0, jump: 0, fall: 0, thrown: 0, dive: 0, damage: 0, stepUps: 0, avoid: 0, locked: 0, airborne: 0 });
function cover(c, s, before) {
  c.ticks++;
  if (s.posture === 'prone') c.prone++;
  if (s.posture === 'crouch') c.crouch++;
  if (s.air) c[s.air]++;
  if (s.air) c.airborne++;
  if (s.dive) c.dive++;
  if (s.collisionDamage > 0) c.damage++;
  if (s.groundAdjustment !== before.groundAdjustment && s.position.y - s.previous.y > 0.05 && !s.air) c.stepUps++;
  if (s.avoidance.lengthSq() > 0) c.avoid++;
  if (s.lock > 0) c.locked++;
}

test('Rust soldier course matches recorded movement', async () => {
  digest=createHash("sha256");
  const rays = await loadRayKernel();
  const { w, props, hulls } = course(rays);
  const kernel = new MovementKernel(w);
  const rand = random(7), matrix = new THREE.Matrix4();
  const defs = [{}, {}, {}, { JetJump: 5, JetPush: 8, JetType: 'hover', JetFuelRechargeRate: 0.06, JetFuelCost: 0.2, JetFuelInitialCost: 0.25, JetFuelMinBorder: 0.24, MaxSpeed: 6.5 },
    { JetJump: 21, JetPush: 0, JetFuelRechargeRate: 0.06, JetFuelCost: 0, JetFuelInitialCost: 0.45, JetFuelMinBorder: 0.45, CollisionScale: [0, 0, 0] },
    { JetJump: 8, JetPush: 8, JetType: 'hover', JetFuelRechargeRate: 0.03, JetFuelCost: 0.1, JetFuelInitialCost: 0.2, JetFuelMinBorder: 0.3, Acceleraton: 30, MaxSpeed: 6 }];
  const soldiers = [];
  for (let n = 0; n < 18; n++) soldiers.push({ pair: twins(w, defs[n % defs.length], [(rand() - 0.5) * 44, 2, (rand() - 0.5) * 44, rand() * 6.3]), input: {}, until: 0 });
  // One walks through a prop as a rider's falling body does (passThrough).
  soldiers[1].pair.forEach((s) => { s.passThrough = props[1]; });
  const c = COVERAGE();
  for (let tick = 0; tick < 1500; tick++) {
    for (const body of props) if (rand() < 0.01) body.enabled = !body.enabled;
    hulls.forEach((hull, k) => w.moveBody(hull, matrix.makeRotationY(k + tick * 0.01).setPosition(k * 12 - 6 + 4 * Math.sin(tick * 0.02), 0.8, 4 + 3 * Math.cos(tick * 0.013))));
    for (const one of soldiers) {
      if (tick >= one.until) {
        one.until = tick + 10 + Math.floor(rand() * 40);
        one.input = { forward: rand() < 0.8 ? 1 : rand() * 2 - 1, right: rand() < 0.5 ? 0 : rand() * 2 - 1, turn: rand() < 0.5 ? 0 : rand() * 2 - 1,
          pitch: rand() < 0.7 ? 0 : rand() * 2 - 1, jump: false, crouch: false, prone: false,
          jetDirection: rand() < 0.2 ? new THREE.Vector3(rand() - 0.5, 1, rand() - 0.5) : null };
      }
      const input = { ...one.input, jump: rand() < 0.04, crouch: rand() < 0.015, prone: rand() < 0.01 };
      // An explosion now and then throws him.
      if (rand() < 0.002) one.pair.slice(1).forEach((s) => { s.air = 'thrown'; s.velocity.set(4, 7, -3); s.stateTime = 0; });
      // A stray command (a page's own turn rate).
      if (rand() < 0.01) input.turn = -0;
      const before = { groundAdjustment: one.pair[1].groundAdjustment };
      tickPair(w, kernel, one.pair, input, 1 / 60);
      same(one.pair[0], one.pair[1], `tick ${tick}`);
      cover(c, one.pair[1], before);
      // His look between ticks, as a page's mouse makes it.
      if (rand() < 0.3) one.pair.slice(1).forEach((s) => s.lookRaw(7, -3, 1.25, false));
      // A bot's path executor reads and clears his avoidance.
      if (rand() < 0.05) one.pair.slice(1).forEach((s) => s.avoidance.set(0, 0, 0));
    }
  }
  assert.equal(digest.digest('hex'),'a7250156e1cf231bb444eab2ae0e83298de5d286eb1edba1bec60b99f7a2d330');
  for (const key of ['prone', 'crouch', 'jet', 'jump', 'fall', 'thrown', 'dive', 'avoid', 'locked']) assert.ok(c[key] > 20, `coverage ${key}: ${JSON.stringify(c)}`);
  assert.ok(c.damage > 0 && c.stepUps > 0, `coverage: ${JSON.stringify(c)}`);
});

test('Rust stairs, crawl and falling match recorded movement', async () => {
  digest=createHash("sha256");
  const rays = await loadRayKernel();
  const { w } = course(rays);
  const kernel = new MovementKernel(w);
  const runs = [
    // Up the staircase, three lateral offsets, and down again.
    ...[-1.2, 0, 1.2].flatMap((x) => [{ spawn: [x, 0, 7, 0], script: (t) => ({ forward: 1 }), ticks: 260 }, { spawn: [x, 1.8, 13.8, Math.PI], script: () => ({ forward: 1 }), ticks: 200 }]),
    // Into a wall at an angle, and along it.
    { spawn: [6, 0, 0, 1.2], script: (t) => ({ forward: 1, right: t > 90 ? 0.6 : 0 }), ticks: 240 },
    // Prone under the slab, try to stand there, crawl out and stand.
    { spawn: [9, 0, -12, Math.PI / 2], script: (t) => ({ forward: t > 30 ? 1 : 0, prone: t === 5, jump: t === 160 || t === 330, crouch: t === 120 }), ticks: 400 },
    // Up the walkable ramp, onto the steep one.
    { spawn: [-15, 0, -1, Math.PI], script: () => ({ forward: 1 }), ticks: 300 },
    { spawn: [-4, 0, -3, Math.PI], script: (t) => ({ forward: 1, jump: t % 70 === 69 }), ticks: 300 },
    // Off the tower: a hard landing that hurts, then the lock.
    { spawn: [16, 10.2, 14, 0], script: () => ({ forward: 1 }), ticks: 260 },
    // Dives either way and jumps on the move.
    { spawn: [-4, 0, 4, 0.3], script: (t) => ({ forward: t % 100 < 50 ? 0 : 1, right: t % 200 < 100 ? 1 : -1, jump: t % 50 === 40 }), ticks: 400 },
  ];
  let damage = 0, climbed = 0;
  for (const [n, run] of runs.entries()) {
    const pair = twins(w, {}, run.spawn);
    let highest = -Infinity;
    for (let t = 0; t < run.ticks; t++) {
      const input = { forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false, ...run.script(t) };
      tickPair(w, kernel, pair, input, 1 / 60);
      same(pair[0], pair[1], `run ${n} tick ${t}`);
      if (pair[1].collisionDamage > 0) damage++;
      if (pair[1].grounded) highest = Math.max(highest, pair[1].position.y);
    }
    if (n < 6 && n % 2 === 0 && highest > 1.7) climbed++;
  }
  assert.equal(digest.digest('hex'),'1ccefdf9c11a2f8b4c2f8c4841b132cf54f34c9ed17de258d2906d85f5bf0505');
  assert.ok(damage > 0, 'the fall hurt');
  assert.equal(climbed, 3, 'all three climbs reached the top');
});


test('unknown movement state reports an error',async()=>{const {w}=course(await loadRayKernel());const s=new Soldier(w,{});s.spawn(0,0,0,0);s.air='swimming';assert.throws(()=>s.tick({forward:1,right:0},1/60),/Invalid soldier movement state/);});
