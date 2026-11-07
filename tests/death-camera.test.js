import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {DeathCamera} from '../src/camera.js';
import {drawnKiller} from '../src/presentation/camera-facts.js';
import {CameraFrame} from './helpers/cameras.js';
import {feedbackRecord,decodeFeedback,presentFeedback} from '../src/runtime/network-combat-feedback.js';
// Numeric death behavior is exercised through frozen phase records and
// native allocation tests. These cases cover the retained identity bridge.
test("the world names the killer in plain numbers: a unit and its life, his Character's slot, or a hull and its life", () => {
  const death = new DeathCamera(null,new CameraFrame().owner);
  const character = {}, other = {};
  death.start(new THREE.PerspectiveCamera(60,16/9,.1,1000), new THREE.Vector3(), true, 1);
  assert.equal(death.deaths, 1);
  assert.deepEqual([death.killerUnit, death.killerCharacter, death.killerHull], [-1, -1, -1]);
  death.blame({ unit: { id: 9 }, generation: 2, character }, [other, character], []);
  assert.deepEqual([death.killerUnit, death.killerLife, death.killerCharacter, death.killerHull], [9, 2, 1, -1]);
  const hull = {};
  death.blame({ vehicle: hull, life: 5 }, [], [{}, hull]);
  assert.deepEqual([death.killerUnit, death.killerHull, death.killerHullLife], [-1, 1, 5]);
  death.blame(null);
  assert.deepEqual([death.killerUnit, death.killerCharacter, death.killerHull], [-1, -1, -1]);
  // A new death starts with none named.
  death.blame({ unit: { id: 9 }, generation: 2, character: null });
  death.start(new THREE.PerspectiveCamera(60,16/9,.1,1000), new THREE.Vector3(), false, 1);
  assert.equal(death.deaths, 2);
  assert.equal(death.killerUnit, -1);
});

test('the page finds the killer where it draws him: his body, his seat, his corpse, his next body', () => {
  const point = (x, y, z) => new THREE.Vector3(x, y, z);
  const unit = { id: 7, generation: 3, alive: true, removed: false, team: 2, soldier: { posture: 'stand' }, actor: { root: { position: point(5, 0, 5) } } };
  const next = { id: 11, generation: 1, alive: true, removed: false, team: 2, soldier: { posture: 'stand' }, actor: { root: { position: point(40, 0, 0) } } };
  const character = { unit: null };
  next.character = character;
  const units = new Map([[7, unit], [11, next]]);
  const hull = { alive: true, team: 1, radius: 4, aiTarget: { generation: 2 }, seatEye: (i, out, presented) => out.set(i, presented ? 9 : -9, 0) };
  const refs = { units: { byId: (id) => units.get(id) ?? null }, characters: [character], vehicles: [hull], relation: (team) => (team === 1 ? 1 : -1) };
  const out = { point: new THREE.Vector3(), size: 0, living: false, friendly: false };
  const death = new DeathCamera(null,new CameraFrame().owner);
  death.blame({ unit, generation: 3, character }, refs.characters, refs.vehicles);
  // Standing, or prone: his eyes over his drawn feet.
  let k = drawnKiller(death, refs, false, out);
  assert.ok(k && k.point.distanceTo(point(5, 1.7, 5)) < 1e-9 && k.living && !k.friendly);
  unit.soldier.posture = 'prone';
  assert.ok(drawnKiller(death, refs, false, out).point.distanceTo(point(5, 0.6, 5)) < 1e-9);
  // In a hull's seat: the seat's eye as the hull is drawn.
  unit.seat = { vehicle: hull, index: 1 };
  k = drawnKiller(death, refs, false, out);
  assert.ok(k.point.distanceTo(point(1, 9, 0)) < 1e-9 && k.size === 8, 'the presented seat');
  unit.seat = null;
  // Dead: Enhanced shows the corpse; Classic, as the original, looks for
  // his Character's next body.
  unit.alive = false;
  k = drawnKiller(death, refs, false, out);
  assert.ok(k && !k.living && k.point.distanceTo(point(5, 0.5, 5)) < 1e-9, 'the corpse');
  assert.equal(drawnKiller(death, refs, true, out)?.living, false, 'no next body yet');
  character.unit = next;
  k = drawnKiller(death, refs, true, out);
  assert.ok(k && k.living && k.point.distanceTo(point(40, 1.7, 0)) < 1e-9, 'his next body');
  // His unit taken by another life, or gone from the world: not him.
  unit.generation = 4;
  assert.equal(drawnKiller(death, refs, false, out), null);
  unit.generation = 3;
  unit.removed = true;
  assert.equal(drawnKiller(death, refs, false, out), null);
  // A hull credited by itself, while it is the same hull's life.
  death.blame({ vehicle: hull, life: 2 }, refs.characters, refs.vehicles);
  k = drawnKiller(death, refs, false, out);
  assert.ok(k && k.point.distanceTo(point(0, 9, 0)) < 1e-9 && k.friendly);
  hull.aiTarget.generation = 3;
  assert.equal(drawnKiller(death, refs, false, out), null, 'a wreck come back is another hull');
});

test("online, the server's damage and death records name who hurt the page's player", () => {
  const own = { id: 8, generation: 3, team: 1, soldier: { aimYaw: 0 } }, bot = { id: 7, generation: 2, team: 2 };
  const notes = [];
  const game = {
    playerUnit: own, units: { byId: (id) => ({ 7: bot, 8: own })[id] ?? null, relation: () => -1 },
    hud: { hit() {}, damageFrom() {}, killMessage() {} },
    noteAttacker: (...args) => notes.push(args),
  };
  const record = (kind, attacker, tick, target = own) => decodeFeedback(feedbackRecord({ id: 1, generation: 1, tick, event: {
    kind, target: { unit: target, generation: target.generation }, attacker: attacker ? { unit: attacker, generation: attacker.generation } : null,
    team: 1, attackerTeam: 2, position: [0, 0, 0], source: [1, 0, 0],
  } }));
  presentFeedback(game, record('damage', bot, 10));
  presentFeedback(game, record('death', bot, 12));
  presentFeedback(game, record('death', null, 14));
  presentFeedback(game, record('death', { id: 99, generation: 1 }, 15));
  // Someone else's death names no one to this page.
  presentFeedback(game, record('death', bot, 16, bot));
  assert.deepEqual(notes, [[bot, false, 10], [bot, true, 12], [null, true, 14], [undefined, true, 15]]);
});
