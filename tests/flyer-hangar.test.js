// Bots out of a hangar and over the terrain as the 2004 game's AI flies them
// (docs/ai-vehicles.md "UnitFlyAgent" and "Vehicle input conversion"): the
// pilot lifts off only while his movement helper is moving him; the climb out
// from under a roof (a straight path that keeps its own heights) is flown as
// laid, with no pull-up into the roof; elsewhere the original game's altitude
// stage raises where he steers to clear the terrain and obstacles ahead by
// 25 m, and the terrain warning still pulls him up as a last resort. The last
// test flies the imported Hoth hangar's snowspeeders (BFE_CONTENT_DIR, or
// public/), which stays out of the repository.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { pilotInput, raiseOverTerrain } from '../src/flight/ai.js';
import { throttleFor } from '../src/flight/config.js';
import { fixture } from './flight-fixture.js';

const DT = 1 / 60;
const at = (x, y, z) => new THREE.Vector3(x, y, z);
// A movement request as Nav's executor writes it on a path (mode 1 at its
// start, 2 along it), or as a stop writes it where the craft is (mode 0).
const request = (target, mode) => ({ valid: true, steeringTarget: target, waypoint: target, segmentDirection: at(0, 0, 1), mode, lower: -1, upper: 1 });
const input = (unit, req, env = {}, aim = null) => pilotInput(unit, req, aim, DT, env, {});
const slowOf = (c) => throttleFor(c, Math.max(c.minSpeed, .25 * c.maxSpeed));

test('a bot pilot stays on his pad while his movement helper stands him still, and lifts off once it moves him', () => {
  const f = fixture();
  f.system.update(DT, f.v, {});
  assert.equal(f.v.flightState, 'Landed');
  const unit = { vehicle: f.v, steer: {} };
  f.v.stateTime = 0;
  // Waiting for his crew, or for his agent's moment: a stop where he is.
  assert.equal(input(unit, request(f.v.position.clone(), 0)).up, 0, 'stood still');
  assert.equal(input(unit, null, {}, at(0, 60, 200)).up, 0, 'only an aim: nowhere to go');
  // Sent along a path: up.
  assert.equal(input(unit, request(at(0, 1, 200), 1)).up, 1, 'at the start of a path');
  assert.equal(input(unit, request(at(0, 1, 200), 2)).up, 1, 'along it');
  // And the craft lifts.
  for (let i = 0; i < 30; i++) f.system.update(DT, f.v, { takeoff: input(unit, request(at(0, 1, 200), 2)).up });
  assert.equal(f.v.flightState, 'TakingOff');
});

// The fixture's craft flying level along +z at cruise, 50 m over a flat
// floor; `roof`: under a deck 12 m above it.
function flying(roof = false) {
  const f = fixture();
  f.system.flightEnvironment.bounds = null;
  f.v.position.set(0, 50, 0); f.v.syncBody();
  f.system.update(DT, f.v, { takeoff: true });
  for (let i = 0; i < 240; i++) f.system.update(DT, f.v, {});
  assert.equal(f.v.flightState, 'Flying');
  f.v.quaternion.identity(); f.v.position.set(0, 50, 0); f.v.syncBody();
  if (roof) {
    const deck = new THREE.BoxGeometry(200, 2, 200).translate(0, 62, 0);
    f.c.addBody(deck.attributes.position.array, deck.index.array, new Uint32Array(deck.index.count / 3));
  }
  return { f, unit: { vehicle: f.v, steer: {} }, ahead: request(at(0, 50, 300), 2) };
}
const flat = { height: () => 0 };

test('the altitude stage raises where a bot steers to clear the terrain ahead by 25 m, and never lowers it', () => {
  const { f, unit, ahead } = flying();
  const v = f.v, reach = 1.5 * v.airspeed;
  // A ridge 80 m high from 40 m ahead: 1.5 s ahead is over it. The point
  // goes onto the climb that is 25 m over it `reach` metres on.
  const ridge = { height: (x, z) => (z > 40 ? 80 : 0) };
  const point = at(0, 50, 300);
  assert.equal(raiseOverTerrain(v, ridge, point), false, 'only terrain ahead');
  assert.ok(Math.abs(point.y - (50 + 300 * (105 - 50) / reach)) < 1e-6, `raised to ${point.y}`);
  assert.ok(input(unit, ahead, ridge).pitch > .5, 'the bot climbs');
  // In a fight too (port: the original game skips the stage with a live target).
  assert.ok(input(unit, ahead, { ...ridge, hasTarget: true }).pitch > .5, 'the bot climbs with a target');
  // Over flat ground 50 m down it is already 25 m clear: left as it is.
  const level = at(0, 50, 300);
  raiseOverTerrain(v, flat, level);
  assert.equal(level.y, 50);
  assert.ok(Math.abs(input(unit, ahead, flat).pitch) < .01, 'level');
  // No higher than the battle's highest fly height.
  const capped = input(unit, ahead, { ...ridge, maxFlyHeight: 60 });
  assert.ok(capped.pitch > 0 && capped.pitch < .1, `capped at 60 m: pitch ${capped.pitch}`);
});

test('an obstacle ahead raises the point over its top and slows the craft', () => {
  const { f, unit, ahead } = flying();
  // A tower 60 m ahead whose top is 90 m up.
  const tower = new THREE.BoxGeometry(40, 90, 10).translate(0, 45, 60);
  f.c.addBody(tower.attributes.position.array, tower.index.array, new Uint32Array(tower.index.count / 3));
  const point = at(0, 50, 300);
  assert.equal(raiseOverTerrain(f.v, flat, point), true, 'met it');
  assert.ok(point.y > 115 + 50, `over its top: ${point.y}`);
  const out = input(unit, ahead, flat);
  assert.ok(out.pitch > .5, `climbing: ${out.pitch}`);
  assert.ok(out.forward <= slowOf(f.v.flightConfig), `slowed: ${out.forward}`);
});

test('on the climb out, a path that keeps its own heights, the craft flies the path as laid, under its roof', () => {
  const { unit, ahead } = flying(true);
  unit.steer.pathAltitude = true; unit.steer.pathOverride = true;
  const ridge = { hasPath: true, height: (x, z) => (z > 40 ? 80 : 0) };
  unit.vehicle.terrainWarning = true;
  const out = input(unit, ahead, ridge);
  assert.ok(Math.abs(out.pitch) < .01, `level along the path: pitch ${out.pitch}`);
  assert.ok(out.forward > 0, `on at its pace: ${out.forward}`);
  // At its first node (mode 1) the original game still runs the stage, and the
  // warning still pulls up.
  assert.equal(input(unit, request(at(0, 50, 300), 1), ridge).pitch, .9);
});

test('in the open the terrain warning still pulls the nose up and slows the craft', () => {
  const { f, unit, ahead } = flying();
  f.v.terrainWarning = true;
  const out = input(unit, ahead, { hasPath: true, ...flat });
  assert.equal(out.pitch, .9);
  assert.equal(out.forward, slowOf(f.v.flightConfig));
});

// The imported Hoth hangar (hot1): its four snowspeeder pads sit under the
// hangar's roof. Each bot pilot waits on his pad for his gunner, lifts off
// as his agent sends him out along the craft's heading, and flies out of
// the door: every takeoff reaches open sky (nothing 100 m above) within a
// few seconds, and none dies under the roof.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
test('imported: Hoth\'s bot snowspeeders fly out of the hangar without hitting its roof', { skip: !fs.existsSync(path.join(CONTENT, 'maps/hot1/map.json')) && 'no hot1 extract' }, async () => {
  process.env.BFE_CONTENT_DIR = CONTENT;
  const { headlessGame, idleCommand } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=hot1&ai=classic&tier=medium&seed=1');
  await game.ready; game.begin?.();
  const T = game.THREE, UP = new T.Vector3(0, 1, 0), hit = { point: new T.Vector3(), normal: new T.Vector3() };
  const roofed = (v) => Boolean(game.collision.cast(v.position, UP, 100, hit, v.body));
  const craft = game.vehicles.vehicles.filter((v) => v.name === 'all_fly_snowspeeder');
  assert.ok(craft.length >= 2 && craft.every((v) => roofed(v)), 'the snowspeeders wait under the hangar roof');
  const runs = new Map(), done = [], lost = [];
  let scrapes = 0;
  for (const v of craft) {
    const damage = v.damage.bind(v);
    v.damage = (amount, attacker, ...rest) => { if (attacker === undefined && amount > 0) scrapes++; return damage(amount, attacker, ...rest); };
  }
  for (let t = 0; t < 45 * 60 && done.length < craft.length; t++) {
    game.step(idleCommand(game));
    for (const v of craft) {
      const run = runs.get(v);
      if (!run && v.flightState === 'TakingOff' && roofed(v)) runs.set(v, { from: t });
      else if (run && !run.end) {
        if (!v.alive) { run.end = t; lost.push(v.name); done.push(run); }
        else if (v.flightState === 'Flying' && !roofed(v)) { run.end = t; done.push(run); }
      }
    }
  }
  assert.ok(done.length >= 2, `${done.length} bot takeoffs from the hangar`);
  assert.deepEqual(lost, [], 'none lost under the roof');
  for (const run of done) assert.ok((run.end - run.from) / 60 < 8, `out in ${((run.end - run.from) / 60).toFixed(1)} s`);
  assert.ok(scrapes < 30 * done.length, `${scrapes} scrapes on the way out`);
});
