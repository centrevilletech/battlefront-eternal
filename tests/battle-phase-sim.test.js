import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Battle, Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { killRegion } from '../src/objectives.js';
import { attachBattleSim } from '../src/battle-sim.js';
import { attachPostsSim } from '../src/posts-sim.js';
import { attachBattlePhaseSim } from '../src/battle-phase-sim.js';
import { loadSim } from '../src/sim/module.js';

async function fixture({ count = 70, owner = 0, ...callbacks } = {}) {
  const matrix = new THREE.Matrix4().toArray();
  const posts = new CommandPosts(null, { commandPost: { neutralizeTime: 1, captureTime: 1 }, commandPosts: [
    { name: 'post', team: owner, matrix, hologram: [0, 0, 0], capture: { type: 'sphere', size: [5, 5, 5], matrix } },
  ] }, { headless: true, playerTeam: 1 });
  const battle = new Battle({ posts, teams: { 1: { reinforcements: 100 }, 2: { reinforcements: 100 } }, own: 1, foe: 2 });
  const bodies = Array.from({ length: count }, (_, id) => {
    const character = new Character(id, 2, false);
    const unit = { id, character, team: 2, soldier: true, alive: true, position: new THREE.Vector3(), classDef: { CapturePosts: 1 } };
    character.unit = unit; battle.characters.push(character);
    return unit;
  });
  const instance = await loadSim(), postsSim = attachPostsSim(instance, posts), battleSim = attachBattleSim(instance, battle);
  const hulls = [], log = [];
  const phase = attachBattlePhaseSim({ battleSim, postsSim, units: { all: () => bodies }, vehicles: () => hulls,
    random: () => 0.5, postEvent: e => log.push([e.kind, e.post.name]), ...callbacks });
  return { battle, posts, bodies, hulls, phase, log, instance };
}

test('the native battle phase awards only the first 64 region members and publishes before nested commands', async () => {
  let f;
  f = await fixture({ postEvent(e) {
    assert.equal(e.post, f.posts.posts[0]);
    assert.equal(e.post.team, 2);
    assert.equal(f.battle.characters[1].captures, 1);
    assert.equal(f.battle.characters[64].captures, 0);
    f.battle.point(f.battle.characters[64]);
    f.posts.events.push({ kind: 'neutralized', post: e.post, from: 2 });
  } });
  f.bodies[0].alive = false;
  // Eligibility limits contributions; region awards still include living
  // members that are seated, held or unable to contribute to capture.
  f.bodies[2].seat = {};
  f.bodies[3].held = {};
  f.bodies[4].classDef.CapturePosts = 0;
  f.bodies[5].soldier = false;
  const characters = f.battle.characters, events = f.posts.events;
  f.phase.update(1);
  assert.equal(f.battle.characters, characters);
  assert.equal(f.posts.events, events);
  assert.equal(characters[0].captures, 0);
  assert.ok(characters.slice(1, 65).every(c => c.captures === 1));
  assert.ok(characters.slice(65).every(c => c.captures === 0));
  assert.deepEqual(events.map(e => e.kind), ['neutralized']);
  assert.equal(f.posts.posts[0].counts.get(2), 65);
});

test('a native destroyed post matches source kill-region draws, effects, victim limit and death reentry', async () => {
  const region = { type: 'box', size: [10, 10, 10], matrix: new THREE.Matrix4().toArray() };
  const expected = [], actual = [], deaths = [];
  let referenceDraws = 0, nativeDraws = 0, f;
  const draw = n => ((n * 1103515245 + 12345) >>> 0) / 4294967296;
  killRegion(region, { random: () => draw(++referenceDraws), play: (name, point) => expected.push([name, point.toArray()]), victims: () => [] });
  f = await fixture({ owner: 1, random: () => draw(++nativeDraws), effect(name, point) {
    actual.push([name, point.toArray()]);
    assert.equal(f.posts.posts[0].active, false);
    f.battle.point(f.battle.characters[69]);
  }, killUnit(unit) {
    deaths.push(unit.id); unit.alive = false;
    f.battle.credit(unit.character);
    if (unit.id === 0) {
      f.instance.exports.memory.grow(1);
      f.battle.point(f.battle.characters[68]);
    }
  } });
  f.posts.posts[0].desc.killRegion = region;
  f.posts.posts[0].building = { alive: false, lastAttacker: f.bodies[0] };
  f.hulls.push({ id: 0, alive: true, position: new THREE.Vector3(), setHealth() { assert.fail('the unit limit must precede hulls'); } });
  f.phase.update(1 / 60);
  assert.equal(nativeDraws, 60);
  assert.equal(referenceDraws, nativeDraws);
  assert.deepEqual(actual, expected);
  assert.deepEqual(deaths, Array.from({ length: 64 }, (_, i) => i));
  assert.equal(f.battle.characters[0].captures, 1);
  assert.equal(f.battle.characters[68].captures, 1);
  assert.equal(f.battle.characters[69].captures, 15);
  assert.ok(f.battle.characters.slice(0, 64).every(c => c.deaths === 1));
  assert.deepEqual(f.log, [['lost', 'post']]);
});

test('native battle-over callbacks can reenter the phase and preserve post and Character edits', async () => {
  const f = await fixture({ count: 1, owner: 1 });
  let calls = 0;
  f.battle.onOver = loser => {
    calls++;
    assert.equal(loser, 2);
    assert.equal(f.battle.over, true);
    f.battle.point(f.battle.characters[0]);
    f.posts.posts[0].team = 2;
    f.phase.update(0.25);
  };
  f.bodies[0].soldier = false;
  f.battle.reinforcements[2] = 0;
  f.phase.update(1 / 60);
  assert.equal(calls, 1);
  assert.equal(f.battle.characters[0].captures, 1);
  assert.equal(f.battle.overTime, 0.25);
  assert.equal(f.posts.posts[0].team, 2);
  assert.equal(f.posts.time, 1 / 60 + 0.25);
});
