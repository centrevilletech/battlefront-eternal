// Leaving the battlefield (src/battlefield.js): the original game's Character
// countdown over what each Character controls, the kill when it runs out, the
// player's warning and his side's voice-over. Synthetic boundaries; then, with
// an import present, a real map's battle.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as THREE from 'three';
import { Battlefield, LeavingWarning, LEAVING_TIME, boundaryFromNav } from '../src/battlefield.js';
import { Boundary } from '../src/ai/classic/nav/boundary.js';
import { Character } from '../src/battle.js';
import { VoiceOver } from '../src/audio/vo.js';

const TICK = 1 / 60;
// A square 200 m across, clockwise in x/z as the maps' are.
const SQUARE = [[-100, 0, -100], [-100, 0, 100], [100, 0, 100], [100, 0, -100]];
const square = () => new Boundary([SQUARE]);

function soldier(x = 0, z = 0) {
  const damage = [];
  const unit = {
    removed: false, alive: true, seat: null, position: new THREE.Vector3(x, 0, z),
    combat: { dead: false, remotes: null, damage(amount) { damage.push(amount); if (amount === Infinity) { this.dead = true; unit.alive = false; } } },
  };
  return { unit, damage };
}
function character(unit) {
  const c = new Character(0, 1, false);
  c.unit = unit;
  return c;
}

test('the map\'s boundary from ai-nav.json: a polygon per resolved Boundary path, missing ones skipped', () => {
  const nav = { paths: [{ name: 'CP1Spawn', points: [[5, 0, 5, 1, 0, 0, 0]] }, { name: 'Boundary', points: SQUARE.map((p) => [...p, 1, 0, 0, 0]) }],
    boundary: { references: [{ hash: 'a', path: 1 }, { hash: 'b', path: null }] } };
  const b = boundaryFromNav(nav);
  assert.equal(b.polygons.length, 1);
  assert.equal(b.outside({ x: 0, z: 0 }), false);
  assert.equal(b.outside({ x: 150, z: 0 }), true);
  // No bnd_ (both Yavin maps): nothing is outside.
  assert.equal(boundaryFromNav({ paths: [], boundary: { configs: [], references: [] } }).outside({ x: 1e4, z: 1e4 }), false);
  assert.equal(new Character(0, 1).outOfBounds, -1);
});

test('outside, a Character has 10 s: the count starts at 10, runs while he stays out, and kills him once below 0', () => {
  const field = new Battlefield(square());
  const { unit, damage } = soldier(150, 0);
  const c = character(unit);
  field.update(TICK, [c]);
  assert.ok(Math.abs(c.outOfBounds - (LEAVING_TIME - TICK)) < 1e-12);
  let ticks = 1;
  while (!unit.combat.dead && ticks < 2000) { field.update(TICK, [c]); ticks++; }
  // 600 ticks of 1/60 s: 10 s.
  assert.equal(ticks, 600);
  assert.deepEqual(damage, [Infinity]);
  assert.ok(c.outOfBounds < 0);
  // Dead, his count is left alone (the original game lets go of the body).
  const left = c.outOfBounds;
  field.update(TICK, [c]);
  assert.equal(c.outOfBounds, left);
});

test('back inside, even for a tick, the count resets; the next time out starts again from 10 s', () => {
  const field = new Battlefield(square());
  const { unit } = soldier(150, 0);
  const c = character(unit);
  for (let i = 0; i < 540; i++) field.update(TICK, [c]);
  assert.ok(c.outOfBounds > 0.9 && c.outOfBounds < 1.1);
  unit.position.x = 50;
  field.update(TICK, [c]);
  assert.equal(c.outOfBounds, -1);
  unit.position.x = 150;
  field.update(TICK, [c]);
  assert.ok(Math.abs(c.outOfBounds - (LEAVING_TIME - TICK)) < 1e-12);
  assert.equal(unit.combat.dead, false);
});

test('in a hull, the hull counts: when the time is up it goes, crediting nobody, and he with it', () => {
  const field = new Battlefield(square());
  const { unit } = soldier(0, 0);
  const destroyed = [];
  const vehicle = { position: new THREE.Vector3(0, 30, 160), lastAttacker: { someone: true }, damage(amount) { destroyed.push([amount, this.lastAttacker]); } };
  unit.seat = { vehicle };
  const c = character(unit);
  for (let i = 0; i < 599; i++) field.update(TICK, [c]);
  assert.equal(destroyed.length, 0);
  field.update(TICK, [c]);
  assert.deepEqual(destroyed, [[Infinity, null]]);
  assert.equal(unit.combat.dead, true);
});

test('a recon droid he flies outside is lost when the time is up; his body inside lives', () => {
  const field = new Battlefield(square());
  const { unit } = soldier(0, 0);
  const droid = { dead: false, position: new THREE.Vector3(160, 5, 0) };
  const killed = [];
  unit.combat.remotes = { piloted: droid, kill(d) { killed.push(d); d.dead = true; this.piloted = null; } };
  const c = character(unit);
  for (let i = 0; i < 600; i++) field.update(TICK, [c]);
  assert.deepEqual(killed, [droid]);
  assert.equal(unit.combat.dead, false);
  field.update(TICK, [c]);
  assert.equal(c.outOfBounds, -1);
});

test('no unit, a dead or removed one, or no boundary: no count', () => {
  const none = character(null);
  const { unit: dead } = soldier(150, 0); dead.combat.dead = true;
  const { unit: removed } = soldier(150, 0); removed.removed = true;
  const field = new Battlefield(square());
  const cs = [none, character(dead), character(removed)];
  field.update(TICK, cs);
  assert.deepEqual(cs.map((c) => c.outOfBounds), [-1, -1, -1]);
  const { unit } = soldier(1e4, 0);
  const c = character(unit);
  new Battlefield(new Boundary()).update(TICK, [c]);
  assert.equal(c.outOfBounds, -1);
});

test('the page\'s warning: what he controls as it sees it, the voice-over once each time out, the flashing spring', () => {
  const field = new Battlefield(square());
  const { unit } = soldier(150, 0);
  assert.equal(field.controlsOutside(unit), true);
  assert.equal(field.controlsOutside(unit, { position: new THREE.Vector3(0, 0, 0) }), false, 'the hull he sits in counts');
  assert.equal(field.controlsOutside(unit, { position: new THREE.Vector3(0, 0, 0) }, { position: new THREE.Vector3(0, 0, 300) }), true, 'and a droid he flies');
  const w = new LeavingWarning();
  assert.equal(w.alpha, 0);
  let tick = 100;
  assert.equal(w.update(tick, true), true);
  assert.equal(w.alpha, 254 / 255);
  const alphas = [];
  let starts = 0;
  for (let i = 0; i < 120; i++) {
    tick++;
    if (w.update(tick, true)) starts++;
    // A page that looks twice a tick (a frame applying ticks, then its own
    // view) flashes no faster.
    if (w.update(tick, true)) starts++;
    alphas.push(w.alpha);
  }
  assert.equal(starts, 0);
  // Between 2 and 255 out of 255, around 128, about 3.6 times a second.
  assert.ok(Math.min(...alphas) >= 2 / 255 && Math.max(...alphas) <= 1);
  assert.ok(Math.min(...alphas) < 0.1 && Math.max(...alphas) > 0.9);
  let peaks = 0;
  for (let i = 1; i < alphas.length - 1; i++) if (alphas[i] > alphas[i - 1] && alphas[i] >= alphas[i + 1] && alphas[i] > 0.8) peaks++;
  assert.ok(peaks >= 6 && peaks <= 8, `${peaks} flashes in 2 s`);
  assert.equal(w.update(++tick, false), false);
  assert.equal(w.alpha, 0);
  assert.equal(w.update(++tick, true), true, 'out again: the line again');
});

test('the voice-over: the listener\'s own side\'s SetOutOfBoundsVoiceOver line', () => {
  const lines = [];
  const audio = { game: { battle: { bleed: {}, over: false } }, streams: { queue: (name) => lines.push(name) } };
  const vo = new VoiceOver(audio, { outOfBounds: { 1: 'repleaving', 2: 'cisleaving' } }, { posts: {} }, 2, { 1: 'Rep', 2: 'Cis' });
  vo.leaving();
  vo.team = 1;
  vo.leaving();
  new VoiceOver(audio, {}, { posts: {} }, 1, {}).leaving();
  assert.deepEqual(lines, ['cisleaving', 'repleaving']);
});

// With an import (BFE_CONTENT_DIR or public/): the battle's own boundary.
const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
const imported = (map) => fs.existsSync(path.join(CONTENT, `maps/${map}/ai-nav.json`));

test('imported maps: every command post inside its boundary; both Yavin maps have none', { skip: !imported('nab2') && 'no import' }, () => {
  for (const map of fs.readdirSync(path.join(CONTENT, 'maps'))) {
    if (!imported(map)) continue;
    const boundary = boundaryFromNav(JSON.parse(fs.readFileSync(path.join(CONTENT, `maps/${map}/ai-nav.json`))));
    const world = JSON.parse(fs.readFileSync(path.join(CONTENT, `maps/${map}/map.json`)));
    if (map.startsWith('yav')) assert.ok(boundary.empty, map);
    for (const post of world.commandPosts || []) {
      const [x, , z] = post.matrix.slice(12, 15);
      assert.equal(boundary.outside({ x, z }), false, `${map} ${post.name}`);
    }
  }
});

test('Theed: a trooper left outside the battlefield dies 10 s after he went out', { skip: !imported('nab2') && 'no import' }, async () => {
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const game = await headlessGame('map=nab2&side=rep&unit=rep_inf_clone_trooper&ai=off&seed=1');
  await game.ready;
  const step = () => { const tick = game.worldClock.tick + 1; game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] }); };
  for (let i = 0; i < 30; i++) step();
  assert.equal(game.battlefield.boundary.empty, false);
  const s = game.soldier;
  // Out past the boundary's first edge, on the ground there (or where it would be).
  const e = game.battlefield.boundary.polygons[0][1];
  const len = Math.hypot(e.nx, e.nz);
  s.position.set(e.x + (e.nx / len) * 20, s.position.y, e.z + (e.nz / len) * 20);
  s.settled.copy(s.position); s.previous.copy(s.position); s.velocity.set(0, 0, 0);
  assert.equal(game.battlefield.outside(s.position), true);
  let ticks = 0;
  while (!game.combat.dead && ticks < 900) { step(); ticks++; }
  assert.equal(ticks, 600);
  assert.equal(game.combat.lastAttacker, null);
});
