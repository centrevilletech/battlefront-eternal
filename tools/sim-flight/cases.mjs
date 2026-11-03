// Cases for the flight package's functions that the battles reach from
// outside a craft's own tick (rust/sim/src/flight): a bot pilot's stick
// (src/flight/ai.js pilotInput, with its altitude stage raiseOverTerrain),
// the assisted shot and a flyer missile's seeker (src/flight/targeting.js
// assistShot, navigateMissile), the transports' spawn points
// (src/flight/spawn.js transportPosts) and a craft's class by its name
// (src/flight/config.js flightClass). Random and edge inputs go through the
// JavaScript itself, on stand-ins for the battle that answer each call from
// a script and record it, and each case is written with what was asked and
// answered and what came out, for rust/sim/tests/flight_cases.rs to replay
// to the last bit. Run it under the port's reference Node (24.22, V8 13.6):
//
//   node tools/sim-flight/cases.mjs OUT.bfecap [--cases N] [--seed S]
//
// The output holds no game data.
import fs from 'node:fs';
import * as THREE from 'three';
import { Writer, RECORD, MAGIC } from '../sim-capture/format.mjs';
import { pilotInput, raiseOverTerrain } from '../../src/flight/ai.js';
import { assistShot, navigateMissile } from '../../src/flight/targeting.js';
import { transportPosts } from '../../src/flight/spawn.js';
import { FLIGHT_CLASSES, flightClass } from '../../src/flight/config.js';

const argv = process.argv.slice(2);
const out = argv.find((a) => !a.startsWith('--'));
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : Number(argv[i + 1]); };
if (!out) { console.error('usage: node tools/sim-flight/cases.mjs OUT.bfecap [--cases N] [--seed S]'); process.exit(2); }
const N = opt('cases', 20000);

// A seeded generator (mulberry32): the cases are the same every run.
let seed = opt('seed', 1) >>> 0;
const rand = () => { seed = (seed + 0x6d2b79f5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const pick = (list) => list[Math.floor(rand() * list.length)];
const chance = (p) => rand() < p;
const SPECIAL = [0, -0, 1, -1, 0.5, 2, 1e-9, -1e-9, 25, 300, 800, 1e6];
const num = (scale = 100) => (chance(0.06) ? pick(SPECIAL) : (rand() - 0.5) * 2 * scale);
const vec = (scale = 100) => new THREE.Vector3(num(scale), num(scale), num(scale));
const unitVec = () => { const v = new THREE.Vector3(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1); return v.lengthSq() < 1e-6 ? new THREE.Vector3(0, 0, 1) : v.normalize(); };
function quat() {
  if (chance(0.05)) return new THREE.Quaternion(0, 0, 0, 1);
  if (chance(0.05)) return new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), pick([0, Math.PI / 2, Math.PI, -Math.PI / 2]));
  return new THREE.Quaternion(rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1, rand() * 2 - 1).normalize();
}
const v3 = (v) => [v.x, v.y, v.z];
const q4 = (q) => [q.x, q.y, q.z, q.w];

// A craft's configuration as flightConfig makes one: its class's live
// tuning under its own speeds and handling.
function config() {
  const kind = pick(Object.keys(FLIGHT_CLASSES));
  const minSpeed = chance(0.2) ? pick([-10, 0, 10]) : rand() * 40 - 10;
  const cruiseSpeed = chance(0.2) ? pick([0, 20, 40]) : minSpeed + rand() * 40;
  const maxSpeed = chance(0.1) ? pick([cruiseSpeed, 72]) : cruiseSpeed + rand() * 60;
  const own = { minSpeed, cruiseSpeed, maxSpeed, acceleration: 5 + rand() * 20, turnRate: 1.5 + rand() * 2, turnFilter: rand() * 12, pitchRate: rand() * 2,
    pitchFilter: rand() * 12, bankAngle: rand(), bankFilter: rand() * 4, levelFilter: rand() * 4, takeoffHeight: rand() * 10, takeoffTime: rand() * 6,
    takeoffSpeed: rand() * 6, landedHeight: rand(), landingTime: rand() * 6, landingSpeed: rand() * 6, boostSpeed: maxSpeed };
  return { kind, own, config: Object.assign(Object.create(FLIGHT_CLASSES[kind]), own) };
}

// A collision stand-in: each cast answered from a script (a hit or none),
// written into `out` as CollisionWorld.cast writes, and recorded.
function scriptedCollision(calls) {
  return {
    cast(origin, direction, far, hitOut, ignore) {
      const hit = chance(0.45) ? { distance: rand() * Math.max(0, far), point: vec(), normal: unitVec(), terrain: chance(0.5) } : null;
      calls.push({ k: 'cast', origin: v3(origin), direction: v3(direction), far, ignore: ignore?.id ?? null, hit: hit && { distance: hit.distance, point: v3(hit.point), normal: v3(hit.normal), terrain: hit.terrain } });
      if (!hit) return null;
      hitOut.distance = hit.distance; hitOut.point.copy(hit.point); hitOut.normal.copy(hit.normal); hitOut.terrain = hit.terrain;
      return hitOut;
    },
  };
}
// A height or water answer: a number, sometimes not finite, sometimes not one.
const height = () => (chance(0.08) ? pick([NaN, Infinity, -Infinity, undefined, null]) : num(80));

function pilotCase(w) {
  const calls = [];
  const { kind, own, config: c } = config();
  const rounds = Array.from({ length: Math.floor(rand() * 4) }, () => ({ dead: chance(0.3), targetId: pick([7, 8]), position: vec(400) }));
  const v = {
    position: vec(), quaternion: quat(), airspeed: chance(0.1) ? pick([0, -5]) : rand() * 80, radius: chance(0.2) ? pick([0, undefined]) : rand() * 10,
    flightState: pick(['Landed', 'TakingOff', 'Flying', 'Landing']), landingReason: pick(['No landing zone', 'Press SPACE to land']),
    grounded: chance(0.2), stateTime: rand() * 5, flightClock: chance(0.2) ? Math.floor(rand() * 40) / 60 : rand() * 30, power: rand() * 100,
    trick: chance(0.8) ? '' : 'barrel', terrainWarning: chance(0.2), body: { id: 77 }, flightConfig: c, aiTarget: { id: 7 },
    system: { collision: scriptedCollision(calls), flightEnvironment: { space: chance(0.1) }, ordnance: { rounds } },
  };
  const steer = { land: chance(0.1), loop: chance(0.2), pathAltitude: chance(0.4) };
  const unit = { vehicle: v, steer };
  const request = chance(0.15) ? null : { valid: chance(0.85), steeringTarget: vec(), mode: pick([0, 1, 2, 3, 3]),
    ...(chance(0.4) ? { blend: chance(0.1) ? NaN : rand() * 1.4 - 0.2 } : {}), ...(chance(0.3) ? { throttle: chance(0.1) ? NaN : rand() * 2 - 1 } : {}) };
  const aim = chance(0.3) ? null : vec();
  const env = { hasPath: chance(0.5), ...(chance(0.7) ? { minFlyHeight: num(50) } : {}), ...(chance(0.7) ? { maxFlyHeight: rand() * 300 } : {}) };
  if (chance(0.9)) env.height = (x, z) => { const h = height(); calls.push({ k: 'height', x, z, h }); return h; };
  if (chance(0.6)) env.water = (x, z) => { const h = height(); calls.push({ k: 'water', x, z, h }); return h; };
  const stick = {};
  const input = { kind, own, rounds: rounds.map((r) => ({ dead: r.dead, targetId: r.targetId, position: v3(r.position) })),
    v: { position: v3(v.position), quaternion: q4(v.quaternion), airspeed: v.airspeed, radius: v.radius, flightState: v.flightState, landingReason: v.landingReason,
      grounded: v.grounded, stateTime: v.stateTime, flightClock: v.flightClock, power: v.power, trick: v.trick, terrainWarning: v.terrainWarning, space: v.system.flightEnvironment.space },
    steer, request: request && { valid: request.valid, steeringTarget: v3(request.steeringTarget), mode: request.mode, blend: request.blend, throttle: request.throttle }, aim: aim && v3(aim),
    env: { hasPath: env.hasPath, minFlyHeight: env.minFlyHeight, maxFlyHeight: env.maxFlyHeight, height: Boolean(env.height), water: Boolean(env.water) } };
  pilotInput(unit, request, aim, 1 / 60, env, stick);
  w('pilot', { input, calls, out: { ...stick } });
}

function raiseCase(w) {
  const calls = [];
  const { kind, own, config: c } = config();
  const v = { position: vec(), quaternion: quat(), airspeed: chance(0.1) ? 0 : rand() * 80, radius: rand() * 10, flightConfig: c, body: { id: 77 },
    system: { collision: scriptedCollision(calls) } };
  const env = { ...(chance(0.7) ? { minFlyHeight: num(50) } : {}) };
  if (chance(0.9)) env.height = (x, z) => { const h = height(); calls.push({ k: 'height', x, z, h }); return h; };
  if (chance(0.6)) env.water = (x, z) => { const h = height(); calls.push({ k: 'water', x, z, h }); return h; };
  const point = vec();
  const blend = pick([false, true, 0, 0.5, 1, 1.5, -0.5, NaN]);
  const before = v3(point);
  const obstacle = raiseOverTerrain(v, env, point, blend);
  w('raise', { input: { kind, own, position: v3(v.position), quaternion: q4(v.quaternion), airspeed: v.airspeed, radius: v.radius, minFlyHeight: env.minFlyHeight,
    height: Boolean(env.height), water: Boolean(env.water), point: before, blend }, calls, out: { point: v3(point), obstacle } });
}

function assistCase(w) {
  const { kind, own, config: c } = config();
  const target = chance(0.15) ? null : { alive: chance(0.85), point: vec() };
  const id = chance(0.1) ? -1 : 5;
  const v = { flightAssistId: id, flightConfig: c, system: { flightAssists: chance(0.85), units: { byId: (n) => (n === 5 ? target && { alive: target.alive, aimPoint: (o) => o.copy(target.point) } : null) } } };
  const origin = vec();
  const toward = target ? target.point.clone().sub(origin).normalize() : unitVec();
  const direction = chance(0.6) ? toward.clone().add(unitVec().multiplyScalar(rand() * 0.1)).normalize() : unitVec();
  const before = v3(direction);
  assistShot(v, direction, origin);
  w('assist', { input: { kind, assistId: id, assists: v.system.flightAssists, target: target && { alive: target.alive, point: v3(target.point) }, origin: v3(origin), direction: before },
    out: { direction: v3(direction) } });
}

function missileCase(w) {
  const position = vec(400);
  const speed = chance(0.05) ? 0.5 : 50 + rand() * 300;
  const velocity = unitVec().multiplyScalar(speed);
  const toward = chance(0.7);
  const targetPosition = toward ? position.clone().addScaledVector(velocity.clone().normalize().add(unitVec().multiplyScalar(rand() * 0.4)), rand() * 600) : vec(400);
  const flightClassName = pick([undefined, '', 'fighter', 'scout', 'bomber', 'transport']);
  const hull = chance(0.4);
  const roll = chance(0.3) ? (rand() - 0.5) * 20 : chance(0.5) ? undefined : (rand() - 0.5) * 4;
  const targetVelocity = chance(0.15) ? undefined : vec(60);
  const target = { position: targetPosition, velocity: targetVelocity, ...(hull ? { vehicle: { flightRollRate: roll } } : { flightRollRate: roll }) };
  const r = { position, velocity: velocity.clone(), flightClass: flightClassName, targetId: 9 };
  const dt = pick([1 / 60, 1 / 30, 0.1]);
  navigateMissile(r, target, dt);
  w('missile', { input: { position: v3(position), velocity: v3(velocity), flightClass: flightClassName, target: { position: v3(targetPosition), velocity: targetVelocity && v3(targetVelocity), roll, hull }, dt },
    out: { velocity: v3(r.velocity), targetId: r.targetId } });
}

function transportCase(w) {
  const calls = [];
  const vehicles = Array.from({ length: 1 + Math.floor(rand() * 4) }, (_, i) => {
    const flying = chance(0.8);
    return { alive: chance(0.85), flightClass: flying ? pick(['transport', 'transport', 'fighter']) : undefined, flightState: flying ? pick(['Landed', 'Landed', 'Flying']) : null,
      team: pick([0, 1, 2, undefined]), bounds: { max: new THREE.Vector3(rand() * 10, 0, 0) }, quaternion: quat(), position: vec(), yaw: num(3), body: { id: 100 + i }, slot: { i }, label: `v${i}` };
  });
  const system = { vehicles, collision: { cast(origin, direction, far, hitOut, ignore) {
    const hit = chance(0.7) ? { distance: rand() * far, point: vec(), normal: unitVec(), terrain: false } : null;
    if (hit && chance(0.5)) hit.normal.set(0, 1, 0);
    calls.push({ k: 'cast', origin: v3(origin), direction: v3(direction), far, ignore: ignore?.id ?? null, hit: hit && { distance: hit.distance, point: v3(hit.point), normal: v3(hit.normal), terrain: false } });
    if (!hit) return null;
    hitOut.distance = hit.distance; hitOut.point.copy(hit.point); hitOut.normal.copy(hit.normal); hitOut.terrain = false;
    return hitOut;
  } } };
  const posts = transportPosts(system);
  w('transport', { input: { vehicles: vehicles.map((v) => ({ alive: v.alive, flightClass: v.flightClass ?? null, flightState: v.flightState, team: v.team, maxX: v.bounds.max.x,
    quaternion: q4(v.quaternion), position: v3(v.position), yaw: v.yaw, body: v.body.id })) }, calls,
    out: { posts: posts.map((p) => ({ hull: vehicles.indexOf(p.transport), team: p.team, point: v3(p.position), yaw: p.ai.spawnPath.points[0][3] })) } });
}

const NAMES = ['rep_fly_gunship', 'cis_fly_maf', 'all_fly_xwing', 'all_fly_ywing', 'imp_fly_tiebomber', 'imp_fly_tiefighter', 'cis_fly_droidfighter', 'cis_fly_geofighter',
  'rep_fly_jedifighter', 'all_fly_snowspeeder', 'bes_fly_cloudcar', 'all_fly_awing', 'imp_fly_interceptor', 'rep_fly_laat', 'X_BWING', 'Gunship', 'MAFx', 'tieFighterTransport', ''];
function classCase(w) {
  const name = chance(0.6) ? pick(NAMES) : Array.from({ length: Math.floor(rand() * 12) }, () => pick('abcdefgilmnoprstuwxyAEFGILMNORSTW_-'.split(''))).join('');
  w('class', { input: { name }, out: { kind: flightClass(name) } });
}

const writer = new Writer();
const fd = fs.openSync(out, 'w');
fs.writeSync(fd, MAGIC);
let count = 0;
const w = (kind, value) => {
  writer.record(RECORD.frame, count++, `flight.${kind}`, value);
  if (writer.length > (8 << 20)) fs.writeSync(fd, writer.take());
};
writer.record(RECORD.header, 0, 'header', { format: 1, tool: 'sim-flight-cases', node: process.version, v8: process.versions.v8, cases: N });
const kinds = [pilotCase, pilotCase, pilotCase, raiseCase, assistCase, missileCase, missileCase, transportCase, classCase];
for (let i = 0; i < N; i++) pick(kinds)(w);
fs.writeSync(fd, writer.take());
fs.closeSync(fd);
console.log(`${count} cases in ${out} (V8 ${process.versions.v8})`);
